//! Software-quality metrics.
//!
//! Computes only the metrics that empirically track maintainability — function/file length,
//! cyclomatic + cognitive complexity, nesting depth, parameter count, comment density —
//! and skips the empirically-weak ones (Halstead, the Maintainability Index). Cheap,
//! deterministic, token+AST based.

pub mod badge;
pub mod cohesion;
pub mod graph;
pub mod modularity;

use badge::{Badge, Color};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{
    Comprehension, ExceptHandler, Expr, ModModule, Parameters, Stmt, StmtClassDef, StmtFunctionDef,
};
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
    /// Non-Commenting Source Statements: count of logical statement nodes in the function's
    /// **own** body. A nested def/class counts as one statement (its declaration) but its body
    /// is excluded — those statements belong to the nested unit's own row — so `ncss` shares
    /// the own-body scope of [`Self::exits`]/[`Self::cognitive`]/[`Self::cyclomatic`]. A
    /// code-size measure that ignores comments, blank lines, and pure-syntax lines, unlike the
    /// physical (nested-inclusive) `loc`.
    pub ncss: usize,
    /// Number of explicit exit points in the function's own body: `return`, `raise`, and
    /// `yield`/`yield from`. Excludes nested defs/lambdas. Multi-exit sprawl is a smell. Note a
    /// `raise` inside an `except` (error translation) is a counted exit — by design, this is the
    /// syntactic count of exit points, not a judgment about which are idiomatic.
    pub exits: usize,
    /// Type-hint coverage (#85): parameters carrying an annotation, out of [`Self::annotatable_params`].
    pub typed_params: usize,
    /// Parameters eligible for an annotation — positional and keyword params, excluding the
    /// `self`/`cls` receiver and `*args`/`**kwargs`. The denominator for parameter annotation
    /// coverage; `0` for a function with no annotatable params (e.g. `def f(self): ...`).
    pub annotatable_params: usize,
    /// Whether the function declares a return-type annotation (`-> T`).
    pub has_return_annotation: bool,
}

/// Metrics for a single class.
#[derive(Debug, Clone)]
pub struct ClassMetrics {
    pub name: String,
    /// Full span of the class statement, decorators included.
    pub range: TextRange,
    /// Span of the class's name identifier.
    pub name_range: TextRange,
    /// Physical lines spanned by the class.
    pub loc: usize,
    /// Methods directly in the class body (including constructors).
    pub methods: usize,
    /// Distinct instance attributes (`self.x` references that are not methods).
    pub attributes: usize,
    /// LCOM4 cohesion: connected components among non-constructor methods. >1 = low cohesion
    /// ("god class" that should be split). See [`cohesion`].
    pub lcom4: usize,
    /// Whether this class counts as "abstract" for Martin's package abstractness ratio (#70).
    /// A documented heuristic ([`class_is_abstract`]), since Python has no interface keyword.
    pub is_abstract: bool,
}

/// Metrics for a single file.
#[derive(Debug, Clone)]
pub struct FileMetrics {
    pub functions: Vec<FunctionMetrics>,
    pub classes: Vec<ClassMetrics>,
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
    /// Type-hint coverage (#85): annotated params / annotatable params across all functions
    /// (0.0–1.0). Low coverage flags under-annotation; high coverage is neutral, never a smell.
    pub param_annotation_coverage: f64,
    /// Fraction of functions that are fully annotated — every annotatable param plus the return
    /// type (0.0–1.0).
    pub fully_annotated_function_rate: f64,
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

    let mut classes = Vec::new();
    collect_classes(&parsed.syntax().body, &mut classes);
    let class_metrics = classes
        .iter()
        .map(|class| class_metrics(source, class))
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
    let mut typed_params_sum = 0usize;
    let mut annotatable_params_sum = 0usize;
    let mut fully_annotated = 0usize;
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
            typed_params_sum += function.typed_params;
            annotatable_params_sum += function.annotatable_params;
            // Fully annotated = every annotatable param typed *and* a return type. A function with
            // no annotatable params still needs its return annotated to count.
            if function.has_return_annotation
                && function.typed_params == function.annotatable_params
            {
                fully_annotated += 1;
            }
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
    repo.param_annotation_coverage = if annotatable_params_sum == 0 {
        0.0
    } else {
        typed_params_sum as f64 / annotatable_params_sum as f64
    };
    repo.fully_annotated_function_rate = if repo.functions == 0 {
        0.0
    } else {
        fully_annotated as f64 / repo.functions as f64
    };
    repo
}

