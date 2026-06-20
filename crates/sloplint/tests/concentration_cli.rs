//! End-to-end tests for the package module-count concentration metric (issue #103), running the
//! real built binary. A "god package" (one directory holding most modules) is invisible to the
//! edge-based architecture metrics; these assert it shows up in the node-distribution rollup, in
//! both the JSON feed and the human text view, and that it's scoped to the production profile.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A fresh temp project dir. `tag` keeps concurrent tests apart; the name avoids the "slp"
/// substring (a path containing it trips other path-sensitive assertions).
fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("concentration-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(project: &Path, rel: &str, contents: &str) {
    let path = project.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(project: &Path, args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(project)
        .args(args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

/// Build a project whose `extractor` package holds most modules (the yt-dlp shape, in miniature):
/// extractor/__init__.py + 9 site files = 10 modules, plus core (__init__ + util = 2) and a root
/// main.py = 1. Total 13 modules over 3 packages.
fn write_god_package(project: &Path) {
    write(project, "extractor/__init__.py", "");
    for i in 0..9 {
        write(project, &format!("extractor/site{i}.py"), &format!("x = {i}\n"));
    }
    write(project, "core/__init__.py", "");
    write(project, "core/util.py", "y = 1\n");
    write(project, "main.py", "z = 1\n");
}

#[test]
fn json_reports_concentration_and_names_the_god_package() {
    let project = make_project("json");
    write_god_package(&project);

    let (stdout, _stderr, code) = run(&project, &["metrics", ".", "--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let c = &value["profiles"]["production"]["packages"]["concentration"];

    assert_eq!(c["total_modules"], 13);
    assert_eq!(c["packages"], 3);
    assert_eq!(c["largest_package"]["package"], "extractor");
    assert_eq!(c["largest_package"]["modules"], 10);
    let share = c["max_package_share"].as_f64().unwrap();
    assert!((share - 10.0 / 13.0).abs() < 1e-9, "share = {share}");
    // One package dominating → positive inequality.
    assert!(c["module_count_gini"].as_f64().unwrap() > 0.0);
}

#[test]
fn text_view_surfaces_concentration_and_offender() {
    let project = make_project("text");
    write_god_package(&project);

    let (stdout, _stderr, code) = run(&project, &["metrics", "."]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("max package share"),
        "text panel shows concentration: {stdout}"
    );
    // The human view names the offending package and its share.
    assert!(
        stdout.contains("extractor, 10/13 modules"),
        "names the god package: {stdout}"
    );
    assert!(stdout.contains("module-count gini"), "stdout: {stdout}");
}

#[test]
fn concentration_is_scoped_to_the_production_profile() {
    // Test files live in the `tests` profile, so they must not count toward production concentration
    // (#96). Production here is just the 2-module `app` package + root main.py = 3 modules.
    let project = make_project("scope");
    write(&project, "app/__init__.py", "");
    write(&project, "app/core.py", "x = 1\n");
    write(&project, "main.py", "z = 1\n");
    for i in 0..8 {
        write(&project, &format!("tests/test_{i}.py"), &format!("def test_{i}():\n    assert True\n"));
    }

    let (stdout, _stderr, code) = run(&project, &["metrics", ".", "--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let prod = &value["profiles"]["production"]["packages"]["concentration"];
    // 8 test modules are excluded: production sees only app(2) + root(1) = 3.
    assert_eq!(prod["total_modules"], 3);
    assert_eq!(prod["largest_package"]["package"], "app");
    assert_eq!(prod["largest_package"]["modules"], 2);
}
