//! Leftover-scaffolding rules.
//!
//! - `SLP140` tutorial / "example usage" `__main__` demo block in a library module (preview).

pub mod example_scaffolding;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP140", RuleGroup::Preview, || {
        Box::new(example_scaffolding::ExampleScaffolding)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp140_example_scaffolding,
        example_scaffolding::ExampleScaffolding,
        "scaffolding",
        "SLP140"
    );
}
