//! Software-quality metrics.
//!
//! Computes only the metrics that empirically track maintainability — function/file length,
//! cyclomatic + cognitive complexity, nesting depth, parameter count, comment density —
//! and skips the empirically-weak ones (Halstead, the Maintainability Index). Cheap,
//! deterministic, token+AST based.

pub mod badge;

use sloplint_python::ast::{ExceptHandler, ModModule, Parameters, Stmt, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange, TokenKind};

/// Metrics for a single function.
#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    pub name: String,
    pub range: TextRange,
    /// Physical lines spanned by the function.
    pub loc: usize,
    /// Cyclomatic complexity: 1 + number of decision points.
    pub cyclomatic: usize,
    /// Cognitive complexity (SonarSource-style: control structures cost more when nested).
    pub cognitive: usize,
    /// Deepest nesting of compound statements inside the function.
    pub max_nesting: usize,
    /// Number of declared parameters.
    pub params: usize,
}

/// Metrics for a single file.
#[derive(Debug, Clone)]
pub struct FileMetrics {
    pub functions: Vec<FunctionMetrics>,
    pub loc: usize,
    pub comment_lines: usize,
}

/// Aggregated metrics across many files — what the badges and PR summary report.
#[derive(Debug, Clone, Default)]
pub struct RepoMetrics {
    pub files: usize,
    pub functions: usize,
    pub total_loc: usize,
    pub avg_function_loc: f64,
    pub max_function_loc: usize,
    pub max_cyclomatic: usize,
    pub max_cognitive: usize,
    pub max_nesting: usize,
    /// Comment lines as a fraction of total lines (0.0–1.0).
    pub comment_density: f64,
}

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

    let comment_lines = parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .count();

    FileMetrics {
        functions: metrics,
        loc: source.lines().count(),
        comment_lines,
    }
}

/// Aggregate per-file metrics into repo-level figures.
pub fn aggregate(files: &[FileMetrics]) -> RepoMetrics {
    let mut repo = RepoMetrics {
        files: files.len(),
        ..RepoMetrics::default()
    };
    let mut function_loc_sum = 0usize;
    for file in files {
        repo.total_loc += file.loc;
        for function in &file.functions {
            repo.functions += 1;
            function_loc_sum += function.loc;
            repo.max_function_loc = repo.max_function_loc.max(function.loc);
            repo.max_cyclomatic = repo.max_cyclomatic.max(function.cyclomatic);
            repo.max_cognitive = repo.max_cognitive.max(function.cognitive);
            repo.max_nesting = repo.max_nesting.max(function.max_nesting);
        }
    }
    repo.avg_function_loc = if repo.functions == 0 {
        0.0
    } else {
        function_loc_sum as f64 / repo.functions as f64
    };
    let comment_lines: usize = files.iter().map(|f| f.comment_lines).sum();
    repo.comment_density = if repo.total_loc == 0 {
        0.0
    } else {
        comment_lines as f64 / repo.total_loc as f64
    };
    repo
}

fn function_metrics(
    source: &str,
    parsed: &Parsed<ModModule>,
    function: &StmtFunctionDef,
    nested: &[TextRange],
) -> FunctionMetrics {
    FunctionMetrics {
        name: function.name.to_string(),
        range: function.range(),
        loc: line_span(source, function.range()),
        cyclomatic: cyclomatic(parsed, function.range(), nested),
        cognitive: cognitive(&function.body, 0),
        max_nesting: max_nesting(&function.body, 0),
        params: param_count(&function.parameters),
    }
}

fn line_span(source: &str, range: TextRange) -> usize {
    let start = u32::from(range.start()) as usize;
    let end = (u32::from(range.end()) as usize).min(source.len());
    source[start..end].lines().count().max(1)
}

fn param_count(parameters: &Parameters) -> usize {
    parameters.posonlyargs.len()
        + parameters.args.len()
        + parameters.kwonlyargs.len()
        + usize::from(parameters.vararg.is_some())
        + usize::from(parameters.kwarg.is_some())
}

