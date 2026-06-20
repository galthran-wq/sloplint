//! End-to-end tests for the production-vs-test partition (#96), over the real built binary and a
//! committed mini-project: a production package `app/` plus a nested `tests/` dir. Pins that one
//! run yields both panels, that `--scope` filters the per-unit feeds, and — crucially — that a
//! test importing production does not manufacture coupling in the production import graph.
//!
//! Run from *inside* the fixture so the classified paths are project-relative (`app/core.py`,
//! `tests/test_core.py`) — the same reason `test_proxies_cli` runs relative.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scope_partition")
}

/// Run `sloplint metrics . <extra...>` from inside the fixture dir; return (stdout, exit code).
fn run(extra: &[&str]) -> (String, i32) {
    let mut args = vec!["metrics", "."];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .current_dir(fixture())
        .args(&args)
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

/// The JSONL feed for a `--format {functions,classes,packages}` run, one parsed row per line.
fn rows(extra: &[&str]) -> Vec<Value> {
    let (stdout, code) = run(extra);
    assert_eq!(code, 0, "metrics exits 0 without a gate");
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each row is valid JSON"))
        .collect()
}

#[test]
fn json_emits_production_top_level_and_a_test_panel_in_one_run() {
    let (stdout, code) = run(&["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");

    // Top level is the PRODUCTION panel: app/core.py's `run` + `build`, one class `Engine`.
    assert_eq!(value["functions"], 2, "production: run + build");
    assert_eq!(value["classes"], 1, "production: Engine");

    // The test panel sits beside it — both from the single run.
    assert_eq!(
        value["tests"]["functions"], 2,
        "test: test_runs + test_negative"
    );
    assert_eq!(
        value["tests"]["classes"], 0,
        "no classes in the test module"
    );

    // test_proxies is the whole-project split (always over all files), unaffected by the panels.
    let proxies = &value["test_proxies"];
    assert_eq!(
        proxies["production_files"], 2,
        "app/__init__.py + app/core.py"
    );
    assert_eq!(proxies["test_files"], 1, "tests/test_core.py");
    assert_eq!(
        proxies["assertions"], 3,
        "2 in test_runs + 1 in test_negative"
    );
}

#[test]
fn feeds_default_to_production_and_scope_selects_the_partition() {
    // Default: production functions only — no `test_*` rows leak in.
    let names: Vec<String> = rows(&["--format", "functions"])
        .iter()
        .map(|r| r["function"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["run", "build"], "default feed is production-only");

    // --scope tests: only the test functions.
    let test_names: Vec<String> = rows(&["--scope", "tests", "--format", "functions"])
        .iter()
        .map(|r| r["function"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(test_names, ["test_runs", "test_negative"]);

    // --scope all: both partitions.
    assert_eq!(rows(&["--scope", "all", "--format", "functions"]).len(), 4);
}

#[test]
fn test_imports_do_not_manufacture_production_coupling() {
    // tests/test_core.py does `from app.core import build`. Built from production modules only,
    // the `app` package has no first-party importer — the test edge is invisible.
    let prod = rows(&["--format", "packages"]);
    let app = prod
        .iter()
        .find(|r| r["package"] == "app")
        .expect("app package in the production feed");
    assert_eq!(
        app["imported_by"],
        serde_json::json!([]),
        "no production module imports app, so Ca is 0 — the test import must not count"
    );
    assert_eq!(app["ca"], 0);

    // Built over all files, the test import DOES surface as coupling — proving the production
    // feed's cleanliness is the partition's doing, not an artifact of the fixture. (`tests/` has
    // no `__init__.py`, so the test module's package is the root `.`.)
    let all = rows(&["--scope", "all", "--format", "packages"]);
    let app_all = all.iter().find(|r| r["package"] == "app").unwrap();
    assert_eq!(
        app_all["imported_by"],
        serde_json::json!(["."]),
        "with tests included, the test module's package imports app"
    );
    assert_eq!(app_all["ca"], 1);
}

#[test]
fn text_view_defaults_to_production_and_all_shows_both_panels() {
    let (default, code) = run(&["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        default.contains("sloplint metrics — production"),
        "default text is the production panel:\n{default}"
    );
    assert!(
        !default.contains("sloplint metrics — tests"),
        "default text does not include the test panel:\n{default}"
    );
    // The test proxies (whole-project split) ride along regardless of scope.
    assert!(
        default.contains("test:code ratio"),
        "proxies present:\n{default}"
    );

    let (all, code) = run(&["--scope", "all", "--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        all.contains("sloplint metrics — production") && all.contains("sloplint metrics — tests"),
        "--scope all shows both panels:\n{all}"
    );
}
