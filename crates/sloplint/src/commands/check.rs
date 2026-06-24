//! The `check` command: discover files, run the per-file rules in one pass, drive the
//! cross-file passes (clones/fanout/imports/ghost), apply `--fix` edits, and report.

use std::fs;
use std::path::PathBuf;

use anyhow::anyhow;
use sloplint_clone::{extract_functions, CloneConfig, FunctionUnit};
use sloplint_diagnostics::fix;
use sloplint_diagnostics::render::render_diagnostics;
use sloplint_linter::config::{Config, Selector};
use sloplint_linter::lint::{check_file, FileContext, Rule};
use sloplint_linter::registry::{Registry, WholeProjectRule};
use sloplint_linter::suppression::Suppressions;
use sloplint_linter::{clones, ghost, imports};
use sloplint_python::parse;
use sloplint_report::ReportEntry;

use crate::args::Format;
use crate::corrupted;
use crate::cross_file::{
    attribute_clones, attribute_fanout, attribute_ghost_scaffolding, attribute_undeclared_imports,
};
use crate::discover::discover_python_files;
use crate::{load_config, FileResult};

/// `--fix` / `--unsafe-fixes` state for a `check` run.
#[derive(Clone, Copy)]
pub(crate) struct FixMode {
    /// Whether to apply fixes at all (`--fix`). `--unsafe-fixes` alone is a no-op.
    enabled: bool,
    /// Whether to also apply `Unsafe` fixes (`--unsafe-fixes`).
    allow_unsafe: bool,
}

impl FixMode {
    pub(crate) fn new(fix: bool, unsafe_fixes: bool) -> Self {
        Self {
            enabled: fix,
            allow_unsafe: unsafe_fixes,
        }
    }
}

/// Per-file lint pass for `check`: read+parse each file, run the enabled per-file rules,
/// and collect the inputs the whole-project passes need — clone units (SLP020), import
/// scans (SLP180), and ghost scans (SLP240). `had_error` flags any read/parse failure.
struct Scanned {
    results: Vec<FileResult>,
    units: Vec<FunctionUnit>,
    unit_result: Vec<usize>,
    import_scans: Vec<(String, Vec<imports::ImportRef>)>,
    ghost_scans: Vec<ghost::FileScan>,
    had_error: bool,
}

fn scan_files(
    files: &[PathBuf],
    selector: &Selector,
    registry: &Registry,
    clone_config: &CloneConfig,
    config: &Config,
) -> Scanned {
    let mut had_error = false;
    // Per-file results are collected first; cross-file clone detection (SLP020) needs every
    // file's functions before it can report duplicates, so we render only at the end.
    let mut results: Vec<FileResult> = Vec::new();
    let mut units: Vec<FunctionUnit> = Vec::new();
    let mut unit_result: Vec<usize> = Vec::new();
    // SLP180 (preview) is a whole-project rule: collect every file's module-level imports,
    // then resolve them against the manifest after the loop.
    let mut import_scans: Vec<(String, Vec<imports::ImportRef>)> = Vec::new();
    // SLP240 (preview) is a whole-project rule: collect each file's defs/refs/config keys, then
    // resolve ghost (unreferenced) scaffolding after the loop.
    let mut ghost_scans: Vec<ghost::FileScan> = Vec::new();

    for path in files {
        let display = path.to_string_lossy().to_string();
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: reading {display}: {err}");
                had_error = true;
                continue;
            }
        };
        // SLP220 (preview): an unparseable `.py` is reported as corrupted/truncated AI output
        // instead of being silently skipped — registry rules never see it, so this is handled here.
        let slp220 = selector.preview()
            && selector.is_enabled(sloplint_linter::corrupted::Corrupted.code(), &display);
        let parsed = match parse(&source) {
            Ok(parsed) => parsed,
            Err(err) => {
                if slp220 {
                    let prose_ratio = selector.limits(&display).corrupted_prose_ratio;
                    results.push(FileResult {
                        diagnostics: vec![corrupted::on_parse_error(&source, prose_ratio)],
                        suppressions: Suppressions::empty(),
                        path: display,
                        source,
                    });
                } else {
                    eprintln!("error: {display}: {err}");
                    had_error = true;
                }
                continue;
            }
        };
        let rules = registry.enabled_for(selector, &display);
        let refs: Vec<&dyn Rule> = rules.iter().map(|rule| rule.as_ref()).collect();
        let ctx = FileContext {
            path: &display,
            source: &source,
            parsed: &parsed,
            // Per-file thresholds: the file's profile deltas over the global limits.
            limits: selector.limits(&display),
            security_extra: &config.security.extra,
            placeholders_extra: &config.placeholders.extra,
            comment_phrases_extra: &config.comments.extra,
            crosslang_allow: &config.crosslang.allow,
        };
        let mut diagnostics = check_file(&ctx, &refs);
        // SLP220 (preview): artifact markers outside strings/comments + prose density, on the parsed
        // file (the unparseable case is handled above).
        if slp220 {
            diagnostics.extend(corrupted::scan_parsed(
                &source,
                &parsed,
                ctx.limits.corrupted_prose_ratio,
            ));
        }

        let result_index = results.len();
        // SLP020 is a whole-tree analysis, not a per-file registry rule, so it's gated by
        // config select/ignore only (enabled by default) — it has no preview/stable group.
        if selector.is_enabled(clones::Clones.code(), &display) {
            for unit in extract_functions(&display, &source, &parsed, clone_config.shingle_k) {
                units.push(unit);
                unit_result.push(result_index);
            }
        }
        // Collect imports for all files when preview is on; emission is gated per-path later.
        if selector.preview() {
            import_scans.push((display.clone(), imports::scan_imports(&parsed)));
            ghost_scans.push(ghost::scan(&display, &parsed));
        }
        let suppressions = Suppressions::parse(&source, &parsed);
        results.push(FileResult {
            path: display,
            source,
            diagnostics,
            suppressions,
        });
    }
    Scanned {
        results,
        units,
        unit_result,
        import_scans,
        ghost_scans,
        had_error,
    }
}

