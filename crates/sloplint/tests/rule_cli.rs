//! End-to-end tests for `sloplint rule` (the rule explainer, like `ruff rule`).

use std::process::Command;

fn run(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(args)
        .output()
        .expect("run sloplint");
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn rule_lists_all_rules() {
    let (stdout, _err, code) = run(&["rule"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("SLP001"), "list:\n{stdout}");
    assert!(
        stdout.contains("redundant-comment"),
        "kebab name:\n{stdout}"
    );
    assert!(stdout.contains("SLP030"), "list:\n{stdout}");
}

#[test]
fn rule_explains_one_rule() {
    let (stdout, _err, code) = run(&["rule", "SLP030"]);
    assert_eq!(code, 0);
    assert!(
        stdout.starts_with("SLP030 (defensive-except) [stable]"),
        "header:\n{stdout}"
    );
    assert!(stdout.contains("## What it does"), "doc:\n{stdout}");
    assert!(stdout.contains("## Why is this bad?"), "doc:\n{stdout}");
    assert!(stdout.contains("## Example"), "doc:\n{stdout}");
}

#[test]
fn rule_lookup_is_case_insensitive() {
    let (stdout, _err, code) = run(&["rule", "slp030"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("## What it does"), "doc:\n{stdout}");
}

#[test]
fn rule_errors_on_unknown_code() {
    let (_out, stderr, code) = run(&["rule", "SLP999"]);
    assert_eq!(code, 2, "unknown rule should exit 2 (tool error)");
    assert!(stderr.contains("unknown rule"), "stderr:\n{stderr}");
}

#[test]
fn rule_json_lists_all_rules() {
    let (stdout, _err, code) = run(&["rule", "--format", "json"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON array");
    let arr = value.as_array().expect("array");
    assert!(arr
        .iter()
        .any(|r| r["code"] == "SLP030" && r["name"] == "defensive-except"));
}

#[test]
fn rule_json_explains_one_rule() {
    let (stdout, _err, code) = run(&["rule", "SLP030", "--format", "json"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON object");
    assert_eq!(value["code"], "SLP030");
    assert_eq!(value["preview"], false);
}

#[test]
fn rule_json_errors_on_unknown_code() {
    let (_out, stderr, code) = run(&["rule", "SLP999", "--format", "json"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown rule"), "stderr:\n{stderr}");
}
