//! Turn each function into a normalized fingerprint for clone detection.
//!
//! Normalization is where the "same logic, slightly different" magic lives: we map every
//! identifier to a single `ID` symbol, every number to `NUM`, every string to `STR`, and
//! keep keywords / operators / structure verbatim. Two functions that differ only in
//! variable and function names therefore produce identical symbol streams (Type-2 clones);
//! functions with small edits produce mostly-overlapping streams (Type-3). We then shingle
//! the stream into k-grams so similarity is a set operation.

use std::collections::HashSet;

use sloplint_python::ast::{
    CmpOp, ExceptHandler, Expr, ModModule, Operator, Stmt, StmtFunctionDef,
};
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
    canonicalize: bool,
) -> Vec<FunctionUnit> {
    let mut functions = Vec::new();
    collect_functions(&parsed.syntax().body, &mut functions);

    functions
        .into_iter()
        .map(|function| {
            // Type-4: an AST walk that sorts commutative operands so reshuffled-but-equivalent
            // code fingerprints identically. Opt-in — the default token stream is unchanged.
            let symbols = if canonicalize {
                canonical_symbols(function, source, parsed)
            } else {
                normalized_symbols(source, function.range(), parsed)
            };
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

// ---- Type-4 (semantic) canonicalization -----------------------------------------------------
//
// A structural AST walk that emits a normalized symbol stream like the token path, but
// *canonicalizes commutative operands*: the children of `+ * & | ^`, boolean `and`/`or`, and
// symmetric `== !=` are sorted into a stable order, so `a + b` and `b + a` (and longer
// reshuffles) fingerprint identically. Node kinds get structural markers kept clear of the
// token symbols (`SYM_*` are tiny; operator/keyword hashes set the high bit). Constructs we
// don't model (comprehensions, lambdas, `with` items, …) fall back to the precise token stream
// for that node's range — deterministic, just without internal canonicalization.
//
// Heuristic, hence opt-in: `+`/`*` are treated as commutative even though they aren't for
// str/list operands. A single mis-ordered operator can't manufacture a clone on its own — the
// whole function must still clear the Jaccard threshold — so this trades a little soundness for
// real Type-4 recall, behind a flag.

/// Base for structural node-kind markers — above `SYM_*` (1–6), below the high-bit operator
/// hashes, so the three symbol spaces never collide.
const MARK: u64 = 0x4000_0000;

fn mark(kind: u64) -> u64 {
    MARK + kind
}

fn is_commutative(op: Operator) -> bool {
    matches!(
        op,
        Operator::Add | Operator::Mult | Operator::BitAnd | Operator::BitOr | Operator::BitXor
    )
}

/// The canonical symbol stream for a whole function body.
///
/// `emit_stmt`/`emit_expr` recurse with the AST's nesting depth; ruff's parser bounds that
/// (it rejects over-deep source before we get here), so there's no unbounded-recursion risk.
fn canonical_symbols(
    function: &StmtFunctionDef,
    source: &str,
    parsed: &Parsed<ModModule>,
) -> Vec<u64> {
    let mut out = Vec::new();
    emit_body(&function.body, source, parsed, &mut out);
    out
}

fn emit_body(body: &[Stmt], source: &str, parsed: &Parsed<ModModule>, out: &mut Vec<u64>) {
    for stmt in body {
        emit_stmt(stmt, source, parsed, out);
    }
}

fn emit_stmt(stmt: &Stmt, source: &str, parsed: &Parsed<ModModule>, out: &mut Vec<u64>) {
    match stmt {
        Stmt::Expr(node) => {
            out.push(mark(1));
            emit_expr(&node.value, source, parsed, out);
        }
        Stmt::Return(node) => {
            out.push(mark(2));
            if let Some(value) = &node.value {
                emit_expr(value, source, parsed, out);
            }
        }
        Stmt::Assign(node) => {
            out.push(mark(3));
            for target in &node.targets {
                emit_expr(target, source, parsed, out);
            }
            emit_expr(&node.value, source, parsed, out);
        }
        Stmt::AugAssign(node) => {
            out.push(mark(4));
            emit_expr(&node.target, source, parsed, out);
            out.push(mark(100 + node.op as u64));
            emit_expr(&node.value, source, parsed, out);
        }
        Stmt::AnnAssign(node) => {
            out.push(mark(5));
            emit_expr(&node.target, source, parsed, out);
            if let Some(value) = &node.value {
                emit_expr(value, source, parsed, out);
            }
        }
        Stmt::If(node) => {
            out.push(mark(6));
            emit_expr(&node.test, source, parsed, out);
            emit_body(&node.body, source, parsed, out);
            for clause in &node.elif_else_clauses {
                out.push(mark(7));
                if let Some(test) = &clause.test {
                    emit_expr(test, source, parsed, out);
                }
                emit_body(&clause.body, source, parsed, out);
            }
        }
        Stmt::For(node) => {
            out.push(mark(8));
            emit_expr(&node.target, source, parsed, out);
            emit_expr(&node.iter, source, parsed, out);
            emit_body(&node.body, source, parsed, out);
            emit_body(&node.orelse, source, parsed, out);
        }
        Stmt::While(node) => {
            out.push(mark(9));
            emit_expr(&node.test, source, parsed, out);
            emit_body(&node.body, source, parsed, out);
            emit_body(&node.orelse, source, parsed, out);
        }
        Stmt::FunctionDef(node) => {
            out.push(mark(10));
            emit_body(&node.body, source, parsed, out);
        }
        Stmt::ClassDef(node) => {
            out.push(mark(11));
            emit_body(&node.body, source, parsed, out);
        }
        // Constructs we don't model structurally: fall back to the precise token stream.
        other => out.extend(normalized_symbols(source, other.range(), parsed)),
    }
}

fn emit_expr(expr: &Expr, source: &str, parsed: &Parsed<ModModule>, out: &mut Vec<u64>) {
    match expr {
        Expr::Name(_) => out.push(SYM_ID),
        Expr::NumberLiteral(_) => out.push(SYM_NUM),
        Expr::StringLiteral(_) | Expr::FString(_) | Expr::BytesLiteral(_) => out.push(SYM_STR),
        Expr::BooleanLiteral(_) => out.push(mark(20)),
        Expr::NoneLiteral(_) => out.push(mark(21)),
        Expr::EllipsisLiteral(_) => out.push(mark(22)),
        Expr::BinOp(node) => {
            out.push(mark(30));
            out.push(mark(100 + node.op as u64));
            let mut left = Vec::new();
            emit_expr(&node.left, source, parsed, &mut left);
            let mut right = Vec::new();
            emit_expr(&node.right, source, parsed, &mut right);
            if is_commutative(node.op) && right < left {
                std::mem::swap(&mut left, &mut right);
            }
            out.extend(left);
            out.extend(right);
        }
        Expr::BoolOp(node) => {
            out.push(mark(31));
            out.push(mark(130 + node.op as u64));
            // `and`/`or` chains are commutative for structural matching — sort the operands.
            let mut operands: Vec<Vec<u64>> = node
                .values
                .iter()
                .map(|value| {
                    let mut sub = Vec::new();
                    emit_expr(value, source, parsed, &mut sub);
                    sub
                })
                .collect();
            operands.sort();
            for operand in operands {
                out.extend(operand);
            }
        }
        Expr::UnaryOp(node) => {
            out.push(mark(32));
            out.push(mark(150 + node.op as u64));
            emit_expr(&node.operand, source, parsed, out);
        }
        Expr::Compare(node) => {
            out.push(mark(33));
            // A single symmetric comparison (`==`/`!=`) is order-independent; sort the sides.
            if node.ops.len() == 1 && matches!(node.ops[0], CmpOp::Eq | CmpOp::NotEq) {
                out.push(mark(140 + node.ops[0] as u64));
                let mut left = Vec::new();
                emit_expr(&node.left, source, parsed, &mut left);
                let mut right = Vec::new();
                emit_expr(&node.comparators[0], source, parsed, &mut right);
                if right < left {
                    std::mem::swap(&mut left, &mut right);
                }
                out.extend(left);
                out.extend(right);
            } else {
                emit_expr(&node.left, source, parsed, out);
                for (op, comparator) in node.ops.iter().zip(node.comparators.iter()) {
                    out.push(mark(140 + *op as u64));
                    emit_expr(comparator, source, parsed, out);
                }
            }
        }
        Expr::Call(node) => {
            out.push(mark(34));
            emit_expr(&node.func, source, parsed, out);
            for arg in node.arguments.args.iter() {
                emit_expr(arg, source, parsed, out);
            }
            for keyword in node.arguments.keywords.iter() {
                out.push(mark(35));
                emit_expr(&keyword.value, source, parsed, out);
            }
        }
        Expr::Attribute(node) => {
            out.push(mark(36));
            emit_expr(&node.value, source, parsed, out);
            out.push(SYM_ID); // the attribute name
        }
        Expr::Subscript(node) => {
            out.push(mark(37));
            emit_expr(&node.value, source, parsed, out);
            emit_expr(&node.slice, source, parsed, out);
        }
        Expr::Tuple(node) => {
            out.push(mark(38));
            for elt in &node.elts {
                emit_expr(elt, source, parsed, out);
            }
        }
        Expr::List(node) => {
            out.push(mark(39));
            for elt in &node.elts {
                emit_expr(elt, source, parsed, out);
            }
        }
        Expr::Set(node) => {
            out.push(mark(40));
            // Set literals are unordered — sort the elements.
            let mut elts: Vec<Vec<u64>> = node
                .elts
                .iter()
                .map(|elt| {
                    let mut sub = Vec::new();
                    emit_expr(elt, source, parsed, &mut sub);
                    sub
                })
                .collect();
            elts.sort();
            for elt in elts {
                out.extend(elt);
            }
        }
        Expr::Dict(node) => {
            out.push(mark(41));
            // Dict preserves insertion order — keep order.
            for item in &node.items {
                if let Some(key) = &item.key {
                    emit_expr(key, source, parsed, out);
                }
                emit_expr(&item.value, source, parsed, out);
            }
        }
        Expr::If(node) => {
            out.push(mark(42));
            emit_expr(&node.test, source, parsed, out);
            emit_expr(&node.body, source, parsed, out);
            emit_expr(&node.orelse, source, parsed, out);
        }
        Expr::Starred(node) => {
            out.push(mark(43));
            emit_expr(&node.value, source, parsed, out);
        }
        Expr::Named(node) => {
            out.push(mark(44));
            emit_expr(&node.target, source, parsed, out);
            emit_expr(&node.value, source, parsed, out);
        }
        // Comprehensions, lambdas, await/yield, slices, …: fall back to the token stream.
        other => out.extend(normalized_symbols(source, other.range(), parsed)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    /// Canonical symbol stream for the first top-level function in `source`.
    fn canon(source: &str) -> Vec<u64> {
        let parsed = parse(source).expect("valid python");
        let Stmt::FunctionDef(function) = &parsed.syntax().body[0] else {
            panic!("expected a function def");
        };
        canonical_symbols(function, source, &parsed)
    }

    // Operands shaped differently (a Name vs a Call) so order is structurally visible, not
    // hidden by identifier normalization.
    #[test]
    fn commutative_binops_are_order_independent() {
        assert_eq!(
            canon("def f():\n    return a + g(b)\n"),
            canon("def f():\n    return g(b) + a\n")
        );
        assert_eq!(
            canon("def f():\n    return a * g(b)\n"),
            canon("def f():\n    return g(b) * a\n")
        );
        assert_eq!(
            canon("def f():\n    return a | g(b)\n"),
            canon("def f():\n    return g(b) | a\n")
        );
    }

    #[test]
    fn non_commutative_binops_preserve_order() {
        assert_ne!(
            canon("def f():\n    return a - g(b)\n"),
            canon("def f():\n    return g(b) - a\n")
        );
        assert_ne!(
            canon("def f():\n    return a / g(b)\n"),
            canon("def f():\n    return g(b) / a\n")
        );
        assert_ne!(
            canon("def f():\n    return a ** g(b)\n"),
            canon("def f():\n    return g(b) ** a\n")
        );
    }

    #[test]
    fn boolean_chains_are_order_independent() {
        assert_eq!(
            canon("def f():\n    return p and q(x) and r\n"),
            canon("def f():\n    return r and p and q(x)\n"),
        );
    }

    #[test]
    fn symmetric_compare_commutes_but_ordering_does_not() {
        assert_eq!(
            canon("def f():\n    return a == g(b)\n"),
            canon("def f():\n    return g(b) == a\n")
        );
        assert_eq!(
            canon("def f():\n    return a != g(b)\n"),
            canon("def f():\n    return g(b) != a\n")
        );
        assert_ne!(
            canon("def f():\n    return a < g(b)\n"),
            canon("def f():\n    return g(b) < a\n")
        );
        // `==` and `!=` are distinct comparisons, not interchangeable.
        assert_ne!(
            canon("def f():\n    return a == g(b)\n"),
            canon("def f():\n    return a != g(b)\n")
        );
    }

    #[test]
    fn chained_compares_are_not_canonicalized() {
        // Only a single symmetric comparison is sorted; a chain emits in order. (Structurally
        // distinct ends — a call vs a name — so the flip is visible past identifier folding.)
        assert_ne!(
            canon("def f():\n    return h(c) < g(b) < a\n"),
            canon("def f():\n    return a < g(b) < h(c)\n")
        );
    }

    #[test]
    fn and_does_not_collapse_into_or() {
        assert_ne!(
            canon("def f():\n    return p and q(x)\n"),
            canon("def f():\n    return p or q(x)\n")
        );
    }

    #[test]
    fn different_operators_stay_distinct() {
        // Canonicalization must not collapse `+` and `*` into the same fingerprint.
        assert_ne!(
            canon("def f():\n    return a + g(b)\n"),
            canon("def f():\n    return a * g(b)\n")
        );
    }
}
