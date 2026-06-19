//! Leftover-template-placeholder rules.
//!
//! - `SLP100` leftover template placeholders (preview — heuristic lexicon).

pub mod leftover_placeholder;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP100", RuleGroup::Preview, || {
        Box::new(leftover_placeholder::LeftoverPlaceholder)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp100_leftover_placeholder,
        leftover_placeholder::LeftoverPlaceholder,
        "placeholders",
        "SLP100"
    );
}
