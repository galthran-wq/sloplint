//! End-to-end test for the SLP003 comment-deodorant rule (issue #8), running the real built
//! binary over a committed Python fixture and reading `check --format json`. SLP003 is a
//! Preview rule, so the run enables `--preview` and selects only SLP003 (via an explicit
//! config) to isolate it from the default comment-ban and other rules.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/comment_deodorant.py")
}

#[test]
fn flags_only_the_hard_and_commented_function() {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("only003.toml");
    std::fs::write(&config_path, "preview = true\nselect = [\"SLP003\"]\n").unwrap();

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

    // `classify` is hard AND heavily commented; `clean` is equally complex but uncommented.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP003");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("classify") && message.contains("comment density"),
        "message: {message}"
    );
}
