//! End-to-end tests for SLP220 (corrupted/truncated AI output, issue #144), running the real
//! built binary. SLP220 is preview-gated and CLI-level (an unparseable file never reaches the
//! registry rules): verifies the leftover-fence / conflict-marker / tag / unparseable / prose
//! signals, that a fence inside a docstring is NOT flagged, and that it's silent by default.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("corrupted-{}-{tag}", std::process::id()));
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

/// SLP220 messages for `file`, parsed from `--format json`.
fn slp220(project: &Path, file: &str, preview: bool) -> Vec<String> {
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
        .filter(|f| f["code"] == "SLP220")
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn leftover_fence_in_unparseable_file() {
    let project = make_project("fence");
    write(&project, "a.py", "```python\ndef f():\n    return 1\n");
    let msgs = slp220(&project, "a.py", true);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("code fence"), "{msgs:?}");
}

#[test]
fn merge_conflict_marker() {
    let project = make_project("conflict");
    write(
        &project,
        "a.py",
        "def f():\n<<<<<<< HEAD\n    return 1\n=======\n    return 2\n>>>>>>> b\n",
    );
    let msgs = slp220(&project, "a.py", true);
    assert!(
        msgs.iter().any(|m| m.contains("merge-conflict")),
        "{msgs:?}"
    );
}

#[test]
fn stray_scaffolding_tag() {
    let project = make_project("tag");
    write(
        &project,
        "a.py",
        "<file path=\"a.py\">\ndef f():\n    return 1\n",
    );
    let msgs = slp220(&project, "a.py", true);
    assert!(
        msgs.iter().any(|m| m.contains("scaffolding tag")),
        "{msgs:?}"
    );
}

#[test]
fn fence_inside_docstring_is_not_flagged() {
    // A Markdown code block inside a docstring is legitimate content — the file parses and SLP220
    // must stay silent.
    let project = make_project("docstring");
    write(
        &project,
        "a.py",
        "def f():\n    \"\"\"Example:\n\n    ```py\n    f()\n    ```\n    \"\"\"\n    return 1\n",
    );
    assert!(slp220(&project, "a.py", true).is_empty());
}

#[test]
fn prose_heavy_file_classified_as_prose() {
    // A pasted explanation saved as .py: many natural-language lines, doesn't parse.
    let project = make_project("prose");
    let prose = "Here is the implementation of the parser module today\n\
                 It reads the incoming tokens one at a time slowly\n\
                 The main function returns a fully parsed result object\n\
                 Finally the routine cleans up every open file handle\n\
                 This design keeps the memory footprint very small\n\
                 Each stage validates its own inputs before continuing\n\
                 The error path logs a clear and actionable message\n\
                 Overall the approach favors clarity over raw speed here\n";
    write(&project, "a.py", prose);
    let msgs = slp220(&project, "a.py", true);
    assert!(
        msgs.iter().any(|m| m.contains("natural-language prose")),
        "{msgs:?}"
    );
}

#[test]
fn clean_file_and_preview_gating() {
    let project = make_project("clean");
    write(&project, "a.py", "def add(a, b):\n    return a + b\n");
    assert!(slp220(&project, "a.py", true).is_empty(), "clean file");

    // A corrupted file is silent without --preview (skipped as before, not an SLP220 finding).
    write(&project, "b.py", "```python\nx = 1\n");
    assert!(
        slp220(&project, "b.py", false).is_empty(),
        "SLP220 is preview, off by default"
    );
    assert!(
        !slp220(&project, "b.py", true).is_empty(),
        "and fires under --preview"
    );
}
