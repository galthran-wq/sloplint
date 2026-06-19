//! Turn each function into a normalized fingerprint for clone detection.
//!
//! Normalization is where the "same logic, slightly different" magic lives: we map every
//! identifier to a single `ID` symbol, every number to `NUM`, every string to `STR`, and
//! keep keywords / operators / structure verbatim. Two functions that differ only in
//! variable and function names therefore produce identical symbol streams (Type-2 clones);
//! functions with small edits produce mostly-overlapping streams (Type-3). We then shingle
//! the stream into k-grams so similarity is a set operation.

use std::collections::HashSet;

use sloplint_python::ast::{ExceptHandler, ModModule, Stmt, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange, TokenKind};

/// A function's clone fingerprint.
pub struct FunctionUnit {
    /// File the function lives in (as the caller supplied it).
    pub file: String,
    /// Function name (for human-readable messages only; not used in matching).
    pub name: String,
    /// Source range of the whole function.
    pub range: TextRange,
    /// Statement count (incl. nested), the size guard against trivially-similar tiny funcs.
    pub statements: usize,
    /// Set of hashed k-gram shingles over the normalized symbol stream.
    pub shingles: HashSet<u64>,
}

// Fixed symbols for normalized token classes. Distinct from operator/keyword hashes, which
// are forced into the high bit range (see `symbol`).
const SYM_ID: u64 = 1;
const SYM_NUM: u64 = 2;
const SYM_STR: u64 = 3;
const SYM_NEWLINE: u64 = 4;
const SYM_INDENT: u64 = 5;
const SYM_DEDENT: u64 = 6;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Extract a fingerprint for every function in `parsed`, descending into classes and
/// compound statements so methods and nested functions are included.
pub fn extract_functions(
    file: &str,
    source: &str,
    parsed: &Parsed<ModModule>,
    shingle_k: usize,
) -> Vec<FunctionUnit> {
    let mut functions = Vec::new();
    collect_functions(&parsed.syntax().body, &mut functions);

    functions
        .into_iter()
        .map(|function| {
            let symbols = normalized_symbols(source, function.range(), parsed);
            FunctionUnit {
                file: file.to_string(),
                name: function.name.to_string(),
                range: function.range(),
                statements: count_statements(&function.body),
                shingles: shingle(&symbols, shingle_k),
            }
        })
        .collect()
}

/// The normalized symbol stream for the tokens inside `range`.
fn normalized_symbols(source: &str, range: TextRange, parsed: &Parsed<ModModule>) -> Vec<u64> {
    let mut symbols = Vec::new();
    for token in parsed.tokens().iter() {
        if token.range().start() < range.start() || token.range().end() > range.end() {
            continue;
        }
        if let Some(symbol) = symbol(token.kind(), &source[token.range()]) {
            symbols.push(symbol);
        }
    }
    symbols
}

/// Map a token to its normalized symbol, or drop it (comments, blank-line noise).
///
/// Not glob-imported: `TokenKind` has a `None` variant (Python's `None`), which would
/// shadow `Option::None`.
fn symbol(kind: TokenKind, text: &str) -> Option<u64> {
    match kind {
        TokenKind::Comment | TokenKind::NonLogicalNewline | TokenKind::EndOfFile => None,
        TokenKind::Name => Some(SYM_ID),
        TokenKind::Int | TokenKind::Float | TokenKind::Complex => Some(SYM_NUM),
        TokenKind::String
        | TokenKind::FStringStart
        | TokenKind::FStringMiddle
        | TokenKind::FStringEnd
        | TokenKind::TStringStart
        | TokenKind::TStringMiddle
        | TokenKind::TStringEnd => Some(SYM_STR),
        TokenKind::Newline => Some(SYM_NEWLINE),
        TokenKind::Indent => Some(SYM_INDENT),
        TokenKind::Dedent => Some(SYM_DEDENT),
        // Keywords, operators, punctuation: keep them verbatim (hashed). The high bit keeps
        // these clear of the small fixed symbols above.
        _ => Some(fnv1a(text.as_bytes()) | (1 << 63)),
    }
}

/// Hash the k-grams of `symbols` into a shingle set. For streams shorter than `k`, the whole
/// stream is one shingle so tiny functions still get a (single) fingerprint.
fn shingle(symbols: &[u64], k: usize) -> HashSet<u64> {
    let k = k.max(1);
    let mut shingles = HashSet::new();
    if symbols.is_empty() {
        return shingles;
    }
    if symbols.len() < k {
        shingles.insert(hash_window(symbols));
        return shingles;
    }
    for window in symbols.windows(k) {
        shingles.insert(hash_window(window));
    }
    shingles
}

fn hash_window(window: &[u64]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &symbol in window {
        hash ^= symbol;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Count statements in a body, including nested ones, as a size proxy.
fn count_statements(body: &[Stmt]) -> usize {
    body.iter().map(|stmt| 1 + nested(stmt)).sum()
}

fn nested(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::FunctionDef(node) => count_statements(&node.body),
        Stmt::ClassDef(node) => count_statements(&node.body),
        Stmt::If(node) => {
            count_statements(&node.body)
                + node
                    .elif_else_clauses
                    .iter()
                    .map(|clause| count_statements(&clause.body))
                    .sum::<usize>()
        }
        Stmt::For(node) => count_statements(&node.body) + count_statements(&node.orelse),
        Stmt::While(node) => count_statements(&node.body) + count_statements(&node.orelse),
        Stmt::With(node) => count_statements(&node.body),
        Stmt::Try(node) => {
            count_statements(&node.body)
                + node
                    .handlers
                    .iter()
                    .map(|handler| {
                        let ExceptHandler::ExceptHandler(handler) = handler;
                        count_statements(&handler.body)
                    })
                    .sum::<usize>()
                + count_statements(&node.orelse)
                + count_statements(&node.finalbody)
        }
        Stmt::Match(node) => node
            .cases
            .iter()
            .map(|case| count_statements(&case.body))
            .sum(),
        _ => 0,
    }
}

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
