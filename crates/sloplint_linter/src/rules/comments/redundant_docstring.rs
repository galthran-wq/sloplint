//! SLP002: redundant docstring.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::{ExceptHandler, Expr, Stmt, StmtFunctionDef};
use sloplint_python::{Ranged, TextRange};

use crate::lint::{FileContext, Rule};
use crate::words::{content_words, overlap_ratio};

const OVERLAP_THRESHOLD: f64 = 0.5;
const MIN_DOC_WORDS: usize = 2;
const MAX_DOC_WORDS: usize = 12;

/// Flags a function/method docstring that merely restates the signature and body —
/// `"""Return the sum of a and b."""` over `def add(a, b): return a + b`. Docstrings that
/// introduce external concepts (units, algorithms, invariants) have low overlap and are
/// left alone. Module/class docstrings are out of scope. Preview group until tuned.
pub struct RedundantDocstring;

impl Rule for RedundantDocstring {
    fn code(&self) -> &'static str {
        "SLP002"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut functions = Vec::new();
        collect_functions(&ctx.parsed.syntax().body, &mut functions);
        for function in functions {
            if let Some(doc_range) = docstring_range(function) {
                check_docstring(ctx.source, function, doc_range, diagnostics);
            }
        }
    }
}

/// The range of a function's docstring (its first statement, if a string literal).
fn docstring_range(function: &StmtFunctionDef) -> Option<TextRange> {
    let Stmt::Expr(expr) = function.body.first()? else {
        return None;
    };
    // Works whether `value` is `Expr` or `Box<Expr>` (deref coercion at the binding).
    let value: &Expr = &expr.value;
    match value {
        Expr::StringLiteral(string) => Some(string.range()),
        _ => None,
    }
}

fn check_docstring(
    source: &str,
    function: &StmtFunctionDef,
    doc_range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let doc_words = content_words(&source[doc_range]);
    if doc_words.len() < MIN_DOC_WORDS || doc_words.len() > MAX_DOC_WORDS {
        return;
    }

    // The function's "code", excluding the docstring's own text: the signature plus the
    // body after the docstring. Excluding the docstring is essential — otherwise it would
    // trivially overlap itself.
    let func_start = u32::from(function.range().start()) as usize;
    let func_end = u32::from(function.range().end()) as usize;
    let doc_start = u32::from(doc_range.start()) as usize;
    let doc_end = u32::from(doc_range.end()) as usize;
    let mut code = String::with_capacity(func_end - func_start);
    code.push_str(&source[func_start..doc_start]);
    code.push(' ');
    code.push_str(&source[doc_end..func_end]);
    let code_words = content_words(&code);

    if overlap_ratio(&doc_words, &code_words) >= OVERLAP_THRESHOLD {
        diagnostics.push(Diagnostic::new(
            "SLP002",
            "docstring restates the signature (redundant docstring)",
            doc_range,
            Severity::Warning,
        ));
    }
}

/// Collect every function definition reachable in `body`, descending through classes and
/// compound statements so nested and method definitions are covered.
fn collect_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(function) => {
                out.push(function);
                collect_functions(&function.body, out);
            }
            Stmt::ClassDef(class) => collect_functions(&class.body, out),
            Stmt::If(node) => {
                collect_functions(&node.body, out);
                for clause in &node.elif_else_clauses {
                    collect_functions(&clause.body, out);
                }
            }
            Stmt::For(node) => {
                collect_functions(&node.body, out);
                collect_functions(&node.orelse, out);
            }
            Stmt::While(node) => {
                collect_functions(&node.body, out);
                collect_functions(&node.orelse, out);
            }
            Stmt::With(node) => collect_functions(&node.body, out),
            Stmt::Try(node) => {
                collect_functions(&node.body, out);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    collect_functions(&handler.body, out);
                }
                collect_functions(&node.orelse, out);
                collect_functions(&node.finalbody, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_functions(&case.body, out);
                }
            }
            _ => {}
        }
    }
}
