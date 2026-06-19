//! End-to-end test for the SLP130 docstring-drift rule (issue #25), running the real built
//! binary over a committed Python fixture and reading `check --format json`. SLP130 is a
//! Preview rule, so the run enables `--preview` and selects only SLP130.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/docstring_drift.py")
}

#[test]
fn flags_only_the_drifted_docstring() {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("only130.toml");
    std::fs::write(&config_path, "preview = true\nselect = [\"SLP130\"]\n").unwrap();

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

    // `parse_config` documents `Raises: ValueError` it never raises; `divide` is accurate.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP130");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("parse_config") && message.contains("Raises: ValueError"),
        "message: {message}"
    );
}
