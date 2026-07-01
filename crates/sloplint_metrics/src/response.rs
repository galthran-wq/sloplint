//! Response For a Class (RFC) — Chidamber & Kemerer (1994): the size of a class's **response
//! set** `|M ∪ R|`, where `M` is the class's own methods and `R` is the set of distinct methods
//! those methods invoke. It measures how much behavior a single message to the class can trigger:
//! a class that reaches into dozens of collaborators is harder to test and understand than its
//! method count alone suggests.
//!
//! This is the classic CK definition (own methods **plus** the distinct methods they call), the
//! one the CK-based empirical-threshold literature is built on. It is an **approximation** in
//! dynamically-typed Python, for the same reason CBO is (see [`crate::inheritance`]): the response
//! set is keyed by the invoked callee's **trailing name** (`self.foo()`/`obj.foo()`/`pkg.foo()` all
//! resolve to `foo`), since there are no static types to resolve a call to a concrete method. Two
//! different `foo`s collapse to one entry (an under-count), and — because Python has no free/method
//! distinction — plain function and builtin calls (`len(...)`, `range(...)`) are counted as
//! invocations too. Calls to the class's own methods (`self.area()`) fold back into `M` and don't
//! grow the set.

use crate::expr_trailing_name;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Stmt, StmtClassDef};
use std::collections::HashSet;

/// Compute RFC for `class`: the number of distinct entries in `M ∪ R` — the class's own
/// directly-declared methods, unioned with the distinct trailing names of every call made in
/// those methods' bodies. Does **not** descend into a nested class (its own unit with its own RFC
/// row), matching `class_wmc`/`coupling_candidates`; it does descend into nested functions and
/// closures, whose calls are still part of the enclosing method's response.
pub fn class_rfc(class: &StmtClassDef) -> usize {
    let mut collector = ResponseSet {
        names: HashSet::new(),
    };
    // M: the class's own directly-declared methods.
    for stmt in &class.body {
        if let Stmt::FunctionDef(method) = stmt {
            collector.names.insert(method.name.as_str());
        }
    }
    // R: distinct methods invoked by those methods (by trailing callee name). Walk each method's
    // *body* only — a parametrized decorator (`@app.route(...)`) or a default-arg call runs at
    // definition time, not in response to a message, so it isn't part of the response set.
    for stmt in &class.body {
        if let Stmt::FunctionDef(method) = stmt {
            for body_stmt in &method.body {
                collector.visit_stmt(body_stmt);
            }
        }
    }
    collector.names.len()
}

/// Collects the trailing names of every call in a method body into the response set, stopping at
/// a nested class boundary.
struct ResponseSet<'a> {
    names: HashSet<&'a str>,
}

impl<'a> Visitor<'a> for ResponseSet<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if matches!(stmt, Stmt::ClassDef(_)) {
            return; // a nested class is its own unit with its own RFC row.
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if let Some(name) = expr_trailing_name(&call.func) {
                self.names.insert(name);
            }
        }
        visitor::walk_expr(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture_source;
    use sloplint_python::parse;
    use std::fmt::Write;

    /// RFC for every class in a fixture, in source order. The fixture documents each case's
    /// response set; the snapshot pins the numbers.
    fn rfc_report(source: &str) -> String {
        let parsed = parse(source).expect("fixture parses");
        let mut out = String::new();
        for stmt in &parsed.syntax().body {
            if let Stmt::ClassDef(class) = stmt {
                writeln!(out, "{}: rfc={}", class.name, class_rfc(class)).unwrap();
            }
        }
        out
    }

    #[test]
    fn rfc_metrics() {
        insta::assert_snapshot!(rfc_report(&fixture_source("cohesion/rfc.py")));
    }
}
