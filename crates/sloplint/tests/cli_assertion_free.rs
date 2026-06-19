//! End-to-end test of SLP070 (assertion-free / tautological tests) through the real CLI.
//!
//! Runs the built `sloplint` binary over a realistic Python test module and asserts which
//! functions are flagged — exercising discovery, config, the preview gate, the rule, and
//! JSON reporting together, on real Python rather than a hand-built AST.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn demo_file() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources/assertion_free_demo.py")
        .to_string_lossy()
        .into_owned()
}

/// Run `sloplint check` over the demo file and return parsed JSON findings.
fn run_check(extra_args: &[&str]) -> Vec<Value> {
    let mut args = vec!["check", "--format", "json"];
    args.extend_from_slice(extra_args);
    let file = demo_file();
    args.push(&file);

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .output()
        .expect("running the sloplint binary");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("parsing JSON output: {e}\n--- stdout ---\n{stdout}"));
    parsed["findings"]
        .as_array()
        .expect("findings is an array")
        .clone()
}

/// The function name inside the backticks of an SLP070 message, e.g.
/// "test `test_x` runs code but asserts nothing" -> "test_x".
fn flagged_function(message: &str) -> &str {
    message
        .split('`')
        .nth(1)
        .unwrap_or_else(|| panic!("no backticked name in message: {message}"))
}

#[test]
fn flags_exactly_the_worthless_tests() {
    let findings = run_check(&["--preview"]);
    let mut flagged: Vec<String> = findings
        .iter()
        .filter(|f| f["code"] == "SLP070")
        .map(|f| flagged_function(f["message"].as_str().unwrap()).to_string())
        .collect();
    flagged.sort();

    let mut expected = vec![
        "test_divide_runs",          // runs code, no assertion
        "test_addition_is_addition", // assert True
        "test_constants_are_equal",  // assert 1 == 1
        "test_echo_local",           // assert on a preceding literal
        "test_divide_smoke",         // unittest method, no assertion
    ];
    expected.sort();

    assert_eq!(flagged, expected);
}

#[test]
fn spares_the_genuine_tests() {
    let findings = run_check(&["--preview"]);
    let flagged: Vec<&str> = findings
        .iter()
        .filter(|f| f["code"] == "SLP070")
        .map(|f| flagged_function(f["message"].as_str().unwrap()))
        .collect();

    // Real asserts, parametrized tests, `pytest.raises`, delegated helpers, skipped tests,
    // non-test helpers, and `self.assert*` methods must never be flagged.
    for genuine in [
        "test_add_returns_sum",
        "test_add_parametrized",
        "test_divide_by_zero_raises",
        "test_parse_via_helper",
        "test_future_feature",
        "check_parsed",
        "test_add",
    ] {
        assert!(
            !flagged.contains(&genuine),
            "{genuine} should not be flagged; flagged = {flagged:?}"
        );
    }
}

#[test]
fn slp070_is_preview_gated() {
    // Without --preview, the rule must not fire at all.
    let findings = run_check(&[]);
    assert!(
        findings.iter().all(|f| f["code"] != "SLP070"),
        "SLP070 is preview-only and must not fire without --preview"
    );
}
