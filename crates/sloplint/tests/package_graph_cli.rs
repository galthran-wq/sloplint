//! End-to-end tests for `sloplint metrics --format packages` and the per-project import-graph
//! rollup in `--format json` (issue #65), run against a committed multi-package Python fixture
//! tree with a known import structure (including a `proj` ↔ `proj.sub` package cycle).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/package_graph")
}

/// Run `sloplint metrics <fixture> --format <format>` and return stdout.
fn run(format: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(["metrics", fixture().to_str().unwrap(), "--format", format])
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
    let proj = &rows["proj"];
    assert_eq!(proj["modules"], 4);
    assert_eq!(proj["imports"], serde_json::json!(["proj.sub"]));
    assert_eq!(proj["imported_by"], serde_json::json!(["proj.sub"]));
    assert_eq!(proj["efferent"], 1);
    assert_eq!(proj["afferent"], 1);

    let sub = &rows["proj.sub"];
    assert_eq!(sub["modules"], 2); // __init__ + helper
    assert_eq!(sub["imports"], serde_json::json!(["proj"]));
    assert_eq!(sub["imported_by"], serde_json::json!(["proj"]));

    // The top-level module lands in the root package with no first-party coupling.
    let root = &rows["."];
    assert_eq!(root["modules"], 1);
    assert_eq!(root["imports"], serde_json::json!([]));
    assert_eq!(root["imported_by"], serde_json::json!([]));
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
