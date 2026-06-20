//! End-to-end test for `sloplint churn` (issue #18), running the real built binary over a real
//! temporary git repository. There is no Python *feature* to exercise here (churn is a
//! git-history signal), so the "real workings" test builds an actual repo with a known history
//! — a file rewritten wholesale commit after commit, plus a stable file — and asserts the
//! reported volatility and the per-PR churn-vs-delta anomaly via `--format json`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Run a git command in `repo`, asserting success.
fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        // Deterministic identity + no signing, so the test is hermetic.
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn write(repo: &Path, name: &str, contents: &str) {
    std::fs::write(repo.join(name), contents).unwrap();
}

fn commit(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
}

/// A repo where `hot.py` is rewritten in every commit while `stable.py` is written once.
/// `name` keeps each test's repo isolated (tests run in parallel).
fn build_repo(name: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("churn_repo_{name}"));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);

    write(&repo, "stable.py", "VALUE = 1\n");
    write(&repo, "hot.py", "def go():\n    return 1\n");
    commit(&repo, "init");

    // Rewrite hot.py wholesale several times (high churn, near-zero net delta).
    for i in 2..=5 {
        write(
            &repo,
            "hot.py",
            &format!("def go():\n    # revision {i}\n    return {i}\n"),
        );
        commit(&repo, &format!("rewrite {i}"));
    }
    repo
}

fn run_churn(repo: &Path, extra: &[&str]) -> Value {
    let mut args = vec![
        "churn",
        "--repo",
        repo.to_str().unwrap(),
        "--format",
        "json",
    ];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .output()
        .expect("run sloplint churn");
    assert!(
        output.status.success(),
        "churn failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON ({e}): {stdout}"))
}

#[test]
fn reports_volatility_with_hot_file_on_top() {
    let repo = build_repo("volatility");
    let value = run_churn(&repo, &[]);

    // hot.py changed 5×, stable.py once -> positive, uneven volatility.
    let cv = value["volatility"]["cv"].as_f64().unwrap();
    assert!(cv > 0.0, "expected non-zero volatility, got {cv}");

    let top = value["volatility"]["top_churn"].as_array().unwrap();
    assert_eq!(top[0]["path"], "hot.py");
    assert_eq!(top[0]["commits"], 5);
}

#[test]
fn flags_in_place_rewrite_as_churn_vs_delta_anomaly() {
    let repo = build_repo("anomaly");
    // Compare the first commit against HEAD: hot.py was rewritten repeatedly but its net delta
    // is tiny, while stable.py never moved.
    let first = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .output()
        .expect("rev-list");
    let base = String::from_utf8(first.stdout).unwrap().trim().to_string();

    let value = run_churn(
        &repo,
        &[
            "--base",
            &base,
            "--min-churn",
            "3",
            "--anomaly-ratio",
            "2.0",
        ],
    );
    let files = value["anomalies"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "anomalies: {files:#?}");
    assert_eq!(files[0]["path"], "hot.py");
    assert!(files[0]["ratio"].as_f64().unwrap() >= 2.0);
}

#[test]
fn renamed_and_unicode_paths_stay_plain() {
    // Regression: renames must not produce `old => new` garbage paths, and a non-ASCII
    // filename must survive (git quotes such paths unless `core.quotePath=false`).
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("churn_repo_rename");
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    // Force rename detection on, to prove `--no-renames` overrides it.
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "diff.renames", "true"]);

    write(&repo, "café.py", "VALUE = 1\n");
    write(&repo, "old.py", "def go():\n    return 1\n");
    commit(&repo, "init");
    git(&repo, &["mv", "old.py", "new.py"]);
    commit(&repo, "rename old -> new");

    let value = run_churn(&repo, &[]);
    let paths: Vec<&str> = value["volatility"]["top_churn"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(
        paths.iter().all(|p| !p.contains("=>")),
        "no rename-arrow garbage paths: {paths:?}"
    );
    assert!(
        paths.contains(&"café.py"),
        "unicode path present: {paths:?}"
    );
    assert!(
        paths.contains(&"new.py"),
        "renamed-to path present: {paths:?}"
    );
}

#[test]
fn empty_repo_reports_zero_without_failing() {
    // A brand-new repo with no commits must not fail the Action.
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("churn_repo_empty");
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);

    let value = run_churn(&repo, &[]);
    assert_eq!(value["volatility"]["cv"].as_f64().unwrap(), 0.0);
    assert!(value["volatility"]["top_churn"]
        .as_array()
        .unwrap()
        .is_empty());
}
