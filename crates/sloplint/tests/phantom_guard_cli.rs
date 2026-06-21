//! End-to-end tests for SLP210 (phantom security-guard calls/decorators, issue #143), running the
//! real built binary. The rule is preview-gated, so it must be silent without `--preview` and fire
//! with it; verifies positives, the import/local-def/attribute negatives, and `[security] extra`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn make_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("phantomguard-{}-{tag}", std::process::id()));
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

/// SLP210 codes + messages for `path`, parsed from `--format json`.
fn slp210(stdout: &str) -> Vec<String> {
    let value: Value = serde_json::from_str(stdout).expect("valid JSON");
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["code"] == "SLP210")
        .map(|f| f["message"].as_str().unwrap().to_string())
        .collect()
}

const SOURCE: &str = "\
from auth import login_required


def real_guard(token):
    return bool(token)


def handle(token):
    if not validate_token(token):
        raise PermissionError
    return real_guard(token)


@requires_auth
def admin():
    return 1


@login_required
def ok():
    return 1


def via_attr(token):
    return validators.verify_jwt(token)
";

#[test]
fn preview_gated_off_by_default() {
    let project = make_project("gated");
    write(&project, "app.py", SOURCE);
    let (stdout, _) = run(&project, &["check", "app.py", "--format", "json"]);
    assert!(
        slp210(&stdout).is_empty(),
        "SLP210 is preview, silent by default"
    );
}

#[test]
fn flags_undefined_guards_under_preview() {
    let project = make_project("preview");
    write(&project, "app.py", SOURCE);
    let (stdout, code) = run(
        &project,
        &["check", "app.py", "--preview", "--format", "json"],
    );
    let findings = slp210(&stdout);
    assert_eq!(code, 1, "findings → exit 1");
    // validate_token (call) + requires_auth (decorator) are undefined → flagged.
    assert!(
        findings.iter().any(|m| m.contains("`validate_token`")),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|m| m.contains("`@requires_auth`")),
        "{findings:?}"
    );
    // login_required is imported, real_guard is locally defined, verify_jwt is an attribute call —
    // none are flagged.
    assert!(
        !findings.iter().any(|m| m.contains("login_required")),
        "imported guard not flagged: {findings:?}"
    );
    assert!(
        !findings.iter().any(|m| m.contains("real_guard")),
        "{findings:?}"
    );
    assert!(
        !findings.iter().any(|m| m.contains("verify_jwt")),
        "attribute call not flagged: {findings:?}"
    );
    assert_eq!(
        findings.len(),
        2,
        "exactly the two phantom guards: {findings:?}"
    );
}

#[test]
fn extra_dictionary_extends_the_catalog() {
    // `require_tenant` isn't a built-in guard; declaring it in [security] extra makes a call to the
    // undefined name a finding.
    let project = make_project("extra");
    write(
        &project,
        "app.py",
        "def view(req):\n    require_tenant(req)\n    return 1\n",
    );
    let (before, _) = run(
        &project,
        &["check", "app.py", "--preview", "--format", "json"],
    );
    assert!(slp210(&before).is_empty(), "not a built-in guard yet");

    write(
        &project,
        "sloplint.toml",
        "[security]\nextra = [\"require_tenant\"]\n",
    );
    let (after, _) = run(
        &project,
        &["check", "app.py", "--preview", "--format", "json"],
    );
    assert!(
        slp210(&after)
            .iter()
            .any(|m| m.contains("`require_tenant`")),
        "extra catalog entry flagged: {after}"
    );
}
