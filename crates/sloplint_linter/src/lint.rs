//! The minimal rule-running seam.
//!
//! This is the contract every rule implements and the function that drives them over one
//! parsed file. The full registry (codes, preview/stable grouping, config-driven
//! selection) is layered on top in the diagnostics/registry slice; rules written against
//! this trait don't change when that lands.

use sloplint_diagnostics::Diagnostic;
use sloplint_python::ast::ModModule;
use sloplint_python::parser::Parsed;

/// Everything a rule needs about a single file under analysis.
pub struct FileContext<'a> {
    /// Path of the file (for messages / per-path config gating).
    pub path: &'a str,
    /// The full source text — comment rules and range slicing need it.
    pub source: &'a str,
    /// The parsed tree: `parsed.syntax()` for the AST, `parsed.tokens()` for comments.
    pub parsed: &'a Parsed<ModModule>,
}

/// A single lint rule. Rules push findings rather than returning them, so one AST/token
/// pass can fan out to many rules cheaply.
pub trait Rule {
    /// Stable code, e.g. `"SLP001"`. Used in output, config, and suppressions.
    fn code(&self) -> &'static str;

    /// Inspect `ctx` and append any findings to `diagnostics`.
    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>);
}

/// Run the given rules over an already-parsed file, collecting all findings.
pub fn check_file(ctx: &FileContext, rules: &[&dyn Rule]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
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
