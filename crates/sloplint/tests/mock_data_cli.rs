//! End-to-end tests for SLP230 (mock/placeholder data in production code, issue #145), running the
//! real built binary. Verifies preview-gating, the production positives, that test paths and real
//! data are not flagged, and `[placeholders] extra`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mockdata-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(project: &Path, rel: &str, contents: &str) {
    let path = project.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(project: &Path, args: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(project)
        .args(args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

fn slp230(project: &Path, file: &str, preview: bool) -> Vec<String> {
    let mut args = vec!["check", file, "--format", "json"];
    if preview {
        args.push("--preview");
    }
    let (stdout, _) = run(project, &args);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["code"] == "SLP230")
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

const PROD: &str = "ADMIN = \"admin@example.com\"\npassword = \"changeme\"\n\n\ndef get():\n    return {\"foo\": \"bar\"}\n\n\nREAL = \"ops@acme-prod.io\"\nstrong = \"a7Fq9zLp2KdM\"\n";

#[test]
fn flags_production_placeholders_under_preview() {
    let project = make_project("prod");
    write(&project, "app.py", PROD);
    let msgs = slp230(&project, "app.py", true);
    assert!(
        msgs.iter().any(|m| m.contains("placeholder email")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter().any(|m| m.contains("placeholder credential")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter().any(|m| m.contains("dummy placeholder dict")),
        "{msgs:?}"
    );
    // Real email and strong password are not flagged.
    assert_eq!(msgs.len(), 3, "only the 3 placeholders: {msgs:?}");
}

#[test]
fn preview_gated_off_by_default() {
    let project = make_project("gated");
    write(&project, "app.py", PROD);
    assert!(slp230(&project, "app.py", false).is_empty());
}

#[test]
fn test_paths_are_excluded() {
    // The same placeholder data under tests/ is expected, not slop.
    let project = make_project("tests");
    write(&project, "tests/test_app.py", PROD);
    assert!(slp230(&project, "tests/test_app.py", true).is_empty());
}

#[test]
fn extra_placeholder_values_are_configurable() {
    let project = make_project("extra");
    write(
        &project,
        "app.py",
        "token = \"REPLACE_ME\"\n\n\ndef f():\n    return \"REPLACE_ME\"\n",
    );
    // REPLACE_ME isn't built-in.
    assert!(slp230(&project, "app.py", true).is_empty());

    write(
        &project,
        "sloplint.toml",
        "[placeholders]\nextra = [\"REPLACE_ME\"]\n",
    );
    let msgs = slp230(&project, "app.py", true);
    assert!(!msgs.is_empty(), "extra value now flagged: {msgs:?}");
}
