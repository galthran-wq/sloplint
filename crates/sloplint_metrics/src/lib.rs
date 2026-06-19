//! Software-quality metrics.
//!
//! Computes only the metrics that empirically track maintainability — function/file length,
//! cyclomatic + cognitive complexity, nesting depth, parameter count, comment density —
//! and skips the empirically-weak ones (Halstead, the Maintainability Index). Cheap,
//! deterministic, token+AST based.

pub mod badge;

use badge::{Badge, Color};
use sloplint_python::ast::{ExceptHandler, ModModule, Parameters, Stmt, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange, TokenKind};

/// McCabe's cyclomatic-complexity risk tiers — the canonical interpretation from McCabe
/// (1976): the higher the decision count, the harder a function is to test and reason about.
/// Boundaries (inclusive): **1–10 low**, **11–20 moderate**, **21–50 high**, **>50 very
/// high**. McCabe recommends prohibiting functions above 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl RiskTier {
    /// Classify a cyclomatic-complexity value into its McCabe risk tier.
    pub fn from_cyclomatic(cyclomatic: usize) -> Self {
        match cyclomatic {
            0..=10 => RiskTier::Low,
            11..=20 => RiskTier::Moderate,
            21..=50 => RiskTier::High,
            _ => RiskTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables, JSON, and badges.
    pub fn label(self) -> &'static str {
        match self {
            RiskTier::Low => "low",
            RiskTier::Moderate => "moderate",
            RiskTier::High => "high",
            RiskTier::VeryHigh => "very high",
        }
    }

    /// Badge color keyed to the tier: low is green, moderate is yellow, high and very-high
    /// are red (both exceed McCabe's recommended ceiling of 10 by a wide margin).
    pub fn color(self) -> Color {
        match self {
            RiskTier::Low => Color::Green,
            RiskTier::Moderate => Color::Yellow,
            RiskTier::High | RiskTier::VeryHigh => Color::Red,
        }
    }
}

/// How many functions fall into each McCabe risk tier across the repo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RiskHistogram {
    pub low: usize,
    pub moderate: usize,
    pub high: usize,
    pub very_high: usize,
}

impl RiskHistogram {
    fn record(&mut self, cyclomatic: usize) {
        match RiskTier::from_cyclomatic(cyclomatic) {
            RiskTier::Low => self.low += 1,
            RiskTier::Moderate => self.moderate += 1,
            RiskTier::High => self.high += 1,
            RiskTier::VeryHigh => self.very_high += 1,
        }
    }

    /// The worst tier that actually has a function in it (the headline risk for a badge).
    /// `None` only when there are no functions at all.
    pub fn worst_tier(self) -> Option<RiskTier> {
        if self.very_high > 0 {
            Some(RiskTier::VeryHigh)
        } else if self.high > 0 {
            Some(RiskTier::High)
        } else if self.moderate > 0 {
            Some(RiskTier::Moderate)
        } else if self.low > 0 {
            Some(RiskTier::Low)
        } else {
            None
        }
    }
}

/// Metrics for a single function.
#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    pub name: String,
    /// Full span of the function statement, decorators included.
    pub range: TextRange,
    /// Span of the function's name identifier — i.e. its `def`/signature line, which (unlike
    /// `range`) is not pushed onto the first decorator. Use this to locate the function.
    pub name_range: TextRange,
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
    /// Mean cyclomatic complexity across all functions.
    pub avg_cyclomatic: f64,
    /// 95th-percentile cyclomatic complexity (nearest-rank). Surfaces the "branchy tail"
    /// even when the mean is dragged down by many trivial accessors.
    pub p95_cyclomatic: usize,
    /// Count of functions in each McCabe risk tier.
    pub cyclomatic_risk: RiskHistogram,
    pub max_cognitive: usize,
    pub max_nesting: usize,
    /// Comment lines as a fraction of total lines (0.0–1.0).
    pub comment_density: f64,
}

impl RepoMetrics {
    /// A badge summarizing cyclomatic-complexity risk: the worst occupied tier plus the peak
    /// value, colored by that tier (`max complexity: 27 (high)`). Color follows the McCabe
    /// tiers, not arbitrary thresholds, so it stays meaningful as the suite grows.
    pub fn cyclomatic_badge(&self) -> Badge {
        match self.cyclomatic_risk.worst_tier() {
            Some(tier) => Badge::new(
                "max complexity",
                format!("{} ({})", self.max_cyclomatic, tier.label()),
                tier.color(),
            ),
            None => Badge::new("max complexity", "n/a", Color::Green),
        }
    }

