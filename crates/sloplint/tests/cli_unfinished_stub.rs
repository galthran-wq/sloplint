//! End-to-end test of SLP034 (unfinished stub + self-admitted debt) through the real CLI.
//!
//! Runs the built `sloplint` binary over a realistic Python module and asserts exactly
//! which functions are flagged — exercising discovery, config, the preview gate, the rule
//! (comment/AST correlation), and JSON reporting together on real Python.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn demo_file() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources/unfinished_stub_demo.py")
        .to_string_lossy()
        .into_owned()
}

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

/// The function name inside the backticks of an SLP034 message.
fn flagged_function(message: &str) -> &str {
    message
        .split('`')
        .nth(1)
        .unwrap_or_else(|| panic!("no backticked name in message: {message}"))
}

#[test]
fn flags_exactly_the_unfinished_stubs() {
    let findings = run_check(&["--preview"]);
    let mut flagged: Vec<String> = findings
        .iter()
        .filter(|f| f["code"] == "SLP034")
        .map(|f| flagged_function(f["message"].as_str().unwrap()).to_string())
        .collect();
    flagged.sort();

    assert_eq!(
        flagged,
        vec!["export_report", "normalize", "warm_cache"],
        "unexpected SLP034 set"
    );
}

#[test]
fn spares_finished_and_legitimate_stubs() {
    let findings = run_check(&["--preview"]);
    let flagged: Vec<&str> = findings
        .iter()
        .filter(|f| f["code"] == "SLP034")
        .map(|f| flagged_function(f["message"].as_str().unwrap()))
        .collect();

    // Finished code, a plain TODO over real logic, an @abstractmethod stub, a normal
    // method, and a docstring-only "interface" must never be flagged.
    for genuine in ["total", "slugify", "read", "write", "documented_interface"] {
        assert!(
            !flagged.contains(&genuine),
            "{genuine} should not be flagged; flagged = {flagged:?}"
        );
    }
}

#[test]
fn slp034_is_preview_gated() {
    let findings = run_check(&[]);
    assert!(
        findings.iter().all(|f| f["code"] != "SLP034"),
        "SLP034 is preview-only and must not fire without --preview"
    );
}
