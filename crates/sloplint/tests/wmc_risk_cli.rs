//! End-to-end tests for the WMC risk-tier distribution (#104): god-class *prevalence* in the
//! metrics aggregate, mirroring the function `cyclomatic_risk` tiers. Exercises the real binary
//! over a temp project with one class per WMC band, in JSON, text, and the GitHub markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// One class with `methods` trivial (cyclomatic-1) methods, so its WMC equals `methods` exactly.
fn klass(name: &str, methods: usize) -> String {
    let mut s = format!("class {name}:\n");
    for i in 0..methods {
        s.push_str(&format!("    def m{i}(self):\n        return {i}\n"));
    }
    s
}

/// Write a module of classes spanning every WMC band into a temp dir and run `sloplint metrics
/// m.py <extra>` from inside it (so the file is production, not test). Returns (stdout, code).
fn run(tag: &str, extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_wmc_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // WMC by method count: 5 (low ≤20), 30 (moderate 21–50), 100 (high 51–200), 250 (very high >200).
    let module = format!(
        "{}{}{}{}",
        klass("Tiny", 5),
        klass("Mid", 30),
        klass("Big", 100),
        klass("Huge", 250),
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
fn json_reports_wmc_band_counts_and_p95() {
    let (stdout, code) = run("json", &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["classes"], 4);
    assert_eq!(prod["max_wmc"], 250, "Huge");
    // Exactly one class in each band — the spread max_wmc alone would hide.
    let risk = &prod["wmc_risk"];
    assert_eq!(risk["low"], 1, "Tiny (5)");
    assert_eq!(risk["moderate"], 1, "Mid (30)");
    assert_eq!(risk["high"], 1, "Big (100)");
    assert_eq!(risk["very_high"], 1, "Huge (250)");
    // p95 (nearest-rank over [5, 30, 100, 250]) lands on the heaviest class.
    assert_eq!(prod["p95_wmc"], 250);
}

#[test]
fn text_panel_surfaces_the_wmc_bands() {
    let (stdout, code) = run("text", &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("WMC bands           low 1 / moderate 1 / high 1 / very high 1"),
        "text panel shows the band counts:\n{stdout}"
    );
    assert!(
        stdout.contains("avg/p95/max WMC"),
        "text panel shows the WMC headline:\n{stdout}"
    );
}

#[test]
fn github_markdown_has_the_wmc_table() {
    let (stdout, code) = run("github", &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("**Class weight (WMC)**"),
        "markdown has the WMC block:\n{stdout}"
    );
    assert!(
        stdout.contains("| very high (>200) | 1 |"),
        "markdown WMC table counts the god-class:\n{stdout}"
    );
}
