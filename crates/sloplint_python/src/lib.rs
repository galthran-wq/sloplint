//! Thin wrappers over Ruff's Python parser.
//!
//! sloplint reuses Ruff's battle-tested lexer/parser rather than maintaining its own.
//! This crate is the single seam between sloplint and the pinned `ruff_*` crates: every
//! other crate depends on the types re-exported here, so a future parser bump is a
//! one-file change.

pub use ruff_python_ast as ast;
pub use ruff_python_ast::token::{TokenKind, Tokens};
pub use ruff_python_parser as parser;
pub use ruff_source_file::{LineIndex, OneIndexed};
pub use ruff_text_size::{Ranged, TextRange, TextSize};

use ruff_python_ast::ModModule;
use ruff_python_parser::{parse_module, ParseError, Parsed};

/// A Python syntax error. Wraps Ruff's [`ParseError`] so the seam — not the underlying
/// `ruff_*` crate — owns the public error type of [`parse`]; `Display` is forwarded unchanged.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct PythonError(#[from] ParseError);

/// Parse Python source into a [`Parsed`] tree: AST (`syntax()`) and token stream
/// (`tokens()`).
///
/// Returns `Err` when the source contains syntax errors. (Ruff's parser builds a
/// best-effort tree internally and is error-resilient; `parse_module` surfaces that as a
/// hard error if any problems were found, which is the behavior a linter wants — don't
/// lint code that doesn't parse.)
pub fn parse(source: &str) -> Result<Parsed<ModModule>, PythonError> {
    Ok(parse_module(source)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_module() {
        let parsed = parse("def add(a, b):\n    return a + b\n").expect("valid source parses");
        // One top-level statement (the function def), and a non-empty token stream.
        assert_eq!(parsed.syntax().body.len(), 1);
        assert!(matches!(parsed.syntax().body[0], ast::Stmt::FunctionDef(_)));
        assert!(!parsed.tokens().is_empty());
    }

    #[test]
    fn rejects_invalid_syntax() {
        // A linter should refuse to lint code that doesn't parse.
        assert!(parse("def f(:\n    pass\n").is_err());
    }
}