fn function_metrics(
    source: &str,
    parsed: &Parsed<ModModule>,
    function: &StmtFunctionDef,
    nested: &[TextRange],
) -> FunctionMetrics {
    let (typed_params, annotatable_params, has_return_annotation) = type_hint_coverage(function);
    FunctionMetrics {
        name: function.name.to_string(),
        range: function.range(),
        name_range: function.name.range(),
        loc: line_span(source, function.range()),
        cyclomatic: cyclomatic(parsed, function.range(), nested),
        cognitive: cognitive(&function.body),
        max_nesting: max_nesting(&function.body, 0),
        params: param_count(&function.parameters),
        ncss: ncss(&function.body),
        exits: exit_count(&function.body),
        typed_params,
        annotatable_params,
        has_return_annotation,
    }
}

/// Per-class metrics: size (methods, distinct instance attributes) and LCOM4 cohesion.
fn class_metrics(source: &str, class: &StmtClassDef) -> ClassMetrics {
    let methods = class
        .body
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::FunctionDef(_)))
        .count();
    ClassMetrics {
        name: class.name.to_string(),
        range: class.range(),
        name_range: class.name.range(),
        loc: line_span(source, class.range()),
        methods,
        attributes: cohesion::class_attribute_count(class),
        lcom4: cohesion::class_cohesion(class).components,
        is_abstract: class_is_abstract(class),
    }
}

/// Heuristic for whether a class is "abstract" for Martin's package abstractness ratio (#70).
/// Python has no interface keyword, so this approximates — a class counts as abstract if it:
///
/// - subclasses `ABC` / `abc.ABC` or `Protocol` / `typing.Protocol` (incl. subscripted
///   `Protocol[T]`),
/// - declares `metaclass=ABCMeta`, or
/// - has any method decorated with `@abstractmethod` (or the `abstractproperty` /
///   `abstractclassmethod` / `abstractstaticmethod` family).
///
/// This is deliberately an approximation — abstractness is fuzzy in Python, and #70 ships the
/// derived metric clearly labeled as heuristic — but it only fires on the genuine abstract-base /
/// protocol idioms. We pointedly do *not* treat a stub body (`class Foo(Bar): ...`) as a signal:
/// such a class has no `def`, so a whole-class stub is always a concrete leaf/marker — an empty
/// exception subclass (`class ReadError(NetworkError): ...`) or a sentinel (`class UnsetType: ...`),
/// not an interface. Counting those inflated Abstractness ~5× on exception-heavy modules and skewed
/// Distance `D` (#81).
fn class_is_abstract(class: &StmtClassDef) -> bool {
    let abstract_base = class
        .bases()
        .iter()
        .filter_map(expr_trailing_name)
        .any(|name| name == "ABC" || name == "Protocol");

    let abc_metaclass = class.keywords().iter().any(|keyword| {
        keyword
            .arg
            .as_ref()
            .is_some_and(|arg| arg.as_str() == "metaclass")
            && expr_trailing_name(&keyword.value) == Some("ABCMeta")
    });

    let has_abstractmethod = class.body.iter().any(|stmt| match stmt {
        Stmt::FunctionDef(func) => func
            .decorator_list
            .iter()
            .filter_map(|decorator| expr_trailing_name(&decorator.expression))
            .any(|name| name.starts_with("abstract")),
        _ => false,
    });

    abstract_base || abc_metaclass || has_abstractmethod
}

/// Trailing identifier of a (possibly dotted or subscripted) name expression — `ABC` from
/// `abc.ABC`, `Protocol` from `typing.Protocol[T]` — or `None` for anything that doesn't name a
/// class (a call, a literal, …).
fn expr_trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Subscript(subscript) => expr_trailing_name(&subscript.value),
        _ => None,
    }
}

/// Number of explicit exit points in the function's own body: `return`, `raise`, and
/// `yield`/`yield from`. Does not descend into nested defs/lambdas (those exits belong to the
/// nested scope).
fn exit_count(body: &[Stmt]) -> usize {
    struct Counter {
        n: usize,
    }
    impl Visitor<'_> for Counter {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {} // nested scope
                Stmt::Return(_) | Stmt::Raise(_) => {
                    self.n += 1;
                    visitor::walk_stmt(self, stmt);
                }
                _ => visitor::walk_stmt(self, stmt),
            }
        }
        fn visit_expr(&mut self, expr: &Expr) {
            match expr {
                Expr::Lambda(_) => {} // nested scope
                Expr::Yield(_) | Expr::YieldFrom(_) => {
                    self.n += 1;
                    visitor::walk_expr(self, expr);
                }
                _ => visitor::walk_expr(self, expr),
            }
        }
    }
    let mut counter = Counter { n: 0 };
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.n
}

