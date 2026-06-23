//! Comment & docstring rules.
//!
//! - `SLP010` comment policy — comments banned by default (stable).
//! - `SLP050` ASCII-only source (stable).
//! - `SLP001` redundant "what" comment (preview — heuristic).
//! - `SLP002` redundant docstring (preview — heuristic).

pub mod ascii_only;
pub mod comment_policy;
pub mod comment_tells;
pub mod redundant_comment;
pub mod redundant_docstring;

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
        slp004_comment_tells,
        comment_tells::CommentTells,
        "comments",
        "SLP004"
    );
}
