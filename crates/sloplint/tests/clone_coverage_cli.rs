//! End-to-end tests for the project-level clone-coverage metric (issue #9), running the real
//! built binary over a committed Python fixture with a known duplicate pair. Covers the JSON
//! metric, and the `--max-clone-coverage` CI gate's exit code.
//!
//! The binary discovers `[clone]` config by walking up from its working directory, so these
//! tests run it with `current_dir` set to a fresh temp dir OUTSIDE the repo — otherwise it
//! would pick up the repo's own `sloplint.toml` and the numbers would stop being defaults.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clone_coverage.py")
}

/// A unique temp directory outside the repo tree (so config discovery finds no `sloplint.toml`).
fn clean_cwd(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sloplint_cov_e2e_{}_{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `sloplint metrics <fixture> <extra...>` from a clean cwd; return (stdout, exit code).
fn run_metrics(tag: &str, extra: &[&str]) -> (String, i32) {
    let fixture = fixture();
    let mut args = vec!["metrics", fixture.to_str().unwrap()];
    args.extend_from_slice(extra);
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(&args)
        .current_dir(clean_cwd(tag))
        .output()
        .expect("failed to run sloplint binary");
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn json_reports_clone_coverage_with_config() {
    let (stdout, code) = run_metrics("json", &["--format", "json"]);
    assert_eq!(code, 0, "no gate -> exit 0");
    let value: Value = serde_json::from_str(&stdout).expect("metrics --format json is valid JSON");
    let cov = &value["clone_coverage"];

    assert_eq!(cov["total_functions"], 3);
    assert_eq!(cov["clone_functions"], 2);
    assert_eq!(cov["clone_pairs"], 1);
    let funcs = cov["coverage_funcs"].as_f64().unwrap();
    assert!((funcs - 2.0 / 3.0).abs() < 1e-6, "coverage_funcs = {funcs}");
    assert!(cov["coverage_lines"].as_f64().unwrap() > 0.0);
    // The metric is reported with the [clone] config it was produced under (reproducibility).
    assert_eq!(cov["min_statements"], 3);
    assert!((cov["similarity"].as_f64().unwrap() - 0.85).abs() < 1e-9);
}

#[test]
fn max_clone_coverage_gate_trips_above_ceiling() {
    // 66.7% coverage > a 25% ceiling -> the gate fails.
    let (_stdout, code) = run_metrics("gate_fail", &["--max-clone-coverage", "25"]);
    assert_eq!(code, 1, "exceeding the ceiling exits 1");
}

#[test]
fn max_clone_coverage_gate_passes_below_ceiling() {
    // 66.7% coverage <= an 80% ceiling -> pass.
    let (_stdout, code) = run_metrics("gate_pass", &["--max-clone-coverage", "80"]);
    assert_eq!(code, 0, "under the ceiling exits 0");
}
