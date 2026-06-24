//! The `metrics` command: measure files, fold them into per-profile [`RepoMetrics`], run the
//! whole-project import-graph / clone passes, and render the panels, JSON, badges, and gates.

use std::fs;
use std::path::PathBuf;

use anyhow::anyhow;
use sloplint_clone::{extract_functions, find_clones, CloneConfig, FunctionUnit};
use sloplint_linter::config::Selector;
use sloplint_linter::detect;
use sloplint_metrics::graph::{self, ImportGraph, ModuleInput};
use sloplint_metrics::test_proxies::{self, FileTestStats};
use sloplint_metrics::{aggregate, file_metrics, FileMetrics, RepoMetrics};

use crate::args::MetricsFormat;
use crate::badges::write_badges;
use crate::compute::{clone_stats_for, concentration_for};
use crate::discover::discover_python_files;
use crate::gates::gate;
use crate::output::{
    metrics_json, metrics_markdown, print_class_rows, print_clone_density, print_concentration,
    print_function_rows, print_metrics_panel, print_package_rows, print_test_proxies_table,
};
use crate::{load_config, module_name, CloneStats, MeasuredFile, Scope};
use sloplint_python::parse;

/// Resolve the `--scope` argument against the configured profiles: absent ⇒ the `default`
/// profile (the quality headline); `all` ⇒ every profile; otherwise it must name a profile.
fn resolve_scope(arg: Option<&str>, selector: &Selector) -> anyhow::Result<Scope> {
    match arg {
        None => {
            let name = selector
                .default_profile()
                .or_else(|| selector.profile_names().first().copied())
                .ok_or_else(|| anyhow!("no metrics profiles are configured"))?;
            Ok(Scope::One(name.to_string()))
        }
        Some("all") => Ok(Scope::All),
        Some(name) if selector.profile_names().contains(&name) => Ok(Scope::One(name.to_string())),
        Some(name) => Err(anyhow!(
            "unknown --scope '{name}'; configured profiles: {} (or 'all')",
            selector.profile_names().join(", ")
        )),
    }
}

/// Per-file measurement pass for `metrics`: parse each file once, classify it into its
/// profile(s), and collect the per-function/-class metrics plus the inputs the whole-project
/// passes need — clone fingerprints (when `needs_clones`), module imports (when `needs_graph`),
/// and static test-proxy stats.
struct Measured {
    per_file: Vec<MeasuredFile>,
    clone_units: Vec<FunctionUnit>,
    unit_profiles: Vec<Vec<String>>,
    module_inputs: Vec<(ModuleInput, Vec<String>)>,
    test_stats: Vec<FileTestStats>,
}

fn measure_files(
    files: Vec<PathBuf>,
    selector: &Selector,
    clone_config: &CloneConfig,
    needs_clones: bool,
    needs_graph: bool,
) -> Measured {
    // Every function's clone fingerprint plus the profiles of the file it came from, so the SLP020
    // pass can run once over the whole tree and be filtered per profile afterwards.
    let mut clone_units: Vec<FunctionUnit> = Vec::new();
    let mut unit_profiles: Vec<Vec<String>> = Vec::new();
    // Keep path + source alongside metrics so the gate can name offending functions with a
    // resolved `path:line` location.
    let mut per_file: Vec<MeasuredFile> = Vec::new();
    // Each module input carries its file's profile membership so the import graph can be built
    // per profile — one profile importing another must not manufacture coupling in the
    // first profile's architecture metrics.
    let mut module_inputs: Vec<(ModuleInput, Vec<String>)> = Vec::new();
    // Static test proxies: one per file. The test/production split is bound to the `tests`
    // profile so the proxies and the panels agree.
    let mut test_stats: Vec<FileTestStats> = Vec::new();
    for path in files {
        let display = path.to_string_lossy().to_string();
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(parsed) = parse(&source) else {
            continue;
        };
        let metrics = file_metrics(&source, &parsed);
        // Machine-generated code is a third category alongside tests/production: its
        // structural numbers are codegen artifacts, so it routes into the `generated` profile and
        // out of the `production` complement. Detection is a cheap header-marker scan.
        let is_generated = detect::is_generated(&source, &display);
        let profiles: Vec<String> = selector
            .profiles_for_file(&display, is_generated)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let is_test = profiles.iter().any(|p| p == "tests");
        // Generated *production* code is excluded from the test:code proxies (it is not
        // human-maintained, so it must not inflate the production-LoC denominator). A generated
        // file that is also a test still counts as a test — the panels claim it under both, so the
        // proxies must agree rather than dropping it from both sides.
        if is_test || !is_generated {
            test_stats.push(test_proxies::file_test_stats(is_test, metrics.loc, &parsed));
        }
        if needs_clones {
            for unit in extract_functions(&display, &source, &parsed, clone_config.shingle_k) {
                clone_units.push(unit);
                unit_profiles.push(profiles.clone());
            }
        }
        if needs_graph {
            if let Some(name) = module_name(&path) {
                module_inputs.push((
                    ModuleInput {
                        name,
                        imports: graph::scan_module_imports(&parsed),
                        loc: metrics.loc,
                        classes: metrics.classes.len(),
                        abstract_classes: metrics.classes.iter().filter(|c| c.is_abstract).count(),
                    },
                    profiles.clone(),
                ));
            }
        }
        per_file.push(MeasuredFile {
            path: display,
            source,
            metrics,
            profiles,
        });
    }
    Measured {
        per_file,
        clone_units,
        unit_profiles,
        module_inputs,
        test_stats,
    }
}

