//! Structural rules.
//!
//! - `SLP030` overly defensive try/except (stable).
//! - `SLP040` redundant type hint (preview — heuristic).
//! - `SLP060` verbose mechanical naming (preview — heuristic).
//! - `SLP080` oversized file (stable).
//! - `SLP082` deep nesting — control flow (stable).
//! - `SLP084` deep data-structure nesting — expression tree (preview — heuristic).
//!
//! (`SLP090` flat-directory fanout is a whole-tree analysis handled in the CLI, not a
//! per-file rule, so it isn't registered here.)

pub mod deep_data_nesting;
pub mod deep_nesting;
pub mod defensive_except;
pub mod oversized_file;
pub mod redundant_type_hint;
pub mod verbose_naming;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![
        RegisteredRule::new("SLP030", RuleGroup::Stable, || {
            Box::new(defensive_except::DefensiveExcept)
        }),
        RegisteredRule::new("SLP080", RuleGroup::Stable, || {
            Box::new(oversized_file::OversizedFile)
        }),
        RegisteredRule::new("SLP082", RuleGroup::Stable, || {
            Box::new(deep_nesting::DeepNesting)
        }),
        RegisteredRule::new("SLP040", RuleGroup::Preview, || {
            Box::new(redundant_type_hint::RedundantTypeHint)
        }),
        RegisteredRule::new("SLP060", RuleGroup::Preview, || {
            Box::new(verbose_naming::VerboseNaming)
        }),
        RegisteredRule::new("SLP084", RuleGroup::Preview, || {
            Box::new(deep_data_nesting::DeepDataNesting)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp030_defensive_except,
        defensive_except::DefensiveExcept,
        "structure",
        "SLP030"
    );
    test_rule!(
        slp040_redundant_type_hint,
        redundant_type_hint::RedundantTypeHint,
        "structure",
        "SLP040"
    );
    test_rule!(
        slp060_verbose_naming,
        verbose_naming::VerboseNaming,
        "structure",
        "SLP060"
    );
    // SLP080 needs a custom (small) line limit, which `test_rule!` can't supply, so it has
    // a dedicated unit test in its own module instead of a snapshot.
    test_rule!(
        slp082_deep_nesting,
        deep_nesting::DeepNesting,
        "structure",
        "SLP082"
    );
    test_rule!(
        slp084_deep_data_nesting,
        deep_data_nesting::DeepDataNesting,
        "structure",
        "SLP084"
    );
}
