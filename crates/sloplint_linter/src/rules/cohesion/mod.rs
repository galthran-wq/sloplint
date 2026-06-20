//! Class-cohesion rules.
//!
//! - `SLP120` low-cohesion "god class" via LCOM4 (preview — heuristic).

pub mod god_class;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP120", RuleGroup::Preview, || {
        Box::new(god_class::GodClass)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(slp120_god_class, god_class::GodClass, "cohesion", "SLP120");
}
