//! Per-function complexity metrics: cyclomatic (McCabe), cognitive (SonarSource), and the
//! deepest control-flow nesting. Each is computed over a single function's own body (nested
//! function/class bodies are measured on their own).

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Comprehension, ExceptHandler, Expr, ModModule, Stmt};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange, TokenKind};

/// Cyclomatic complexity, McCabe (1976): `CC = decisions + 1`.
///
/// Exact counting rules (documented so the number is reproducible — conventions vary between
/// radon/mccabe/lizard). Start at 1, then add 1 for each of the following *tokens* belonging
/// to THIS function:
/// - `if` / `elif` — includes ternary `x if c else y` and comprehension `if` filters, which
///   reuse the `if` keyword token (each adds a branch, matching McCabe's decision count);
/// - `for` / `while` — loop headers, including comprehension `for` clauses;
/// - `except` — each exception handler;
/// - `case` — each `match` arm;
/// - `and` / `or` — each boolean operator (a short-circuit decision point).
///
/// `else`/`finally` add no decision (no alternative branch test). Counting *tokens* rather
/// than source text means keywords inside string literals aren't counted; excluding `nested`
/// ranges keeps a parent from absorbing the complexity of functions defined inside it (those
/// are measured on their own).
pub(crate) fn cyclomatic(
    parsed: &Parsed<ModModule>,
    range: TextRange,
    nested: &[TextRange],
) -> usize {
    let mut count = 1;
    for token in parsed.tokens().iter() {
        let token_range = token.range();
        if token_range.start() < range.start() || token_range.end() > range.end() {
            continue;
        }
        if nested
            .iter()
            .any(|r| token_range.start() >= r.start() && token_range.end() <= r.end())
        {
            continue;
        }
        if is_branch_token(token.kind()) {
            count += 1;
        }
    }
    count
}

fn is_branch_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::If
            | TokenKind::Elif
            | TokenKind::For
            | TokenKind::While
            | TokenKind::Except
            | TokenKind::Case
            | TokenKind::And
            | TokenKind::Or
    )
}

/// Cognitive complexity (SonarSource, Campbell 2018) — a readability-oriented complement to
/// cyclomatic complexity that *penalizes nesting* and ignores shorthand that aids reading. The
/// headline difference from CC: a flat `match` of N cases scores 1 (you read the cases
/// linearly), while N deeply-nested `if`s score far higher.
///
/// Documented increment rules (the exact ruleset this targets):
/// - **+1 plus the current nesting level** for each: `if`, `for`, `while`, `except` handler,
///   ternary (`x if c else y`), and a `match` statement — counted **once**, not per `case`.
/// - **+1 flat** (no nesting) for each: `elif`, `else`, and each boolean-operator *sequence*
///   (one `and`/`or` chain is one `BoolOp` node = +1; `a and b or c` is two sequences = +2).
/// - **+1 plus nesting** for each comprehension `if` filter.
/// - **Nesting deepens by one** inside the body of `if`/`elif`/`else`, `for`/`while` (and their
///   `else`), `except`, and each `match` case.
/// - **No increment and no nesting change** for `try`, `with`, `finally`, and nested
///   function/class declarations (a nested function is measured on its own).
///
/// Boolean ops / ternaries in any condition position are scored — `if`/`while` tests, `for`
/// iterables, `match` subjects and case guards, `with`-item context expressions, and simple
/// statements.
///
/// Documented simplification vs. the full spec: nesting is tracked at the *statement* level —
/// a ternary or boolean op nested inside another expression is scored at its enclosing
/// statement's nesting rather than accruing extra intra-expression nesting. The comprehension
/// generator itself is not counted (only its `if` filters).
pub(crate) fn cognitive(body: &[Stmt]) -> usize {
    let mut scorer = Cognitive::default();
    scorer.block(body, 0);
    scorer.score
}

#[derive(Default)]
struct Cognitive {
    score: usize,
}

