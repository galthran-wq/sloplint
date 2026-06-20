//! End-to-end tests for NOC (Number of Children, #113): inheritance *breadth* — direct
//! first-party subclasses per class — in the metrics aggregate, the CK pair of DIT depth.
//! Exercises the real binary over a temp project, in the class feed, JSON, text, and markdown.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Write a module with a hub base extended by `children` subclasses (plus the hub itself) into a
/// temp dir and run `sloplint metrics m.py <extra>` from inside it. Returns (stdout, code).
fn run(tag: &str, children: usize, extra: &[&str]) -> (String, i32) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("sloplint_noc_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut src = String::from("class Hub:\n    pass\n");
    for i in 0..children {
        src.push_str(&format!("class C{i}(Hub):\n    pass\n"));
    }
    std::fs::write(dir.join("m.py"), src).unwrap();
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
fn class_feed_reports_noc() {
    // Hub with 7 children → Hub.noc = 7, each child noc = 0.
    let (stdout, code) = run("feed", 7, &["--format", "classes"]);
    assert_eq!(code, 0);
    let rows: std::collections::HashMap<String, Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let r: Value = serde_json::from_str(l).unwrap();
            (r["class"].as_str().unwrap().to_string(), r)
        })
        .collect();
    assert_eq!(rows["Hub"]["noc"], 7, "seven direct subclasses");
    assert_eq!(rows["C0"]["noc"], 0, "a leaf has no children");
}

#[test]
fn json_reports_noc_distribution() {
    // Hub with 7 children: Hub is a high-band base (6–20), the 7 leaves are low.
    let (stdout, code) = run("json", 7, &["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["max_noc"], 7, "the hub base");
    let risk = &prod["noc_risk"];
    assert_eq!(risk["low"], 7, "the seven leaves (NOC 0)");
    assert_eq!(risk["high"], 1, "Hub (NOC 7 → high band)");
    assert_eq!(risk["moderate"], 0);
    assert_eq!(risk["very_high"], 0);
    // p95 (nearest-rank over eight values [0×7, 7]) lands on the hub.
    assert_eq!(prod["p95_noc"], 7);
}

#[test]
fn text_panel_surfaces_the_noc_bands() {
    let (stdout, code) = run("text", 7, &["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("NOC bands           low 7 / moderate 0 / high 1 / very high 0"),
        "text panel shows the band counts:\n{stdout}"
    );
}

#[test]
fn github_markdown_has_the_noc_table() {
    // 21 children → Hub lands in the very-high band (>20).
    let (stdout, code) = run("md", 21, &["--format", "github"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("**Inheritance breadth (NOC)**"),
        "markdown has the NOC block:\n{stdout}"
    );
    assert!(
        stdout.contains("| very high (>20) | 1 |"),
        "markdown table counts the high-leverage base:\n{stdout}"
    );
}
