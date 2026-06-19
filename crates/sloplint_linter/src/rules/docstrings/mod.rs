//! Docstring rules.
//!
//! - `SLP130` docstring drift — `Raises:`/`Returns:` vs the actual body (preview — heuristic).

pub mod docstring_drift;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP130", RuleGroup::Preview, || {
        Box::new(docstring_drift::DocstringDrift)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp130_docstring_drift,
        docstring_drift::DocstringDrift,
        "docstrings",
        "SLP130"
    );
}
