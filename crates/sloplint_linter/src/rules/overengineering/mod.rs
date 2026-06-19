//! Over-engineering rules — needless indirection in low-complexity code.
//!
//! - `SLP110` pass-through wrapper (preview — heuristic).

pub mod passthrough_wrapper;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP110", RuleGroup::Preview, || {
        Box::new(passthrough_wrapper::PassthroughWrapper)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp110_passthrough_wrapper,
        passthrough_wrapper::PassthroughWrapper,
        "overengineering",
        "SLP110"
    );
}
