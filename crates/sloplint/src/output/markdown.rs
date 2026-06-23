//! GitHub-flavored markdown rendering for the `metrics` PR summary.

use sloplint_metrics::test_proxies::TestProxies;
use sloplint_metrics::RepoMetrics;

use crate::CloneStats;

use super::opt_ratio;

/// GitHub-flavored markdown for the PR summary: the cyclomatic risk block from `sloplint_metrics`
/// for each in-scope profile, under its own heading, then the test proxies. `--scope all`
/// renders one block per profile side by side — never a combined panel that would mix profiles'
/// norms. Pairs with the `cyclomatic-risk` badge.
pub(crate) fn metrics_markdown(
    panels: &[(&str, RepoMetrics, CloneStats)],
    proxies: &TestProxies,
) -> String {
    let mut out = String::from("### sloplint metrics\n\n");
    for (name, repo, clone) in panels {
        out.push_str(&format!(
            "#### {name}\n\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            repo.cyclomatic_markdown(),
            repo.cognitive_markdown(),
            repo.params_markdown(),
            repo.wmc_markdown(),
            repo.noc_markdown(),
            repo.cbo_markdown(),
            repo.module_size_markdown(),
            repo.exception_markdown(),
            clone_markdown(clone),
        ));
    }
    out.push_str(&test_proxies_markdown(proxies));
    out
}

/// A one-line markdown summary of duplication density — the SLP020 clone ratio with its
/// pair count and largest cluster. Descriptive cohort signal, never a gate.
fn clone_markdown(c: &CloneStats) -> String {
    format!(
        "**Duplication** — clone ratio {:.2} ({} of {} functions in SLP020 clone pairs; \
         {} pairs, largest cluster {}). Descriptive, never a gate.\n",
        c.ratio(),
        c.functions_in_clones,
        c.total_functions,
        c.pairs,
        c.largest_cluster,
    )
}

/// A markdown block for the static test proxies, explicitly captioned as *proxies, not
/// coverage* so the PR summary can't be read as a gate.
fn test_proxies_markdown(proxies: &TestProxies) -> String {
    format!(
        "**Test proxies** (static estimates — _not coverage_, descriptive only) — \
         test:code ratio {} ({} test / {} prod LoC), assertion density {} ({} assertions over \
         {} test functions), assertion-free rate {} ({} of {} test functions assert nothing). A \
         high assertion-free rate next to a high test:code ratio flags a suite that looks tested \
         but verifies little. Doctest coverage {} ({} of {} production functions carry a `>>>` \
         example; {} examples) captures doctests, which live in production files and so are \
         invisible to the path-based test:code ratio. These suggest under-testing across a \
         cohort; they are never a per-repo pass/fail verdict.\n",
        opt_ratio(proxies.test_code_ratio),
        proxies.test_loc,
        proxies.production_loc,
        opt_ratio(proxies.assertion_density),
        proxies.assertions,
        proxies.test_functions,
        opt_ratio(proxies.assertion_free_rate),
        proxies.assertion_free_tests,
        proxies.test_functions,
        opt_ratio(proxies.doctest_coverage),
        proxies.functions_with_doctest,
        proxies.production_functions,
        proxies.doctest_examples,
    )
}
