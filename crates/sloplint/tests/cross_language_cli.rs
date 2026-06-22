//! End-to-end tests for SLP250 (cross-language pollution, issue #148), running the real built
//! binary. Preview-gated; verifies the foreign-idiom positives, the FP-prone negatives, and
//! `[crosslang] allow`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("crosslang-{}-{tag}", std::process::id()));
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

fn slp250(project: &Path, file: &str, preview: bool) -> Vec<String> {
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
        .filter(|f| f["code"] == "SLP250")
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

const SOURCE: &str = "a = obj.toString()\nn = arr.length\narray_push(b, 1)\nclean = re.sub(p, r, s)\nstack.push(x)\ntotal = q.size()\n";

#[test]
fn flags_foreign_idioms_under_preview() {
    let project = make_project("preview");
    write(&project, "app.py", SOURCE);
    let msgs = slp250(&project, "app.py", true);
    assert!(msgs.iter().any(|m| m.contains("toString")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("length")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("array_push")), "{msgs:?}");
    // FP-prone names stay quiet: re.sub, stack.push, q.size().
    assert!(!msgs.iter().any(|m| m.contains("`sub`")), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("`push`")), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("`size`")), "{msgs:?}");
    assert_eq!(msgs.len(), 3, "exactly the three foreign idioms: {msgs:?}");
}

#[test]
fn preview_gated_off_by_default() {
    let project = make_project("gated");
    write(&project, "app.py", SOURCE);
    assert!(slp250(&project, "app.py", false).is_empty());
}

#[test]
fn allow_list_is_configurable() {
    let project = make_project("allow");
    write(&project, "app.py", "a = obj.toString()\n");
    assert!(
        !slp250(&project, "app.py", true).is_empty(),
        "flagged by default"
    );

    write(
        &project,
        "sloplint.toml",
        "[crosslang]\nallow = [\"toString\"]\n",
    );
    assert!(
        slp250(&project, "app.py", true).is_empty(),
        "allow-list suppresses it"
    );
}
