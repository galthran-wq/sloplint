//! End-to-end tests for configurable profiles (#96): a profile is a named, path-matched slice of
//! the tree carrying its own rule deltas (thresholds, ignores) *and* its own metrics panel. These
//! exercise the real built binary over temp projects with a `sloplint.toml`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Materialize `files` (relative path → contents) plus a `sloplint.toml` in a fresh temp dir, run
/// `sloplint <args>` from inside it, and return (stdout, exit code).
fn run(tag: &str, toml: &str, files: &[(&str, &str)], args: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_prof_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sloplint.toml"), toml).unwrap();
    for (rel, contents) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&dir)
        .args(args)
        .output()
        .expect("run sloplint");
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

fn findings(stdout: &str) -> Vec<Value> {
    let value: Value = serde_json::from_str(stdout).expect("check --format json is valid JSON");
    value["findings"].as_array().cloned().unwrap_or_default()
}

/// The headline new capability: rule thresholds are per-profile, layered over global defaults.
/// A global `file_max_lines = 5` trips SLP080 in production, but a `tests` profile that raises the
/// ceiling to 1000 spares the identical file under `tests/`.
#[test]
fn per_profile_threshold_relaxes_a_rule_for_matching_paths() {
    let toml = r#"
select = ["SLP080"]
limits = { file_max_lines = 5 }

[[profiles]]
name = "tests"
match = ["tests/**"]
limits = { file_max_lines = 1000 }

[[profiles]]
name = "production"
default = true
"#;
    // A 12-line module — over the global ceiling of 5, under the tests profile's 1000.
    let body: String = (0..12).map(|i| format!("x{i} = {i}\n")).collect();
    let files = [
        ("src/big.py", body.as_str()),
        ("tests/test_big.py", body.as_str()),
    ];

    // Production file: over the global ceiling → SLP080 fires, exit 1.
    let (out, code) = run(
        "thresh_prod",
        toml,
        &files,
        &["check", "src/big.py", "--format", "json"],
    );
    let prod_findings = findings(&out);
    assert_eq!(
        prod_findings.len(),
        1,
        "production findings: {prod_findings:#?}"
    );
    assert_eq!(prod_findings[0]["code"], "SLP080");
    assert_eq!(code, 1, "a finding exits non-zero");

    // Same file under the tests profile's raised ceiling → nothing fires, exit 0.
    let (out, code) = run(
        "thresh_test",
        toml,
        &files,
        &["check", "tests/test_big.py", "--format", "json"],
    );
    assert!(
        findings(&out).is_empty(),
        "test file is under the profile's raised ceiling: {}",
        out
    );
    assert_eq!(code, 0, "no findings exits clean");
}

/// A profile `ignore` disables a rule for matching paths only — the generalization of the old
/// per-path override, now also a metrics partition.
#[test]
fn profile_ignore_disables_a_rule_for_matching_paths_only() {
    let toml = r#"
select = ["SLP080"]
limits = { file_max_lines = 5 }

[[profiles]]
name = "generated"
match = ["gen/**"]
ignore = ["SLP080"]

[[profiles]]
name = "production"
default = true
"#;
    let body: String = (0..12).map(|i| format!("x{i} = {i}\n")).collect();
    let files = [("gen/big.py", body.as_str()), ("src/big.py", body.as_str())];

    let (out, _) = run(
        "ign_gen",
        toml,
        &files,
        &["check", "gen/big.py", "--format", "json"],
    );
    assert!(
        findings(&out).is_empty(),
        "SLP080 ignored under gen/: {out}"
    );

    let (out, _) = run(
        "ign_src",
        toml,
        &files,
        &["check", "src/big.py", "--format", "json"],
    );
    assert_eq!(findings(&out).len(), 1, "still fires in production: {out}");
}

/// Custom profiles define their own metrics panels, beyond the built-in tests/production pair.
#[test]
fn metrics_reports_a_custom_profile_panel() {
    let toml = r#"
[[profiles]]
name = "examples"
match = ["examples/**"]

[[profiles]]
name = "production"
default = true
"#;
    let files = [
        ("examples/demo.py", "def demo():\n    return 1\n"),
        (
            "src/app.py",
            "def a():\n    return 1\n\ndef b():\n    return 2\n",
        ),
    ];

    // JSON emits a panel per configured profile, keyed by name.
    let (out, code) = run(
        "cust_json",
        toml,
        &files,
        &["metrics", ".", "--format", "json"],
    );
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&out).expect("valid JSON");
    let profiles = value["profiles"].as_object().expect("profiles map");
    assert_eq!(profiles["examples"]["functions"], 1, "examples: demo");
    assert_eq!(profiles["production"]["functions"], 2, "production: a + b");

    // `--scope <name>` selects a custom profile's feed.
    let (out, code) = run(
        "cust_feed",
        toml,
        &files,
        &[
            "metrics",
            ".",
            "--scope",
            "examples",
            "--format",
            "functions",
        ],
    );
    assert_eq!(code, 0);
    let names: Vec<String> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["function"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(names, ["demo"], "only the examples profile's function");
}