/// Returns `Ok(true)` when the run is clean, `Ok(false)` when there are findings or
/// per-file read/parse errors, and `Err` only when the run could not start (bad config).
pub(crate) fn run_check(
    paths: &[String],
    config_path: Option<&str>,
    preview: bool,
    format: Format,
    fix_mode: FixMode,
) -> anyhow::Result<bool> {
    let config = load_config(config_path, preview, true)?;
    let selector = config
        .prepare()
        .map_err(|e| anyhow!("invalid glob in config: {e}"))?;
    let registry = Registry::shipped();
    let clone_config = CloneConfig {
        min_statements: config.clone.min_statements,
        similarity: config.clone.similarity,
        ..CloneConfig::default()
    };

    let (files, mut had_error) = discover_python_files(paths);

    // First-party module names come from the full discovered tree (incl. files that fail to
    // parse), so SLP180 never mistakes a local package for a third-party import.
    let all_display: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    let Scanned {
        mut results,
        units,
        unit_result,
        import_scans,
        ghost_scans,
        had_error: scan_err,
    } = scan_files(&files, &selector, &registry, &clone_config, &config);
    had_error |= scan_err;

    attribute_clones(&units, &unit_result, &clone_config, &mut results);
    attribute_fanout(&mut results, &selector, config.limits.dir_max_modules);
    if selector.preview() {
        attribute_undeclared_imports(
            &import_scans,
            &all_display,
            &config.imports.extra,
            &selector,
            &mut results,
        );
        attribute_ghost_scaffolding(&ghost_scans, &selector, &mut results);
    }

    // Inline `# sloplint: allow` suppression runs last, so it filters whole-tree findings
    // (SLP020 clones, SLP090 fanout, SLP180 imports) as well as the per-file rules.
    for result in &mut results {
        result.suppressions.filter(&mut result.diagnostics);
    }

    // Autofix (`--fix`) runs *after* selection and suppression, so opted-out paths and
    // `# noqa`-suppressed findings are never rewritten. Fixed findings are dropped from
    // `diagnostics`; what remains is reported (against the original source) as usual.
    let fixed = if fix_mode.enabled {
        let (count, write_failed) = apply_fixes(&mut results, fix_mode.allow_unsafe);
        had_error |= write_failed;
        count
    } else {
        0
    };

    let findings: usize = results.iter().map(|r| r.diagnostics.len()).sum();
    let entries: Vec<ReportEntry> = results
        .iter()
        .map(|result| ReportEntry {
            path: &result.path,
            source: &result.source,
            diagnostics: &result.diagnostics,
        })
        .collect();

    match format {
        Format::Text => {
            for result in &results {
                if !result.diagnostics.is_empty() {
                    print!(
                        "{}\n{}",
                        result.path,
                        render_diagnostics(&result.source, &result.diagnostics)
                    );
                }
            }
            if findings == 0 && !had_error {
                eprintln!("sloplint: no issues found");
            } else {
                eprintln!("sloplint: {findings} issue(s)");
            }
        }
        Format::Json => println!("{}", sloplint_report::to_json(&entries)),
        Format::Sarif => println!("{}", sloplint_report::to_sarif(&entries)),
        Format::Github => println!("{}", sloplint_report::to_github_markdown(&entries)),
        Format::Agent => print!("{}", sloplint_report::to_agent(&entries)),
    }

    // The fix tally goes to stderr (like the issue summary) so it never pollutes the machine
    // formats on stdout. Printed whenever `--fix` was requested, even if nothing matched.
    if fix_mode.enabled {
        eprintln!("sloplint: fixed {fixed} issue(s)");
        // Remaining findings were located in the pre-fix source, so once we've actually rewritten
        // files their reported line:col can be stale. Say so, rather than print misleading numbers.
        if fixed > 0 && findings > 0 {
            eprintln!("sloplint: note: positions above predate --fix; re-run to refresh them");
        }
    }

    Ok(findings == 0 && !had_error)
}

/// Apply each file's available fixes, rewrite changed files in place, and drop the fixed findings
/// from the report. Returns the total number of findings fixed across all files, and whether any
/// file failed to write.
///
/// A write failure is reported and recorded (so the run exits non-zero) but does **not** abort the
/// batch — later files are still fixed, mirroring how `run_check` handles per-file read/parse
/// errors. The remaining diagnostics keep their original ranges and are still rendered against the
/// original `source`, so a re-run is the way to see refreshed positions.
fn apply_fixes(results: &mut [FileResult], allow_unsafe: bool) -> (usize, bool) {
    let mut total = 0;
    let mut write_failed = false;
    for result in results.iter_mut() {
        let applied = fix::apply(&result.source, &result.diagnostics, allow_unsafe);
        if !applied.changed() {
            continue;
        }
        if let Err(err) = fs::write(&result.path, &applied.output) {
            eprintln!("error: writing fixes to {}: {err}", result.path);
            write_failed = true;
            continue; // leave this file's findings in the report; keep fixing the rest.
        }
        let fixed: std::collections::HashSet<usize> = applied.fixed.into_iter().collect();
        let mut index = 0;
        result.diagnostics.retain(|_| {
            let keep = !fixed.contains(&index);
            index += 1;
            keep
        });
        total += fixed.len();
    }
    (total, write_failed)
}
