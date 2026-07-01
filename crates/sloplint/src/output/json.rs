//! JSON renderings for the `metrics` command feeds (`--format json`).

use sloplint_metrics::graph::{ImportGraph, PackageRow};
use sloplint_metrics::test_proxies::TestProxies;
use sloplint_metrics::{FileMetrics, RepoMetrics};

use crate::CloneStats;

/// Assemble the full JSON feed: a panel for **every** configured profile under `profiles`
/// (keyed by name), plus the project-wide `test_proxies` split (always over all files). `--scope`
/// does not affect this feed — it always reports every profile.
pub(crate) fn metrics_json(
    panels: &[(String, RepoMetrics, ImportGraph, CloneStats)],
    proxies: &TestProxies,
) -> String {
    let mut profiles = serde_json::Map::new();
    for (name, repo, graph, clone) in panels {
        profiles.insert(
            name.clone(),
            serde_json::Value::Object(panel_json(repo, graph, clone)),
        );
    }
    let mut root = serde_json::Map::new();
    root.insert("profiles".to_string(), serde_json::Value::Object(profiles));
    // Static test proxies: test:code ratio + assertion density + assertion-free rate.
    root.insert("test_proxies".to_string(), test_proxies_json(proxies));
    serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap()
}

/// One metric panel as a JSON object: every aggregate plus the import-graph rollup for the
/// panel's file set. Shared by every profile section so they stay identical in shape.
fn panel_json(
    repo: &RepoMetrics,
    graph: &ImportGraph,
    clone: &CloneStats,
) -> serde_json::Map<String, serde_json::Value> {
    let summary = graph.summary();
    let god = repo.god_units();
    let serde_json::Value::Object(map) = serde_json::json!({
        "files": repo.files,
        "functions": repo.functions,
        "total_loc": repo.total_loc,
        "avg_function_loc": repo.avg_function_loc,
        "max_function_loc": repo.max_function_loc,
        // Longest *logic* function: excludes data/config-init blobs (cognitive < 5) so the
        // god-function signal isn't crowned by a giant assignment run that `max_function_loc` ranks
        // first. Report both — LoC is only meaningful next to complexity.
        "max_logic_function_loc": repo.max_logic_function_loc,
        "avg_cyclomatic": repo.avg_cyclomatic,
        "p95_cyclomatic": repo.p95_cyclomatic,
        "max_cyclomatic": repo.max_cyclomatic,
        "cyclomatic_risk": {
            "low": repo.cyclomatic_risk.low,
            "moderate": repo.cyclomatic_risk.moderate,
            "high": repo.cyclomatic_risk.high,
            "very_high": repo.cyclomatic_risk.very_high,
        },
        // Parameter-count distribution: Long Parameter List prevalence, which `avg` hides.
        // Caller-facing arity (self/cls excluded, *args/**kwargs once). Bands ≤4 / 5–6 / 7–10 / >10,
        // descriptive, never a gate.
        "params": {
            "avg": repo.avg_params,
            "max": repo.max_params,
            "p95": repo.p95_params,
        },
        "param_count_risk": {
            "low": repo.param_count_risk.low,
            "moderate": repo.param_count_risk.moderate,
            "high": repo.param_count_risk.high,
            "very_high": repo.param_count_risk.very_high,
        },
        // Cognitive complexity at parity with cyclomatic: the readability distribution, not
        // just the max. The more readability-relevant of the two complexity metrics.
        "avg_cognitive": repo.avg_cognitive,
        "p95_cognitive": repo.p95_cognitive,
        "max_cognitive": repo.max_cognitive,
        "cognitive_risk": {
            "low": repo.cognitive_risk.low,
            "moderate": repo.cognitive_risk.moderate,
            "high": repo.cognitive_risk.high,
            "very_high": repo.cognitive_risk.very_high,
        },
        "max_nesting": repo.max_nesting,
        "comment_density": repo.comment_density,
        // Type-hint coverage: a quality proxy for under-annotation. Low coverage is the
        // smell; high coverage is neutral (fully-typed code is not slop).
        "param_annotation_coverage": repo.param_annotation_coverage,
        "fully_annotated_function_rate": repo.fully_annotated_function_rate,
        // Module size distribution: the third size leg. NLOC = non-comment, non-blank
        // lines per file; the band counts surface god-module *prevalence*, which `total_loc` and
        // `avg` collapse. Bands (≤250 / 251–500 / 501–1000 / >1000), descriptive, never a gate.
        "module_nloc": {
            "avg": repo.avg_module_nloc,
            "max": repo.max_module_nloc,
            "p95": repo.p95_module_nloc,
        },
        "module_size_risk": {
            "low": repo.module_size_risk.low,
            "moderate": repo.module_size_risk.moderate,
            "high": repo.module_size_risk.high,
            "very_high": repo.module_size_risk.very_high,
        },
        // Top-level-code ratio: fraction of a module's executable logic at module scope vs.
        // inside functions — catches undecomposed procedural script-dumps (Streamlit/Dash/notebook
        // exports) that complexity (linear code) and module-size (moderate) both miss. `avg` is over
        // modules with logic; `undecomposed` counts non-trivial modules above the ratio threshold.
        "top_level_code": {
            "avg_ratio": repo.avg_top_level_ratio,
            "max_ratio": repo.max_top_level_ratio,
            "undecomposed_modules": repo.undecomposed_modules,
        },
        // God-unit tail: counts of very-high-tier units that per-unit *averages* wash out —
        // a repo can have a dozen god-modules and a cognitive-172 god-function yet a clean
        // `avg_cognitive`. This is the tail term that surfaces the outliers (over-engineering as a
        // whole is a documented static-analysis limitation; this is the part we *can* measure).
        "god_units": {
            "very_high_cognitive_functions": god.cognitive_functions,
            "very_high_cyclomatic_functions": god.cyclomatic_functions,
            "very_high_wmc_classes": god.wmc_classes,
            "very_high_size_modules": god.size_modules,
            "total": god.total(),
        },
        // CK class metrics: WMC weight and first-party DIT depth, aggregated over all
        // classes. DIT is a conservative under-count — external (stdlib/third-party) ancestry is
        // invisible. Per-class rows live in `metrics --format classes`.
        "classes": repo.classes,
        "max_wmc": repo.max_wmc,
        "avg_wmc": repo.avg_wmc,
        // WMC size-band counts: god-class *prevalence*, which avg/max collapse. Descriptive
        // bands (≤20 / 21–50 / 51–200 / >200), never a gate. p95 surfaces the heavy tail.
        "p95_wmc": repo.p95_wmc,
        "wmc_risk": {
            "low": repo.wmc_risk.low,
            "moderate": repo.wmc_risk.moderate,
            "high": repo.wmc_risk.high,
            "very_high": repo.wmc_risk.very_high,
        },
        "max_dit": repo.max_dit,
        "avg_dit": repo.avg_dit,
        // NOC: inheritance breadth — direct first-party subclasses per class. The
        // fragile-base-class signal DIT (depth) can't see; band counts flag high-leverage bases.
        // Descriptive bands (≤1 / 2–5 / 6–20 / >20), never a gate.
        "max_noc": repo.max_noc,
        "avg_noc": repo.avg_noc,
        "p95_noc": repo.p95_noc,
        "noc_risk": {
            "low": repo.noc_risk.low,
            "moderate": repo.noc_risk.moderate,
            "high": repo.noc_risk.high,
            "very_high": repo.noc_risk.very_high,
        },
        // CBO: class-to-class coupling — distinct first-party classes a class is wired to
        // (bases, instantiations, isinstance/issubclass, annotations). The class-level coupling the
        // package Ce/Ca can't localize; a LOWER BOUND in dynamically-typed code (duck-typed coupling
        // is invisible). Descriptive bands (≤4 / 5–9 / 10–20 / >20), never a gate.
        "max_cbo": repo.max_cbo,
        "avg_cbo": repo.avg_cbo,
        "p95_cbo": repo.p95_cbo,
        "cbo_risk": {
            "low": repo.cbo_risk.low,
            "moderate": repo.cbo_risk.moderate,
            "high": repo.cbo_risk.high,
            "very_high": repo.cbo_risk.very_high,
        },
        // RFC: response-set size — own methods plus the distinct methods they invoke. The
        // behavior one message to a class can trigger; complements CBO (collaborators) and WMC
        // (own weight). A LOWER BOUND in dynamically-typed code. Descriptive bands (≤20 / 21–50 /
        // 51–100 / >100), never a gate.
        "max_rfc": repo.max_rfc,
        "avg_rfc": repo.avg_rfc,
        "p95_rfc": repo.p95_rfc,
        "rfc_risk": {
            "low": repo.rfc_risk.low,
            "moderate": repo.rfc_risk.moderate,
            "high": repo.rfc_risk.high,
            "very_high": repo.rfc_risk.very_high,
        },
        // Documentation coverage — distinct from comment_density (docstrings, not
        // `#`-comments). Low coverage = under-documented public API; a high docstring/code ratio
        // = AI over-documentation of trivia.
        "docstring_coverage": repo.docstring_coverage,
        "docstring_code_ratio": repo.docstring_code_ratio,
        // Exception-handling hygiene: broad-except / silent-swallow rates over every
        // `except` handler. A cohort discriminator default Ruff can't aggregate; `swallow_rate` is
        // the strongest sub-signal. Descriptive, never a gate.
        "exception_handling": {
            "handlers": repo.exception.handlers,
            "bare": repo.exception.bare,
            "broad": repo.exception.broad,
            "swallow": repo.exception.swallow,
            "broad_rate": repo.broad_except_rate,
            "swallow_rate": repo.swallow_except_rate,
        },
        // Per-project import-graph rollup (foundation figures + cyclic-dependency tangles +
        // propagation cost + modularity).
        "packages": {
            "modules": summary.modules,
            "packages": summary.packages,
            "module_edges": summary.module_edges,
            "package_edges": summary.package_edges,
            "cycles": cycles_json(graph, summary.modules),
            // Whole-system coupling: density of the module reachability matrix.
            "propagation_cost": graph.propagation_cost(),
            // Newman–Girvan modularity: declared package partition vs. detected.
            "modularity": modularity_json(graph),
            // Node-distribution concentration: god-package / flat dumping-ground.
            "concentration": concentration_json(graph),
        },
        // Duplication density: SLP020 clone detection surfaced as a cohort aggregate.
        // `ratio` = fraction of the profile's functions in ≥1 clone pair; copy-paste codebases
        // (a scraper per site) score high, clean libraries ≈ 0. Descriptive, never a gate.
        "duplication": {
            "clone_ratio": clone.ratio(),
            "functions_in_clones": clone.functions_in_clones,
            "functions": clone.total_functions,
            "clone_pairs": clone.pairs,
            "largest_clone_cluster": clone.largest_cluster,
        },
    }) else {
        unreachable!("a json object literal is an object")
    };
    map
}

