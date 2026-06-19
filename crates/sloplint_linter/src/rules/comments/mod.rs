//! Comment & docstring rules.
//!
//! - `SLP010` comment policy — comments banned by default (stable).
//! - `SLP050` ASCII-only source (stable).
//! - `SLP001` redundant "what" comment (preview — heuristic).
//! - `SLP002` redundant docstring (preview — heuristic).
//! - `SLP003` comment "deodorant" — dense comments over complex code (preview — heuristic).

pub mod ascii_only;
pub mod comment_deodorant;
pub mod comment_policy;
pub mod redundant_comment;
pub mod redundant_docstring;

use crate::registry::{RegisteredRule, RuleGroup};

/// This category's registry entries. High-confidence rules ship stable; the fuzzy
/// overlap heuristics ship in preview until tuned against real corpora.
pub fn rules() -> Vec<RegisteredRule> {
    vec![
        RegisteredRule::new("SLP010", RuleGroup::Stable, || {
            Box::new(comment_policy::CommentPolicy)
        }),
        RegisteredRule::new("SLP050", RuleGroup::Stable, || {
            Box::new(ascii_only::AsciiOnly)
        }),
        RegisteredRule::new("SLP001", RuleGroup::Preview, || {
            Box::new(redundant_comment::RedundantComment)
        }),
        RegisteredRule::new("SLP002", RuleGroup::Preview, || {
            Box::new(redundant_docstring::RedundantDocstring)
        }),
        RegisteredRule::new("SLP003", RuleGroup::Preview, || {
            Box::new(comment_deodorant::CommentDeodorant)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp010_comment_policy,
        comment_policy::CommentPolicy,
        "comments",
        "SLP010"
    );
    test_rule!(
        slp050_ascii_only,
        ascii_only::AsciiOnly,
        "comments",
        "SLP050"
    );
    test_rule!(
        slp001_redundant_comment,
        redundant_comment::RedundantComment,
        "comments",
        "SLP001"
    );
    test_rule!(
        slp002_redundant_docstring,
        redundant_docstring::RedundantDocstring,
        "comments",
        "SLP002"
    );
    test_rule!(
        slp003_comment_deodorant,
        comment_deodorant::CommentDeodorant,
        "comments",
        "SLP003"
    );
}
