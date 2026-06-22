//! End-to-end tests for SLP240 (ghost scaffolding), running the real built binary.
//! Whole-project + preview-gated: verifies unreferenced defs are flagged, cross-file references /
//! exports / entry-points suppress, and the ghost-config-flag check.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ghostscaffold-{}-{tag}", std::process::id()));
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

fn slp240(project: &Path, preview: bool) -> Vec<String> {
    let mut args = vec!["check", ".", "--format", "json"];
    if preview {
        args.push("--preview");
    }
    let (stdout, _) = run(project, &args);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["code"] == "SLP240")
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

fn setup(project: &Path) {
    // a.py: a used class, a ghost helper, an orphan class, a base type, an entry point.
    write(
        project,
        "a.py",
        "class Widget:\n    pass\n\n\ndef ghost_helper():\n    return 2\n\n\nclass Orphan:\n    pass\n\n\nclass WidgetBase:\n    pass\n\n\ndef main():\n    pass\n",
    );
    // b.py references Widget (cross-file), defines a constant, reads a ghost + a defined flag.
    write(
        project,
        "b.py",
        "from a import Widget\n\nMAX = 3\nENABLE_REAL = True\n\n\ndef build():\n    w = Widget()\n    if settings.ENABLE_GHOST:\n        return MAX\n    if settings.ENABLE_REAL:\n        return w\n    return None\n\n\nuse = build()\n",
    );
}

#[test]
fn flags_ghost_defs_and_config_under_preview() {
    let project = make_project("preview");
    setup(&project);
    let msgs = slp240(&project, true);
    assert!(
        msgs.iter().any(|m| m.contains("`ghost_helper`")),
        "{msgs:?}"
    );
    assert!(msgs.iter().any(|m| m.contains("`Orphan`")), "{msgs:?}");
    assert!(
        msgs.iter()
            .any(|m| m.contains("ghost config flag") && m.contains("ENABLE_GHOST")),
        "{msgs:?}"
    );
    // Suppressed: Widget (referenced cross-file), WidgetBase (base suffix), main (entry point),
    // build/use (referenced), ENABLE_REAL (defined).
    assert!(!msgs.iter().any(|m| m.contains("`Widget`")), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("WidgetBase")), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("`main`")), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("ENABLE_REAL")), "{msgs:?}");
}

#[test]
fn preview_gated_off_by_default() {
    let project = make_project("gated");
    setup(&project);
    assert!(slp240(&project, false).is_empty());
}

#[test]
fn exported_def_is_not_ghost() {
    let project = make_project("export");
    write(
        &project,
        "api.py",
        "__all__ = [\"PublicThing\"]\n\n\nclass PublicThing:\n    pass\n",
    );
    assert!(
        !slp240(&project, true)
            .iter()
            .any(|m| m.contains("PublicThing")),
        "exported class is not ghost"
    );
}