/// The test-proxies rollup for the JSON feed. The `_note` is emitted inline so any
/// consumer of the raw JSON sees the caveat: these are *static estimates*, NOT coverage, and
/// must never be turned into a pass/fail gate. Undefined ratios (no production code / no test
/// functions) serialize as `null`, not `0`, so consumers don't mistake "undefined" for "zero".
fn test_proxies_json(proxies: &TestProxies) -> serde_json::Value {
    serde_json::json!({
        "_note": "Static proxies, NOT coverage. Descriptive cohort statistics only — never a \
                  pass/fail gate. Many asserts do not guarantee a meaningful test, and few do \
                  not prove a weak one.",
        "test_files": proxies.test_files,
        "production_files": proxies.production_files,
        "test_loc": proxies.test_loc,
        "production_loc": proxies.production_loc,
        "test_code_ratio": proxies.test_code_ratio,
        "test_functions": proxies.test_functions,
        "assertions": proxies.assertions,
        "assertion_density": proxies.assertion_density,
        // Test-substance: fraction of test functions that assert nothing ("test
        // theater"). High alongside a high test_code_ratio = a suite that looks tested but isn't.
        "assertion_free_tests": proxies.assertion_free_tests,
        "assertion_free_rate": proxies.assertion_free_rate,
        // Doctest-awareness: doctests live in production docstrings, so the path-based
        // test_code_ratio misses this whole testing style. Reported alongside, not folded in.
        "production_functions": proxies.production_functions,
        "functions_with_doctest": proxies.functions_with_doctest,
        "doctest_examples": proxies.doctest_examples,
        "doctest_coverage": proxies.doctest_coverage,
    })
}

