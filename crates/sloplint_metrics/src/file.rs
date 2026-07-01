//! Per-file metric extraction: walk one parsed module into a [`FileMetrics`] (function/class
//! metrics, NLOC, top-level-code ratio, docstrings).

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ModModule, Stmt, StmtClassDef, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{LineIndex, Ranged, TextRange, TextSize, TokenKind};

use sloplint_python::{docstring_range, is_docstring_stmt};

use crate::complexity::{cognitive, cyclomatic, max_nesting};
use crate::exception::exception_stats;
use crate::inheritance::{class_is_abstract, coupling_candidates, static_call_candidates};
use crate::model::{ClassMetrics, FileMetrics, FunctionMetrics};
use crate::size::{caller_arity, exit_count, line_span, ncss, param_count};
use crate::syntactic::syntactic_counts;
use crate::types::type_hint_coverage;
use crate::{cohesion, expr_trailing_name, response};
use sloplint_python::{collect_classes, collect_functions};

/// Compute metrics for one parsed file.
pub fn file_metrics(source: &str, parsed: &Parsed<ModModule>) -> FileMetrics {
    let mut functions = Vec::new();
    collect_functions(&parsed.syntax().body, &mut functions);
    let all_ranges: Vec<TextRange> = functions.iter().map(|f| f.range()).collect();

    let metrics = functions
        .iter()
        .map(|function| {
            let f_range = function.range();
            // Ranges of functions nested inside this one — excluded from its metrics, since
            // each nested function is measured independently.
            let nested: Vec<TextRange> = all_ranges
                .iter()
                .copied()
                .filter(|r| {
                    *r != f_range && r.start() >= f_range.start() && r.end() <= f_range.end()
                })
                .collect();
            function_metrics(source, parsed, function, &nested)
        })
        .collect();

    let mut classes = Vec::new();
    collect_classes(&parsed.syntax().body, &mut classes);
    let class_metrics = classes
        .iter()
        .map(|class| class_metrics(source, parsed, class))
        .collect();

    let comment_lines = parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .count();

    FileMetrics {
        functions: metrics,
        classes: class_metrics,
        loc: source.lines().count(),
        comment_lines,
        nloc: file_nloc(source, parsed),
        exception: exception_stats(&parsed.syntax().body),
        top_level_code: top_level_logic(&parsed.syntax().body),
        function_code: function_logic(&parsed.syntax().body),
    }
}

/// Executable-logic statements at module scope: statements in the module body that are not
/// imports, the module docstring, the `__main__` guard, a def/class declaration, or a pure constant
/// assignment — counted recursively through compound statements (a top-level `for`/`if`/`with`
/// counts its nested body too) but stopping at function/class boundaries.
fn top_level_logic(body: &[Stmt]) -> usize {
    let mut total = 0;
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            // Declarations / imports / type aliases are structure, not "logic dumped at top level".
            Stmt::FunctionDef(_)
            | Stmt::ClassDef(_)
            | Stmt::Import(_)
            | Stmt::ImportFrom(_)
            | Stmt::TypeAlias(_) => {}
            _ if (i == 0 && is_docstring_stmt(stmt))
                || is_main_guard(stmt)
                || is_constant_assign(stmt) => {}
            // `ncss` counts this statement plus its nested body, stopping at any def/class.
            other => total += ncss(std::slice::from_ref(other)),
        }
    }
    total
}

/// Executable-logic statements inside all functions/methods — the sum of each function's own
/// body NCSS (nested-def bodies belong to their own function, counted there), across every function
/// anywhere in the module.
fn function_logic(body: &[Stmt]) -> usize {
    struct Collector {
        total: usize,
    }
    impl Visitor<'_> for Collector {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            if let Stmt::FunctionDef(func) = stmt {
                self.total += ncss(&func.body);
            }
            visitor::walk_stmt(self, stmt);
        }
    }
    let mut collector = Collector { total: 0 };
    for stmt in body {
        collector.visit_stmt(stmt);
    }
    collector.total
}

/// Whether `stmt` is an `if __name__ == "__main__":` guard (either operand order).
fn is_main_guard(stmt: &Stmt) -> bool {
    let Stmt::If(if_stmt) = stmt else {
        return false;
    };
    let Expr::Compare(cmp) = if_stmt.test.as_ref() else {
        return false;
    };
    if cmp.ops.len() != 1 || cmp.comparators.len() != 1 {
        return false;
    }
    let name_is = |e: &Expr| matches!(e, Expr::Name(n) if n.id.as_str() == "__name__");
    let main_is = |e: &Expr| matches!(e, Expr::StringLiteral(s) if s.value.to_str() == "__main__");
    (name_is(&cmp.left) && main_is(&cmp.comparators[0]))
        || (main_is(&cmp.left) && name_is(&cmp.comparators[0]))
}

/// Whether `stmt` is an assignment whose value is a pure constant literal — module-level config, not
/// "logic dumped at top level".
fn is_constant_assign(stmt: &Stmt) -> bool {
    let value = match stmt {
        Stmt::Assign(assign) => Some(assign.value.as_ref()),
        Stmt::AnnAssign(ann) => ann.value.as_deref(),
        _ => None,
    };
    value.is_some_and(is_constant_expr)
}

