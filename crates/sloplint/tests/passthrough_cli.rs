//! End-to-end test for the SLP110 pass-through-wrapper rule (issue #23), running the real
//! built binary over a committed Python fixture and reading `check --format json`. SLP110 is a
//! Preview rule, so the run enables `--preview` and selects only SLP110 (via an explicit
//! config) to isolate it from the comment/other rules.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/passthrough.py")
}

#[test]
fn flags_only_the_pure_pass_through_wrapper() {
    // Select only SLP110, with preview on, written to the per-suite temp dir.
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("only110.toml");
    std::fs::write(&config_path, "preview = true\nselect = [\"SLP110\"]\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            fixture().to_str().unwrap(),
            "--preview",
            "--config",
            config_path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run sloplint binary");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("check --format json must be valid JSON ({e}):\n{stdout}"));
    let findings = value["findings"].as_array().unwrap();

    // Only `fetch` is a pure pass-through; `fetch_checked` adds a guard and `add` transforms.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP110");
    assert_eq!(findings[0]["line"], 4, "the `fetch` definition line");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("pass-through wrapper") && message.contains("_backend.fetch"),
        "message: {message}"
    );
}
