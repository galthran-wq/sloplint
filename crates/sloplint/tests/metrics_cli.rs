//! End-to-end tests for the `sloplint metrics` cyclomatic-complexity reporting (issue #10),
//! exercising the real built binary over a committed Python fixture with known per-function
//! complexity. Covers the JSON aggregates + risk histogram, the markdown PR-summary, and the
//! `--max-cyclomatic` CI gate's exit code and offender report.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture() -> PathBuf {
    fixtures_dir().join("cyclomatic.py")
}

/// Run `sloplint metrics cyclomatic.py <extra args...>` from *inside* the fixtures dir, so the
/// classified path is the bare `cyclomatic.py` (production). Running from the repo root would put
/// a `tests/fixtures/` ancestor in the path and classify the fixture as a test file (#96),
/// emptying the production panel these assertions read.
fn run_metrics(extra: &[&str]) -> (String, String, i32) {
    let mut args = vec!["metrics", "cyclomatic.py"];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(fixtures_dir())
        .args(&args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn json_reports_cyclomatic_aggregates_and_risk_histogram() {
    let (stdout, _stderr, code) = run_metrics(&["--format", "json"]);
    assert_eq!(code, 0, "metrics without a gate exits 0");
    let value: Value = serde_json::from_str(&stdout).expect("metrics --format json is valid JSON");
    // The fixture is production code; its panel lives under `profiles.production` (#96).
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["functions"], 3);
    assert_eq!(prod["max_cyclomatic"], 12);
    // max_logic_function_loc (#155) is reported and never exceeds the raw max (it's a subset of
    // functions — those with cognitive ≥ 5); the fixture's `moderate` qualifies, so it's > 0.
    let max_loc = prod["max_function_loc"].as_u64().unwrap();
    let max_logic = prod["max_logic_function_loc"].as_u64().unwrap();
    assert!(
        max_logic > 0 && max_logic <= max_loc,
        "logic {max_logic} <= max {max_loc}"
    );
    // p95 (nearest-rank over [1, 3, 12]) lands on the worst function.
    assert_eq!(prod["p95_cyclomatic"], 12);
    // mean = (1 + 3 + 12) / 3.
    let avg = prod["avg_cyclomatic"].as_f64().unwrap();
    assert!((avg - 16.0 / 3.0).abs() < 1e-6, "avg_cyclomatic = {avg}");

    let risk = &prod["cyclomatic_risk"];
    assert_eq!(risk["low"], 2, "trivial + comprehension are low-risk");
    assert_eq!(risk["moderate"], 1, "moderate() is in the 11-20 tier");
    assert_eq!(risk["high"], 0);
    assert_eq!(risk["very_high"], 0);

    // God-unit tail (#152): the fixture has no very-high-tier units (max cog 10, cc 12), so the
    // tail total is 0 and the block is present/well-formed.
    let god = &prod["god_units"];
    assert_eq!(god["total"], 0, "no very-high-tier units in the fixture");
    assert_eq!(god["very_high_cognitive_functions"], 0);

    // Cognitive complexity at parity with cyclomatic (#110): per-function cognitive is
    // trivial=0, comprehension=1, moderate=10 → mean 11/3, p95/max 10, two low + one moderate.
    assert_eq!(prod["max_cognitive"], 10);
    assert_eq!(prod["p95_cognitive"], 10);
    let avg_cog = prod["avg_cognitive"].as_f64().unwrap();
    assert!(
        (avg_cog - 11.0 / 3.0).abs() < 1e-6,
        "avg_cognitive = {avg_cog}"
    );
    let cog = &prod["cognitive_risk"];
    assert_eq!(cog["low"], 2, "trivial + comprehension are ≤5");
    assert_eq!(
        cog["moderate"], 1,
        "moderate() cognitive 10 is in the 6-15 band"
    );
    assert_eq!(cog["high"], 0);
    assert_eq!(cog["very_high"], 0);

    // Type-hint coverage (#85) is wired into the aggregate. The fixture is fully unannotated, so
    // both land at 0.0 — the precise ratio math is covered by the metrics-crate unit tests.
    assert_eq!(prod["param_annotation_coverage"], 0.0);
    assert_eq!(prod["fully_annotated_function_rate"], 0.0);
}

#[test]
fn github_markdown_surfaces_worst_tier_and_table() {
    let (stdout, _stderr, code) = run_metrics(&["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("worst tier: moderate"),
        "markdown names the worst tier:\n{stdout}"
    );
    assert!(
        stdout.contains("| moderate (11\u{2013}20) | 1 |"),
        "markdown has the risk-tier table:\n{stdout}"
    );
    // Cognitive complexity is reported at parity (#110): its own block + readability-band table.
    assert!(
        stdout.contains("**Cognitive complexity**"),
        "markdown has a cognitive block:\n{stdout}"
    );
    assert!(
        stdout.contains("| moderate (6\u{2013}15) | 1 |"),
        "markdown has the cognitive band table:\n{stdout}"
    );
}

#[test]
fn max_cyclomatic_gate_fails_and_names_the_offender() {
    // moderate() has CC 12, so a ceiling of 10 trips the gate.
    let (_stdout, stderr, code) = run_metrics(&["--max-cyclomatic", "10"]);
    assert_eq!(code, 1, "exceeding the ceiling exits 1");
    assert!(
        stderr.contains("over the cyclomatic ceiling of 10"),
        "stderr explains the gate:\n{stderr}"
    );
    assert!(
        stderr.contains("`moderate` has cyclomatic complexity 12"),
        "stderr names the offending function with its value:\n{stderr}"
    );
    // The location must point at the `def` line, not the `@memoize` decorator above it.
    let fixture = fixture();
    let source = std::fs::read_to_string(&fixture).unwrap();
    let def_line = source
        .lines()
        .position(|l| l.trim_start().starts_with("def moderate"))
        .expect("fixture has `def moderate`")
        + 1;
    let expected = format!("cyclomatic.py:{def_line}:");
    assert!(
        stderr.contains(&expected),
        "offender location must be the def line ({expected}), not the decorator:\n{stderr}"
    );
    // The gate must NOT emit a diagnostic/finding (that would duplicate Ruff's C901).
    assert!(
        !stderr.to_lowercase().contains("slp"),
        "the gate is an exit code, not an SLP finding:\n{stderr}"
    );
}

#[test]
fn max_cyclomatic_gate_passes_at_or_below_ceiling() {
    // The worst function is exactly 12, and the gate is `>` (strictly over), so 12 passes.
    let (_stdout, _stderr, code) = run_metrics(&["--max-cyclomatic", "12"]);
    assert_eq!(code, 0, "a ceiling equal to the max complexity passes");
}
