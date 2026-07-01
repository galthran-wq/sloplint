//! Per-function syntactic `*Qty` counters (the CK `*Qty` family): cheap single-pass AST tallies of
//! loops, comparisons, numeric/string literals, binary math/bit operations, distinct local
//! variables, and the distinct-identifier vocabulary. All are **own-body** measures — like
//! [`ncss`](crate::size) and `exit_count` they do not descend into nested `def`/`class`/`lambda`
//! bodies, which are measured on their own row.
//!
//! (CK's `parenthesizedExpsQty` is intentionally absent: Python's AST does not model grouping
//! parentheses as nodes — unlike the Eclipse JDT `ParenthesizedExpression` CK reads — so it needs a
//! token-level pass rather than this AST walk.)

use std::collections::HashSet;

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Stmt};

/// The finalized per-function syntactic counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SyntacticCounts {
    /// `for`/`while` statements plus comprehension `for` clauses (generators): the count of
    /// iteration constructs. A double comprehension (`[.. for a in .. for b in ..]`) is 2.
    pub loops: usize,
    /// Comparison operators — each operator in a chain counts (`a < b < c` is 2). All Python
    /// comparison operators count, including `is`/`is not`/`in`/`not in`.
    pub comparisons: usize,
    /// Numeric-literal occurrences (int/float/complex), not deduplicated.
    pub numbers: usize,
    /// String-literal occurrences (plain and f-strings; byte literals excluded). A bare-string
    /// docstring is itself a string literal, so it counts.
    pub strings: usize,
    /// Binary arithmetic and bitwise operator occurrences (`+ - * / // % ** @` and `& | ^ << >>`).
    /// Augmented assignments (`+=`) and unary signs are not counted.
    pub math_ops: usize,
    /// Distinct local names bound as an assignment/loop/`with`/comprehension target in the own body.
    /// Parameters are excluded — they have their own count ([`crate::FunctionMetrics::params`]).
    pub variables: usize,
    /// Distinct identifiers referenced in the own body — the vocabulary. Python keywords are syntax,
    /// not identifiers, so they are naturally excluded (no keyword stoplist needed).
    pub unique_words: usize,
}

/// Tally the [`SyntacticCounts`] over a function's own body in a single walk.
pub(crate) fn syntactic_counts(body: &[Stmt]) -> SyntacticCounts {
    let mut visitor = SyntacticVisitor {
        loops: 0,
        comparisons: 0,
        numbers: 0,
        strings: 0,
        math_ops: 0,
        variables: HashSet::new(),
        words: HashSet::new(),
    };
    for stmt in body {
        visitor.visit_stmt(stmt);
    }
    SyntacticCounts {
        loops: visitor.loops,
        comparisons: visitor.comparisons,
        numbers: visitor.numbers,
        strings: visitor.strings,
        math_ops: visitor.math_ops,
        variables: visitor.variables.len(),
        unique_words: visitor.words.len(),
    }
}

struct SyntacticVisitor<'a> {
    loops: usize,
    comparisons: usize,
    numbers: usize,
    strings: usize,
    math_ops: usize,
    variables: HashSet<&'a str>,
    words: HashSet<&'a str>,
}

impl<'a> Visitor<'a> for SyntacticVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            // Nested scopes are their own unit (matches `ncss`/`exit_count`).
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::For(_) | Stmt::While(_) => {
                self.loops += 1;
                visitor::walk_stmt(self, stmt);
            }
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return, // nested scope — its own row.
            // A comprehension's `for` clauses are loops; still recurse for the inner expressions.
            Expr::ListComp(c) => self.loops += c.generators.len(),
            Expr::SetComp(c) => self.loops += c.generators.len(),
            Expr::DictComp(c) => self.loops += c.generators.len(),
            Expr::Generator(c) => self.loops += c.generators.len(),
            Expr::Compare(c) => self.comparisons += c.ops.len(),
            Expr::NumberLiteral(_) => self.numbers += 1,
            Expr::StringLiteral(_) | Expr::FString(_) => self.strings += 1,
            Expr::BinOp(_) => self.math_ops += 1,
            Expr::Name(name) => {
                let id = name.id.as_str();
                self.words.insert(id);
                if name.ctx.is_store() {
                    self.variables.insert(id);
                }
            }
            _ => {}
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

    /// The seven syntactic counters for every top-level function in a fixture, in source order.
    /// The fixture documents each function's expected counts; the snapshot pins them.
    fn syntactic_report(source: &str) -> String {
        let parsed = parse(source).expect("fixture parses");
        let mut out = String::new();
        for stmt in &parsed.syntax().body {
            if let Stmt::FunctionDef(func) = stmt {
                let c = syntactic_counts(&func.body);
                writeln!(
                    out,
                    "{}: loops={} comparisons={} numbers={} strings={} math_ops={} variables={} unique_words={}",
                    func.name,
                    c.loops,
                    c.comparisons,
                    c.numbers,
                    c.strings,
                    c.math_ops,
                    c.variables,
                    c.unique_words,
                )
                .unwrap();
            }
        }
        out
    }

    #[test]
    fn syntactic_counters() {
        insta::assert_snapshot!(syntactic_report(&fixture_source("complexity/syntactic.py")));
    }
}
