//! End-to-end test for SLP180 (undeclared third-party import), running the real
//! built binary over a temporary mini-project. The rule is whole-project and Preview: it
//! resolves the manifest from the working directory and the first-party package set from the
//! tree, so the test builds a small project (with a `pyproject.toml` and a local package) in
//! a temp dir *outside* the repo, then runs `check . --preview` from inside it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Lay out a mini-project in a fresh temp dir and return its path. `tag` keeps concurrent
/// tests in distinct directories.
fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sloplint-imports-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("mypkg")).unwrap();

    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\ndependencies = [\"requests\", \"PyYAML\"]\n",
    )
    .unwrap();
    // os: stdlib. requests: declared. yaml: declared via the PyYAML mapping. mypkg: local.
    // numpy: the only undeclared third-party import -> the one SLP180.
    std::fs::write(
        root.join("app.py"),
        "import os\nimport requests\nimport yaml\nimport numpy\nfrom mypkg import helper\n",
    )
    .unwrap();
    std::fs::write(root.join("mypkg").join("__init__.py"), "helper = 1\n").unwrap();
    root
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

#[test]
fn flags_only_the_undeclared_third_party_import() {
    let project = make_project("fires");
    let config = project.join("only180.toml");
    std::fs::write(&config, "preview = true\nselect = [\"SLP180\"]\n").unwrap();

    let (stdout, code) = run(
        &project,
        &[
            "check",
            ".",
            "--preview",
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
        ],
    );

    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("check --format json must be valid JSON ({e}):\n{stdout}"));
    let findings = value["findings"].as_array().unwrap();

    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert_eq!(findings[0]["code"], "SLP180");
    let message = findings[0]["message"].as_str().unwrap();
    assert!(message.contains("`numpy`"), "message: {message}");
    assert_eq!(code, 1, "findings -> non-zero exit");

    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn silent_without_preview() {
    let project = make_project("silent");
    // No --preview and a config that leaves preview off: the Preview rule must not fire.
    let config = project.join("stable.toml");
    std::fs::write(&config, "preview = false\nselect = [\"SLP180\"]\n").unwrap();

    let (stdout, code) = run(
        &project,
        &[
            "check",
            ".",
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
        ],
    );

    let value: Value = serde_json::from_str(&stdout).unwrap();
    let findings = value["findings"].as_array().unwrap();
    assert!(
        findings.is_empty(),
        "preview off -> no SLP180: {findings:#?}"
    );
    assert_eq!(code, 0);

    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn single_file_run_does_not_flag_local_packages() {
    // Regression: scanning one file must still resolve first-party packages from the whole
    // project tree (manifest root), so `mypkg` isn't mistaken for an undeclared third party.
    let project = make_project("single");
    let config = project.join("only180.toml");
    std::fs::write(&config, "preview = true\nselect = [\"SLP180\"]\n").unwrap();

    let (stdout, _code) = run(
        &project,
        &[
            "check",
            "app.py", // only this file, not the whole tree
            "--preview",
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
        ],
    );

    let value: Value = serde_json::from_str(&stdout).unwrap();
    let findings = value["findings"].as_array().unwrap();
    // Only numpy is undeclared; mypkg (local) must NOT be flagged even on a single-file run.
    assert_eq!(findings.len(), 1, "findings: {findings:#?}");
    assert!(findings[0]["message"].as_str().unwrap().contains("`numpy`"));

    let _ = std::fs::remove_dir_all(&project);
}
