//! End-to-end test for Type-3 gapped-clone detection (issue #7), running the real built
//! binary over a committed Python fixture. The fixture's two functions are the same logic with
//! statements reordered: the ordered (default) pass misses them, and `[clone] detect_gapped`
//! catches them — both observed through `check --format json` (SLP020).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/gapped_clone.py")
}

/// Number of SLP020 findings from `check`-ing the fixture under `config_toml`.
fn clone_findings(tag: &str, config_toml: &str) -> usize {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("clone_{tag}.toml"));
    std::fs::write(&config_path, config_toml).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            fixture().to_str().unwrap(),
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
        .filter(|f| f["code"] == "SLP020")
        .count()
}

#[test]
fn reordered_clone_is_flagged_only_with_gapped_enabled() {
    // Default config: the ordered pass misses the reordering.
    assert_eq!(clone_findings("off", "[clone]\nsimilarity = 0.85\n"), 0);

    // With gapped detection on, the reordered pair is reported.
    assert!(
        clone_findings("on", "[clone]\ndetect_gapped = true\n") >= 1,
        "the reordered clone should be flagged when detect_gapped is enabled"
    );
}
