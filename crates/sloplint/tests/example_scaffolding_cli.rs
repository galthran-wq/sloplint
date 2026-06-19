//! End-to-end test for the SLP140 example-scaffolding rule (issue #22), running the real built
//! binary over a committed Python fixture. SLP140 is a Preview rule, so the run enables
//! `--preview` and selects only SLP140, observed through `check --format json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/example_scaffolding.py")
}

#[test]
fn flags_the_library_demo_block() {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("only140.toml");
    std::fs::write(&config_path, "preview = true\nselect = [\"SLP140\"]\n").unwrap();

    let fixture = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            fixture.to_str().unwrap(),
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

    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP140");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("__main__") && message.contains("scaffolding"),
        "message: {message}"
    );
}