    /// A one-line-plus-table markdown block for the PR summary: headline aggregates and the
    /// risk-tier histogram. Reproducible from the same inputs; pairs with the badge.
    pub fn cyclomatic_markdown(&self) -> String {
        let risk = self.cyclomatic_risk;
        format!(
            "**Cyclomatic complexity** — mean {:.1}, p95 {}, max {} (worst tier: {}).\n\n\
             | Risk tier | Functions |\n| --- | ---: |\n\
             | low (1–10) | {} |\n| moderate (11–20) | {} |\n\
             | high (21–50) | {} |\n| very high (>50) | {} |\n",
            self.avg_cyclomatic,
            self.p95_cyclomatic,
            self.max_cyclomatic,
            risk.worst_tier().map(RiskTier::label).unwrap_or("n/a"),
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }
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
    let mut cyclomatic_sum = 0usize;
    let mut cyclomatic_values: Vec<usize> = Vec::new();
    for file in files {
        repo.total_loc += file.loc;
        for function in &file.functions {
            repo.functions += 1;
            function_loc_sum += function.loc;
            cyclomatic_sum += function.cyclomatic;
            cyclomatic_values.push(function.cyclomatic);
            repo.cyclomatic_risk.record(function.cyclomatic);
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
    repo.avg_cyclomatic = if repo.functions == 0 {
        0.0
    } else {
        cyclomatic_sum as f64 / repo.functions as f64
    };
    repo.p95_cyclomatic = percentile(&mut cyclomatic_values, 0.95);
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
        name_range: function.name.range(),
        loc: line_span(source, function.range()),
        cyclomatic: cyclomatic(parsed, function.range(), nested),
        cognitive: cognitive(&function.body, 0),
        max_nesting: max_nesting(&function.body, 0),
        params: param_count(&function.parameters),
    }
}

/// Nearest-rank percentile of an unsorted slice (sorts it in place). `p` is a fraction in
/// `0.0..=1.0`. Rank = ceil(p * n), clamped to `1..=n`; returns the value at that 1-based
/// rank. Empty input yields 0. Documented explicitly because percentile conventions differ
/// between tools and the reported number must be reproducible.
fn percentile(values: &mut [usize], p: f64) -> usize {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let n = values.len();
    let rank = (p * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    values[rank - 1]
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
    fn name_range_points_past_decorators() {
        // `range` covers the decorator; `name_range` must land on the `def` line so callers
        // (e.g. the CLI gate) report the function's location, not its decorator.
        let source = "@memoize\ndef f():\n    return 1\n";
        let f = &metrics(source).functions[0];
        let def_line_start = u32::from(f.range.start()) as usize;
        let name_start = u32::from(f.name_range.start()) as usize;
        assert!(source[def_line_start..].starts_with("@memoize"));
        assert!(source[name_start..].starts_with("f("));
    }

    #[test]
    fn risk_tier_boundaries_follow_mccabe() {
        assert_eq!(RiskTier::from_cyclomatic(1), RiskTier::Low);
        assert_eq!(RiskTier::from_cyclomatic(10), RiskTier::Low);
        assert_eq!(RiskTier::from_cyclomatic(11), RiskTier::Moderate);
        assert_eq!(RiskTier::from_cyclomatic(20), RiskTier::Moderate);
        assert_eq!(RiskTier::from_cyclomatic(21), RiskTier::High);
        assert_eq!(RiskTier::from_cyclomatic(50), RiskTier::High);
        assert_eq!(RiskTier::from_cyclomatic(51), RiskTier::VeryHigh);
    }

    #[test]
    fn percentile_nearest_rank() {
        // Empty -> 0.
        assert_eq!(percentile(&mut [], 0.95), 0);
        // Single value -> itself.
        assert_eq!(percentile(&mut [7], 0.95), 7);
        // 1..=20: ceil(0.95*20)=19 -> the 19th smallest = 19.
        let mut v: Vec<usize> = (1..=20).collect();
        assert_eq!(percentile(&mut v, 0.95), 19);
        // Two values: ceil(0.95*2)=2 -> the max. Unsorted input is sorted in place.
        assert_eq!(percentile(&mut [9, 2], 0.95), 9);
    }

    #[test]
    fn histogram_and_worst_tier() {
        let mut h = RiskHistogram::default();
        assert_eq!(h.worst_tier(), None);
        h.record(3); // low
        h.record(15); // moderate
        h.record(15); // moderate
        assert_eq!(h.low, 1);
        assert_eq!(h.moderate, 2);
        assert_eq!(h.worst_tier(), Some(RiskTier::Moderate));
        h.record(60); // very high jumps the worst tier
        assert_eq!(h.worst_tier(), Some(RiskTier::VeryHigh));
    }

    #[test]
    fn aggregate_reports_cyclomatic_distribution() {
        // Two functions: one trivial (CC 1), one branchy (CC 4). Mean = 2.5, max = 4, both
        // land in the low tier, p95 (nearest-rank over 2 values) = the max.
        let source = "\
def a():
    return 1

def b(xs):
    for x in xs:
        if x and x > 0:
            return x
    return 0
";
        let repo = aggregate(&[metrics(source)]);
        assert_eq!(repo.functions, 2);
        assert_eq!(repo.max_cyclomatic, 4);
        assert!((repo.avg_cyclomatic - 2.5).abs() < 1e-9);
        assert_eq!(repo.p95_cyclomatic, 4);
        assert_eq!(repo.cyclomatic_risk.low, 2);
        assert_eq!(repo.cyclomatic_risk.worst_tier(), Some(RiskTier::Low));
    }

    #[test]
    fn cyclomatic_badge_and_markdown_reflect_worst_tier() {
        let mut repo = RepoMetrics {
            functions: 1,
            max_cyclomatic: 27,
            ..RepoMetrics::default()
        };
        repo.cyclomatic_risk.high = 1;
        let badge = repo.cyclomatic_badge();
        assert_eq!(badge.message, "27 (high)");
        assert_eq!(badge.color, Color::Red);
        let md = repo.cyclomatic_markdown();
        assert!(md.contains("worst tier: high"));
        assert!(md.contains("| high (21–50) | 1 |"));
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
