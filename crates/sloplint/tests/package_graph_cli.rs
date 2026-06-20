//! End-to-end tests for `sloplint metrics --format packages` and the per-project import-graph
//! rollup in `--format json` (issue #65), run against a committed multi-package Python fixture
//! tree with a known import structure (including a `proj` ↔ `proj.sub` package cycle).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/package_graph")
}

/// Run `sloplint metrics . --format <format>` from *inside* the fixture dir, so the classified
/// paths are project-relative (`proj/a.py`, …) and count as production. Running from the repo
/// root would put a `tests/fixtures/` ancestor in every path and classify the whole fixture as
/// test code (#96), emptying the production import graph this feed reports. Module names are
/// unaffected — `module_name` derives the package root from the `__init__.py` walk, not the cwd.
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

/// Parse the JSONL package feed into rows keyed by package name.
fn package_rows() -> std::collections::HashMap<String, Value> {
    run("packages")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).expect("each package row is valid JSON");
            (row["package"].as_str().unwrap().to_string(), row)
        })
        .collect()
}

#[test]
fn packages_feed_aggregates_modules_and_first_party_coupling() {
    let rows = package_rows();

    // Three packages: the two real packages plus the root `.` for the top-level module.
    assert_eq!(rows.len(), 3, "rows: {:?}", rows.keys().collect::<Vec<_>>());

    // `proj` holds __init__, a, b, c and imports the `proj.sub` package; the cross-package edge
    // from proj.sub.helper back into proj.b makes it a dependency cycle, visible on both rows.
    // The cycle puts both packages at instability 0.5 (Ce = Ca = 1).
    let proj = &rows["proj"];
    assert_eq!(proj["modules"], 4);
    assert_eq!(proj["imports"], serde_json::json!(["proj.sub"]));
    assert_eq!(proj["imported_by"], serde_json::json!(["proj.sub"]));
    assert_eq!(proj["ce"], 1);
    assert_eq!(proj["ca"], 1);
    assert_eq!(proj["instability"], 0.5);
    // loc is the summed physical line count of __init__/a/b/c — non-zero for a package with code.
    assert!(proj["loc"].as_u64().unwrap() > 0, "proj has source lines");

    let sub = &rows["proj.sub"];
    assert_eq!(sub["modules"], 2); // __init__ + helper
    assert_eq!(sub["imports"], serde_json::json!(["proj"]));
    assert_eq!(sub["imported_by"], serde_json::json!(["proj"]));
    assert_eq!(sub["ce"], 1);
    assert_eq!(sub["ca"], 1);
    assert_eq!(sub["instability"], 0.5);

    // Both real packages have a module in the cycle (see the cycle test below); the root does not.
    assert_eq!(proj["in_cycle"], true);
    assert_eq!(sub["in_cycle"], true);

    // The top-level module lands in the root package with no first-party coupling, so Ce+Ca=0
    // and instability is defined as 0.0 rather than NaN.
    let root = &rows["."];
    assert_eq!(root["modules"], 1);
    assert_eq!(root["imports"], serde_json::json!([]));
    assert_eq!(root["imported_by"], serde_json::json!([]));
    assert_eq!(root["ce"], 0);
    assert_eq!(root["ca"], 0);
    assert_eq!(root["instability"], 0.0);
    assert_eq!(root["in_cycle"], false);
}

#[test]
fn json_rollup_reports_graph_totals() {
    let value: Value = serde_json::from_str(&run("json")).expect("metrics --format json is valid");
    let packages = &value["packages"];

    // 7 modules (proj: __init__/a/b/c, proj.sub: __init__/helper, top), 3 packages.
    assert_eq!(packages["modules"], 7);
    assert_eq!(packages["packages"], 3);
    // 5 first-party module edges: a->b, a->proj.sub.helper, a->c (TYPE_CHECKING), b->a,
    // helper->b. stdlib `os` is not first-party, so it is not an edge.
    assert_eq!(packages["module_edges"], 5);
    // 2 cross-package edges: proj->proj.sub and proj.sub->proj.
    assert_eq!(packages["package_edges"], 2);
}

#[test]
fn json_rollup_reports_cyclic_tangles() {
    let value: Value = serde_json::from_str(&run("json")).expect("metrics --format json is valid");
    let cycles = &value["packages"]["cycles"];

    // One tangle: proj.a -> proj.b -> proj.a, with proj.sub.helper joining via
    // proj.a -> proj.sub.helper -> proj.b. proj.c is reachable only through a TYPE_CHECKING
    // edge and has no outgoing edge, so it is not in the cycle.
    assert_eq!(cycles["tangles"], 1);
    assert_eq!(cycles["largest_tangle"], 3);
    assert_eq!(cycles["modules_in_cycles"], 3);
    assert_eq!(
        cycles["members"],
        serde_json::json!([["proj.a", "proj.b", "proj.sub.helper"]])
    );
    // The cycle is built from runtime edges, so it survives dropping TYPE_CHECKING-only edges.
    assert_eq!(cycles["runtime_tangles"], 1);
    // 3 of 7 modules participate.
    let pct = cycles["pct_modules_in_cycles"].as_f64().unwrap();
    assert!((pct - 3.0 / 7.0).abs() < 1e-9, "pct = {pct}");
}

#[test]
fn json_rollup_reports_propagation_cost() {
    let value: Value = serde_json::from_str(&run("json")).expect("metrics --format json is valid");
    let pc = value["packages"]["propagation_cost"].as_f64().unwrap();

    // Reachability (incl. self) over the 7 modules: the 3 cycle members each reach the 4-node
    // {a, b, c, helper} set; proj/proj.sub/c/top each reach only themselves.
    // (4*3 + 1*4) / 7^2 = 16/49.
    assert!((pc - 16.0 / 49.0).abs() < 1e-9, "propagation_cost = {pc}");
}

#[test]
fn json_rollup_reports_modularity() {
    let value: Value = serde_json::from_str(&run("json")).expect("metrics --format json is valid");
    let m = &value["packages"]["modularity"];

    // Three declared packages (proj, proj.sub, root `.`).
    assert_eq!(m["communities_declared"], 3);

    // The declared partition scores *negative* modularity: proj.sub.helper is more coupled to
    // proj (imports proj.b, imported by proj.a) than to its own proj.sub package, so the declared
    // boundaries are worse than random. Q = (6/10 - (8/10)^2) + (-(2/10)^2) = -0.08.
    let q_declared = m["q_declared"].as_f64().unwrap();
    assert!(
        (q_declared + 0.08).abs() < 1e-9,
        "q_declared = {q_declared}"
    );

    // Louvain recovers the connected component as one community (Q = 0), beating the declared
    // partition — so the gap is positive, flagging the mismatch.
    assert_eq!(m["q_detected"], 0.0);
    let gap = m["gap"].as_f64().unwrap();
    assert!((gap - 0.08).abs() < 1e-9, "gap = {gap}");
}
