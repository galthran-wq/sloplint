//! End-to-end test for the SLP111 single-use single-method class detector (issue #23), running
//! the real built binary over a committed Python fixture. SLP111 is a preview, whole-project
//! analysis, so it fires only with `--preview` and only on a class instantiated exactly once.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/single_use_class.py")
}

/// SLP111 findings from `check`-ing the fixture, with `--preview` on or off.
fn slp111_findings(tag: &str, preview: bool) -> Vec<Value> {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("su_{tag}.toml"));
    std::fs::write(&config_path, "select = [\"SLP111\"]\n").unwrap();

    let fixture = fixture();
    let mut args = vec!["check", fixture.to_str().unwrap()];
    if preview {
        args.push("--preview");
    }
    args.extend([
        "--config",
        config_path.to_str().unwrap(),
        "--format",
        "json",
    ]);

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .output()
        .expect("failed to run sloplint binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("check --format json must be valid JSON ({e}):\n{stdout}"));
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["code"] == "SLP111")
        .cloned()
        .collect()
}

#[test]
fn flags_only_the_single_use_class_and_only_under_preview() {
    // Off without preview (it's a preview rule).
    assert!(slp111_findings("off", false).is_empty());

    // With preview: `Formatter` (one method, instantiated once) is flagged; `Reused`
    // (instantiated twice) is not.
    let findings = slp111_findings("on", true);
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["line"], 4, "the `Formatter` class line");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("Formatter") && message.contains("single method"),
        "message: {message}"
    );
}
