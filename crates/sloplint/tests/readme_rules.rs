//! Documentation single-source guard: every shipped rule must be documented in the README rules
//! table, so a new rule can't land without a user-facing entry. (The metadata source of truth is
//! the rule's doc-comment via `ViolationMetadata`; this keeps the README in step with it.)

use std::path::Path;

use sloplint_linter::registry::Registry;

#[test]
fn every_shipped_rule_appears_in_the_readme_table() {
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .expect("read README.md");
    for code in Registry::shipped().codes() {
        assert!(
            readme.contains(code),
            "rule {code} is not documented in the README rules table (add a row for it)"
        );
    }
}
