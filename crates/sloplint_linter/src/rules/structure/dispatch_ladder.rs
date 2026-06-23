//! SLP130: literal-dispatch and isinstance ladders.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{CmpOp, Expr, Stmt};
use sloplint_python::Ranged;

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags a long `if`/`elif` chain testing the *same* value against a series of literals
/// (`x == "a"` …) or types (`isinstance(x, A)` …) past `dispatch_max_branches`.
///
/// ## Why is this bad?
/// Hand-unrolled dispatch is a textbook generated-code shape — verbose and error-prone — that
/// should be a lookup table (`dict`), `match`, or polymorphism; Ruff has no equivalent.
/// Conservative (uniform same-subject chains only); preview-gated.
#[derive(ViolationMetadata)]
pub struct DispatchLadder;

impl Rule for DispatchLadder {
    fn code(&self) -> &'static str {
        "SLP130"
    }

    fn check_stmt(&self, stmt: &Stmt, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let Stmt::If(node) = stmt else { return };
        let max = ctx.limits.dispatch_max_branches;
        let source = ctx.source;
        // The chain's conditions: the leading `if` plus every `elif` (the trailing `else`,
        // if any, carries no test and is skipped).
        let tests: Vec<&Expr> = std::iter::once(node.test.as_ref())
            .chain(
                node.elif_else_clauses
                    .iter()
                    .filter_map(|clause| clause.test.as_ref()),
            )
            .collect();
        if tests.len() <= max {
            return;
        }
        let n = tests.len();
        match ladder_kind(&tests, source) {
            Some(LadderKind::Literal) => diagnostics.push(Diagnostic::new(
                self.code(),
                format!(
                    "dispatch ladder: {n} `==` branches on the same value — use a lookup \
                         table (dict) or `match` instead of an if/elif chain"
                ),
                node.test.range(),
                Severity::Warning,
            )),
            Some(LadderKind::IsInstance) => diagnostics.push(Diagnostic::new(
                self.code(),
                format!(
                    "isinstance ladder: {n} type checks on the same value — use \
                         polymorphism or `match` instead of an if/elif chain"
                ),
                node.test.range(),
                Severity::Warning,
            )),
            None => {}
        }
    }
}

enum LadderKind {
    Literal,
    IsInstance,
}

/// Classify a chain of `if`/`elif` tests: a uniform `subject == <literal>` ladder, a uniform
/// `isinstance(subject, _)` ladder (same subject throughout), or neither.
fn ladder_kind(tests: &[&Expr], source: &str) -> Option<LadderKind> {
    if uniform(tests, source, eq_literal_subject) {
        Some(LadderKind::Literal)
    } else if uniform(tests, source, isinstance_subject) {
        Some(LadderKind::IsInstance)
    } else {
        None
    }
}

/// True when `extract` yields the same (non-empty) subject text for every test.
fn uniform(
    tests: &[&Expr],
    source: &str,
    extract: for<'e, 's> fn(&'e Expr, &'s str) -> Option<&'s str>,
) -> bool {
    let mut subject: Option<&str> = None;
    for test in tests {
        let Some(s) = extract(test, source) else {
            return false;
        };
        match subject {
            None => subject = Some(s),
            Some(prev) => {
                if prev != s {
                    return false;
                }
            }
        }
    }
    subject.is_some()
}

/// For `subject == <literal>` (a single `==` against a string/number literal), the source text
/// of `subject`; otherwise `None`.
fn eq_literal_subject<'s>(test: &Expr, source: &'s str) -> Option<&'s str> {
    let Expr::Compare(cmp) = test else {
        return None;
    };
    if cmp.ops.len() != 1 || cmp.ops[0] != CmpOp::Eq || cmp.comparators.len() != 1 {
        return None;
    }
    if !matches!(
        &cmp.comparators[0],
        Expr::StringLiteral(_) | Expr::NumberLiteral(_)
    ) {
        return None;
    }
    Some(&source[cmp.left.range()])
}

/// For `isinstance(subject, _)` (exactly two positional args, no keywords/unpacking), the source
/// text of `subject`; otherwise `None`.
fn isinstance_subject<'s>(test: &Expr, source: &'s str) -> Option<&'s str> {
    let Expr::Call(call) = test else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    if name.id.as_str() != "isinstance" {
        return None;
    }
    let args = &call.arguments;
    if !args.keywords.is_empty() || args.args.len() != 2 {
        return None;
    }
    if args.args.iter().any(|a| matches!(a, Expr::Starred(_))) {
        return None;
    }
    Some(&source[args.args[0].range()])
}
