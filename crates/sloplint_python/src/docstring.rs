//! PEP 257 docstring helpers shared across crates: a body's leading bare string-literal
//! expression. Single-sourced in the seam so the metrics engine and the linter rules can't
//! drift on what counts as a docstring.

use ruff_python_ast::{Expr, ExprStringLiteral, Stmt};
use ruff_text_size::{Ranged, TextRange};

/// The leading docstring of a body — its first statement, when that is a bare string-literal
/// expression (PEP 257) — or `None` for any other leading statement.
fn docstring(body: &[Stmt]) -> Option<&ExprStringLiteral> {
    match body.first()? {
        Stmt::Expr(expr) => match expr.value.as_ref() {
            Expr::StringLiteral(literal) => Some(literal),
            _ => None,
        },
        _ => None,
    }
}

/// Whether `stmt` is a bare string-literal expression statement (a docstring).
pub fn is_docstring_stmt(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_)))
}

/// Source range of a body's docstring (its leading bare string-literal expression), or `None`.
pub fn docstring_range(body: &[Stmt]) -> Option<TextRange> {
    docstring(body).map(|literal| literal.range())
}

/// Text of a body's docstring (its leading bare string-literal expression), or `None`.
pub fn docstring_text(body: &[Stmt]) -> Option<&str> {
    docstring(body).map(|literal| literal.value.to_str())
}
