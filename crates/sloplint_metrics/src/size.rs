//! Per-function size/shape metrics: physical span (`line_span`), logical statement count
//! (NCSS), explicit exit points, raw parameter count, and caller-facing arity (the
//! `self`/`cls` receiver excluded). All are own-body measures — they do not descend into
//! nested function/class bodies, which are measured on their own.

use crate::expr_trailing_name;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Parameters, Stmt, StmtFunctionDef};
use sloplint_python::TextRange;

/// Number of explicit exit points in the function's own body: `return`, `raise`, and
/// `yield`/`yield from`. Does not descend into nested defs/lambdas (those exits belong to the
/// nested scope).
pub(crate) fn exit_count(body: &[Stmt]) -> usize {
    struct Counter {
        n: usize,
    }
    impl Visitor<'_> for Counter {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {} // nested scope
                Stmt::Return(_) | Stmt::Raise(_) => {
                    self.n += 1;
                    visitor::walk_stmt(self, stmt);
                }
                _ => visitor::walk_stmt(self, stmt),
            }
        }
        fn visit_expr(&mut self, expr: &Expr) {
            match expr {
                Expr::Lambda(_) => {} // nested scope
                Expr::Yield(_) | Expr::YieldFrom(_) => {
                    self.n += 1;
                    visitor::walk_expr(self, expr);
                }
                _ => visitor::walk_expr(self, expr),
            }
        }
    }
    let mut counter = Counter { n: 0 };
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.n
}

/// Non-Commenting Source Statements: count every statement node in the function's own body.
/// A nested def/class counts as a single statement (its declaration) but we don't descend into
/// it — its statements belong to that nested unit's own row, so `ncss` stays own-body like
/// `exit_count`/`cognitive`, never double-counting a helper's body into its parent. Comments
/// and blank lines are not statements, so they're naturally excluded — a logical code-size
/// measure distinct from physical `loc`.
pub(crate) fn ncss(body: &[Stmt]) -> usize {
    struct Counter {
        n: usize,
    }
    impl Visitor<'_> for Counter {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            self.n += 1;
            match stmt {
                // The nested def/class declaration counts (above); its body is a separate unit.
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
                _ => visitor::walk_stmt(self, stmt),
            }
        }
    }
    let mut counter = Counter { n: 0 };
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.n
}

/// Physical lines spanned by `range` in `source` (at least 1).
pub(crate) fn line_span(source: &str, range: TextRange) -> usize {
    let start = u32::from(range.start()) as usize;
    let end = (u32::from(range.end()) as usize).min(source.len());
    source[start..end].lines().count().max(1)
}

/// Raw declared-parameter count: positional-only + regular + keyword-only, plus one each for
/// `*args`/`**kwargs`. Includes the `self`/`cls` receiver (see [`caller_arity`] for the
/// caller-facing count).
pub(crate) fn param_count(parameters: &Parameters) -> usize {
    parameters.posonlyargs.len()
        + parameters.args.len()
        + parameters.kwonlyargs.len()
        + usize::from(parameters.vararg.is_some())
        + usize::from(parameters.kwarg.is_some())
}

/// Whether the function's first positional parameter is a `self`/`cls` receiver (`1`) or not
/// (`0`). A non-static method whose first parameter (positional-only first, else the first regular
/// arg) is named `self`/`cls` carries one. Caller-invisible, so it counts toward neither annotation
/// coverage nor arity.
pub(crate) fn receiver_count(function: &StmtFunctionDef) -> usize {
    let params = &function.parameters;
    usize::from(
        !is_staticmethod(function)
            && params
                .posonlyargs
                .first()
                .or_else(|| params.args.first())
                .is_some_and(|param| matches!(param.parameter.name.as_str(), "self" | "cls")),
    )
}

/// Caller-facing arity: every declared parameter a caller passes — positional-only,
/// regular, keyword-only, and `*args`/`**kwargs` (each variadic once) — minus the `self`/`cls`
/// receiver. The input to the Long-Parameter-List bands.
pub(crate) fn caller_arity(function: &StmtFunctionDef) -> usize {
    param_count(&function.parameters) - receiver_count(function)
}

/// Whether a function carries `@staticmethod` — so its first parameter is a genuine argument, not
/// a `self`/`cls` receiver.
fn is_staticmethod(function: &StmtFunctionDef) -> bool {
    function
        .decorator_list
        .iter()
        .filter_map(|decorator| expr_trailing_name(&decorator.expression))
        .any(|name| name == "staticmethod")
}
