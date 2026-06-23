//! SLP084: deeply nested data-structure literals.

use std::ptr;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{walk_expr, Visitor};
use sloplint_python::ast::{Expr, ExprContext};
use sloplint_python::Ranged;

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags the outermost container of a chain of directly nested data-structure literals or
/// comprehensions (`list`/`dict`/`tuple`/`set`) deeper than the configured limit —
/// `{...: [{...}]}`.
///
/// ## Why is this bad?
/// A deep inline literal is hard to read, hard to diff, and easy to get subtly wrong (a
/// misplaced comma or key is nearly invisible); a named type (a dataclass) makes the structure
/// explicit. This is the expression-tree nesting axis, distinct from SLP082's control flow.
///
/// ## Example
/// ```python
/// config = {"a": {"b": {"c": [{"d": 1}]}}}
/// ```
#[derive(ViolationMetadata)]
pub struct DeepDataNesting;

impl Rule for DeepDataNesting {
    fn code(&self) -> &'static str {
        "SLP084"
    }

    fn check_source(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut visitor = NestingVisitor {
            code: self.code(),
            max_depth: ctx.limits.data_nesting_max_depth,
            diagnostics,
        };
        for stmt in &ctx.parsed.syntax().body {
            visitor.visit_stmt(stmt);
        }
    }
}

/// Drives statement traversal via the trait's default recursion; each `visit_expr` is the
/// root of a statement-level expression tree (we stop the trait's expression recursion and
/// walk expressions ourselves in `scan`, so we can track container-of-container nesting).
struct NestingVisitor<'a> {
    code: &'static str,
    max_depth: usize,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> Visitor<'a> for NestingVisitor<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        scan(self.code, expr, false, self.max_depth, self.diagnostics);
    }
}

/// Walk an expression tree, flagging each container chain whose depth exceeds `max_depth`.
/// `parent_is_container` is true when `expr` is a direct element/key/value of an enclosing
/// container — used to report only the *outermost* container of a nested chain.
fn scan(
    code: &'static str,
    expr: &Expr,
    parent_is_container: bool,
    max_depth: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let elements = container_elements(expr);
    if elements.is_some() && !parent_is_container {
        let depth = container_depth(expr);
        if depth > max_depth {
            diagnostics.push(Diagnostic::new(
                code,
                format!(
                    "data-structure literal nested {depth} levels deep (max {max_depth}); \
                     model it with a named type (e.g. a dataclass) instead of an inline literal"
                ),
                expr.range(),
                Severity::Warning,
            ));
        }
    }
    // Recurse into every sub-expression, but only the container's *data elements* count as
    // "directly nested" — a container reached through a structural child (a comprehension's
    // `iter`/`if`, a call argument, …) re-roots a fresh chain rather than extending this one.
    let elements = elements.unwrap_or_default();
    for child in direct_child_exprs(expr) {
        let is_element = elements.iter().any(|element| ptr::eq(*element, child));
        scan(code, child, is_element, max_depth, diagnostics);
    }
}

/// The maximum chain of directly nested containers rooted at `expr` (1 for a container with
/// no container elements, 0 for a non-container). Only chains through container
/// elements/keys/values — a non-container element contributes 0.
fn container_depth(expr: &Expr) -> usize {
    match container_elements(expr) {
        Some(children) => 1 + children.into_iter().map(container_depth).max().unwrap_or(0),
        None => 0,
    }
}

/// The element/key/value sub-expressions of a container *literal*, or `None` if `expr` is
/// not a data-structure literal/comprehension. A `List`/`Tuple` in `Store`/`Del` context is
/// an assignment-target unpacking (`[[a]] = x`), not a literal, so it is not a container.
fn container_elements(expr: &Expr) -> Option<Vec<&Expr>> {
    match expr {
        Expr::List(node) if is_load(&node.ctx) => Some(node.elts.iter().collect()),
        Expr::Tuple(node) if is_load(&node.ctx) => Some(node.elts.iter().collect()),
        Expr::Set(node) => Some(node.elts.iter().collect()),
        Expr::Dict(node) => {
            let mut elements = Vec::with_capacity(node.items.len() * 2);
            for item in &node.items {
                if let Some(key) = &item.key {
                    elements.push(key);
                }
                elements.push(&item.value);
            }
            Some(elements)
        }
        Expr::ListComp(node) => Some(vec![node.elt.as_ref()]),
        Expr::SetComp(node) => Some(vec![node.elt.as_ref()]),
        Expr::DictComp(node) => {
            let mut elements = Vec::with_capacity(2);
            if let Some(key) = node.key.as_deref() {
                elements.push(key);
            }
            elements.push(node.value.as_ref());
            Some(elements)
        }
        _ => None,
    }
}

fn is_load(ctx: &ExprContext) -> bool {
    matches!(ctx, ExprContext::Load)
}

/// The immediate child expressions of `expr` (one level down), via the AST visitor — used
/// to traverse the whole expression tree without enumerating every node kind by hand.
fn direct_child_exprs(expr: &Expr) -> Vec<&Expr> {
    struct Collector<'a> {
        out: Vec<&'a Expr>,
    }
    impl<'a> Visitor<'a> for Collector<'a> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            // Don't recurse: capture only the direct children walked by `walk_expr`.
            self.out.push(expr);
        }
    }
    let mut collector = Collector { out: Vec::new() };
    walk_expr(&mut collector, expr);
    collector.out
}
