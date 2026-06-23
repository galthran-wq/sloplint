//! Human-readable stdout tables for the `metrics` command.

use std::io::{self, Write};

use sloplint_metrics::graph::{Concentration, ImportGraph};
use sloplint_metrics::test_proxies::TestProxies;
use sloplint_metrics::RepoMetrics;

use crate::{CloneStats, MeasuredFile, Scope};

use super::json::{class_row, function_row, package_row};
use super::opt_ratio;

/// Print one labeled metric panel — the per-partition aggregates, without the test
/// proxies (those are the project-wide split and are printed once, after the panel(s)).
pub(crate) fn print_metrics_panel(label: &str, repo: &RepoMetrics) {
    println!("sloplint metrics — {label}");
    println!("  files               {}", repo.files);
    println!("  functions           {}", repo.functions);
    println!("  total lines         {}", repo.total_loc);
    println!("  avg function LoC    {:.1}", repo.avg_function_loc);
    println!(
        "  max function LoC    {}  (logic {})",
        repo.max_function_loc, repo.max_logic_function_loc
    );
    println!("  avg cyclomatic      {:.1}", repo.avg_cyclomatic);
    println!("  p95 cyclomatic      {}", repo.p95_cyclomatic);
    println!("  max cyclomatic      {}", repo.max_cyclomatic);
    let risk = repo.cyclomatic_risk;
    println!(
        "  CC risk tiers       low {} / moderate {} / high {} / very high {}",
        risk.low, risk.moderate, risk.high, risk.very_high
    );
    // Parameter count (caller-facing arity) distribution: Long-Parameter-List prevalence.
    println!(
        "  avg/p95/max params  {:.1} / {} / {}",
        repo.avg_params, repo.p95_params, repo.max_params
    );
    let params = repo.param_count_risk;
    println!(
        "  arity bands         low {} / moderate {} / high {} / very high {}",
        params.low, params.moderate, params.high, params.very_high
    );
    // Cognitive complexity at parity with cyclomatic — the better readability signal.
    println!("  avg cognitive       {:.1}", repo.avg_cognitive);
    println!("  p95 cognitive       {}", repo.p95_cognitive);
    println!("  max cognitive       {}", repo.max_cognitive);
    let cog = repo.cognitive_risk;
    println!(
        "  CoCo risk tiers     low {} / moderate {} / high {} / very high {}",
        cog.low, cog.moderate, cog.high, cog.very_high
    );
    println!("  max nesting         {}", repo.max_nesting);
    println!("  comment density     {:.1}%", repo.comment_density * 100.0);
    println!(
        "  docstring coverage  {:.1}%",
        repo.docstring_coverage * 100.0
    );
    println!("  docstring/code      {:.2}", repo.docstring_code_ratio);
    // Exception-handling hygiene: broad-except / silent-swallow rates.
    let exc = repo.exception;
    println!(
        "  except broad/swallow {:.2} / {:.2}  ({} broad, {} swallow, {} bare / {} handlers)",
        repo.broad_except_rate,
        repo.swallow_except_rate,
        exc.broad,
        exc.swallow,
        exc.bare,
        exc.handlers
    );
    // Class weight (WMC) distribution: god-class prevalence, not just the worst class.
    println!("  classes             {}", repo.classes);
    println!(
        "  avg/p95/max WMC     {:.1} / {} / {}",
        repo.avg_wmc, repo.p95_wmc, repo.max_wmc
    );
    let wmc = repo.wmc_risk;
    println!(
        "  WMC bands           low {} / moderate {} / high {} / very high {}",
        wmc.low, wmc.moderate, wmc.high, wmc.very_high
    );
    // Inheritance breadth (NOC) distribution: fragile-base-class prevalence.
    println!(
        "  avg/p95/max NOC     {:.1} / {} / {}",
        repo.avg_noc, repo.p95_noc, repo.max_noc
    );
    let noc = repo.noc_risk;
    println!(
        "  NOC bands           low {} / moderate {} / high {} / very high {}",
        noc.low, noc.moderate, noc.high, noc.very_high
    );
    // Class coupling (CBO) distribution: hub-class prevalence (lower bound in dynamic code).
    println!(
        "  avg/p95/max CBO     {:.1} / {} / {}",
        repo.avg_cbo, repo.p95_cbo, repo.max_cbo
    );
    let cbo = repo.cbo_risk;
    println!(
        "  CBO bands           low {} / moderate {} / high {} / very high {}",
        cbo.low, cbo.moderate, cbo.high, cbo.very_high
    );
    // Module size (NLOC) distribution: god-module prevalence — the third size leg.
    println!(
        "  avg/p95/max module  {:.1} / {} / {}  NLOC",
        repo.avg_module_nloc, repo.p95_module_nloc, repo.max_module_nloc
    );
    let module = repo.module_size_risk;
    println!(
        "  module NLOC bands   low {} / moderate {} / high {} / very high {}",
        module.low, module.moderate, module.high, module.very_high
    );
    // Top-level-code ratio: undecomposed script-dump modules complexity/size metrics miss.
    println!(
        "  top-level code      avg {:.0}% / max {:.0}%  ({} undecomposed module(s))",
        repo.avg_top_level_ratio * 100.0,
        repo.max_top_level_ratio * 100.0,
        repo.undecomposed_modules,
    );
    // God-unit tail: the very-high-tier outliers per-unit averages wash out.
    let god = repo.god_units();
    println!(
        "  god-unit tail       {}  (cognitive {} / cyclomatic {} / WMC {} / module {})",
        god.total(),
        god.cognitive_functions,
        god.cyclomatic_functions,
        god.wmc_classes,
        god.size_modules,
    );
}