/// The modularity rollup for the JSON feed: Q of the declared package partition, Q of
/// the Louvain-detected partition, their community counts, and the gap (detected − declared) — a
/// large gap means the declared package boundaries don't match the natural structure.
fn modularity_json(graph: &ImportGraph) -> serde_json::Value {
    let report = graph.modularity();
    serde_json::json!({
        "q_declared": report.q_declared,
        "communities_declared": report.communities_declared,
        "q_detected": report.q_detected,
        "communities_detected": report.communities_detected,
        "gap": report.gap(),
    })
}

/// The node-distribution concentration rollup for the JSON feed: how modules are
/// piled across packages, the axis the edge-based metrics can't see. `largest_package` names the
/// offender (or `null` when there are no packages). Descriptive only — never a gate.
fn concentration_json(graph: &ImportGraph) -> serde_json::Value {
    let c = graph.concentration();
    serde_json::json!({
        "total_modules": c.total_modules,
        "packages": c.packages,
        "max_package_share": c.max_package_share,
        "module_count_gini": c.module_count_gini,
        "largest_package": c.largest_package.map(|(package, modules)| {
            serde_json::json!({ "package": package, "modules": modules })
        }),
    })
}

/// The cyclic-dependency (SCC) rollup for the JSON feed: tangle counts over the
/// full graph, the same count over the runtime graph (TYPE_CHECKING-only edges dropped) and over
/// the load-bearing graph (function-local/deferred edges *also* dropped), the share of
/// modules in cycles, and the member modules of each tangle.
fn cycles_json(graph: &ImportGraph, modules: usize) -> serde_json::Value {
    let report = graph.cycles();
    let in_cycles = report.modules_in_cycles();
    let pct = if modules == 0 {
        0.0
    } else {
        in_cycles as f64 / modules as f64
    };
    serde_json::json!({
        "tangles": report.tangle_count(),
        "largest_tangle": report.largest_tangle(),
        "modules_in_cycles": in_cycles,
        "pct_modules_in_cycles": pct,
        "runtime_tangles": graph.runtime_cycles().tangle_count(),
        // Hard cycles only: module-top-level runtime edges, dropping function-local/deferred imports.
        // `0` ⇒ every cycle was deliberately deferred (milder smell); `> 0` ⇒ genuine
        // load-time circular dependencies that can raise `ImportError`. Not a strict subset of
        // `tangles` by count — dropping edges can split one SCC into several.
        "load_bearing_tangles": graph.load_bearing_cycles().tangle_count(),
        "members": report.tangles,
    })
}

