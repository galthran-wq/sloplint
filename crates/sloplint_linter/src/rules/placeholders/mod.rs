//! Placeholder / mock-data rules.
//!
//! - `SLP230` mock / placeholder data left in production code — placeholder emails/phones/UUIDs,
//!   weak credentials, and dummy return values (preview — heuristic, non-test paths only).

pub mod mock_data;

use crate::registry::{RegisteredRule, RuleGroup};

/// This category's registry entries. SLP230 ships in preview until its precision is corpus-validated.
pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP230", RuleGroup::Preview, || {
        Box::new(mock_data::MockData)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp230_mock_data,
        mock_data::MockData,
        "placeholders",
        "SLP230"
    );
}