/// Non-Commenting Source Statements: count every statement node in the function's own body.
/// A nested def/class counts as a single statement (its declaration) but we don't descend into
/// it — its statements belong to that nested unit's own row, so `ncss` stays own-body like
/// `exit_count`/`cognitive`, never double-counting a helper's body into its parent. Comments
/// and blank lines are not statements, so they're naturally excluded — a logical code-size
/// measure distinct from physical `loc`.
fn ncss(body: &[Stmt]) -> usize {
    struct Counter {
        n: usize,
    }
    impl Visitor<'_> for Counter {
        fn visit_stmt(&mut self, stmt: &Stmt) {
            self.n += 1;
            match stmt {
                // The nested def/class declaration counts (above); its body is a separate unit.
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
                _ => visitor::walk_stmt(self, stmt),
            }
        }
    }
    let mut counter = Counter { n: 0 };
    for stmt in body {
        counter.visit_stmt(stmt);
    }
    counter.n
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

/// Whether a function carries `@staticmethod` — so its first parameter is a genuine argument, not
/// a `self`/`cls` receiver.
fn is_staticmethod(function: &StmtFunctionDef) -> bool {
    function
        .decorator_list
        .iter()
        .filter_map(|decorator| expr_trailing_name(&decorator.expression))
        .any(|name| name == "staticmethod")
}

