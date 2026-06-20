//! End-to-end tests for the abstractness / distance-from-main-sequence columns of
//! `sloplint metrics --format packages` (issue #70), run against a committed fixture with three
//! packages deliberately placed on and off Martin's "main sequence":
//!
//! - `iface` — two pure interfaces (an `abc.ABC` + a `typing.Protocol`): fully abstract, depended
//!   on but depending on nothing → A=1, I=0, on the main sequence (D=0).
//! - `impl`  — one concrete class importing both other packages: A=0, I=1, also on the sequence.
//! - `core`  — one concrete class that is depended on but imports nothing: A=0, I=0 → the "zone of
//!   pain" corner, D=1.
//! - `mixed` — one abstract + one concrete class, no coupling: a fractional A=0.5 (and D=0.5)
//!   computed end-to-end from real source.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/package_abstractness")
}

/// Parse the JSONL package feed into rows keyed by package name.
fn package_rows() -> std::collections::HashMap<String, Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_sloplint"))
        .args([
            "metrics",
            fixture().to_str().unwrap(),
            "--format",
            "packages",
        ])
        .output()
        .expect("failed to run sloplint binary");
    assert_eq!(
        output.status.code(),
        Some(0),
        "metrics exits 0 without a gate"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).expect("each package row is valid JSON");
            (row["package"].as_str().unwrap().to_string(), row)
        })
        .collect()
}

#[test]
fn abstractness_and_distance_span_the_main_sequence() {
    let rows = package_rows();
    assert_eq!(rows.len(), 4, "rows: {:?}", rows.keys().collect::<Vec<_>>());

    // iface: Shape (abc.ABC + @abstractmethod) and Renderer (typing.Protocol) are both abstract,
    // so A=1. It is imported by `impl` but imports nothing first-party, so I=0 → ideal abstract
    // + stable, exactly on the main sequence (D=0).
    let iface = &rows["iface"];
    assert_eq!(iface["classes"], 2);
    assert_eq!(iface["abstract_classes"], 2);
    assert_eq!(iface["abstractness"], 1.0);
    assert_eq!(iface["instability"], 0.0);
    assert_eq!(iface["distance"], 0.0);

    // impl: one concrete class (subclasses Shape but has real method bodies), A=0; it imports both
    // other packages (I=1) → ideal concrete + unstable, also on the sequence (D=0).
    let imp = &rows["impl"];
    assert_eq!(imp["classes"], 1);
    assert_eq!(imp["abstract_classes"], 0);
    assert_eq!(imp["abstractness"], 0.0);
    assert_eq!(imp["instability"], 1.0);
    assert_eq!(imp["distance"], 0.0);

    // core: one concrete class, depended on by `impl` but importing nothing → A=0, I=0. Concrete
    // *and* heavily depended on is the "zone of pain": maximal distance (D=1).
    let core = &rows["core"];
    assert_eq!(core["classes"], 1);
    assert_eq!(core["abstractness"], 0.0);
    assert_eq!(core["instability"], 0.0);
    assert_eq!(core["distance"], 1.0);

    // mixed: one abstract + one concrete class → a *fractional* A=0.5 counted from real source
    // (one module, two classes). No coupling (I=0), so D = |0.5 + 0 − 1| = 0.5.
    let mixed = &rows["mixed"];
    assert_eq!(mixed["classes"], 2);
    assert_eq!(mixed["abstract_classes"], 1);
    assert_eq!(mixed["abstractness"], 0.5);
    assert_eq!(mixed["distance"], 0.5);
}
