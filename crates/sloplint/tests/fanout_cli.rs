//! End-to-end test for SLP090 (flat-directory fanout): the whole-tree rule, after its logic
//! moved into `sloplint_linter::fanout`, must still fire end-to-end via `sloplint check`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// A unique temp project dir (avoids the `slp` substring some metrics tests assert against).
fn temp_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fanout-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn check_flags_a_flat_directory_over_the_default_limit() {
    let project = temp_project("over");
    let pkg = project.join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    // Default dir_max_modules is 15; 16 trivial modules trip SLP090.
    for i in 0..16 {
        fs::write(pkg.join(format!("m{i}.py")), "x = 1\n").unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&project)
        .args(["check", ".", "--format", "json"])
        .output()
        .expect("run sloplint");
    let stdout = String::from_utf8(out.stdout).unwrap();

    assert!(
        stdout.contains("SLP090"),
        "expected SLP090 finding:\n{stdout}"
    );
    assert!(
        stdout.contains("split it into sub-packages"),
        "expected the fanout message:\n{stdout}"
    );

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn check_is_silent_when_under_the_limit() {
    let project = temp_project("under");
    let pkg = project.join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    for i in 0..5 {
        fs::write(pkg.join(format!("m{i}.py")), "x = 1\n").unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&project)
        .args(["check", ".", "--format", "json"])
        .output()
        .expect("run sloplint");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("SLP090"),
        "unexpected SLP090 under the limit:\n{stdout}"
    );

    let _ = fs::remove_dir_all(&project);
}
