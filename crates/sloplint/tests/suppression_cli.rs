//! End-to-end test for inline `# noqa` suppression (#94), exercised against its motivating case:
//! SLP020 near-duplicate functions. Runs the real built binary over a temp file and reads
//! `check --format json`. SLP020 is on by default ("disallowed duplication"), so the run selects
//! only SLP020 to isolate it from unrelated rules.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Write `source` to a temp `.py` file and return its SLP020 finding count.
fn slp020_count(name: &str, source: &str) -> usize {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mod.py");
    std::fs::write(&file, source).unwrap();
    let config = dir.join("only020.toml");
    std::fs::write(&config, "select = [\"SLP020\"]\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "check",
            file.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run sloplint binary");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("check --format json must be valid JSON ({e}):\n{stdout}"));
    let findings = value["findings"].as_array().unwrap();
    for finding in findings {
        assert_eq!(finding["code"], "SLP020", "config selected only SLP020");
    }
    findings.len()
}

/// Two verbatim-identical functions (names aren't used in clone matching) — a guaranteed clone.
const DUP: &str = "\
def alpha(items):
    total = 0
    for item in items:
        total += item
    return total


def beta(items):
    total = 0
    for item in items:
        total += item
    return total
";

#[test]
fn slp020_fires_by_default_on_duplicates() {
    // Disallowed-by-default: a clone is reported at each end (one finding per duplicated function).
    assert_eq!(
        slp020_count("dup_plain", DUP),
        2,
        "one SLP020 per duplicated function"
    );
}

#[test]
fn line_noqa_suppresses_each_acknowledged_site() {
    // A `# noqa` on ONE function's reported (def) line clears only that end — clones are per-site.
    let one = DUP.replacen(
        "def alpha(items):",
        "def alpha(items):  # noqa: SLP020 (intentional twin)",
        1,
    );
    assert_eq!(
        slp020_count("dup_one", &one),
        1,
        "one end acknowledged, the other remains"
    );

    // Acknowledging both ends clears the pair entirely.
    let both = one.replacen("def beta(items):", "def beta(items):  # noqa: SLP020", 1);
    assert_eq!(slp020_count("dup_both", &both), 0, "both ends acknowledged");
}

#[test]
fn a_noqa_for_another_code_does_not_suppress_slp020() {
    let other = DUP.replacen(
        "def alpha(items):",
        "def alpha(items):  # noqa: SLP999",
        1,
    );
    assert_eq!(
        slp020_count("dup_other", &other),
        2,
        "an unrelated code must not suppress SLP020"
    );
}