impl Cognitive {
    fn block(&mut self, body: &[Stmt], nesting: usize) {
        for stmt in body {
            self.stmt(stmt, nesting);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, nesting: usize) {
        match stmt {
            Stmt::If(node) => {
                self.score += 1 + nesting;
                self.expr(&node.test, nesting);
                self.block(&node.body, nesting + 1);
                for clause in &node.elif_else_clauses {
                    // `elif`/`else`: a flat increment, no nesting penalty.
                    self.score += 1;
                    if let Some(test) = &clause.test {
                        self.expr(test, nesting);
                    }
                    self.block(&clause.body, nesting + 1);
                }
            }
            Stmt::For(node) => {
                self.score += 1 + nesting;
                self.expr(&node.iter, nesting);
                self.block(&node.body, nesting + 1);
                self.block(&node.orelse, nesting + 1);
            }
            Stmt::While(node) => {
                self.score += 1 + nesting;
                self.expr(&node.test, nesting);
                self.block(&node.body, nesting + 1);
                self.block(&node.orelse, nesting + 1);
            }
            Stmt::Try(node) => {
                // `try`/`finally` are not flow breaks: no increment, no nesting change.
                self.block(&node.body, nesting);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    self.score += 1 + nesting;
                    self.block(&handler.body, nesting + 1);
                }
                self.block(&node.orelse, nesting);
                self.block(&node.finalbody, nesting);
            }
            // `with` is not a flow break, but its context expressions can still hold boolean
            // ops / ternaries that count.
            Stmt::With(node) => {
                for item in &node.items {
                    self.expr(&item.context_expr, nesting);
                }
                self.block(&node.body, nesting);
            }
            // A nested function is measured on its own.
            Stmt::FunctionDef(_) => {}
            Stmt::ClassDef(node) => self.block(&node.body, nesting),
            Stmt::Match(node) => {
                self.expr(&node.subject, nesting);
                // The whole `match` is one structure read top-to-bottom — counted ONCE.
                self.score += 1 + nesting;
                for case in &node.cases {
                    // A `case ... if guard:` guard is a condition; score its expression-level
                    // increments (boolean ops / ternaries).
                    if let Some(guard) = &case.guard {
                        self.expr(guard, nesting);
                    }
                    self.block(&case.body, nesting + 1);
                }
            }
            // Simple statement: score the boolean ops / ternaries / comprehensions it contains.
            other => self.scan(other, nesting),
        }
    }

    /// Add the expression-level increments inside a compound statement's condition `expr`.
    fn expr(&mut self, expr: &Expr, nesting: usize) {
        let mut scan = Scan { score: 0, nesting };
        scan.visit_expr(expr);
        self.score += scan.score;
    }

    /// Add the expression-level increments across a whole simple statement.
    fn scan(&mut self, stmt: &Stmt, nesting: usize) {
        let mut scan = Scan { score: 0, nesting };
        scan.visit_stmt(stmt);
        self.score += scan.score;
    }
}

/// Walks an expression tree adding the cognitive increments that live in expressions: each
/// boolean-op sequence (+1), each ternary (+1+nesting), each comprehension `if` filter
/// (+1+nesting). Nesting is fixed for the walk (the documented statement-level simplification).
struct Scan {
    score: usize,
    nesting: usize,
}

impl<'a> Visitor<'a> for Scan {
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::BoolOp(_) => self.score += 1,
            Expr::If(_) => self.score += 1 + self.nesting,
            Expr::ListComp(comp) => self.score += comp_filters(&comp.generators, self.nesting),
            Expr::SetComp(comp) => self.score += comp_filters(&comp.generators, self.nesting),
            Expr::DictComp(comp) => self.score += comp_filters(&comp.generators, self.nesting),
            Expr::Generator(comp) => self.score += comp_filters(&comp.generators, self.nesting),
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

fn comp_filters(generators: &[Comprehension], nesting: usize) -> usize {
    let filters: usize = generators.iter().map(|g| g.ifs.len()).sum();
    filters * (1 + nesting)
}

/// The deepest control-flow nesting of compound statements in `body` (starting at `depth`).
pub(crate) fn max_nesting(body: &[Stmt], depth: usize) -> usize {
    let mut deepest = depth;
    for stmt in body {
        let child = match stmt {
            Stmt::If(node) => {
                let mut d = max_nesting(&node.body, depth + 1);
                for clause in &node.elif_else_clauses {
                    d = d.max(max_nesting(&clause.body, depth + 1));
                }
                d
            }
            Stmt::For(node) => {
                max_nesting(&node.body, depth + 1).max(max_nesting(&node.orelse, depth + 1))
            }
            Stmt::While(node) => {
                max_nesting(&node.body, depth + 1).max(max_nesting(&node.orelse, depth + 1))
            }
            Stmt::With(node) => max_nesting(&node.body, depth + 1),
            Stmt::Try(node) => {
                let mut d = max_nesting(&node.body, depth + 1);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    d = d.max(max_nesting(&handler.body, depth + 1));
                }
                d.max(max_nesting(&node.orelse, depth + 1))
                    .max(max_nesting(&node.finalbody, depth + 1))
            }
            // Nested defs/classes start their own nesting count.
            _ => depth,
        };
        deepest = deepest.max(child);
    }
    deepest
}
