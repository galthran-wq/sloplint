//! SLP082: deep nesting inside a function.
//!
//! Ruff has no direct max-nesting gate (cognitive complexity captures it only indirectly),
//! so this is ours. Within each function, flag the first statement nested deeper than the
//! configured limit — one finding per function keeps the noise down. Nested function
//! definitions start their own depth count (handled per-function), so a method inside a
//! class doesn't inherit the class's nesting.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{ExceptHandler, Stmt};
use sloplint_python::{Ranged, TextRange};

use crate::ast_util::collect_functions;
use crate::lint::{FileContext, Rule};

pub struct DeepNesting;

impl Rule for DeepNesting {
    fn code(&self) -> &'static str {
        "SLP082"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let max_depth = ctx.limits.nesting_max_depth;
        let mut functions = Vec::new();
        collect_functions(&ctx.parsed.syntax().body, &mut functions);
        for function in functions {
            if let Some(range) = first_too_deep(&function.body, 0, max_depth) {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    format!(
                        "nesting deeper than {max_depth} levels in function `{}`",
                        function.name
                    ),
                    range,
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Range of the first statement nested deeper than `max` within `body` (depth counts
/// enclosing compound blocks; the function body itself is depth 0). Does not descend into
/// nested function/class definitions — those are analyzed separately.
fn first_too_deep(body: &[Stmt], depth: usize, max: usize) -> Option<TextRange> {
    for stmt in body {
        if depth > max {
            return Some(stmt.range());
        }
        if let Some(range) = nested_too_deep(stmt, depth, max) {
            return Some(range);
        }
    }
    None
}

fn nested_too_deep(stmt: &Stmt, depth: usize, max: usize) -> Option<TextRange> {
    let next = depth + 1;
    match stmt {
        Stmt::If(node) => first_too_deep(&node.body, next, max).or_else(|| {
            node.elif_else_clauses
                .iter()
                .find_map(|clause| first_too_deep(&clause.body, next, max))
        }),
        Stmt::For(node) => first_too_deep(&node.body, next, max)
            .or_else(|| first_too_deep(&node.orelse, next, max)),
        Stmt::While(node) => first_too_deep(&node.body, next, max)
            .or_else(|| first_too_deep(&node.orelse, next, max)),
        Stmt::With(node) => first_too_deep(&node.body, next, max),
        Stmt::Try(node) => first_too_deep(&node.body, next, max)
            .or_else(|| {
                node.handlers.iter().find_map(|handler| {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    first_too_deep(&handler.body, next, max)
                })
            })
            .or_else(|| first_too_deep(&node.orelse, next, max))
            .or_else(|| first_too_deep(&node.finalbody, next, max)),
        // Nested functions/classes are separate scopes — don't count their nesting here;
        // and non-compound statements have no nested body.
        _ => None,
    }
}