/// Whether an expression is a constant literal (scalar, or a collection of constants).
fn is_constant_expr(expr: &Expr) -> bool {
    match expr {
        Expr::NumberLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => true,
        Expr::UnaryOp(u) => is_constant_expr(&u.operand),
        Expr::Tuple(t) => t.elts.iter().all(is_constant_expr),
        Expr::List(l) => l.elts.iter().all(is_constant_expr),
        Expr::Set(s) => s.elts.iter().all(is_constant_expr),
        Expr::Dict(d) => d.items.iter().all(|item| {
            item.key.as_ref().is_some_and(is_constant_expr) && is_constant_expr(&item.value)
        }),
        _ => false,
    }
}

/// NLOC for a file: the count of physical lines that carry at least one non-comment,
/// non-trivia token. A line is counted if it bears code or string-literal content; blank lines
/// (no token) and comment-only lines (only a `Comment` token, which `is_trivia`) are not. Lines
/// spanned by a multi-line string literal count as code — consistent with "non-comment,
/// non-blank" — so a blank line *inside* a docstring counts; that's a deliberate, deterministic
/// simplification, immaterial at the god-module scale this metric targets.
fn file_nloc(source: &str, parsed: &Parsed<ModModule>) -> usize {
    let line_index = LineIndex::from_source_text(source);
    let mut code_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for token in parsed.tokens() {
        // `is_trivia` covers comments and the newline/indent/dedent layout tokens; zero-width
        // tokens (e.g. the synthetic EOF) carry no line content.
        if token.kind().is_trivia() || token.range().is_empty() {
            continue;
        }
        let first = line_index.line_index(token.range().start());
        // The end offset is exclusive, so map the last *content* byte to find the closing line.
        let last_byte = token.range().end().checked_sub(TextSize::from(1)).unwrap();
        let last = line_index.line_index(last_byte);
        for line in first.to_zero_indexed()..=last.to_zero_indexed() {
            code_lines.insert(line);
        }
    }
    code_lines.len()
}

fn function_metrics(
    source: &str,
    parsed: &Parsed<ModModule>,
    function: &StmtFunctionDef,
    nested: &[TextRange],
) -> FunctionMetrics {
    let (typed_params, annotatable_params, has_return_annotation) = type_hint_coverage(function);
    let syntactic = syntactic_counts(&function.body);
    FunctionMetrics {
        name: function.name.to_string(),
        range: function.range(),
        name_range: function.name.range(),
        loc: line_span(source, function.range()),
        cyclomatic: cyclomatic(parsed, function.range(), nested),
        cognitive: cognitive(&function.body),
        max_nesting: max_nesting(&function.body, 0),
        params: param_count(&function.parameters),
        arity: caller_arity(function),
        ncss: ncss(&function.body),
        exits: exit_count(&function.body),
        typed_params,
        annotatable_params,
        has_return_annotation,
        has_docstring: docstring_range(&function.body).is_some(),
        docstring_lines: docstring_lines(source, &function.body),
        loop_qty: syntactic.loops,
        comparisons_qty: syntactic.comparisons,
        numbers_qty: syntactic.numbers,
        string_literals_qty: syntactic.strings,
        math_ops_qty: syntactic.math_ops,
        variables_qty: syntactic.variables,
        unique_words_qty: syntactic.unique_words,
    }
}

/// Physical line span of a body's docstring, or 0 if it has none.
fn docstring_lines(source: &str, body: &[Stmt]) -> usize {
    docstring_range(body).map_or(0, |range| line_span(source, range))
}

/// Per-class metrics: size (methods, distinct instance attributes), LCOM4 cohesion, WMC, and RFC
/// (all single-file). `dit`/`noc`/`cbo` are left at 0 here — inheritance depth/breadth and coupling
/// are project-wide properties filled in later by [`resolve_inheritance`], once every file's
/// classes are known.
fn class_metrics(source: &str, parsed: &Parsed<ModModule>, class: &StmtClassDef) -> ClassMetrics {
    let methods = class
        .body
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::FunctionDef(_)))
        .count();
    let cohesion = cohesion::class_cohesion(class);
    ClassMetrics {
        name: class.name.to_string(),
        range: class.range(),
        name_range: class.name.range(),
        loc: line_span(source, class.range()),
        methods,
        attributes: cohesion::class_attribute_count(class),
        lcom4: cohesion.components,
        tcc: cohesion.tcc,
        lcc: cohesion.lcc,
        lcom_star: cohesion.lcom_star,
        wmc: class_wmc(parsed, class),
        dit: 0,
        noc: 0,
        bases: class
            .bases()
            .iter()
            .filter_map(|base| expr_trailing_name(base).map(str::to_string))
            .collect(),
        cbo: 0,
        coupled: coupling_candidates(class),
        static_call_candidates: static_call_candidates(class),
        fan_out: 0,
        fan_in: 0,
        cbo_modified: 0,
        nosi: 0,
        rfc: response::class_rfc(class),
        is_abstract: class_is_abstract(class),
        has_docstring: docstring_range(&class.body).is_some(),
        docstring_lines: docstring_lines(source, &class.body),
    }
}

/// WMC — the sum of the cyclomatic complexity of the class's **direct** methods. Each method is
/// measured own-body (nested defs/lambdas excluded), mirroring [`function_metrics`], so a method
/// and its helpers aren't double-counted. Methods inherited or defined in nested classes are not
/// this class's weight and don't contribute.
fn class_wmc(parsed: &Parsed<ModModule>, class: &StmtClassDef) -> usize {
    class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(method) => Some(method),
            _ => None,
        })
        .map(|method| {
            let mut nested = Vec::new();
            collect_functions(&method.body, &mut nested);
            let nested_ranges: Vec<TextRange> = nested.iter().map(|f| f.range()).collect();
            cyclomatic(parsed, method.range(), &nested_ranges)
        })
        .sum()
}
