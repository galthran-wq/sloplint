//! Security rules.
//!
//! - `SLP210` phantom security-guard call/decorator — a call to / decorator of a known
//!   security-guard name (`validate_token`, `@requires_auth`, …) that is never defined or imported
//!   in the module (preview — heuristic, behind a curated dictionary).

pub mod phantom_guard;

use crate::registry::{RegisteredRule, RuleGroup};

/// This category's registry entries. SLP210 ships in preview until its precision is corpus-validated.
pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new(RuleGroup::Preview, || {
        Box::new(phantom_guard::PhantomGuard)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp210_phantom_guard,
        phantom_guard::PhantomGuard,
        "security",
        "SLP210"
    );
}
