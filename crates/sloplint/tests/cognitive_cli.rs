//! End-to-end test for the SonarSource cognitive-complexity metric (issue #11), running the
//! real built binary over a committed Python fixture and reading `metrics --format json`. It
//! pins the headline behavior: a flat `match` scores far below a nested `if` tangle.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/cognitive.py")
}

#[test]
fn reports_cognitive_complexity_penalizing_nesting() {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args(["metrics", fixture().to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run sloplint binary");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("metrics --format json must be valid JSON ({e}):\n{stdout}"));

    // The fixture is production code; its panel lives under `profiles.production` (#96).
    let prod = &value["profiles"]["production"];
    assert_eq!(prod["functions"], 2);
    // The nested `tangle` (cognitive 6) dominates; the flat `classify` match is only 1.
    assert_eq!(prod["max_cognitive"], 6, "json: {value}");
}
