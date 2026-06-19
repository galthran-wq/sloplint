//! End-to-end tests for the SLP100 leftover-placeholder rule (issue #24), running the real
//! built binary over a committed Python fixture. Covers the built-in lexicon firing on real
//! source, the user-extendable `[placeholders] extra` config, and the `tests/` self-exemption
//! — all observed through `check --format json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/placeholder_residue.py")
}

/// Write `config` into the per-suite temp dir and return its path.
fn write_config(name: &str, config: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, config).unwrap();
    path
}

/// Run `sloplint check <target> --preview --config <config> --format json` and return the
/// parsed findings array.
fn run_check(target: &Path, config_path: &Path) -> Vec<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            target.to_str().unwrap(),
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
    value["findings"].as_array().cloned().unwrap_or_default()
}

/// Only SLP100, preview on — so the fixture's other potential findings stay out of the way.
const ONLY_SLP100: &str = "preview = true\nselect = [\"SLP100\"]\n";

#[test]
fn builtin_lexicon_flags_the_leaked_secret_only() {
    let config = write_config("only100.toml", ONLY_SLP100);
    let findings = run_check(&fixture(), &config);

    // The built-in lexicon catches `your_api_key_here`; the custom `fill_me_in` slot is not
    // yet configured, so it stays clean.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP100");
    assert_eq!(findings[0]["line"], 8, "the API_KEY assignment line");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(
        message.contains("your_api_key_here") && message.contains("secret placeholder"),
        "message: {message}"
    );
}

#[test]
fn user_extra_phrase_adds_a_finding() {
    let config = write_config(
        "extra100.toml",
        "preview = true\nselect = [\"SLP100\"]\n\n[placeholders]\nextra = [\"fill_me_in\"]\n",
    );
    let findings = run_check(&fixture(), &config);

    // Now both the built-in secret and the configured `fill_me_in` slot are flagged.
    assert_eq!(findings.len(), 2, "findings: {findings:#?}");
    let messages: Vec<&str> = findings
        .iter()
        .map(|f| f["message"].as_str().unwrap())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("fill_me_in")),
        "the configured phrase should fire: {messages:#?}"
    );
}

#[test]
fn placeholders_in_a_tests_path_are_exempt() {
    // Copy the same residue under a `tests/` path: SLP100 must stay silent there.
    // Name the file `sample.py` (not `test_*.py`) so this exercises the `tests/` *directory*
    // exemption branch, not the filename branch.
    let tests_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    let target = tests_dir.join("sample.py");
    std::fs::write(&target, "API_KEY = \"your_api_key_here\"\n").unwrap();

    let config = write_config("exempt100.toml", ONLY_SLP100);
    let findings = run_check(&target, &config);
    assert!(
        findings.is_empty(),
        "tests/ files are exempt, got: {findings:#?}"
    );
}
