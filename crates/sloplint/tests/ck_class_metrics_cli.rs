//! End-to-end tests for the CK class metrics over the real built binary: WMC (weighted
//! methods per class) and DIT (first-party depth of inheritance tree). Run against a committed
//! two-module fixture whose inheritance chain (`Unit -> Circle -> Shape`) crosses a file
//! boundary, so the test also pins the project-wide DIT resolution and the
//! third-party-base-is-invisible under-count.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ck_class_metrics")
}

/// Run `sloplint metrics . --format <format>` from *inside* the fixture dir, so the classified
/// paths are project-relative (`base.py`, `shapes.py`) and count as production. Running from the
/// repo root would put a `tests/fixtures/` ancestor in every path and classify the fixture as
/// test code, emptying the production panel/feed these assertions read.
fn run(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(fixture())
        .args(["metrics", ".", "--format", format])
        .output()
        .expect("failed to run sloplint binary");
    assert_eq!(
        output.status.code(),
        Some(0),
        "metrics exits 0 without a gate"
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Parse the JSONL class feed into rows keyed by class name.
fn class_rows() -> HashMap<String, Value> {
    run("classes")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).expect("each class row is valid JSON");
            (row["class"].as_str().unwrap().to_string(), row)
        })
        .collect()
}

#[test]
fn classes_feed_reports_wmc_and_first_party_dit() {
    let rows = class_rows();

    // WMC = sum of the methods' cyclomatic complexity.
    assert_eq!(
        rows["Shape"]["wmc"], 4,
        "area (1) + describe (if + and = 3)"
    );
    assert_eq!(rows["Circle"]["wmc"], 2, "__init__ (1) + area (1)");
    assert_eq!(
        rows["Unit"]["wmc"], 3,
        "area override: for + if over base 1"
    );

    // DIT counts first-party hops only; `object` and third-party bases terminate the chain.
    assert_eq!(
        rows["Shape"]["dit"], 0,
        "Shape's only base is object → root"
    );
    assert_eq!(rows["Circle"]["dit"], 1, "Circle -> Shape (cross-file)");
    assert_eq!(rows["Unit"]["dit"], 2, "Unit -> Circle -> Shape");
    assert_eq!(rows["Panel"]["dit"], 0, "Widget is third-party → invisible");

    // CBO: distinct first-party classes coupled to. Shape couples to nothing first-party;
    // Circle -> Shape (base, cross-file); Unit -> Circle (base); Panel's only base Widget is
    // third-party → 0. `range(...)` in Unit is not a first-party class, so it doesn't count.
    assert_eq!(rows["Shape"]["cbo"], 0);
    assert_eq!(rows["Circle"]["cbo"], 1, "base Shape");
    assert_eq!(rows["Unit"]["cbo"], 1, "base Circle");
    assert_eq!(rows["Panel"]["cbo"], 0, "Widget is third-party");

    // RFC = |own methods ∪ distinct invoked callees|. Own-method calls (Shape.describe ->
    // self.area()) fold back into the method set; free/builtin calls (Unit.area -> range) count.
    assert_eq!(
        rows["Shape"]["rfc"], 2,
        "{{area, describe}}; self.area() folds in"
    );
    assert_eq!(rows["Circle"]["rfc"], 2, "{{__init__, area}}; no calls");
    assert_eq!(rows["Unit"]["rfc"], 2, "{{area, range}}");
    assert_eq!(rows["Panel"]["rfc"], 1, "{{render}}; no calls");
}

#[test]
fn json_reports_wmc_and_dit_aggregates() {
    let value: Value =
        serde_json::from_str(&run("json")).expect("metrics --format json is valid JSON");
    // The fixture is production code; its panel lives under `profiles.production`.
    let prod = &value["profiles"]["production"];

    assert_eq!(prod["classes"], 4, "Shape, Circle, Unit, Panel");
    // Heaviest class is Shape (WMC 4); deepest is Unit (DIT 2).
    assert_eq!(prod["max_wmc"], 4);
    assert_eq!(prod["max_dit"], 2);
    // avg_dit = (0 + 1 + 2 + 0) / 4 = 0.75.
    let avg_dit = prod["avg_dit"].as_f64().unwrap();
    assert!((avg_dit - 0.75).abs() < 1e-9, "avg_dit = {avg_dit}");
    // avg_wmc = (4 + 2 + 3 + Panel's 1) / 4 = 2.5.
    let avg_wmc = prod["avg_wmc"].as_f64().unwrap();
    assert!((avg_wmc - 2.5).abs() < 1e-9, "avg_wmc = {avg_wmc}");

    // CBO: Circle and Unit each couple to one first-party base; Shape and Panel to none.
    assert_eq!(prod["max_cbo"], 1);
    let avg_cbo = prod["avg_cbo"].as_f64().unwrap();
    assert!((avg_cbo - 0.5).abs() < 1e-9, "avg_cbo = {avg_cbo}"); // (0+1+1+0)/4
    assert_eq!(prod["cbo_risk"]["low"], 4, "all four classes are ≤4 (low)");

    // RFC: Shape/Circle/Unit each respond to 2, Panel to 1. max 2; mean (2+2+2+1)/4 = 1.75.
    assert_eq!(prod["max_rfc"], 2);
    let avg_rfc = prod["avg_rfc"].as_f64().unwrap();
    assert!((avg_rfc - 1.75).abs() < 1e-9, "avg_rfc = {avg_rfc}");
    assert_eq!(prod["rfc_risk"]["low"], 4, "all four classes are ≤20 (low)");
}
