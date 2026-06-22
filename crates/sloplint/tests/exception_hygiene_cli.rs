//! End-to-end tests for the exception-handling hygiene aggregate: broad-except and
//! silent-swallow rates over every `except` handler, surfaced in `metrics` as a cohort
//! discriminator default Ruff can't aggregate. Exercises the real binary in JSON, text, and the
//! GitHub markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Write `body` as `m.py` in a temp dir and run `sloplint metrics m.py <extra>` from inside it.
fn run(tag: &str, body: &str, extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_exc_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.py"), body).unwrap();
    let mut args = vec!["metrics", "m.py"];
    args.extend_from_slice(extra);
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&dir)
        .args(&args)
        .output()
        .expect("run sloplint");
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

/// Four handlers: broad+log, narrow, bare+pass, broad-tuple + ellipsis.
const MIXED: &str = "\
def a():
    try:
        risky()
    except Exception:
        log()

def b():
    try:
        risky()
    except ValueError:
        recover()

def c():
    try:
        risky()
    except:
        pass

def d():
    try:
        risky()
    except (KeyError, BaseException):
        ...
";

#[test]
fn json_reports_exception_hygiene() {
    let (stdout, code) = run("json", MIXED, &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let exc = &value["profiles"]["production"]["exception_handling"];

    assert_eq!(exc["handlers"], 4);
    assert_eq!(exc["bare"], 1, "the bare `except:`");
    assert_eq!(exc["broad"], 2, "Exception + the (…, BaseException) tuple");
    assert_eq!(exc["swallow"], 2, "the `pass` and the `...`");
    // 2 of 4 handlers broad / swallow.
    assert_eq!(exc["broad_rate"].as_f64().unwrap(), 0.5);
    assert_eq!(exc["swallow_rate"].as_f64().unwrap(), 0.5);
}

#[test]
fn narrow_handlers_score_zero() {
    let clean = "\
def f():
    try:
        risky()
    except ValueError as e:
        raise RuntimeError from e
    except KeyError:
        return None
";
    let (stdout, code) = run("clean", clean, &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    let exc = &value["profiles"]["production"]["exception_handling"];
    assert_eq!(exc["handlers"], 2);
    assert_eq!(exc["broad"], 0);
    assert_eq!(exc["swallow"], 0);
    assert_eq!(exc["broad_rate"].as_f64().unwrap(), 0.0);
    assert_eq!(exc["swallow_rate"].as_f64().unwrap(), 0.0);
}

#[test]
fn text_and_markdown_surface_exception_hygiene() {
    let (text, code) = run("text", MIXED, &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        text.contains("except broad/swallow"),
        "text panel shows the exception line:\n{text}"
    );

    let (md, code) = run("md", MIXED, &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        md.contains("**Exception handling**") && md.contains("broad-except rate"),
        "markdown has the exception block:\n{md}"
    );
}
