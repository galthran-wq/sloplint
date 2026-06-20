//! Padding rules — physical size inflated by comments / blank lines rather than code.
//!
//! - `SLP150` comment/blank padding ratio per function (preview — heuristic).

pub mod comment_blank_padding;

use crate::registry::{RegisteredRule, RuleGroup};

pub fn rules() -> Vec<RegisteredRule> {
    vec![RegisteredRule::new("SLP150", RuleGroup::Preview, || {
        Box::new(comment_blank_padding::CommentBlankPadding)
    })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rule;

    test_rule!(
        slp150_comment_blank_padding,
        comment_blank_padding::CommentBlankPadding,
        "padding",
        "SLP150"
    );
}
