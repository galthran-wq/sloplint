//! Cross-language rules.
//!
//! - `SLP250` cross-language pollution — wrong-language idioms (`.toString()`, `.length`,
//!   `array_push`) leaking into Python (preview — heuristic; the FP-riskiest rule, so the blocklist
//!   is narrow and an allow-list suppresses FP-prone names).

pub mod cross_language;

use crate::registry::{RegisteredRule, RuleGroup};

/// This category's registry entries. SLP250 ships in preview until corpus precision is proven.
pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP250", RuleGroup::Preview, || {
        Box::new(cross_language::CrossLanguage)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp250_cross_language,
        cross_language::CrossLanguage,
        "crosslang",
        "SLP250"
    );
}