/// JSON row for one class in the `metrics --format json` feed.
pub(crate) fn class_row(path: &str, class: &sloplint_metrics::ClassMetrics) -> serde_json::Value {
    serde_json::json!({
        "file": path,
        "class": class.name,
        "loc": class.loc,
        "methods": class.methods,
        "attributes": class.attributes,
        "lcom4": class.lcom4,
        "wmc": class.wmc,
        "dit": class.dit,
        // NOC: direct first-party subclasses — inheritance breadth / fragile-base risk.
        "noc": class.noc,
        // CBO: distinct first-party classes this one couples to — a lower bound in
        // dynamically-typed code (duck-typed coupling not counted).
        "cbo": class.cbo,
        // RFC: response-set size — own methods plus the distinct methods they invoke (by
        // trailing callee name). A lower bound in dynamically-typed code.
        "rfc": class.rfc,
        "is_abstract": class.is_abstract,
        "has_docstring": class.has_docstring,
        "docstring_lines": class.docstring_lines,
    })
}

/// JSON row for one package in the `metrics --format json` feed.
pub(crate) fn package_row(row: &PackageRow) -> serde_json::Value {
    serde_json::json!({
        "package": row.package,
        "modules": row.modules,
        "loc": row.loc,
        "imports": row.imports,
        "imported_by": row.imported_by,
        "ce": row.imports.len(),
        "ca": row.imported_by.len(),
        "instability": row.instability,
        "in_cycle": row.in_cycle,
        "classes": row.classes,
        "abstract_classes": row.abstract_classes,
        "abstractness": row.abstractness,
        "distance": row.distance,
    })
}