/// Cyclomatic complexity from the token stream: 1 + each branch keyword token (`if`/`elif`/
/// `for`/`while`/`except`/`case`) and boolean operator (`and`/`or`) belonging to THIS
/// function. Counting *tokens* (not source text) means branch keywords inside string
/// literals aren't counted; excluding `nested` ranges keeps a parent from absorbing the
/// complexity of functions defined inside it (those are measured on their own).
fn cyclomatic(parsed: &Parsed<ModModule>, range: TextRange, nested: &[TextRange]) -> usize {
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

/// Cognitive complexity: each control structure adds `1 + nesting`, and nesting deepens as
/// we descend. A flattened structure scores lower than a deeply nested one of equal size.
fn cognitive(body: &[Stmt], depth: usize) -> usize {
    let mut score = 0;
    for stmt in body {
        match stmt {
            Stmt::If(node) => {
                score += 1 + depth;
                score += cognitive(&node.body, depth + 1);
                for clause in &node.elif_else_clauses {
                    // `elif`/`else` each add a small flat increment.
                    score += 1;
                    score += cognitive(&clause.body, depth + 1);
                }
            }
            Stmt::For(node) => {
                score += 1 + depth;
                score += cognitive(&node.body, depth + 1);
                score += cognitive(&node.orelse, depth + 1);
            }
            Stmt::While(node) => {
                score += 1 + depth;
                score += cognitive(&node.body, depth + 1);
                score += cognitive(&node.orelse, depth + 1);
            }
            Stmt::Try(node) => {
                score += cognitive(&node.body, depth);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    score += 1 + depth;
                    score += cognitive(&handler.body, depth + 1);
                }
                score += cognitive(&node.orelse, depth + 1);
                score += cognitive(&node.finalbody, depth);
            }
            Stmt::With(node) => score += cognitive(&node.body, depth + 1),
            // A nested function is measured on its own; don't fold its score into ours.
            Stmt::FunctionDef(_) => {}
            Stmt::ClassDef(node) => score += cognitive(&node.body, depth),
            Stmt::Match(node) => {
                for case in &node.cases {
                    score += 1 + depth;
                    score += cognitive(&case.body, depth + 1);
                }
            }
            _ => {}
        }
    }
    score
}

fn max_nesting(body: &[Stmt], depth: usize) -> usize {
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

/// Collect functions (methods + nested) so each is measured independently.
fn collect_functions<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtFunctionDef>) {
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(function) => {
                out.push(function);
                collect_functions(&function.body, out);
            }
            Stmt::ClassDef(node) => collect_functions(&node.body, out),
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

    fn metrics(source: &str) -> FileMetrics {
        file_metrics(source, &parse(source).unwrap())
    }

    #[test]
    fn measures_a_simple_function() {
        let m = metrics("def add(a, b):\n    return a + b\n");
        assert_eq!(m.functions.len(), 1);
        let f = &m.functions[0];
        assert_eq!(f.params, 2);
        assert_eq!(f.cyclomatic, 1);
        assert_eq!(f.cognitive, 0);
        assert_eq!(f.max_nesting, 0);
    }

    #[test]
    fn complexity_and_nesting_grow_with_branches() {
        let source = "\
def f(xs):
    total = 0
    for x in xs:
        if x and x > 0:
            total += x
    return total
";
        let f = &metrics(source).functions[0];
        // for + if + and  => 1 + 3
        assert_eq!(f.cyclomatic, 4);
        assert_eq!(f.max_nesting, 2);
        assert!(f.cognitive >= 3);
    }

    #[test]
    fn keywords_in_strings_do_not_inflate_complexity() {
        // Regression: branch keywords inside a string literal must not be counted.
        let source = "def f():\n    msg = \"if and or while for except\"\n    return msg\n";
        assert_eq!(metrics(source).functions[0].cyclomatic, 1);
    }

    #[test]
    fn nested_function_logic_not_double_counted() {
        let source = "\
def outer(xs):
    def inner(x):
        if x:
            return 1
        return 0
    return [inner(x) for x in xs]
";
        let file = metrics(source);
        let outer = file.functions.iter().find(|f| f.name == "outer").unwrap();
        let inner = file.functions.iter().find(|f| f.name == "inner").unwrap();
        // inner owns the `if`; outer owns only the comprehension `for`.
        assert_eq!(inner.cyclomatic, 2);
        assert_eq!(outer.cyclomatic, 2);
        // outer's cognitive must not absorb inner's branch.
        assert_eq!(outer.cognitive, 0);
        assert!(inner.cognitive >= 1);
    }

    #[test]
    fn aggregate_avg_and_max() {
        let source = "\
def a():
    return 1

def b(x):
    if x:
        return 1
    return 0
";
        let file = metrics(source);
        let repo = aggregate(&[file]);
        assert_eq!(repo.functions, 2);
        assert!(repo.max_cyclomatic >= 2);
        assert!(repo.avg_function_loc > 0.0);
    }
}
