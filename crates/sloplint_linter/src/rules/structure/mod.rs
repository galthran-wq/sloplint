//! Structural rules.
//!
//! - `SLP030` overly defensive try/except (stable).
//! - `SLP040` redundant type hint (preview — heuristic).
//! - `SLP060` verbose mechanical naming (preview — heuristic).
//! - `SLP080` oversized file (stable).
//! - `SLP082` deep nesting — control flow (stable).
//! - `SLP084` deep data-structure nesting — expression tree (preview — heuristic).
//! - `SLP130` literal-dispatch / isinstance ladder (preview — heuristic).
//!
//! (`SLP090` flat-directory fanout is a whole-tree analysis handled in the CLI, not a
//! per-file rule, so it isn't registered here.)

pub mod deep_data_nesting;
pub mod deep_nesting;
pub mod defensive_except;
pub mod dispatch_ladder;
pub mod oversized_file;
pub mod redundant_type_hint;
pub mod verbose_naming;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Limits;
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
    // SLP080 needs a small line limit, so its snapshots pass an explicit `Limits`: the
    // 4-line fixture flags when over the ceiling and is silent exactly at it.
    test_rule!(
        slp080_over_limit,
        oversized_file::OversizedFile,
        "structure",
        "SLP080",
        Limits {
            file_max_lines: 3,
            ..Default::default()
        }
    );
    test_rule!(
        slp080_at_limit,
        oversized_file::OversizedFile,
        "structure",
        "SLP080",
        Limits {
            file_max_lines: 4,
            ..Default::default()
        }
    );
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
    test_rule!(
        slp130_dispatch_ladder,
        dispatch_ladder::DispatchLadder,
        "structure",
        "SLP130"
    );
    // Threshold: the same 4-branch ladder is spared when the branch limit is raised to 4 (a
    // chain must *exceed* the limit) and flagged when lowered to 2.
    test_rule!(
        slp130_threshold_raised,
        dispatch_ladder::DispatchLadder,
        "structure",
        "SLP130_threshold",
        Limits {
            dispatch_max_branches: 4,
            ..Default::default()
        }
    );
    test_rule!(
        slp130_threshold_lowered,
        dispatch_ladder::DispatchLadder,
        "structure",
        "SLP130_threshold",
        Limits {
            dispatch_max_branches: 2,
            ..Default::default()
        }
    );
    // Threshold: the same 4-deep literal is spared when the depth limit is raised above it
    // and flagged when lowered below it.
    test_rule!(
        slp084_threshold_raised,
        deep_data_nesting::DeepDataNesting,
        "structure",
        "SLP084_threshold",
        Limits {
            data_nesting_max_depth: 4,
            ..Default::default()
        }
    );
    test_rule!(
        slp084_threshold_lowered,
        deep_data_nesting::DeepDataNesting,
        "structure",
        "SLP084_threshold",
        Limits {
            data_nesting_max_depth: 2,
            ..Default::default()
        }
    );
}
