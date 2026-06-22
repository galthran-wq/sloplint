//! End-to-end test for the top-level-code-ratio metric (issue #141), running the real built binary.
//! A procedural script-dump (no functions) must surface as undecomposed; a well-decomposed module
//! must not.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("toplevel-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(project: &Path, rel: &str, contents: &str) {
    std::fs::write(project.join(rel), contents).unwrap();
}

#[test]
fn reports_top_level_code_ratio() {
    let project = make_project("ratio");
    // A 20-statement top-level script-dump (no functions) — complexity/size metrics see nothing.
    let script: String = (0..20).map(|i| format!("render(step_{i}())\n")).collect();
    write(&project, "dashboard.py", &script);
    // A well-decomposed module: all logic inside a function.
    let decomposed = "def build():\n".to_string()
        + &(0..20)
            .map(|i| format!("    render(step_{i}())\n"))
            .collect::<String>();
    write(&project, "service.py", &decomposed);

    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&project)
        .args(["metrics", ".", "--format", "json"])
        .output()
        .expect("run sloplint");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let tl = &value["profiles"]["production"]["top_level_code"];

    // The dashboard is entirely top-level → max ratio 1.0; it's the one undecomposed module.
    assert_eq!(tl["undecomposed_modules"], 1, "{stdout}");
    assert!(
        (tl["max_ratio"].as_f64().unwrap() - 1.0).abs() < 1e-9,
        "{stdout}"
    );
    // avg over the two logic-bearing modules: (1.0 + 0.0) / 2 = 0.5.
    assert!(
        (tl["avg_ratio"].as_f64().unwrap() - 0.5).abs() < 1e-9,
        "{stdout}"
    );
}
