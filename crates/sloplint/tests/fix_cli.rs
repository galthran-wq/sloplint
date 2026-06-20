//! End-to-end tests for autofix (`sloplint check --fix`), running the real built binary.
//!
//! Covers the flagship fixable rule SLP010 (comment deletion): rewriting files in place, leaving
//! profile-opted-out paths untouched, and the `--unsafe-fixes` gate (a no-op for SLP010, whose
//! fix is Safe). The fixture project is built under the temp dir (outside the repo, so a discovered
//! ancestor `sloplint.toml` can't interfere) with a name avoiding the "slp" substring.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A fresh temp project dir. `tag` keeps concurrent tests apart; the name avoids "slp".
fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("autofix-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn write(project: &Path, rel: &str, contents: &str) {
    let path = project.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn read(project: &Path, rel: &str) -> String {
    std::fs::read_to_string(project.join(rel)).unwrap()
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

#[test]
fn fix_deletes_banned_comments_in_place() {
    let project = make_project("delete");
    write(
        &project,
        "a.py",
        "def f():\n    x = 1  # inline\n    # own line\n    return x\n# module level\n",
    );

    let (_, stderr, code) = run(&project, &["check", "a.py", "--fix"]);

    // Every comment had a safe fix, so the run ends clean (exit 0) after fixing.
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.contains("fixed 3 issue(s)"), "stderr: {stderr}");
    // Inline comments lose just the trailing `  # ...`; own-line comments lose the whole line.
    assert_eq!(read(&project, "a.py"), "def f():\n    x = 1\n    return x\n");
}

#[test]
fn fix_respects_profile_opt_out() {
    // A profile that opts a path back into comments (ignores SLP010) must never be rewritten.
    let project = make_project("profile");
    write(&project, "src/app.py", "# banned\nx = 1\n");
    write(&project, "alembic/m.py", "# allowed\ny = 2\n");
    write(
        &project,
        "sloplint.toml",
        "[[profiles]]\nname = \"migrations\"\nmatch = [\"alembic/**\"]\nignore = [\"SLP010\"]\nallow_comments = true\n",
    );

    let (_, stderr, code) = run(&project, &["check", ".", "--fix"]);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.contains("fixed 1 issue(s)"), "stderr: {stderr}");
    assert_eq!(read(&project, "src/app.py"), "x = 1\n");
    // Untouched: the migrations profile opted out of SLP010.
    assert_eq!(read(&project, "alembic/m.py"), "# allowed\ny = 2\n");
}

#[test]
fn no_fix_flag_leaves_files_untouched() {
    let project = make_project("noflag");
    let original = "# keep me\nx = 1\n";
    write(&project, "a.py", original);

    let (_, _stderr, code) = run(&project, &["check", "a.py"]);

    // Findings, no fix requested -> exit 1 and the file is unchanged.
    assert_eq!(code, 1);
    assert_eq!(read(&project, "a.py"), original);
}

#[test]
fn unsafe_fixes_alone_does_not_apply() {
    // `--unsafe-fixes` without `--fix` is a no-op (documented): nothing is rewritten.
    let project = make_project("unsafe-noop");
    let original = "# keep\nx = 1\n";
    write(&project, "a.py", original);

    let (_, _stderr, code) = run(&project, &["check", "a.py", "--unsafe-fixes"]);

    assert_eq!(code, 1);
    assert_eq!(read(&project, "a.py"), original);
}

#[test]
fn directive_comments_are_never_fixed() {
    // Tool directives / suppressions are exempt from SLP010, so --fix must leave them in place.
    let project = make_project("directives");
    let original = "x = 1  # noqa: E501\n# type: ignore\ny = 2\n";
    write(&project, "a.py", original);

    let (_, stderr, code) = run(&project, &["check", "a.py", "--fix"]);

    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.contains("fixed 0 issue(s)"), "stderr: {stderr}");
    assert_eq!(read(&project, "a.py"), original);
}