/// JSON row for one function in the `metrics --format json` feed.
pub(crate) fn function_row(
    path: &str,
    file: &FileMetrics,
    function: &sloplint_metrics::FunctionMetrics,
) -> serde_json::Value {
    let comment_density = if file.loc == 0 {
        0.0
    } else {
        file.comment_lines as f64 / file.loc as f64
    };
    serde_json::json!({
        "file": path,
        "function": function.name,
        "loc": function.loc,
        "ncss": function.ncss,
        "cyclomatic": function.cyclomatic,
        "cognitive": function.cognitive,
        "max_nesting": function.max_nesting,
        "params": function.params,
        // Caller-facing arity: params minus the self/cls receiver — the Long-Parameter-List
        // signal. `*args`/`**kwargs` each count once.
        "arity": function.arity,
        "exits": function.exits,
        // Type-hint coverage: annotated vs. annotatable params, and whether a return type is
        // declared. `annotatable_params` excludes the self/cls receiver and *args/**kwargs.
        "typed_params": function.typed_params,
        "annotatable_params": function.annotatable_params,
        "has_return_annotation": function.has_return_annotation,
        "has_docstring": function.has_docstring,
        "docstring_lines": function.docstring_lines,
        "file_loc": file.loc,
        "file_comment_density": comment_density,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_metrics::{file_metrics, graph};
    use sloplint_python::parse;

    #[test]
    fn function_row_has_features_and_file_comment_density() {
        let source =
            "# a comment\ndef f(a: int, b) -> str:\n    if a:\n        return b\n    return a\n";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = function_row("pkg/m.py", &metrics, &metrics.functions[0]);

        assert_eq!(row["file"], "pkg/m.py");
        assert_eq!(row["function"], "f");
        assert_eq!(row["params"], 2);
        assert!(
            row["cyclomatic"].as_u64().unwrap() >= 2,
            "the `if` is a branch"
        );
        // Type-hint coverage: 1 of 2 params annotated, return type present.
        assert_eq!(row["typed_params"], 1);
        assert_eq!(row["annotatable_params"], 2);
        assert_eq!(row["has_return_annotation"], true);
        // 1 comment line over the file's physical lines.
        let density = row["file_comment_density"].as_f64().unwrap();
        assert!(density > 0.0 && density < 1.0, "got {density}");
        // `f` has no docstring (a `#`-comment is not a docstring).
        assert_eq!(row["has_docstring"], false);
        assert_eq!(row["docstring_lines"], 0);
    }

    #[test]
    fn function_row_reports_docstring_size() {
        let source = "def f():\n    \"\"\"Two\n    lines.\"\"\"\n    return 1\n";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = function_row("pkg/m.py", &metrics, &metrics.functions[0]);
        assert_eq!(row["has_docstring"], true);
        assert_eq!(row["docstring_lines"], 2);
    }

    #[test]
    fn class_row_has_size_and_cohesion_fields() {
        let source = "\
class Counter:
    def __init__(self):
        self.total = 0
    def add(self, n):
        self.total += n
    def show(self):
        return self.total
";
        let parsed = parse(source).unwrap();
        let metrics = file_metrics(source, &parsed);
        let row = class_row("pkg/m.py", &metrics.classes[0]);

        assert_eq!(row["file"], "pkg/m.py");
        assert_eq!(row["class"], "Counter");
        assert_eq!(row["methods"], 3);
        assert_eq!(row["attributes"], 1); // self.total
        assert_eq!(row["lcom4"], 1, "add/show share self.total");
        assert!(row["loc"].as_u64().unwrap() >= 7);
        assert_eq!(row["is_abstract"], false, "a plain concrete class");
        // No leading string literal in the class body, so no docstring.
        assert_eq!(row["has_docstring"], false);
        assert_eq!(row["docstring_lines"], 0);
    }

    #[test]
    fn package_row_has_module_count_and_coupling() {
        let instability = graph::instability(1, 2);
        let abstractness = graph::abstractness(1, 4); // 1 of 4 classes abstract
        let row = PackageRow {
            package: "pkg".to_string(),
            modules: 2,
            loc: 42,
            imports: vec!["pkg.sub".to_string()],
            imported_by: vec!["app".to_string(), "cli".to_string()],
            in_cycle: true,
            instability,
            classes: 4,
            abstract_classes: 1,
            abstractness,
            distance: graph::distance(abstractness, instability),
        };
        let value = package_row(&row);
        assert_eq!(value["package"], "pkg");
        assert_eq!(value["modules"], 2);
        assert_eq!(value["loc"], 42);
        assert_eq!(value["imports"], serde_json::json!(["pkg.sub"]));
        // Ce = 1 (pkg.sub), Ca = 2 (app, cli) → I = 1 / 3.
        assert_eq!(value["ce"], 1);
        assert_eq!(value["ca"], 2);
        assert_eq!(value["instability"], 1.0 / 3.0);
        assert_eq!(value["in_cycle"], true);
        // A = 1/4 = 0.25; D = |0.25 + 1/3 − 1| = |−0.41666…| = 0.41666…
        assert_eq!(value["classes"], 4);
        assert_eq!(value["abstract_classes"], 1);
        assert_eq!(value["abstractness"], 0.25);
        assert_eq!(value["distance"], (0.25 + 1.0 / 3.0 - 1.0_f64).abs());
    }
}
