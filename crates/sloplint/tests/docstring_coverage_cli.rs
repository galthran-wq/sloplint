//! End-to-end tests for the documentation/docstring-coverage metric (issue #83), exercised
//! against the real built binary over a committed Python fixture with a known mix of
//! documented/undocumented public and private units. Covers the `--format json` project
//! aggregate, the per-function `--format functions` feed, and the per-class `--format classes`
//! feed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docstrings.py")
}

/// Run `sloplint metrics <fixture> <extra args...>` and return (stdout, exit code).
fn run_metrics(extra: &[&str]) -> (String, i32) {
    let fixture = fixture();
    let mut args = vec!["metrics", fixture.to_str().unwrap()];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn json_reports_docstring_coverage_and_ratio() {
    let (stdout, code) = run_metrics(&["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("metrics --format json is valid JSON");

    // Public units: documented (doc), undocumented (no doc), Service (doc), run (no doc).
    // `_private_helper` is `_`-prefixed and excluded. 2 of 4 documented => 0.5.
    let coverage = value["docstring_coverage"].as_f64().unwrap();
    assert!((coverage - 0.5).abs() < 1e-9, "coverage = {coverage}");

    // The ratio is function-scoped on both sides: function docstring lines (4 for `documented`,
    // the only documented function) over total function NCSS (2 + 1 + 1 + 1 = 5) => 0.8. The
    // `Service` class docstring drives coverage, not the ratio.
    let ratio = value["docstring_code_ratio"].as_f64().unwrap();
    assert!((ratio - 0.8).abs() < 1e-9, "ratio = {ratio}");
}

#[test]
fn function_feed_marks_each_function_has_docstring() {
    let (stdout, code) = run_metrics(&["--format", "functions"]);
    assert_eq!(code, 0);
    let rows: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("each function row is valid JSON"))
        .collect();

    let by_name = |name: &str| {
        rows.iter()
            .find(|r| r["function"] == name)
            .unwrap_or_else(|| panic!("missing function row for {name}"))
            .clone()
    };

    let documented = by_name("documented");
    assert_eq!(documented["has_docstring"], true);
    assert_eq!(documented["docstring_lines"], 4);

    let undocumented = by_name("undocumented");
    assert_eq!(undocumented["has_docstring"], false);
    assert_eq!(undocumented["docstring_lines"], 0);
}

#[test]
fn class_feed_marks_class_has_docstring() {
    let (stdout, code) = run_metrics(&["--format", "classes"]);
    assert_eq!(code, 0);
    let row: Value = stdout
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|r| r["class"] == "Service")
        .expect("Service class row");
    assert_eq!(row["has_docstring"], true);
    assert_eq!(row["docstring_lines"], 1);
}

#[test]
fn text_table_includes_docstring_coverage() {
    let (stdout, code) = run_metrics(&[]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("docstring coverage  50.0%"),
        "text table reports docstring coverage:\n{stdout}"
    );
}
