//! End-to-end tests for the agent-loop integration (issue #50), running the real built
//! binary: `sloplint init` wiring AI-tool hook configs, and `sloplint check --hook` reading an
//! edited path from stdin and reporting back via exit code 2 + stderr.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A fresh temp project dir. `tag` keeps concurrent tests apart. The name avoids the substring
/// "slp" so it can't trip path-sensitive assertions elsewhere.
fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("agentloop-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
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

/// Run with a stdin payload (for `check --hook`).
fn run_with_stdin(project: &Path, args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(project)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn sloplint binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn init_writes_claude_and_cursor_hooks_idempotently() {
    let project = make_project("init");

    let (stdout, _, code) = run(&project, &["init", "--tool", "claude", "--tool", "cursor"]);
    assert_eq!(code, 0, "init should succeed: {stdout}");

    let claude = std::fs::read_to_string(project.join(".claude/settings.json")).unwrap();
    assert!(claude.contains("PostToolUse"), "claude: {claude}");
    assert!(
        claude.contains("sloplint check --hook --format agent"),
        "claude: {claude}"
    );
    assert!(claude.contains("Edit|Write|MultiEdit"), "claude: {claude}");

    let cursor = std::fs::read_to_string(project.join(".cursor/hooks.json")).unwrap();
    assert!(cursor.contains("afterFileEdit"), "cursor: {cursor}");
    assert!(
        cursor.contains("sloplint check --hook --format agent"),
        "cursor: {cursor}"
    );

    // Re-running must not duplicate the hook — the file is byte-identical.
    let (stdout2, _, code2) = run(&project, &["init", "--tool", "claude", "--tool", "cursor"]);
    assert_eq!(code2, 0);
    assert!(stdout2.contains("already wired"), "second run: {stdout2}");
    assert_eq!(
        claude,
        std::fs::read_to_string(project.join(".claude/settings.json")).unwrap()
    );

    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn init_dry_run_writes_nothing() {
    let project = make_project("dryrun");
    let (stdout, _, code) = run(&project, &["init", "--tool", "claude", "--dry-run"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("would write"), "dry run: {stdout}");
    assert!(
        !project.join(".claude/settings.json").exists(),
        "dry run must not create files"
    );
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn init_autodetects_from_markers() {
    let project = make_project("detect");
    // A CLAUDE.md marks a Claude Code repo; no Cursor/Aider markers present.
    std::fs::write(project.join("CLAUDE.md"), "# project\n").unwrap();
    let (stdout, _, code) = run(&project, &["init"]);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("Claude Code"), "detect: {stdout}");
    assert!(
        project.join(".claude/settings.json").exists(),
        "claude config written on autodetect"
    );
    assert!(
        !project.join(".cursor/hooks.json").exists(),
        "cursor not detected -> not configured"
    );
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn init_reports_when_nothing_detected() {
    let project = make_project("nodetect");
    let (stdout, _, code) = run(&project, &["init"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("no AI coding tool detected"), "{stdout}");
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn hook_flags_edited_file_and_exits_two() {
    let project = make_project("hookfires");
    // A banned comment trips SLP010 (Stable, on by default).
    let file = project.join("app.py");
    std::fs::write(&file, "x = 1  # set x to one\n").unwrap();
    std::fs::write(project.join("sloplint.toml"), "select = [\"SLP010\"]\n").unwrap();

    let payload = format!(
        r#"{{"tool_name":"Edit","tool_input":{{"file_path":"{}"}}}}"#,
        file.display()
    );
    let (stdout, stderr, code) = run_with_stdin(&project, &["check", "--hook"], &payload);

    assert_eq!(code, 2, "findings -> exit 2 so the agent sees them");
    assert!(stdout.is_empty(), "hook output goes to stderr, not stdout");
    assert!(stderr.contains("SLP010"), "stderr findings: {stderr}");
    assert!(stderr.contains("app.py:1:"), "agent format: {stderr}");

    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn hook_is_silent_on_clean_file() {
    let project = make_project("hookclean");
    let file = project.join("clean.py");
    std::fs::write(&file, "x = 1\n").unwrap();
    std::fs::write(project.join("sloplint.toml"), "select = [\"SLP010\"]\n").unwrap();

    let payload = format!(r#"{{"tool_input":{{"file_path":"{}"}}}}"#, file.display());
    let (stdout, stderr, code) = run_with_stdin(&project, &["check", "--hook"], &payload);

    assert_eq!(code, 0, "clean file -> exit 0");
    assert!(stdout.is_empty() && stderr.is_empty(), "silent when clean");
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn hook_ignores_non_python_and_missing_paths() {
    let project = make_project("hookskip");
    // A non-Python edit (Cursor afterFileEdit shape) -> nothing to lint, exit 0.
    let (_, _, code) =
        run_with_stdin(&project, &["check", "--hook"], r#"{"file_path":"README.md"}"#);
    assert_eq!(code, 0);
    // A payload with no path at all (e.g. a Bash tool call) -> exit 0.
    let (_, _, code2) = run_with_stdin(&project, &["check", "--hook"], r#"{"tool_name":"Bash"}"#);
    assert_eq!(code2, 0);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn agent_format_is_one_finding_per_line() {
    let project = make_project("agentfmt");
    std::fs::write(project.join("app.py"), "x = 1  # set x to one\n").unwrap();
    std::fs::write(project.join("sloplint.toml"), "select = [\"SLP010\"]\n").unwrap();

    let (stdout, _, code) = run(&project, &["check", "app.py", "--format", "agent"]);
    assert_eq!(code, 1, "findings -> exit 1 for a normal check");
    assert!(stdout.starts_with("app.py:1:"), "agent line: {stdout}");
    assert!(stdout.contains("SLP010"), "agent line: {stdout}");
    assert_eq!(stdout.lines().count(), 1, "one line per finding: {stdout}");
    let _ = std::fs::remove_dir_all(&project);
}