/// Emit one JSONL row per package: the first-party import graph collapsed to directory level.
/// The package-level discovery feed, mirroring `print_function_rows`/`print_class_rows`.
pub(crate) fn print_package_rows(graph: &ImportGraph) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for row in graph.package_rows() {
        let _ = writeln!(out, "{}", package_row(&row));
    }
}

/// Print the package module-count concentration block beneath a metric panel: how piled the
/// modules are across packages, and which package holds the most. A descriptive distribution
/// statistic — never a gate (a small repo's one main package scores high and that's fine).
pub(crate) fn print_concentration(c: &Concentration) {
    let largest = match &c.largest_package {
        Some((name, modules)) => format!("{name}, {modules}/{} modules", c.total_modules),
        None => "n/a".to_string(),
    };
    println!(
        "  max package share   {:.2}  ({largest})",
        c.max_package_share
    );
    println!(
        "  module-count gini   {:.2}  (over {} packages)",
        c.module_count_gini, c.packages
    );
}

/// Print the duplication-density block beneath a metric panel: the SLP020 clone ratio plus
/// the pair count and largest cluster. Descriptive — high duplication is a vibe-slop tell
/// ("a scraper per site" → copy-paste), but it's a cohort signal, never a per-repo gate.
pub(crate) fn print_clone_density(c: &CloneStats) {
    println!(
        "  clone ratio         {:.2}  ({} fns in clones / {} ; {} pairs, largest cluster {})",
        c.ratio(),
        c.functions_in_clones,
        c.total_functions,
        c.pairs,
        c.largest_cluster,
    );
}

/// Print the static test proxies block once, beneath the panel(s). Always the full
/// project-wide split (production vs test), independent of `--scope` — descriptive only, NOT
/// coverage and never a gate.
pub(crate) fn print_test_proxies_table(proxies: &TestProxies) {
    println!(
        "  test:code ratio     {}  ({} test / {} prod LoC)",
        opt_ratio(proxies.test_code_ratio),
        proxies.test_loc,
        proxies.production_loc,
    );
    println!(
        "  assertion density   {}  ({} assertions / {} test fns)",
        opt_ratio(proxies.assertion_density),
        proxies.assertions,
        proxies.test_functions,
    );
    println!(
        "  assertion-free rate {}  ({} of {} test fns assert nothing)",
        opt_ratio(proxies.assertion_free_rate),
        proxies.assertion_free_tests,
        proxies.test_functions,
    );
    println!(
        "  doctest coverage    {}  ({} of {} prod fns doctested; {} examples)",
        opt_ratio(proxies.doctest_coverage),
        proxies.functions_with_doctest,
        proxies.production_functions,
        proxies.doctest_examples,
    );
    println!("  (test proxies are static estimates, not coverage — descriptive only)");
}

/// Emit one JSONL row per function: raw per-function features plus the enclosing file's
/// length and comment density. This is the discovery feed — `analyze.py` mines these rows
/// for features that separate the slop and clean cohorts.
pub(crate) fn print_function_rows(per_file: &[MeasuredFile], scope: &Scope) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for file in per_file.iter().filter(|f| scope.includes(&f.profiles)) {
        for function in &file.metrics.functions {
            let _ = writeln!(out, "{}", function_row(&file.path, &file.metrics, function));
        }
    }
}

/// Emit one JSONL row per class: size (methods, attributes) + LCOM4 cohesion. The class-level
/// discovery feed, mirroring `print_function_rows`.
pub(crate) fn print_class_rows(per_file: &[MeasuredFile], scope: &Scope) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for file in per_file.iter().filter(|f| scope.includes(&f.profiles)) {
        for class in &file.metrics.classes {
            let _ = writeln!(out, "{}", class_row(&file.path, class));
        }
    }
}
