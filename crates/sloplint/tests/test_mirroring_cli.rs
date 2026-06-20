//! End-to-end test for the SLP160 test-mirroring rule (issue #16), running the real built
//! binary over a committed fixture *project* (a production module + its mechanically-mirrored,
//! assertion-free test module). SLP160 is a whole-project Preview rule, so the run enables
//! `--preview` and selects only SLP160, observed through `check --format json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/mirror_project")
}

/// The SLP160 findings (as JSON objects) from `check`-ing the fixture project under `config`.
fn slp160(tag: &str, config: &str) -> Vec<Value> {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mirror_{tag}.toml"));
    std::fs::write(&config_path, config).unwrap();

    let dir = fixture_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            dir.to_str().unwrap(),
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
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["code"] == "SLP160")
        .cloned()
        .collect()
}

#[test]
fn flags_the_mechanically_mirrored_test_module() {
    let findings = slp160("plain", "preview = true\nselect = [\"SLP160\"]\n");

    // Exactly one finding, attributed to the mirrored test module.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    let path = findings[0]["path"].as_str().unwrap();
    assert!(path.ends_with("test_widgets.py"), "path: {path}");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("3 of 3") && message.contains("mirrors"),
        "message: {message}"
    );
}

#[test]
fn ignoring_slp160_on_production_does_not_drop_detection() {
    // Production symbols must still be gathered when SLP160 is ignored on those files; ignoring
    // it on the *test* file is what suppresses the finding.
    let prod_ignored = slp160(
        "prod",
        "preview = true\nselect = [\"SLP160\"]\n[[overrides]]\npath = \"**/widgets.py\"\nignore = [\"SLP160\"]\n",
    );
    assert_eq!(prod_ignored.len(), 1, "still flagged: {prod_ignored:#?}");

    let test_ignored = slp160(
        "test",
        "preview = true\nselect = [\"SLP160\"]\n[[overrides]]\npath = \"**/test_widgets.py\"\nignore = [\"SLP160\"]\n",
    );
    assert!(test_ignored.is_empty(), "suppressed: {test_ignored:#?}");
}
