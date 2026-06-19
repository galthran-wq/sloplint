//! End-to-end test of SLP084 (deeply nested data-structure literals) through the real CLI.
//!
//! Runs the built `sloplint` binary over a realistic Python module and asserts that only
//! the over-nested literals are flagged — exercising discovery, config, the preview gate,
//! the rule, and JSON reporting together on real Python.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn demo_file() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/resources/deep_data_nesting_demo.py")
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

fn slp084(findings: &[Value]) -> Vec<&Value> {
    findings.iter().filter(|f| f["code"] == "SLP084").collect()
}

#[test]
fn flags_only_the_over_nested_literals() {
    let findings = run_check(&["--preview"]);
    let nesting = slp084(&findings);

    // Exactly the three deep literals in the demo: SETTINGS, GRID, PERMISSIONS.
    assert_eq!(
        nesting.len(),
        3,
        "expected 3 SLP084 findings, got {nesting:#?}"
    );
    // Each finding reports a depth past the default limit of 3 — proving shallow data
    // (FLAGS, POINTS, NESTED_ONCE, TWO_DEEP) and deep control flow are never reported.
    for finding in &nesting {
        let message = finding["message"].as_str().unwrap();
        assert!(
            message.contains("levels deep (max 3)"),
            "unexpected message: {message}"
        );
    }
}

#[test]
fn slp084_is_preview_gated() {
    let findings = run_check(&[]);
    assert!(
        slp084(&findings).is_empty(),
        "SLP084 is preview-only and must not fire without --preview"
    );
}
