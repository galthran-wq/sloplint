//! The shipped rule catalog — the single source of truth for which rules exist and their
//! stability, mirroring ruff's `codes.rs` `map_codes!`.
//!
//! Previously this mapping was spread across each rule category's `rules()` function; centralizing
//! it here means one place lists every shipped rule. Each rule's `SLP` code comes from the rule
//! itself (`code()`), so this table records only stability and the constructor — there is no code
//! literal here to drift from the rule.

use crate::registry::{RegisteredMeta, RegisteredRule, RuleGroup};
use crate::rules;

/// Declare the shipped rules as a central `Group => RulePath` table and generate
/// [`shipped_rules`]. Mirrors ruff's `map_codes!`.
macro_rules! map_codes {
    ($($group:ident => $rule:path),+ $(,)?) => {
        /// Every rule that ships with sloplint, in registration order.
        pub(crate) fn shipped_rules() -> Vec<RegisteredRule> {
            vec![
                $(RegisteredRule::new(RuleGroup::$group, || Box::new($rule)),)+
            ]
        }
    };
}

map_codes! {
    // Comments
    Stable => rules::comments::comment_policy::CommentPolicy,
    Stable => rules::comments::ascii_only::AsciiOnly,
    Preview => rules::comments::redundant_comment::RedundantComment,
    Preview => rules::comments::redundant_docstring::RedundantDocstring,
    Preview => rules::comments::comment_tells::CommentTells,
    // Structure
    Stable => rules::structure::defensive_except::DefensiveExcept,
    Stable => rules::structure::oversized_file::OversizedFile,
    Stable => rules::structure::deep_nesting::DeepNesting,
    Preview => rules::structure::redundant_type_hint::RedundantTypeHint,
    Preview => rules::structure::verbose_naming::VerboseNaming,
    Preview => rules::structure::deep_data_nesting::DeepDataNesting,
    Preview => rules::structure::dispatch_ladder::DispatchLadder,
    // Cohesion
    Preview => rules::cohesion::god_class::GodClass,
    // Security
    Preview => rules::security::phantom_guard::PhantomGuard,
    // Placeholders
    Preview => rules::placeholders::mock_data::MockData,
    // Cross-language
    Preview => rules::crosslang::cross_language::CrossLanguage,
}

/// Whole-tree rules: they analyze the whole project at once (every function, every path) and are
/// driven by the binary's cross-file pass, so they are not per-file [`shipped_rules`]. Listed here
/// so the catalog, `codes()`, and the docs guard see them like any other shipped rule. Their code
/// comes from the rule itself (`code()`), the single source of truth.
pub(crate) fn whole_project_rules() -> Vec<RegisteredMeta> {
    vec![
        RegisteredMeta::new(RuleGroup::Stable, || Box::new(crate::clones::Clones)),
        RegisteredMeta::new(RuleGroup::Stable, || Box::new(crate::fanout::Fanout)),
        RegisteredMeta::new(RuleGroup::Preview, || {
            Box::new(crate::imports::UndeclaredImports)
        }),
        RegisteredMeta::new(RuleGroup::Preview, || {
            Box::new(crate::ghost::GhostScaffolding)
        }),
        RegisteredMeta::new(RuleGroup::Preview, || Box::new(crate::corrupted::Corrupted)),
    ]
}