/// Compute and report software-quality metrics; optionally emit badges and enforce
/// complexity gates. Returns `Ok(false)` only when a `--max-*` ceiling is set and some function
/// exceeds it — the CI gate. Reporting/badge writing always happens first so the numbers are
/// visible even on a failing gate.
pub(crate) fn run_metrics(
    paths: &[String],
    format: MetricsFormat,
    badges: Option<&str>,
    config_path: Option<&str>,
    max_cyclomatic: Option<usize>,
    max_cognitive: Option<usize>,
    scope: Option<String>,
) -> anyhow::Result<bool> {
    // Profiles drive classification (which panel a file feeds) the same way they drive `check`'s
    // rule config. Load best-effort: an explicit --config is strict, discovery falls back to the
    // built-in profiles so a malformed ancestor toml can't break `metrics`.
    let config = load_config(config_path, false, false)?;
    let selector = config
        .prepare()
        .map_err(|e| anyhow!("invalid glob in config: {e}"))?;
    let scope = resolve_scope(scope.as_deref(), &selector)?;
    let profile_names: Vec<String> = selector
        .profile_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let (files, _) = discover_python_files(paths);
    // The package feed and the JSON rollup need the first-party import graph, which is a
    // whole-project pass (like SLP180): collect every file's module-level imports here, then
    // build the graph once after the loop.
    let needs_graph = matches!(format, MetricsFormat::Packages | MetricsFormat::Json);
    // Duplication density is surfaced only on the aggregate panels, not the per-unit feeds.
    let needs_clones = matches!(
        format,
        MetricsFormat::Text | MetricsFormat::Json | MetricsFormat::Github
    );
    let clone_config = CloneConfig {
        min_statements: config.clone.min_statements,
        similarity: config.clone.similarity,
        ..CloneConfig::default()
    };
    let Measured {
        mut per_file,
        clone_units,
        unit_profiles,
        module_inputs,
        test_stats,
    } = measure_files(files, &selector, &clone_config, needs_clones, needs_graph);

    // DIT/NOC are whole-project properties: a class's inheritance depth and breadth depend on
    // bases/children defined in *other* files (a class in one profile may extend a base in
    // another), so resolve them across the FULL set — before any per-profile split — so every
    // panel/feed that surfaces them (the class feed, the JSON/text/github DIT-NOC figures) sees the
    // real values. Skip only the formats that show neither (the per-function and per-package feeds).
    if !matches!(format, MetricsFormat::Functions | MetricsFormat::Packages) {
        let mut metrics: Vec<&mut FileMetrics> =
            per_file.iter_mut().map(|f| &mut f.metrics).collect();
        sloplint_metrics::resolve_inheritance(&mut metrics);
    }

    // The aggregate panel for one profile: the files that profile claims.
    let panel_of = |name: &str| {
        let metrics: Vec<FileMetrics> = per_file
            .iter()
            .filter(|f| f.profiles.iter().any(|p| p == name))
            .map(|f| f.metrics.clone())
            .collect();
        aggregate(&metrics)
    };
    // The import graph for one profile: only that profile's modules, so resolution can't reach
    // across a profile boundary.
    let graph_of = |name: &str| {
        ImportGraph::build(
            module_inputs
                .iter()
                .filter(|(_, ps)| ps.iter().any(|p| p == name))
                .map(|(m, _)| m.clone())
                .collect(),
        )
    };
    // SLP020 clone detection, run once over every function. Per-profile density is derived
    // by keeping only pairs whose *both* functions are in the profile — duplication internal to it,
    // consistent with how the import graph is scoped.
    let clone_pairs = find_clones(&clone_units, &clone_config);
    let clone_of = |name: &str| clone_stats_for(name, &unit_profiles, &clone_pairs);

    if let MetricsFormat::Functions = format {
        print_function_rows(&per_file, &scope);
    } else if let MetricsFormat::Classes = format {
        print_class_rows(&per_file, &scope);
    } else if let MetricsFormat::Packages = format {
        // For `all`, the graph is the whole tree; for one profile, just its modules.
        let graph = match &scope {
            Scope::All => {
                ImportGraph::build(module_inputs.iter().map(|(m, _)| m.clone()).collect())
            }
            Scope::One(name) => graph_of(name),
        };
        print_package_rows(&graph);
    } else {
        let proxies = test_proxies::aggregate_test_proxies(&test_stats);
        // The profile(s) this scope reports, in declaration order.
        let scoped: Vec<&str> = match &scope {
            Scope::All => profile_names.iter().map(String::as_str).collect(),
            Scope::One(name) => vec![name.as_str()],
        };
        match format {
            MetricsFormat::Text => {
                // One panel per in-scope profile; the proxies (always the full split) follow once.
                for name in &scoped {
                    print_metrics_panel(name, &panel_of(name));
                    // Package module-count concentration — node distribution, computed from
                    // the panel's own files (edge-free, so no import graph is needed in text mode).
                    print_concentration(&concentration_for(&per_file, name));
                    // Duplication density: SLP020 clone ratio for the profile's functions.
                    print_clone_density(&clone_of(name));
                }
                print_test_proxies_table(&proxies);
            }
            // JSON is the comprehensive machine feed and ignores `--scope`: a panel for every
            // configured profile under `profiles`, plus the all-files `test_proxies`.
            MetricsFormat::Json => {
                let panels: Vec<(String, RepoMetrics, ImportGraph, CloneStats)> = profile_names
                    .iter()
                    .map(|name| (name.clone(), panel_of(name), graph_of(name), clone_of(name)))
                    .collect();
                println!("{}", metrics_json(&panels, &proxies));
            }
            MetricsFormat::Github => {
                let panels: Vec<(&str, RepoMetrics, CloneStats)> = scoped
                    .iter()
                    .map(|name| (*name, panel_of(name), clone_of(name)))
                    .collect();
                println!("{}", metrics_markdown(&panels, &proxies));
            }
            MetricsFormat::Functions | MetricsFormat::Classes | MetricsFormat::Packages => {
                unreachable!()
            }
        }
        if let Some(dir) = badges {
            // Badges report the scoped panel — the `default` profile by default, the quality
            // headline; for `all`, the whole tree.
            let repo = match &scope {
                Scope::All => aggregate(
                    &per_file
                        .iter()
                        .map(|f| f.metrics.clone())
                        .collect::<Vec<_>>(),
                ),
                Scope::One(name) => panel_of(name),
            };
            write_badges(dir, &repo, &config.badges)?;
        }
    }

    // CI gates: run both so all offenders are reported, then fail if either tripped.
    let over_cyclomatic = gate(&per_file, max_cyclomatic, "cyclomatic", |f| f.cyclomatic);
    let over_cognitive = gate(&per_file, max_cognitive, "cognitive", |f| f.cognitive);
    Ok(!over_cyclomatic && !over_cognitive)
}
