//! End-to-end tests for SLP004 (hedging/deferral + structural-noise comment tells),
//! running the real built binary. Preview-gated; verifies the classes, the legitimate-comment
//! negatives, per-class severity, and `[comments] extra`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("commenttells-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(project: &Path, rel: &str, contents: &str) {
    std::fs::write(project.join(rel), contents).unwrap();
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

/// SLP004 findings as (severity, message) for `file`.
fn slp004(project: &Path, file: &str, preview: bool) -> Vec<(String, String)> {
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
        .filter(|f| f["code"] == "SLP004")
        .map(|f| {
            (
                f["severity"].as_str().unwrap().to_string(),
                f["message"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

const SOURCE: &str = "x = 1  # for now, hardcode it\ny = 2  # should work in most cases\n# Step 1: begin\n# this function does the thing\nz = 3  # because the API needs a non-empty body\n";

#[test]
fn flags_tells_with_per_class_severity_under_preview() {
    let project = make_project("preview");
    write(&project, "app.py", SOURCE);
    let found = slp004(&project, "app.py", true);
    // Deferral is error; hedging/structural are warning.
    assert!(
        found
            .iter()
            .any(|(s, m)| s == "error" && m.contains("for now")),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|(s, m)| s == "warning" && m.contains("should work")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|(_, m)| m.contains("step-narration")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|(_, m)| m.contains("narrator")),
        "{found:?}"
    );
    // The WHY comment ("because …") is not a tell.
    assert!(
        !found.iter().any(|(_, m)| m.contains("because")),
        "{found:?}"
    );
    assert_eq!(found.len(), 4, "exactly the four tells: {found:?}");
}

#[test]
fn preview_gated_off_by_default() {
    let project = make_project("gated");
    write(&project, "app.py", SOURCE);
    assert!(slp004(&project, "app.py", false).is_empty());
}

#[test]
fn extra_lexicon_is_configurable() {
    let project = make_project("extra");
    write(
        &project,
        "app.py",
        "v = 1  # revisit later when we have time\n",
    );
    assert!(slp004(&project, "app.py", true).is_empty(), "not built-in");

    write(
        &project,
        "sloplint.toml",
        "[comments]\nextra = [\"revisit later\"]\n",
    );
    assert!(
        !slp004(&project, "app.py", true).is_empty(),
        "extra phrase now flagged"
    );
}
