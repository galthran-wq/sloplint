//! Test-support for metric snapshots.
//!
//! Like Ruff (and `sloplint_linter`), metric tests run over a Python fixture under
//! `resources/test/fixtures/<category>/<name>.py` and snapshot a deterministic rendering of the
//! computed metric with `insta`, instead of hand-writing assertions inside the source module.
//! Regenerate snapshots with `cargo insta review` (or `INSTA_UPDATE=always cargo test`).

use std::fs;

/// Read the fixture at `resources/test/fixtures/<rel>` (e.g. `"cohesion/cohesion.py"`).
pub fn fixture_source(rel: &str) -> String {
    let path = format!(
        "{}/resources/test/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        rel
    );
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading fixture {path}: {e}"))
}
