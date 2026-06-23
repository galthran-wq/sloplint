//! The minimal rule-running seam.
//!
//! This is the contract every rule implements and the function that drives them over one
//! parsed file. The full registry (codes, preview/stable grouping, config-driven
//! selection) is layered on top in the diagnostics/registry slice; rules written against
//! this trait don't change when that lands.

use sloplint_diagnostics::Diagnostic;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{ModModule, Stmt};
use sloplint_python::parser::Parsed;

use crate::config::Limits;
use sloplint_python::{Ranged, TextRange, TokenKind};

/// Everything a rule needs about a single file under analysis.
pub struct FileContext<'a> {
    /// Path of the file (for messages / per-path config gating).
    pub path: &'a str,
    /// The full source text — comment rules and range slicing need it.
    pub source: &'a str,
    /// The parsed tree: `parsed.syntax()` for the AST, `parsed.tokens()` for comments.
    pub parsed: &'a Parsed<ModModule>,
    /// Tunable thresholds for the structural rules.
    pub limits: Limits,
    /// Extra security-guard names beyond the built-in catalog (`[security] extra` in config),
    /// used by SLP210. Empty by default; non-CLI callers pass `&[]`.
    pub security_extra: &'a [String],
    /// Extra placeholder literal values beyond the built-in sets (`[placeholders] extra` in config),
    /// used by SLP230. Empty by default; non-CLI callers pass `&[]`.
    pub placeholders_extra: &'a [String],
    /// Extra hedging/deferral comment phrases beyond the built-in lexicon (`[comments] extra` in
    /// config), used by SLP004. Empty by default; non-CLI callers pass `&[]`.
    pub comment_phrases_extra: &'a [String],
    /// Extra names to treat as legitimate Python (allow-list) for SLP250 cross-language detection
    /// (`[crosslang] allow` in config). Empty by default; non-CLI callers pass `&[]`.
    pub crosslang_allow: &'a [String],
}

/// A single lint rule. Rules push findings rather than returning them, so one AST/token
/// pass can fan out to many rules cheaply.
pub trait Rule: sloplint_diagnostics::ViolationMetadata {
    /// Stable code, e.g. `"SLP001"`. Used in output, config, and suppressions.
    fn code(&self) -> &'static str;

    /// Legacy whole-file entry point: inspect `ctx` and append findings. Rules that have moved to
    /// single-pass node hooks (e.g. [`Rule::check_comment`]) leave this as the default no-op — a
    /// rule runs either here or via its node hooks, never both.
    fn check(&self, _ctx: &FileContext, _diagnostics: &mut Vec<Diagnostic>) {}

    /// Single-pass hook called once per comment token (in source order) during [`check_file`]'s
    /// token pass, so comment rules don't each re-walk the token stream. `range` is the comment
    /// token's range. Default: no-op.
    fn check_comment(
        &self,
        _ctx: &FileContext,
        _range: TextRange,
        _diagnostics: &mut Vec<Diagnostic>,
    ) {
    }

    /// Single-pass hook called once per statement node (pre-order) during [`check_file`]'s single
    /// AST walk, so statement rules don't each re-walk the tree. Default: no-op.
    fn check_stmt(&self, _stmt: &Stmt, _ctx: &FileContext, _diagnostics: &mut Vec<Diagnostic>) {}

    /// One-shot whole-file hook, called once per file during [`check_file`]. For rules whose
    /// analysis doesn't fit the shared token/AST node dispatch — a raw-source/line scan, or a
    /// rule that runs its own bespoke AST walk (controlled-recursion or two-pass). Default: no-op.
    fn check_source(&self, _ctx: &FileContext, _diagnostics: &mut Vec<Diagnostic>) {}

    /// Batch hook called once per file with the ranges of all `Name` tokens (source order),
    /// collected during [`check_file`]'s single token pass — for name rules that need per-file
    /// state (e.g. de-dup) without re-walking the token stream. Default: no-op.
    fn check_names(
        &self,
        _ctx: &FileContext,
        _names: &[TextRange],
        _diagnostics: &mut Vec<Diagnostic>,
    ) {
    }
}

/// Walks the AST once, dispatching each statement to every rule's [`Rule::check_stmt`].
struct NodeDispatch<'a, 'r> {
    rules: &'r [&'r dyn Rule],
    ctx: &'r FileContext<'a>,
    diagnostics: &'r mut Vec<Diagnostic>,
}

impl<'a> Visitor<'a> for NodeDispatch<'a, '_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        for rule in self.rules {
            rule.check_stmt(stmt, self.ctx, self.diagnostics);
        }
        visitor::walk_stmt(self, stmt);
    }
}

/// Run the given rules over an already-parsed file, collecting all findings.
/// Run `rules` over one parsed file in a single pass: one walk of the token stream dispatches each
/// comment to [`Rule::check_comment`], then any rule still on the legacy [`Rule::check`] runs. The
/// rendered output is range-sorted (see `sloplint_diagnostics::render`), so the emission order here
/// is not observable.
pub fn check_file(ctx: &FileContext, rules: &[&dyn Rule]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // One token pass: dispatch each comment to comment rules, and collect every `Name` token's
    // range for the name rules (so neither group re-walks the token stream).
    let mut name_ranges: Vec<TextRange> = Vec::new();
    for token in ctx.parsed.tokens().iter() {
        match token.kind() {
            TokenKind::Comment => {
                for rule in rules {
                    rule.check_comment(ctx, token.range(), &mut diagnostics);
                }
            }
            TokenKind::Name => name_ranges.push(token.range()),
            _ => {}
        }
    }
    for rule in rules {
        rule.check_names(ctx, &name_ranges, &mut diagnostics);
    }

    // One AST walk, shared by every statement rule (instead of one walk per rule).
    {
        let mut dispatch = NodeDispatch {
            rules,
            ctx,
            diagnostics: &mut diagnostics,
        };
        for stmt in &ctx.parsed.syntax().body {
            dispatch.visit_stmt(stmt);
        }
    }

    // One-shot whole-source pass (file-level rules: line count, raw-char scan).
    for rule in rules {
        rule.check_source(ctx, &mut diagnostics);
    }

    // Legacy whole-file pass for rules not yet migrated to node hooks.
    for rule in rules {
        rule.check(ctx, &mut diagnostics);
    }

    diagnostics
}

/// The shipped rules enabled under the default config (path-agnostic).
///
/// Delegates to the registry so it always reflects the *actual shipped behavior* — empty
/// until real rules land. The corpus runner uses this; per-path selection is applied by
/// callers that have a real config (the CLI).
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    let config = crate::config::Config::default();
    let selector = config
        .prepare()
        .expect("the default config contains no globs and always compiles");
    crate::registry::Registry::shipped().enabled_for(&selector, "")
}
