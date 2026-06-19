//! Test-quality rules.
//!
//! - `SLP070` assertion-free / tautological tests (preview — heuristic).
//!
//! These target test code that *looks* protective but verifies nothing — a smell coverage
//! tools are structurally blind to. They ship in preview until tuned against real corpora.

pub mod assertion_free;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP070", RuleGroup::Preview, || {
        Box::new(assertion_free::AssertionFreeTest)
    })]
}

#[cfg(test)]
mod tests {
    use crate::test_rule;

    test_rule!(
        slp070_assertion_free,
        super::assertion_free::AssertionFreeTest,
        "test_quality",
        "SLP070"
    );
}
