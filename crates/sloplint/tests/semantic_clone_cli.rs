//! End-to-end test for Type-4 (semantic) clone detection (issue #7), running the real built
//! binary over two committed Python modules that are the *same logic with commutative operands
//! reshuffled*. With `[clone] canonicalize_commutative = true` the engine flags them (SLP020);
//! without it — the default — they slip through, proving the flag does real work.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/semantic_clone")
}

/// The SLP020 findings from `check`-ing the fixture dir under `config_toml`.
fn slp020(tag: &str, config_toml: &str) -> usize {
    let config_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("semclone_{tag}.toml"));
    std::fs::write(&config_path, config_toml).unwrap();

    let dir = fixture_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            dir.to_str().unwrap(),
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
fn canonicalization_flag_catches_the_semantic_clone() {
    // One SLP020 per function in the pair (each points at its partner).
    let with_flag = slp020("on", "[clone]\ncanonicalize_commutative = true\n");
    assert_eq!(
        with_flag, 2,
        "expected both functions flagged with the flag on"
    );
}

#[test]
fn default_behavior_does_not_flag_the_reordered_pair() {
    // Default config (flag off): the order-sensitive token path doesn't pair them.
    let default = slp020("off", "[clone]\nmin_statements = 3\n");
    assert_eq!(default, 0, "default behavior must be unchanged (no SLP020)");
}
