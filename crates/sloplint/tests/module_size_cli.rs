//! End-to-end tests for the module-size distribution (#107): god-module prevalence (NLOC per
//! file) in the metrics aggregate — the third leg of the size triad, mirroring the function
//! `cyclomatic_risk` and class `wmc_risk` tiers. Exercises the real binary over a temp project
//! with one module per NLOC band, in JSON, text, and the GitHub markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A module of `lines` trivial statements, so its NLOC equals `lines` exactly (no blanks/comments).
fn module(lines: usize) -> String {
    (0..lines).map(|i| format!("v{i} = {i}\n")).collect()
}

/// Write modules spanning every NLOC band into a temp dir and run `sloplint metrics . <extra>`
/// from inside it (so they're production). Returns (stdout, code).
fn run(tag: &str, extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_mod_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // NLOC by line count: 10 (low ≤250), 300 (moderate 251–500), 700 (high 501–1000), 1500 (>1000).
    std::fs::write(dir.join("tiny.py"), module(10)).unwrap();
    std::fs::write(dir.join("mid.py"), module(300)).unwrap();
    std::fs::write(dir.join("big.py"), module(700)).unwrap();
    std::fs::write(dir.join("huge.py"), module(1500)).unwrap();
    let mut args = vec!["metrics", "."];
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

#[test]
fn json_reports_module_size_bands_and_p95() {
    let (stdout, code) = run("json", &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["module_nloc"]["max"], 1500, "huge.py");
    // Exactly one module in each band — the spread total_loc/avg would hide.
    let risk = &prod["module_size_risk"];
    assert_eq!(risk["low"], 1, "tiny (10)");
    assert_eq!(risk["moderate"], 1, "mid (300)");
    assert_eq!(risk["high"], 1, "big (700)");
    assert_eq!(risk["very_high"], 1, "huge (1500)");
    // p95 (nearest-rank over [10, 300, 700, 1500]) lands on the god-module.
    assert_eq!(prod["module_nloc"]["p95"], 1500);
}

#[test]
fn text_panel_surfaces_the_module_bands() {
    let (stdout, code) = run("text", &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("module NLOC bands   low 1 / moderate 1 / high 1 / very high 1"),
        "text panel shows the band counts:\n{stdout}"
    );
}

#[test]
fn github_markdown_has_the_module_table() {
    let (stdout, code) = run("github", &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("**Module size (NLOC)**"),
        "markdown has the module block:\n{stdout}"
    );
    assert!(
        stdout.contains("| very high (>1000) | 1 |"),
        "markdown table counts the god-module:\n{stdout}"
    );
}
