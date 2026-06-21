//! End-to-end tests for the static test proxies (issue #86): `test:code ratio` and
//! `assertion density`, exercised over a committed mini-project with one production file and
//! one test file with known test functions and assertions.
//!
//! The proxies are deliberately *descriptive*: there is no gate to test (no `--max-*`), so
//! these tests assert the reported figures and — crucially — the "not coverage / not a gate"
//! caveat that must travel with them in every format.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_proxies")
}

/// Run `sloplint metrics <targets> <extra args...>` from *inside* the fixture dir, so the paths
/// the binary classifies are project-relative (`src/...`, `tests/...`). Running from the repo
/// root would put a `tests/fixtures/` ancestor in every path and misclassify production files as
/// tests — exactly what a real project (relative paths from its root) avoids.
fn run_metrics(targets: &[&str], extra: &[&str]) -> (String, String, i32) {
    let mut args = vec!["metrics"];
    args.extend_from_slice(targets);
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(fixture_dir())
        .args(&args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

/// Physical line count of a fixture file, matching `FileMetrics::loc` (`str::lines`).
fn loc(rel: &str) -> usize {
    let path = fixture_dir().join(rel);
    std::fs::read_to_string(path).unwrap().lines().count()
}

#[test]
fn json_reports_test_proxies_with_exact_counts() {
    let (stdout, _stderr, code) = run_metrics(&["."], &["--format", "json"]);
    assert_eq!(code, 0, "metrics without a gate exits 0");
    let value: Value = serde_json::from_str(&stdout).expect("metrics --format json is valid JSON");

    let proxies = &value["test_proxies"];
    assert_eq!(proxies["test_files"], 1);
    assert_eq!(proxies["production_files"], 1);
    // test_add, test_mul, test_raises, test_mul_table; helper_not_a_test ignored.
    assert_eq!(proxies["test_functions"], 4);
    // test_add (2) + test_mul (1) + test_raises (pytest.raises, 1) + test_mul_table (1).
    assert_eq!(proxies["assertions"], 5);

    // assertion density = 5 / 4.
    let density = proxies["assertion_density"].as_f64().unwrap();
    assert!(
        (density - 5.0 / 4.0).abs() < 1e-9,
        "assertion_density = {density}"
    );

    // Test-substance (#121): the three one-liner tests are trivial (cognitive ≤ 1); the
    // doubly-nested test_mul_table is not → trivial-test rate = 3 / 4.
    assert_eq!(proxies["trivial_test_functions"], 3);
    let trivial = proxies["trivial_test_rate"].as_f64().unwrap();
    assert!(
        (trivial - 3.0 / 4.0).abs() < 1e-9,
        "trivial_test_rate = {trivial}"
    );

    // test:code ratio = test LoC / production LoC, computed from the same line definition.
    let expected_ratio = loc("tests/test_calc.py") as f64 / loc("src/calc.py") as f64;
    let ratio = proxies["test_code_ratio"].as_f64().unwrap();
    assert!(
        (ratio - expected_ratio).abs() < 1e-9,
        "test_code_ratio = {ratio}"
    );
    assert_eq!(
        proxies["test_loc"].as_u64().unwrap(),
        loc("tests/test_calc.py") as u64
    );
    assert_eq!(
        proxies["production_loc"].as_u64().unwrap(),
        loc("src/calc.py") as u64
    );

    // The honest-limitations caveat must travel with the numbers in the raw JSON.
    let note = proxies["_note"].as_str().unwrap();
    assert!(
        note.contains("NOT coverage"),
        "_note keeps the caveat: {note}"
    );
    assert!(note.contains("never a"), "_note disclaims a gate: {note}");
}

#[test]
fn undefined_ratios_serialize_as_null_not_zero() {
    // Point metrics at the production file ALONE: no test files → assertion density undefined,
    // and (since there is production code but no tests) the ratio is a real 0.0, not null.
    let (stdout, _stderr, _code) = run_metrics(&["src/calc.py"], &["--format", "json"]);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    let proxies = &value["test_proxies"];
    assert_eq!(proxies["test_functions"], 0);
    // No test functions → density is undefined → null, never a misleading 0.
    assert!(
        proxies["assertion_density"].is_null(),
        "density null with no tests"
    );
    // Same for the trivial-test rate: no test functions → null, not a misleading 0 (#121).
    assert!(
        proxies["trivial_test_rate"].is_null(),
        "trivial_test_rate null with no tests"
    );
    assert_eq!(proxies["trivial_test_functions"], 0);
    // There IS production code, so the ratio is a defined 0.0 (no test LoC over some prod LoC).
    assert_eq!(proxies["test_code_ratio"].as_f64().unwrap(), 0.0);
}

#[test]
fn text_and_markdown_label_the_proxies_as_not_coverage() {
    let (text, _stderr, code) = run_metrics(&["."], &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("test:code ratio"),
        "text table has the ratio:\n{text}"
    );
    assert!(
        text.contains("assertion density"),
        "text table has assertion density:\n{text}"
    );
    assert!(
        text.contains("trivial-test rate"),
        "text table has the trivial-test rate (#121):\n{text}"
    );
    assert!(
        text.contains("not coverage"),
        "text table keeps the caveat:\n{text}"
    );

    let (md, _stderr, code) = run_metrics(&["."], &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        md.contains("Test proxies"),
        "markdown has the proxies block:\n{md}"
    );
    assert!(
        md.contains("trivial-test rate"),
        "markdown surfaces the trivial-test rate (#121):\n{md}"
    );
    assert!(
        md.contains("not coverage"),
        "markdown labels the proxies as not-coverage:\n{md}"
    );
}