/// Type-hint coverage for one function signature (#85): `(typed_params, annotatable_params,
/// has_return_annotation)`.
///
/// *Annotatable* params are the positional and keyword params (positional-only + regular +
/// keyword-only). The `self`/`cls` receiver of a non-static method is excluded — it is
/// conventionally unannotated and not a quality signal — as are `*args`/`**kwargs`, which are
/// variadic collectors that are rarely annotated and would only dilute the ratio. A function with
/// no annotatable params yields `0/0`: it contributes nothing to coverage rather than being
/// penalized.
///
/// This measures *under*-annotation as a quality concern (missing types are harder to read and
/// refactor and weaken tooling). The "bad" direction is **low** coverage only — fully-typed code
/// is neutral-to-good and is never itself a slop signal (slop is badness, not provenance).
fn type_hint_coverage(function: &StmtFunctionDef) -> (usize, usize, bool) {
    let params = &function.parameters;
    // The receiver, when present, is the first positional parameter (positional-only first,
    // otherwise the first regular arg). Drop exactly one leading positional for a non-static
    // method whose first parameter is named `self`/`cls`.
    let skip_receiver = usize::from(
        !is_staticmethod(function)
            && params
                .posonlyargs
                .first()
                .or_else(|| params.args.first())
                .is_some_and(|param| matches!(param.parameter.name.as_str(), "self" | "cls")),
    );

    let mut annotatable = 0usize;
    let mut typed = 0usize;
    for param in params
        .posonlyargs
        .iter()
        .chain(&params.args)
        .chain(&params.kwonlyargs)
        .skip(skip_receiver)
    {
        annotatable += 1;
        if param.parameter.annotation.is_some() {
            typed += 1;
        }
    }
    (typed, annotatable, function.returns.is_some())
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
fn cognitive(body: &[Stmt]) -> usize {
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

/// Recursively collect every class definition (top-level, nested in functions, classes, or
/// compound statements), mirroring [`collect_functions`].
fn collect_classes<'a>(body: &'a [Stmt], out: &mut Vec<&'a StmtClassDef>) {
    for stmt in body {
        match stmt {
            Stmt::ClassDef(node) => {
                out.push(node);
                collect_classes(&node.body, out);
            }
            Stmt::FunctionDef(node) => collect_classes(&node.body, out),
            Stmt::If(node) => {
                collect_classes(&node.body, out);
                for clause in &node.elif_else_clauses {
                    collect_classes(&clause.body, out);
                }
            }
            Stmt::For(node) => {
                collect_classes(&node.body, out);
                collect_classes(&node.orelse, out);
            }
            Stmt::While(node) => {
                collect_classes(&node.body, out);
                collect_classes(&node.orelse, out);
            }
            Stmt::With(node) => collect_classes(&node.body, out),
            Stmt::Try(node) => {
                collect_classes(&node.body, out);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    collect_classes(&handler.body, out);
                }
                collect_classes(&node.orelse, out);
                collect_classes(&node.finalbody, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_classes(&case.body, out);
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

    /// Cognitive complexity of the first function in `source`.
    fn cog(source: &str) -> usize {
        metrics(source).functions[0].cognitive
    }

    #[test]
    fn flat_match_scores_far_below_a_nested_tangle() {
        // The headline of SonarSource cognitive complexity: a flat `match` is read linearly.
        let flat = "\
def classify(x):
    match x:
        case 1:
            return \"one\"
        case 2:
            return \"two\"
        case 3:
            return \"three\"
        case _:
            return \"other\"
";
        // The whole match is one structure (+1), counted once regardless of case count.
        assert_eq!(cog(flat), 1);

        let tangle = "\
def tangle(a, b, c):
    if a:
        if b:
            if c:
                return 1
    return 0
";
        // if(1+0) + if(1+1) + if(1+2) = 6 — nesting is penalized.
        assert_eq!(cog(tangle), 6);
    }

    #[test]
    fn boolean_operator_sequences_each_add_one() {
        // `a and b and c` is one And sequence (+1); `a and b or c` is two sequences (+2).
        let source = "\
def f(a, b, c):
    if a and b and c:
        return 1
    if a and b or c:
        return 2
    return 0
";
        // if(1) + 1 boolop  +  if(1) + 2 boolops  = 2 + 3 = 5
        assert_eq!(cog(source), 5);
    }

    #[test]
    fn ternary_and_comprehension_filters_count_with_nesting() {
        assert_eq!(
            cog("def f(a, b):\n    x = a if b else 0\n    return x\n"),
            1
        );
        // ternary nested inside an `if` body: if(1) + ternary(1+1) = 3
        assert_eq!(
            cog("def f(a, b, c):\n    if a:\n        return b if c else 0\n    return 0\n"),
            3
        );
        // a comprehension `if` filter, top level: +1
        assert_eq!(cog("def f(xs):\n    return [x for x in xs if x > 0]\n"), 1);
        // comprehension filter nested in a loop: for(1) + filter(1+1) = 3
        assert_eq!(
            cog("def f(xss):\n    out = []\n    for xs in xss:\n        out.append([x for x in xs if x])\n    return out\n"),
            3
        );
    }

    #[test]
    fn with_and_try_are_not_flow_breaks() {
        // `with` adds no increment and no nesting, so the inner `if` stays at level 0.
        assert_eq!(
            cog("def f(path):\n    with open(path) as fh:\n        if fh:\n            return 1\n    return 0\n"),
            1
        );
        // `try` adds nothing; only the `except` handler increments.
        assert_eq!(
            cog("def f(x):\n    try:\n        return risky(x)\n    except ValueError:\n        return 0\n"),
            1
        );
    }

    #[test]
    fn else_adds_a_flat_increment() {
        // if(1+0) + else(+1 flat) = 2
        assert_eq!(
            cog("def f(a):\n    if a:\n        return 1\n    else:\n        return 0\n"),
            2
        );
    }

    #[test]
    fn match_guard_conditions_are_scored() {
        // match(+1) + the guard's `a and b` boolean op (+1) = 2 (regression: guards were dropped).
        let source = "\
def f(x, a, b):
    match x:
        case 1 if a and b:
            return 1
        case _:
            return 0
";
        assert_eq!(cog(source), 2);
    }

    #[test]
    fn with_item_conditions_are_scored() {
        // `with` is not a flow break, but the `a and b` in its context expression counts (+1).
        assert_eq!(
            cog("def f(a, b):\n    with make(a and b) as fh:\n        return fh\n"),
            1
        );
    }

    #[test]
    fn nested_ternary_uses_statement_level_nesting() {
        // Documented simplification: both ternaries score at the statement's nesting (0), so
        // `a if b else (c if d else e)` is 1 + 1 = 2, not 1 + 2.
        assert_eq!(
            cog("def f(a, b, c, d, e):\n    return a if b else (c if d else e)\n"),
            2
        );
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

    #[test]
    fn ncss_counts_statements_not_lines() {
        // body: if (1) + raise (1) + aug-assign (1) + return (1) = 4 statements; the blank line
        // and the `def` header are not counted.
        let f = &metrics(
            "\
def add(self, n):
    if n < 0:

        raise ValueError(n)
    self.total += n
    return self.total
",
        )
        .functions[0];
        assert_eq!(f.ncss, 4);
        assert!(f.loc > f.ncss, "physical loc exceeds logical ncss");
    }

    #[test]
    fn ncss_is_own_body_excluding_nested_def_bodies() {
        // outer's own body: `def helper` (1, the declaration) + `return` (1) = 2. helper's two
        // statements (a = 1; return a) belong to helper's own row, not outer's.
        let file = metrics(
            "\
def outer():
    def helper():
        a = 1
        return a
    return helper()
",
        );
        let outer = file.functions.iter().find(|f| f.name == "outer").unwrap();
        let helper = file.functions.iter().find(|f| f.name == "helper").unwrap();
        assert_eq!(
            outer.ncss, 2,
            "nested helper body excluded from outer's ncss"
        );
        assert_eq!(helper.ncss, 2);
    }

    #[test]
    fn exits_count_return_raise_yield_excluding_nested() {
        let f = &metrics(
            "\
def f(x):
    def nested():
        return 1          # nested scope — not counted
    if x:
        raise ValueError
    yield x
    return x
",
        )
        .functions[0];
        assert_eq!(
            f.exits, 3,
            "raise + yield + return (nested return excluded)"
        );
    }

    #[test]
    fn class_metrics_report_size_and_lcom4() {
        let file = metrics(
            "\
class Counter:
    def __init__(self):
        self.total = 0
        self.name = 'c'
    def add(self, n):
        self.total += n
    def show(self):
        return self.total

class Utils:
    def parse(self, t):
        return self.parser.run(t)
    def render(self, n):
        return self.formatter.go(n)
",
        );
        let counter = &file.classes[0];
        assert_eq!(counter.name, "Counter");
        assert_eq!(counter.methods, 3);
        assert_eq!(counter.attributes, 2); // total, name
        assert_eq!(counter.lcom4, 1, "all methods share self.total");

        let utils = &file.classes[1];
        assert_eq!(utils.methods, 2);
        assert_eq!(utils.lcom4, 2, "parse/render touch disjoint attributes");
    }

    /// Every documented abstractness signal (#70) is recognized, and a plain concrete class is
    /// not. One class per source so `file.classes[0]` is unambiguous.
    #[test]
    fn class_is_abstract_recognizes_each_signal() {
        let abstract_cases = [
            ("abc.ABC base", "import abc\nclass A(abc.ABC):\n    def f(self): return 1\n"),
            ("bare ABC base", "from abc import ABC\nclass A(ABC):\n    def f(self): return 1\n"),
            ("Protocol base", "from typing import Protocol\nclass A(Protocol):\n    def f(self): ...\n"),
            ("subscripted Protocol", "from typing import Protocol\nclass A(Protocol[int]):\n    def f(self): ...\n"),
            ("ABCMeta metaclass", "import abc\nclass A(metaclass=abc.ABCMeta):\n    def f(self): return 1\n"),
            ("@abstractmethod", "from abc import abstractmethod\nclass A:\n    @abstractmethod\n    def f(self): ...\n"),
            ("@abstractproperty family", "import abc\nclass A:\n    @abc.abstractproperty\n    def f(self): ...\n"),
            // A stub body is still abstract when paired with a genuine signal (Protocol base): the
            // base, not the `...`, is what counts.
            ("Protocol base, stub body", "from typing import Protocol\nclass A(Protocol): ...\n"),
        ];
        for (label, src) in abstract_cases {
            assert!(
                metrics(src).classes[0].is_abstract,
                "expected abstract: {label}"
            );
        }

        let concrete_cases = [
            ("plain class", "class A:\n    def f(self): return 1\n"),
            (
                "non-abstract base",
                "class B: pass\nclass A(B):\n    def f(self): return 1\n",
            ),
            (
                "docstring + real method",
                "class A:\n    \"\"\"doc\"\"\"\n    def f(self): return 1\n",
            ),
            (
                "non-metaclass keyword",
                "class A(foo=1):\n    def f(self): return 1\n",
            ),
            // #81: a whole-class stub is a concrete leaf/marker, not an abstraction. A stub body
            // has no `def`, so these can never be interfaces.
            ("empty exception subclass", "class E(ValueError): ...\n"),
            ("ellipsis marker, no base", "class Marker:\n    ...\n"),
            ("pass marker, no base", "class Marker:\n    pass\n"),
            ("docstring-only marker", "class Marker:\n    \"\"\"just a marker\"\"\"\n"),
            ("sentinel stub", "class UnsetType: ...\n"),
        ];
        for (label, src) in concrete_cases {
            let classes = &metrics(src).classes;
            // The class under test is the last one defined (a helper base may precede it).
            assert!(
                !classes.last().unwrap().is_abstract,
                "expected concrete: {label}"
            );
        }
    }

    /// #81: an exception-heavy module (the httpx idiom) must not read as nearly-all-abstract. Only
    /// the genuine ABC counts; the empty exception subclasses and the sentinel are concrete, so
    /// Abstractness here is 1/6, not 6/6.
    #[test]
    fn empty_exception_subclasses_are_not_abstract() {
        let src = "\
from abc import ABC, abstractmethod

class HTTPError(Exception): ...
class TimeoutException(HTTPError): ...
class ConnectTimeout(TimeoutException): ...
class ReadError(HTTPError): ...
class UnsetType: ...

class BaseTransport(ABC):
    @abstractmethod
    def handle_request(self): ...
";
        let classes = &metrics(src).classes;
        let abstract_names: Vec<&str> = classes
            .iter()
            .filter(|c| c.is_abstract)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            abstract_names,
            vec!["BaseTransport"],
            "only the genuine ABC is abstract; exception subclasses and the sentinel are concrete"
        );
    }

    /// #85: type-hint coverage on a single signature — the receiver is excluded, `*args`/`**kwargs`
    /// don't count, and the return annotation is reported separately.
    #[test]
    fn type_hint_coverage_counts_annotatable_params() {
        let f = |src: &str| {
            let m = &metrics(src).functions[0];
            (m.typed_params, m.annotatable_params, m.has_return_annotation)
        };

        assert_eq!(f("def g(a, b): ...\n"), (0, 2, false), "no hints");
        assert_eq!(f("def g(a: int, b) -> str: ...\n"), (1, 2, true), "partial + return");
        assert_eq!(f("def g(a: int, b: str) -> None: ...\n"), (2, 2, true), "full");
        // `self` is excluded from the denominator; `cls` likewise.
        assert_eq!(f("class C:\n    def m(self, a: int): ...\n"), (1, 1, false), "self excluded");
        assert_eq!(
            f("class C:\n    @classmethod\n    def m(cls, a): ...\n"),
            (0, 1, false),
            "cls excluded"
        );
        // A staticmethod has no receiver — its first param counts.
        assert_eq!(
            f("class C:\n    @staticmethod\n    def m(a: int): ...\n"),
            (1, 1, false),
            "staticmethod first param counts"
        );
        // *args/**kwargs are not annotatable params.
        assert_eq!(f("def g(a: int, *args, **kwargs): ...\n"), (1, 1, false), "variadics ignored");
        // A positional-only receiver (`self, /`) is still the receiver and is excluded.
        assert_eq!(
            f("class C:\n    def m(self, /, a: int): ...\n"),
            (1, 1, false),
            "positional-only self excluded"
        );
        // Keyword-only params (after a bare `*`) count toward the denominator.
        assert_eq!(f("def g(*, a: int, b): ...\n"), (1, 2, false), "keyword-only counted");
        // `async def` is a function definition too — same treatment.
        assert_eq!(f("async def g(a: int) -> str: ...\n"), (1, 1, true), "async");
        // No annotatable params: a zero denominator, return type tracked independently.
        assert_eq!(f("def g() -> int: ...\n"), (0, 0, true), "nullary with return");
    }

    /// #85: project aggregates — coverage is over the param pool, and the fully-annotated rate
    /// requires every annotatable param *and* a return type.
    #[test]
    fn type_hint_aggregates_over_the_project() {
        // 4 annotatable params total (1 + 2 + 1), 2 typed → 1/2 coverage. Only `full` is fully
        // annotated (all params typed + return), so 1/3 of functions.
        let src = "\
def full(a: int) -> int: ...
def partial(a: int, b) -> int: ...
def bare(a): ...
";
        let repo = aggregate(&[metrics(src)]);
        assert_eq!(repo.functions, 3);
        assert!(
            (repo.param_annotation_coverage - 0.5).abs() < 1e-9,
            "2 of 4 params typed, got {}",
            repo.param_annotation_coverage
        );
        assert!(
            (repo.fully_annotated_function_rate - 1.0 / 3.0).abs() < 1e-9,
            "1 of 3 functions fully annotated, got {}",
            repo.fully_annotated_function_rate
        );

        // No annotatable params anywhere → coverage is a neutral 0.0, not a div-by-zero.
        let none = aggregate(&[metrics("def f(): ...\n")]);
        assert_eq!(none.param_annotation_coverage, 0.0);
    }
}
