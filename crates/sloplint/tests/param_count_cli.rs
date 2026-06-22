//! End-to-end tests for the parameter-count distribution: Long-Parameter-List prevalence
//! (caller-facing function arity) in the metrics aggregate — the fourth size/arity tier family,
//! mirroring the function `cyclomatic_risk`, class `wmc_risk`, and module `module_size_risk`.
//! Exercises the real binary in JSON, text, and the GitHub markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A module-level function `name` taking `arity` plain parameters (no receiver).
fn func(name: &str, arity: usize) -> String {
    let params: Vec<String> = (0..arity).map(|i| format!("p{i}")).collect();
    format!("def {name}({}):\n    return 0\n", params.join(", "))
}

/// Write functions spanning every arity band into a temp dir and run `sloplint metrics m.py
/// <extra>` from inside it. Returns (stdout, code).
fn run(tag: &str, extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_arity_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Arities: 3 (low ≤4), 5 (moderate 5–6), 8 (high 7–10), 12 (very high >10).
    let module = format!(
        "{}{}{}{}",
        func("a", 3),
        func("b", 5),
        func("c", 8),
        func("d", 12),
    );
    std::fs::write(dir.join("m.py"), module).unwrap();
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

#[test]
fn json_reports_arity_bands_and_p95() {
    let (stdout, code) = run("json", &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["params"]["max"], 12, "the 12-param function");
    // Exactly one function in each band — the spread the mean would flatten.
    let risk = &prod["param_count_risk"];
    assert_eq!(risk["low"], 1, "arity 3");
    assert_eq!(risk["moderate"], 1, "arity 5");
    assert_eq!(risk["high"], 1, "arity 8");
    assert_eq!(risk["very_high"], 1, "arity 12");
    // p95 (nearest-rank over [3, 5, 8, 12]) lands on the widest signature.
    assert_eq!(prod["params"]["p95"], 12);
}

#[test]
fn function_feed_reports_caller_facing_arity() {
    // The per-function feed carries `arity` alongside raw `params`; for a method, arity drops the
    // self receiver.
    let dir = std::env::temp_dir().join(format!("sloplint_arity_feed_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("m.py"),
        "class C:\n    def method(self, x, y):\n        return x\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(&dir)
        .args(["metrics", "m.py", "--format", "functions"])
        .output()
        .expect("run sloplint");
    let row: Value = serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim())
        .expect("one function row");
    assert_eq!(row["function"], "method");
    assert_eq!(row["params"], 3, "raw params include self");
    assert_eq!(row["arity"], 2, "arity excludes the self receiver");
}

#[test]
fn text_panel_surfaces_the_arity_bands() {
    let (stdout, code) = run("text", &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("arity bands         low 1 / moderate 1 / high 1 / very high 1"),
        "text panel shows the band counts:\n{stdout}"
    );
}

#[test]
fn github_markdown_has_the_param_table() {
    let (stdout, code) = run("github", &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("**Parameter count**"),
        "markdown has the parameter block:\n{stdout}"
    );
    assert!(
        stdout.contains("| very high (>10) | 1 |"),
        "markdown table counts the long-parameter-list function:\n{stdout}"
    );
}
