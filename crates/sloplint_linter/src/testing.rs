//! Test-support: the `test_rule!` snapshot macro and a tiny example rule.
//!
//! Every real rule is developed test-first against a fixture under
//! `resources/test/fixtures/<category>/<CODE>.py` that contains **both** violations and
//! non-violations; `test_rule!` runs the rule over that fixture and snapshots the
//! rendered diagnostics with `insta`. Regenerate snapshots with `cargo insta review`
//! (or `INSTA_UPDATE=always cargo test`).

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TokenKind};

use crate::lint::{FileContext, Rule};

/// Example rule (`SLP000`): flags the literal `TODO` inside a comment.
///
/// Its only purpose is to exercise the token-phase seam and the snapshot harness. It is
/// intentionally **not** part of [`crate::lint::all_rules`], so it never affects corpus
/// metrics or real runs.
pub struct ExampleTodo;

impl Rule for ExampleTodo {
    fn code(&self) -> &'static str {
        "SLP000"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        for token in ctx.parsed.tokens().iter() {
            if token.kind() == TokenKind::Comment && ctx.source[token.range()].contains("TODO") {
                diagnostics.push(Diagnostic::new(
                    "SLP000",
                    "example: TODO found in comment",
                    token.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Define a snapshot test for a rule against its fixture.
///
/// `test_rule!(test_name, RuleExpr, "category", "SLP0xx")` reads
/// `resources/test/fixtures/<category>/<SLP0xx>.py`, runs the rule, and snapshots the
/// rendered diagnostics. The fixture should contain both violations and non-violations
/// so the snapshot pins down false-positive behavior too.
///
/// Like Ruff, rule tests live in this crate alongside the rules, so this is primarily an
/// in-crate macro. If ever used from another crate, that crate must depend on
/// `sloplint_python`, `sloplint_diagnostics`, and `insta` (the names this macro references)
/// and replicate the `resources/test/fixtures/<category>/<CODE>.py` layout.
#[macro_export]
macro_rules! test_rule {
    ($name:ident, $rule:expr, $category:literal, $code:literal) => {
        #[test]
        fn $name() {
            const FIXTURE: &str = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/test/fixtures/",
                $category,
                "/",
                $code,
                ".py"
            );
            let source = ::std::fs::read_to_string(FIXTURE)
                .unwrap_or_else(|e| panic!("reading fixture {FIXTURE}: {e}"));
            let parsed = ::sloplint_python::parse(&source).expect("fixture must parse");
            let rule = $rule;
            let ctx = $crate::lint::FileContext {
                path: FIXTURE,
                source: &source,
                parsed: &parsed,
                limits: ::core::default::Default::default(),
                placeholders: &[],
            };
            let diagnostics = $crate::lint::check_file(&ctx, &[&rule as &dyn $crate::lint::Rule]);
            let rendered =
                ::sloplint_diagnostics::render::render_diagnostics(&source, &diagnostics);
            ::insta::assert_snapshot!(rendered);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::ExampleTodo;

    // Proves the harness end-to-end: the SLP000 fixture has two TODO comments (violations)
    // and two ordinary comments (non-violations); the snapshot must show exactly two.
    test_rule!(example_todo, ExampleTodo, "internal", "SLP000");
}
