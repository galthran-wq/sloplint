//! End-to-end tests for the built-in `generated` profile, over the real built binary
//! and a committed mini-project: a human `src/app.py`, an OpenAPI-generated `src/openapi_client.py`
//! (detected by its header marker, *not* its path), and a protobuf `proto/thing_pb2.py` (detected
//! by the `_pb2.py` path convention). Pins that generated code is segregated into its own panel and
//! kept out of the production aggregates by default.
//!
//! Run from *inside* the fixture so the classified paths are project-relative, like the other
//! profile/scope e2e tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generated_profile")
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

#[test]
fn json_segregates_generated_from_production() {
    let (stdout, code) = run(&["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).expect("valid JSON");

    // Production: only the human file — `add`. The generated code is NOT here.
    let prod = &value["profiles"]["production"];
    assert_eq!(prod["functions"], 1, "production: just app.add");
    assert_eq!(prod["classes"], 0, "production: no classes");

    // Generated: the OpenAPI client (DefaultApi.get + .post, one class) detected by *marker*, plus
    // the protobuf helper detected by *path* — 3 functions, 1 class.
    let generated = &value["profiles"]["generated"];
    assert_eq!(
        generated["functions"], 3,
        "generated: DefaultApi.get + .post + pb2 helper"
    );
    assert_eq!(generated["classes"], 1, "generated: DefaultApi");
}

#[test]
fn test_proxies_exclude_generated_from_production_loc() {
    let (stdout, code) = run(&["--format", "json"]);
    assert_eq!(code, 0);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    let proxies = &value["test_proxies"];
    // Only the human production file counts toward the test:code denominator — the two generated
    // files must not inflate production LoC.
    assert_eq!(
        proxies["production_files"], 1,
        "only src/app.py is production"
    );
    assert_eq!(proxies["test_files"], 0);
}

#[test]
fn default_scope_reports_production_only_generated_under_its_own_scope() {
    // Default scope is `production`: one function, and no `generated` panel header.
    let (prod, code) = run(&["--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        prod.contains("production"),
        "default scope shows production:\n{prod}"
    );
    assert!(
        !prod.contains("metrics — generated"),
        "default scope must NOT show the generated panel:\n{prod}"
    );

    // `--scope generated` reports the generated panel (3 functions).
    let (gen, code) = run(&["--scope", "generated", "--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        gen.contains("metrics — generated"),
        "--scope generated shows the generated panel:\n{gen}"
    );

    // `--scope all` shows every panel side by side, generated included.
    let (all, code) = run(&["--scope", "all", "--format", "text"]);
    assert_eq!(code, 0);
    assert!(
        all.contains("metrics — generated") && all.contains("metrics — production"),
        "--scope all shows both generated and production:\n{all}"
    );
}
