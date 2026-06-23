//! Software-quality metrics.
//!
//! Computes only the metrics that empirically track maintainability — function/file length,
//! cyclomatic + cognitive complexity, nesting depth, parameter count, comment density —
//! and skips the empirically-weak ones (Halstead, the Maintainability Index). Cheap,
//! deterministic, token+AST based.

pub mod badge;
pub mod cohesion;
mod collect;
mod complexity;
mod exception;
pub mod graph;
mod inheritance;
mod model;
pub mod modularity;
mod risk;
mod size;
pub mod test_proxies;
mod types;

#[cfg(test)]
mod testing;

pub use inheritance::resolve_inheritance;
pub use model::{ClassMetrics, ExceptionStats, FileMetrics, FunctionMetrics};
pub use risk::{
    CboTier, CognitiveTier, ModuleSizeTier, NocTier, ParamCountTier, RiskHistogram, RiskTier,
    WmcTier,
};

use badge::{Badge, Color};
use collect::{collect_classes, collect_functions};
use complexity::{cognitive, cyclomatic, max_nesting};
use exception::exception_stats;
use inheritance::{class_is_abstract, coupling_candidates};
use size::{caller_arity, exit_count, line_span, ncss, param_count};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ModModule, Stmt, StmtClassDef, StmtFunctionDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{LineIndex, Ranged, TextRange, TextSize, TokenKind};
use types::type_hint_coverage;

/// Aggregated metrics across many files — what the badges and PR summary report.
#[derive(Debug, Clone, Default)]
pub struct RepoMetrics {
    pub files: usize,
    pub functions: usize,
    pub total_loc: usize,
    pub avg_function_loc: f64,
    pub max_function_loc: usize,
    /// Longest function whose cognitive complexity is ≥ [`LOGIC_FUNCTION_MIN_COGNITIVE`] —
    /// the longest *logic* function, excluding straight-line data/config-init blobs (a 2,733-line
    /// `__init__` of `self.x = …` assignments has cognitive ~1 and is left out). This is the
    /// god-function signal `max_function_loc` mis-ranks; report both.
    pub max_logic_function_loc: usize,
    pub max_cyclomatic: usize,
    /// Mean cyclomatic complexity across all functions.
    pub avg_cyclomatic: f64,
    /// 95th-percentile cyclomatic complexity (nearest-rank). Surfaces the "branchy tail"
    /// even when the mean is dragged down by many trivial accessors.
    pub p95_cyclomatic: usize,
    /// Count of functions in each McCabe risk tier.
    pub cyclomatic_risk: RiskHistogram,
    /// Mean caller-facing arity across all functions ([`FunctionMetrics::arity`]).
    pub avg_params: f64,
    /// Highest caller-facing arity — the worst Long Parameter List, which the mean hides.
    pub max_params: usize,
    /// 95th-percentile caller-facing arity (nearest-rank) — the heavy tail, mirroring
    /// [`Self::p95_cyclomatic`].
    pub p95_params: usize,
    /// Count of functions in each arity band — Long-Parameter-List *prevalence*. Counts
    /// caller-facing arity (`self`/`cls` excluded, `*args`/`**kwargs` once). Descriptive bands
    /// ([`ParamCountTier`]), never a gate.
    pub param_count_risk: RiskHistogram,
    pub max_cognitive: usize,
    /// Mean cognitive complexity across all functions.
    pub avg_cognitive: f64,
    /// 95th-percentile cognitive complexity (nearest-rank) — the "hard-to-read tail", mirroring
    /// [`Self::p95_cyclomatic`].
    pub p95_cognitive: usize,
    /// Count of functions in each cognitive readability band ([`CognitiveTier`]). Brings
    /// cognitive to parity with cyclomatic, which already has full distribution + tiers; cognitive
    /// is the better readability signal, so its distribution (not just the max) is the one to watch.
    pub cognitive_risk: RiskHistogram,
    pub max_nesting: usize,
    /// Comment lines as a fraction of total lines (0.0–1.0).
    pub comment_density: f64,
    /// Type-hint coverage: annotated params / annotatable params across all functions
    /// (0.0–1.0). Low coverage flags under-annotation; high coverage is neutral, never a smell.
    pub param_annotation_coverage: f64,
    /// Fraction of functions that are fully annotated — every annotatable param plus the return
    /// type (0.0–1.0).
    pub fully_annotated_function_rate: f64,
    /// Mean module NLOC across all files — the size triad's third leg.
    pub avg_module_nloc: f64,
    /// Largest module by NLOC. The single god-module the repo sum/`avg` would otherwise hide.
    pub max_module_nloc: usize,
    /// 95th-percentile module NLOC (nearest-rank) — the heavy tail, mirroring
    /// [`Self::p95_cyclomatic`]/[`Self::p95_wmc`].
    pub p95_module_nloc: usize,
    /// Count of files in each module-size band — god-module *prevalence*, which the repo
    /// `total_loc` sum and the `avg` collapse. Descriptive bands ([`ModuleSizeTier`]), never a
    /// gate.
    pub module_size_risk: RiskHistogram,
    /// Mean top-level-code ratio across modules that contain executable logic — how
    /// undecomposed the code is on average. Modules with no logic (pure libraries) are excluded so
    /// they don't dilute the signal. `0.0` when no module has logic.
    pub avg_top_level_ratio: f64,
    /// Highest top-level-code ratio of any module — the most script-like file.
    pub max_top_level_ratio: f64,
    /// Count of **undecomposed** modules: non-trivial modules (≥ [`TOP_LEVEL_MIN_LOGIC`]
    /// logic statements) whose top-level-code ratio is ≥ [`TOP_LEVEL_RATIO_THRESHOLD`] — procedural
    /// script-dumps that complexity and module-size metrics miss. Descriptive, never a gate.
    pub undecomposed_modules: usize,
    /// Number of classes across all files — the denominator for the WMC/DIT averages.
    pub classes: usize,
    /// Heaviest class by WMC (sum of its methods' cyclomatic complexity).
    pub max_wmc: usize,
    /// Mean WMC across all classes.
    pub avg_wmc: f64,
    /// 95th-percentile class WMC (nearest-rank). Surfaces the heavy tail even when the mean is
    /// pulled down by many tiny classes — the WMC counterpart to [`Self::p95_cyclomatic`].
    pub p95_wmc: usize,
    /// Count of classes in each WMC size band — god-class *prevalence*, which `avg`/`max`
    /// alone hide: the same `max_wmc` can come from one justified hub or many. Descriptive bands
    /// ([`WmcTier`]), never a gate.
    pub wmc_risk: RiskHistogram,
    /// Deepest first-party inheritance chain (DIT). Requires [`resolve_inheritance`] to have run
    /// over the file set first; otherwise every `dit` is 0.
    pub max_dit: usize,
    /// Mean DIT across all classes.
    pub avg_dit: f64,
    /// Most direct first-party subclasses any class has (NOC) — the worst fragile-base-class
    /// blast radius. Requires [`resolve_inheritance`] to have run.
    pub max_noc: usize,
    /// Mean NOC across all classes.
    pub avg_noc: f64,
    /// 95th-percentile class NOC (nearest-rank) — the breadth tail; most classes are leaves
    /// (NOC 0), so p95 surfaces the hubs the mean buries.
    pub p95_noc: usize,
    /// Count of classes in each NOC breadth band — fragile-base-class *prevalence*.
    /// Descriptive bands ([`NocTier`]), never a gate.
    pub noc_risk: RiskHistogram,
    /// Most first-party classes any single class couples to (CBO) — the worst hub. Requires
    /// [`resolve_inheritance`] to have run. A lower bound in dynamically-typed code.
    pub max_cbo: usize,
    /// Mean CBO across all classes.
    pub avg_cbo: f64,
    /// 95th-percentile class CBO (nearest-rank) — the coupling tail; most classes couple to few,
    /// so p95 surfaces the hubs the mean buries.
    pub p95_cbo: usize,
    /// Count of classes in each CBO coupling band — hub-class *prevalence*. Descriptive
    /// bands ([`CboTier`]), never a gate; a lower bound on dynamically-typed code.
    pub cbo_risk: RiskHistogram,
    /// Docstring coverage: public defs/classes carrying a docstring, as a fraction of all public
    /// defs/classes (0.0–1.0). "Public" = a name not `_`-prefixed. Distinct from
    /// `comment_density` (which counts `#`-comments, not docstrings) — low coverage flags an
    /// under-documented public API. 0.0 when there are no public units.
    pub docstring_coverage: f64,
    /// Docstring-to-code ratio: total **function** docstring lines over total **function** NCSS
    /// (which counts the docstring's own expression statement). Function-scoped on both sides so
    /// the ratio has one unit — class docstrings count toward [`Self::docstring_coverage`], not
    /// here. A high ratio flags AI **over-documentation** — verbose docstrings piled onto trivial
    /// code. 0.0 when there are no functions.
    pub docstring_code_ratio: f64,
    /// Exception-handling hygiene totals across all files: summed [`ExceptionStats`].
    pub exception: ExceptionStats,
    /// Broad handlers as a fraction of all handlers (`broad / handlers`); 0.0 with no handlers.
    /// Low for disciplined libraries, high for "make-it-work" code. Descriptive, never a gate.
    pub broad_except_rate: f64,
    /// Swallow handlers as a fraction of all handlers (`swallow / handlers`); 0.0 with no
    /// handlers. The strongest sub-signal — silently discarding errors is rarely justified.
    pub swallow_except_rate: f64,
}

/// Counts of units in the worst (`very_high`) band of each distribution — the "god-unit tail"
/// that per-unit averages hide. Descriptive; never a gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GodUnits {
    /// Functions with `very_high` cognitive complexity (the hardest to read).
    pub cognitive_functions: usize,
    /// Functions with `very_high` cyclomatic complexity (the branchiest).
    pub cyclomatic_functions: usize,
    /// Classes with `very_high` WMC (god classes).
    pub wmc_classes: usize,
    /// Modules with `very_high` NLOC (god modules).
    pub size_modules: usize,
}

impl GodUnits {
    /// Total very-high-tier units across functions, classes, and modules.
    pub fn total(&self) -> usize {
        self.cognitive_functions + self.cyclomatic_functions + self.wmc_classes + self.size_modules
    }
}

impl RepoMetrics {
    /// The god-unit **tail**: how many units land in the worst (`very_high`) band of each
    /// distribution. Per-unit *averages* wash these outliers out — a repo can have a dozen
    /// god-modules and a cognitive-172 god-function yet a clean `avg_cognitive` because they're
    /// diluted across thousands of units — so the count of very-high-tier units is the signal that
    /// surfaces them. Reads the existing risk histograms; no extra computation.
    pub fn god_units(&self) -> GodUnits {
        GodUnits {
            cognitive_functions: self.cognitive_risk.very_high,
            cyclomatic_functions: self.cyclomatic_risk.very_high,
            wmc_classes: self.wmc_risk.very_high,
            size_modules: self.module_size_risk.very_high,
        }
    }

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

    /// The arity counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max parameters plus
    /// the Long-Parameter-List histogram. Caller-facing arity (`self`/`cls` excluded). Descriptive
    /// bands ([`ParamCountTier`]) — high `high`/`very high` counts flag *functions to read*, never
    /// defects.
    pub fn params_markdown(&self) -> String {
        let risk = self.param_count_risk;
        format!(
            "**Parameter count** — mean {:.1}, p95 {}, max {}.\n\n\
             | Arity band | Functions |\n| --- | ---: |\n\
             | low (≤4) | {} |\n| moderate (5–6) | {} |\n\
             | high (7–10) | {} |\n| very high (>10) | {} |\n",
            self.avg_params,
            self.p95_params,
            self.max_params,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// A badge summarizing cognitive-complexity risk: the worst occupied band plus the peak
    /// value, colored by that band (`max cognitive: 145 (very high)`). The cognitive counterpart to
    /// [`Self::cyclomatic_badge`] — and the more readability-relevant of the two.
    pub fn cognitive_badge(&self) -> Badge {
        match self.cognitive_risk.worst_cognitive_tier() {
            Some(tier) => Badge::new(
                "max cognitive",
                format!("{} ({})", self.max_cognitive, tier.label()),
                tier.color(),
            ),
            None => Badge::new("max cognitive", "n/a", Color::Green),
        }
    }

    /// The cognitive counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max cognitive plus
    /// the readability-band histogram, anchored on SonarSource's 15/function guidance. Descriptive
    /// bands ([`CognitiveTier`]) — high `high`/`very high` counts flag functions to *read*, not
    /// defects.
    pub fn cognitive_markdown(&self) -> String {
        let risk = self.cognitive_risk;
        format!(
            "**Cognitive complexity** — mean {:.1}, p95 {}, max {} (worst tier: {}).\n\n\
             | Risk tier | Functions |\n| --- | ---: |\n\
             | low (≤5) | {} |\n| moderate (6–15) | {} |\n\
             | high (16–40) | {} |\n| very high (>40) | {} |\n",
            self.avg_cognitive,
            self.p95_cognitive,
            self.max_cognitive,
            risk.worst_cognitive_tier()
                .map(CognitiveTier::label)
                .unwrap_or("n/a"),
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The class-size counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max WMC plus
    /// the god-class-prevalence histogram. Descriptive bands ([`WmcTier`]) — high `high`/`very
    /// high` counts flag *candidates to read*, never defects.
    pub fn wmc_markdown(&self) -> String {
        let risk = self.wmc_risk;
        format!(
            "**Class weight (WMC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | WMC band | Classes |\n| --- | ---: |\n\
             | low (≤20) | {} |\n| moderate (21–50) | {} |\n\
             | high (51–200) | {} |\n| very high (>200) | {} |\n",
            self.avg_wmc,
            self.p95_wmc,
            self.max_wmc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The inheritance-breadth counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max
    /// NOC plus the fragile-base-class histogram. Descriptive bands ([`NocTier`]) — high
    /// `high`/`very high` counts flag *bases to review before changing*, never defects.
    pub fn noc_markdown(&self) -> String {
        let risk = self.noc_risk;
        format!(
            "**Inheritance breadth (NOC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | NOC band | Classes |\n| --- | ---: |\n\
             | low (≤1) | {} |\n| moderate (2–5) | {} |\n\
             | high (6–20) | {} |\n| very high (>20) | {} |\n",
            self.avg_noc,
            self.p95_noc,
            self.max_noc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The class-coupling counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max CBO plus
    /// the hub-class histogram. Descriptive bands ([`CboTier`]) — high `high`/`very high` counts flag
    /// *hubs to review before changing*, never defects. A **lower bound** in dynamically-typed code
    /// (duck-typed coupling is invisible), so the caption says so.
    pub fn cbo_markdown(&self) -> String {
        let risk = self.cbo_risk;
        format!(
            "**Class coupling (CBO)** — mean {:.1}, p95 {}, max {} _(approximate — \
             misses duck-typed coupling)_.\n\n\
             | CBO band | Classes |\n| --- | ---: |\n\
             | low (≤4) | {} |\n| moderate (5–9) | {} |\n\
             | high (10–20) | {} |\n| very high (>20) | {} |\n",
            self.avg_cbo,
            self.p95_cbo,
            self.max_cbo,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// The module-size counterpart to [`Self::cyclomatic_markdown`]: mean/p95/max NLOC plus
    /// the god-module-prevalence histogram. Descriptive NLOC bands ([`ModuleSizeTier`]) — high
    /// `high`/`very high` counts flag *files to read*, never defects.
    pub fn module_size_markdown(&self) -> String {
        let risk = self.module_size_risk;
        format!(
            "**Module size (NLOC)** — mean {:.1}, p95 {}, max {}.\n\n\
             | NLOC band | Files |\n| --- | ---: |\n\
             | low (≤250) | {} |\n| moderate (251–500) | {} |\n\
             | high (501–1000) | {} |\n| very high (>1000) | {} |\n",
            self.avg_module_nloc,
            self.p95_module_nloc,
            self.max_module_nloc,
            risk.low,
            risk.moderate,
            risk.high,
            risk.very_high,
        )
    }

    /// A one-line markdown summary of exception-handling hygiene: the broad/swallow rates
    /// with the underlying counts. Descriptive cohort signal — broad except is sometimes correct
    /// (daemon loops, plugin boundaries), so it's never a gate.
    pub fn exception_markdown(&self) -> String {
        let exc = self.exception;
        format!(
            "**Exception handling** — broad-except rate {:.2} ({} of {} handlers), swallow rate \
             {:.2} ({} `pass`/`continue`/`...`), {} bare. Descriptive, never a gate.\n",
            self.broad_except_rate,
            exc.broad,
            exc.handlers,
            self.swallow_except_rate,
            exc.swallow,
            exc.bare,
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

/// Whether `stmt` is a bare string-literal expression statement (a docstring).
fn is_docstring_stmt(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_)))
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

/// Aggregate per-file metrics into repo-level figures.
/// Minimum cognitive complexity for a function to count toward [`RepoMetrics::max_logic_function_loc`].
/// Excludes straight-line data/config-init blobs (cognitive ≈ 0–1) from the "longest logic
/// function" signal, so a 2,733-line `__init__` of assignments doesn't outrank a real god-function.
pub const LOGIC_FUNCTION_MIN_COGNITIVE: usize = 5;

/// Minimum module-scope logic statements for a module to be considered for the undecomposed flag
/// — small scripts / entry points legitimately have a little top-level code.
pub const TOP_LEVEL_MIN_LOGIC: usize = 15;

/// Top-level-code ratio at/above which a non-trivial module is "undecomposed" — a procedural
/// script-dump. Descriptive, calibrated; never a gate.
pub const TOP_LEVEL_RATIO_THRESHOLD: f64 = 0.6;

pub fn aggregate(files: &[FileMetrics]) -> RepoMetrics {
    let mut repo = RepoMetrics {
        files: files.len(),
        ..RepoMetrics::default()
    };
    let mut function_loc_sum = 0usize;
    let mut cyclomatic_sum = 0usize;
    let mut cyclomatic_values: Vec<usize> = Vec::new();
    let mut arity_sum = 0usize;
    let mut arity_values: Vec<usize> = Vec::new();
    let mut cognitive_sum = 0usize;
    let mut cognitive_values: Vec<usize> = Vec::new();
    let mut typed_params_sum = 0usize;
    let mut annotatable_params_sum = 0usize;
    let mut fully_annotated = 0usize;
    let mut wmc_sum = 0usize;
    let mut wmc_values: Vec<usize> = Vec::new();
    let mut dit_sum = 0usize;
    let mut noc_sum = 0usize;
    let mut noc_values: Vec<usize> = Vec::new();
    let mut cbo_sum = 0usize;
    let mut cbo_values: Vec<usize> = Vec::new();
    let mut module_nloc_sum = 0usize;
    let mut module_nloc_values: Vec<usize> = Vec::new();
    // Top-level-code ratio: averaged only over modules that contain executable logic.
    let mut top_level_ratio_sum = 0f64;
    let mut modules_with_logic = 0usize;
    // Docstring coverage: every public def/class (functions *and* classes) is in the
    // denominator, those carrying a docstring in the numerator. The docstring/code ratio is
    // kept strictly function-scoped — function docstring lines over function NCSS — so its two
    // sides share one unit (NCSS exists only for functions). Class docstrings drive coverage,
    // not the ratio.
    let mut public_units = 0usize;
    let mut public_documented = 0usize;
    let mut fn_docstring_lines_sum = 0usize;
    let mut ncss_sum = 0usize;
    for file in files {
        repo.total_loc += file.loc;
        repo.exception.handlers += file.exception.handlers;
        repo.exception.bare += file.exception.bare;
        repo.exception.broad += file.exception.broad;
        repo.exception.swallow += file.exception.swallow;
        module_nloc_sum += file.nloc;
        module_nloc_values.push(file.nloc);
        repo.max_module_nloc = repo.max_module_nloc.max(file.nloc);
        repo.module_size_risk.record_module_size(file.nloc);
        // Top-level-code ratio: only meaningful for modules that contain executable logic.
        let module_logic = file.top_level_code + file.function_code;
        if module_logic > 0 {
            let ratio = file.top_level_code as f64 / module_logic as f64;
            top_level_ratio_sum += ratio;
            modules_with_logic += 1;
            if ratio > repo.max_top_level_ratio {
                repo.max_top_level_ratio = ratio;
            }
            if file.top_level_code >= TOP_LEVEL_MIN_LOGIC && ratio >= TOP_LEVEL_RATIO_THRESHOLD {
                repo.undecomposed_modules += 1;
            }
        }
        for function in &file.functions {
            repo.functions += 1;
            function_loc_sum += function.loc;
            cyclomatic_sum += function.cyclomatic;
            cyclomatic_values.push(function.cyclomatic);
            repo.cyclomatic_risk.record(function.cyclomatic);
            arity_sum += function.arity;
            arity_values.push(function.arity);
            repo.param_count_risk.record_arity(function.arity);
            repo.max_params = repo.max_params.max(function.arity);
            repo.max_function_loc = repo.max_function_loc.max(function.loc);
            // Longest *logic* function: ignore data/config-init blobs (very low cognitive)
            // so the god-function signal isn't crowned by a 2,733-line assignment run.
            if function.cognitive >= LOGIC_FUNCTION_MIN_COGNITIVE {
                repo.max_logic_function_loc = repo.max_logic_function_loc.max(function.loc);
            }
            repo.max_cyclomatic = repo.max_cyclomatic.max(function.cyclomatic);
            repo.max_cognitive = repo.max_cognitive.max(function.cognitive);
            cognitive_sum += function.cognitive;
            cognitive_values.push(function.cognitive);
            repo.cognitive_risk.record_cognitive(function.cognitive);
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
            ncss_sum += function.ncss;
            fn_docstring_lines_sum += function.docstring_lines;
            if is_public(&function.name) {
                public_units += 1;
                public_documented += usize::from(function.has_docstring);
            }
        }
        for class in &file.classes {
            repo.classes += 1;
            wmc_sum += class.wmc;
            wmc_values.push(class.wmc);
            repo.wmc_risk.record_wmc(class.wmc);
            dit_sum += class.dit;
            noc_sum += class.noc;
            noc_values.push(class.noc);
            repo.noc_risk.record_noc(class.noc);
            cbo_sum += class.cbo;
            cbo_values.push(class.cbo);
            repo.cbo_risk.record_cbo(class.cbo);
            repo.max_wmc = repo.max_wmc.max(class.wmc);
            repo.max_dit = repo.max_dit.max(class.dit);
            repo.max_noc = repo.max_noc.max(class.noc);
            repo.max_cbo = repo.max_cbo.max(class.cbo);
            if is_public(&class.name) {
                public_units += 1;
                public_documented += usize::from(class.has_docstring);
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
    repo.avg_params = if repo.functions == 0 {
        0.0
    } else {
        arity_sum as f64 / repo.functions as f64
    };
    repo.p95_params = percentile(&mut arity_values, 0.95);
    repo.avg_cognitive = if repo.functions == 0 {
        0.0
    } else {
        cognitive_sum as f64 / repo.functions as f64
    };
    repo.p95_cognitive = percentile(&mut cognitive_values, 0.95);
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
    repo.avg_wmc = if repo.classes == 0 {
        0.0
    } else {
        wmc_sum as f64 / repo.classes as f64
    };
    repo.p95_wmc = percentile(&mut wmc_values, 0.95);
    repo.avg_module_nloc = if repo.files == 0 {
        0.0
    } else {
        module_nloc_sum as f64 / repo.files as f64
    };
    repo.p95_module_nloc = percentile(&mut module_nloc_values, 0.95);
    repo.avg_top_level_ratio = if modules_with_logic == 0 {
        0.0
    } else {
        top_level_ratio_sum / modules_with_logic as f64
    };
    repo.avg_dit = if repo.classes == 0 {
        0.0
    } else {
        dit_sum as f64 / repo.classes as f64
    };
    repo.avg_noc = if repo.classes == 0 {
        0.0
    } else {
        noc_sum as f64 / repo.classes as f64
    };
    repo.p95_noc = percentile(&mut noc_values, 0.95);
    repo.avg_cbo = if repo.classes == 0 {
        0.0
    } else {
        cbo_sum as f64 / repo.classes as f64
    };
    repo.p95_cbo = percentile(&mut cbo_values, 0.95);
    repo.docstring_coverage = if public_units == 0 {
        0.0
    } else {
        public_documented as f64 / public_units as f64
    };
    repo.docstring_code_ratio = if ncss_sum == 0 {
        0.0
    } else {
        fn_docstring_lines_sum as f64 / ncss_sum as f64
    };
    let handlers = repo.exception.handlers;
    repo.broad_except_rate = if handlers == 0 {
        0.0
    } else {
        repo.exception.broad as f64 / handlers as f64
    };
    repo.swallow_except_rate = if handlers == 0 {
        0.0
    } else {
        repo.exception.swallow as f64 / handlers as f64
    };
    repo
}

/// Whether a def/class name is "public" for docstring coverage — i.e. not `_`-prefixed.
/// Dunder methods (`__init__`, `__repr__`) start with `_`, so they are treated as non-public and
/// excluded from the coverage denominator, matching the convention that documentation effort
/// targets the public API. The test is purely a name-prefix check applied to *every* collected
/// def/class regardless of nesting depth — a function-local helper or a setter is still a unit,
/// matching how the rest of the crate collects functions.
fn is_public(name: &str) -> bool {
    !name.starts_with('_')
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
        arity: caller_arity(function),
        ncss: ncss(&function.body),
        exits: exit_count(&function.body),
        typed_params,
        annotatable_params,
        has_return_annotation,
        has_docstring: docstring_range(&function.body).is_some(),
        docstring_lines: docstring_lines(source, &function.body),
    }
}

/// Range of a body's docstring — the first statement, when it is a bare string-literal
/// expression (PEP 257). `None` for any other leading statement. Used for both functions and
/// classes; a docstring is a `StringLiteral`, never a `Comment`, so it is invisible to
/// `comment_density` and this metric is purely additive.
fn docstring_range(body: &[Stmt]) -> Option<TextRange> {
    match body.first() {
        Some(Stmt::Expr(expr)) => match expr.value.as_ref() {
            Expr::StringLiteral(literal) => Some(literal.range()),
            _ => None,
        },
        _ => None,
    }
}

/// Physical line span of a body's docstring, or 0 if it has none.
fn docstring_lines(source: &str, body: &[Stmt]) -> usize {
    docstring_range(body).map_or(0, |range| line_span(source, range))
}

/// Per-class metrics: size (methods, distinct instance attributes), LCOM4 cohesion, and WMC.
/// `dit`/`noc` are left at 0 here — inheritance depth and breadth are project-wide properties
/// filled in later by [`resolve_inheritance`], once every file's classes are known.
fn class_metrics(source: &str, parsed: &Parsed<ModModule>, class: &StmtClassDef) -> ClassMetrics {
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

/// Trailing identifier of a (possibly dotted or subscripted) name expression — `ABC` from
/// `abc.ABC`, `Protocol` from `typing.Protocol[T]` — or `None` for anything that doesn't name a
/// class (a call, a literal, …).
pub(crate) fn expr_trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Subscript(subscript) => expr_trailing_name(&subscript.value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture_source;
    use sloplint_python::parse;

    fn metrics(source: &str) -> FileMetrics {
        file_metrics(source, &parse(source).unwrap())
    }

    /// Cognitive complexity of every function in a fixture, in source order. The fixture
    /// documents each SonarSource-style case; the snapshot pins the scores.
    #[test]
    fn cognitive_complexity() {
        use std::fmt::Write;
        let source = fixture_source("complexity/cognitive.py");
        let mut out = String::new();
        for function in &metrics(&source).functions {
            writeln!(out, "{}: cognitive={}", function.name, function.cognitive).unwrap();
        }
        insta::assert_snapshot!(out);
    }

    #[test]
    fn function_metrics() {
        // Per-function size/shape over the fixture (class methods and nested functions each get a
        // row). Pins params/arity (receiver + variadic handling), ncss (own-body statement count),
        // exits (own-body return/raise/yield), and the basic complexity/nesting/loc fields.
        use std::fmt::Write;
        let source = fixture_source("complexity/function_metrics.py");
        let mut out = String::new();
        for f in &metrics(&source).functions {
            writeln!(
                out,
                "{}: params={} arity={} ncss={} exits={} cyclomatic={} cognitive={} max_nesting={} loc={}",
                f.name, f.params, f.arity, f.ncss, f.exits, f.cyclomatic, f.cognitive, f.max_nesting, f.loc
            )
            .unwrap();
        }
        insta::assert_snapshot!(out);
    }

    #[test]
    fn cyclomatic_and_nesting() {
        // Per-function cyclomatic/cognitive/max_nesting over the fixture (nested functions get
        // their own rows). Pins: branchy = 4/·/2; string keywords don't inflate (1); and an
        // outer function does not absorb a nested function's branch (outer cognitive 0).
        use std::fmt::Write;
        let source = fixture_source("complexity/cyclomatic.py");
        let mut out = String::new();
        for function in &metrics(&source).functions {
            writeln!(
                out,
                "{}: cyclomatic={} cognitive={} max_nesting={}",
                function.name, function.cyclomatic, function.cognitive, function.max_nesting
            )
            .unwrap();
        }
        insta::assert_snapshot!(out);
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
    fn wmc_tier_boundaries() {
        // Descriptive bands: ≤20 low, 21–50 moderate, 51–200 high, >200 very high.
        assert_eq!(WmcTier::from_wmc(0), WmcTier::Low);
        assert_eq!(WmcTier::from_wmc(20), WmcTier::Low);
        assert_eq!(WmcTier::from_wmc(21), WmcTier::Moderate);
        assert_eq!(WmcTier::from_wmc(50), WmcTier::Moderate);
        assert_eq!(WmcTier::from_wmc(51), WmcTier::High);
        assert_eq!(WmcTier::from_wmc(200), WmcTier::High);
        assert_eq!(WmcTier::from_wmc(201), WmcTier::VeryHigh);
    }

    #[test]
    fn noc_tier_boundaries() {
        // Descriptive breadth bands: ≤1 low, 2–5 moderate, 6–20 high, >20 very high.
        assert_eq!(NocTier::from_noc(0), NocTier::Low);
        assert_eq!(NocTier::from_noc(1), NocTier::Low);
        assert_eq!(NocTier::from_noc(2), NocTier::Moderate);
        assert_eq!(NocTier::from_noc(5), NocTier::Moderate);
        assert_eq!(NocTier::from_noc(6), NocTier::High);
        assert_eq!(NocTier::from_noc(20), NocTier::High);
        assert_eq!(NocTier::from_noc(21), NocTier::VeryHigh);
    }

    #[test]
    fn module_size_tier_boundaries() {
        // Descriptive NLOC bands: ≤250 low, 251–500 moderate, 501–1000 high, >1000 very high.
        assert_eq!(ModuleSizeTier::from_nloc(0), ModuleSizeTier::Low);
        assert_eq!(ModuleSizeTier::from_nloc(250), ModuleSizeTier::Low);
        assert_eq!(ModuleSizeTier::from_nloc(251), ModuleSizeTier::Moderate);
        assert_eq!(ModuleSizeTier::from_nloc(500), ModuleSizeTier::Moderate);
        assert_eq!(ModuleSizeTier::from_nloc(501), ModuleSizeTier::High);
        assert_eq!(ModuleSizeTier::from_nloc(1000), ModuleSizeTier::High);
        assert_eq!(ModuleSizeTier::from_nloc(1001), ModuleSizeTier::VeryHigh);
    }

    #[test]
    fn param_count_tier_boundaries() {
        // Descriptive arity bands: ≤4 low, 5–6 moderate, 7–10 high, >10 very high.
        assert_eq!(ParamCountTier::from_arity(0), ParamCountTier::Low);
        assert_eq!(ParamCountTier::from_arity(4), ParamCountTier::Low);
        assert_eq!(ParamCountTier::from_arity(5), ParamCountTier::Moderate);
        assert_eq!(ParamCountTier::from_arity(6), ParamCountTier::Moderate);
        assert_eq!(ParamCountTier::from_arity(7), ParamCountTier::High);
        assert_eq!(ParamCountTier::from_arity(10), ParamCountTier::High);
        assert_eq!(ParamCountTier::from_arity(11), ParamCountTier::VeryHigh);
    }

    #[test]
    fn aggregate_reports_param_count_distribution() {
        // Arities 3 (low), 5 (moderate), 8 (high), 12 (very high) — one function each, so the
        // histogram shows the full spread the mean would flatten.
        let src = "\
def a(p0, p1, p2): return 0
def b(p0, p1, p2, p3, p4): return 0
def c(p0, p1, p2, p3, p4, p5, p6, p7): return 0
def d(p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11): return 0
";
        let repo = aggregate(&[metrics(src)]);
        assert_eq!(repo.functions, 4);
        assert_eq!(repo.max_params, 12);
        assert_eq!(repo.param_count_risk.low, 1, "arity 3");
        assert_eq!(repo.param_count_risk.moderate, 1, "arity 5");
        assert_eq!(repo.param_count_risk.high, 1, "arity 8");
        assert_eq!(repo.param_count_risk.very_high, 1, "arity 12");
        // p95 (nearest-rank over [3, 5, 8, 12]) is the widest signature.
        assert_eq!(repo.p95_params, 12);
        // mean = (3 + 5 + 8 + 12) / 4 = 7.0.
        assert!(
            (repo.avg_params - 7.0).abs() < 1e-9,
            "avg = {}",
            repo.avg_params
        );
    }

    #[test]
    fn nloc_excludes_blank_and_comment_lines_but_counts_docstrings() {
        // import(1) + def(1) + 4 docstring lines (incl. its internal blank) + return(1) = 7.
        // The leading comment line and the two blank separators do not count.
        let file = metrics(
            "\
# module comment

import os


def f():
    \"\"\"Doc.

    More.
    \"\"\"
    return os.getpid()
",
        );
        assert_eq!(
            file.nloc, 7,
            "code + docstring lines only; blanks/comments excluded"
        );
        assert!(
            file.loc > file.nloc,
            "physical loc includes the excluded lines"
        );
    }

    #[test]
    fn aggregate_reports_module_size_distribution() {
        // Two modules: a tiny one (low) and one padded past 250 NLOC (moderate). The histogram
        // must show the spread; total_loc/avg would hide the big file among the small.
        let small = "x = 1\ny = 2\n";
        let big: String = (0..260).map(|i| format!("v{i} = {i}\n")).collect();
        let repo = aggregate(&[metrics(small), metrics(&big)]);

        assert_eq!(repo.files, 2);
        assert_eq!(repo.max_module_nloc, 260);
        assert_eq!(repo.module_size_risk.low, 1, "the 2-line module");
        assert_eq!(repo.module_size_risk.moderate, 1, "the 260-line module");
        assert_eq!(repo.module_size_risk.high, 0);
        assert_eq!(repo.module_size_risk.very_high, 0);
        // p95 (nearest-rank over [2, 260]) is the bigger module.
        assert_eq!(repo.p95_module_nloc, 260);
    }

    #[test]
    fn top_level_code_counts_module_scope_logic() {
        // A procedural script: imports/docstring/constants/__main__ excluded; the loop + calls at
        // module scope are top-level logic; the helper's body is function logic.
        let file = metrics(
            "\
\"\"\"Module docstring.\"\"\"
import os

MAX = 10


def helper(x):
    y = x + 1
    return y


total = 0
for i in range(MAX):
    total += helper(i)
print(total)

if __name__ == \"__main__\":
    print(\"done\")
",
        );
        // top-level logic: `for ...` + `total += helper(i)` (2) + `print(total)` (1) = 3.
        // Excluded: docstring, import, `MAX`/`total = 0` constant assignments, the def, the
        // `__main__` guard.
        assert_eq!(file.top_level_code, 3, "module-scope logic");
        // function logic: helper body `y = x + 1` + `return y` = 2.
        assert_eq!(file.function_code, 2, "in-function logic");
    }

    #[test]
    fn well_decomposed_and_library_modules_have_low_ratio() {
        // All logic inside functions → ratio 0 (no top-level logic).
        let lib = metrics(
            "import os\n\nCONST = 1\n\n\ndef a():\n    return os.getpid()\n\n\ndef b(x):\n    return x * CONST\n",
        );
        assert_eq!(lib.top_level_code, 0);
        assert!(lib.function_code >= 2);
    }

    #[test]
    fn aggregate_flags_undecomposed_script() {
        // A 20-statement top-level script (no functions) → ratio 1.0, flagged undecomposed; a
        // well-decomposed module is not.
        let script: String = (0..20).map(|i| format!("print({i})\n")).collect();
        let decomposed = "def main():\n".to_string()
            + &(0..20)
                .map(|i| format!("    print({i})\n"))
                .collect::<String>();
        let repo = aggregate(&[metrics(&script), metrics(&decomposed)]);
        assert!(
            (repo.max_top_level_ratio - 1.0).abs() < 1e-9,
            "the script is all top-level"
        );
        assert_eq!(
            repo.undecomposed_modules, 1,
            "only the script is undecomposed"
        );
        // avg over the two logic-bearing modules: (1.0 + 0.0) / 2 = 0.5.
        assert!(
            (repo.avg_top_level_ratio - 0.5).abs() < 1e-9,
            "{}",
            repo.avg_top_level_ratio
        );
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
    fn god_units_count_very_high_tier_across_unit_kinds() {
        let mut repo = RepoMetrics::default();
        repo.cognitive_risk.very_high = 3;
        repo.cyclomatic_risk.very_high = 2;
        repo.wmc_risk.very_high = 1;
        repo.module_size_risk.very_high = 12;
        // (Other bands don't count toward the tail.)
        repo.cognitive_risk.low = 4000;
        let god = repo.god_units();
        assert_eq!(god.cognitive_functions, 3);
        assert_eq!(god.size_modules, 12);
        assert_eq!(god.total(), 3 + 2 + 1 + 12);
    }

    #[test]
    fn cognitive_tier_bands() {
        assert_eq!(CognitiveTier::from_cognitive(0), CognitiveTier::Low);
        assert_eq!(CognitiveTier::from_cognitive(5), CognitiveTier::Low);
        assert_eq!(CognitiveTier::from_cognitive(6), CognitiveTier::Moderate);
        assert_eq!(CognitiveTier::from_cognitive(15), CognitiveTier::Moderate);
        assert_eq!(CognitiveTier::from_cognitive(16), CognitiveTier::High);
        assert_eq!(CognitiveTier::from_cognitive(40), CognitiveTier::High);
        assert_eq!(CognitiveTier::from_cognitive(41), CognitiveTier::VeryHigh);
    }

    #[test]
    fn aggregate_reports_cognitive_distribution() {
        // A flat function (cognitive 0) and a deeply-nested one (cognitive > 0): the histogram and
        // avg/p95 must surface the spread, not just `max_cognitive`.
        let source = "\
def a():
    return 1

def b(xs):
    for x in xs:
        if x:
            if x > 0:
                return x
    return 0
";
        let repo = aggregate(&[metrics(source)]);
        assert_eq!(repo.functions, 2);
        // Cognitive of the nested function: for(+1) + if(+2) + if(+3) = 6 -> moderate band.
        assert_eq!(repo.max_cognitive, 6);
        assert!((repo.avg_cognitive - 3.0).abs() < 1e-9, "mean of [0, 6]");
        assert_eq!(repo.p95_cognitive, 6, "nearest-rank over 2 values = max");
        assert_eq!(repo.cognitive_risk.low, 1, "the trivial function");
        assert_eq!(repo.cognitive_risk.moderate, 1, "the nested function");
        assert_eq!(
            repo.cognitive_risk.worst_cognitive_tier(),
            Some(CognitiveTier::Moderate)
        );
    }

    #[test]
    fn cognitive_badge_and_markdown_reflect_worst_tier() {
        let mut repo = RepoMetrics {
            functions: 1,
            max_cognitive: 145,
            avg_cognitive: 145.0,
            p95_cognitive: 145,
            ..RepoMetrics::default()
        };
        repo.cognitive_risk.very_high = 1;
        let badge = repo.cognitive_badge();
        assert_eq!(badge.message, "145 (very high)");
        assert_eq!(badge.color, Color::Red);
        let md = repo.cognitive_markdown();
        assert!(md.contains("worst tier: very high"));
        assert!(md.contains("| very high (>40) | 1 |"));
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
    fn max_logic_function_loc_excludes_data_init_blobs() {
        // A long data-init blob (cognitive ~0) vs. a short but logic-dense function. `max_function_loc`
        // is crowned by the blob; `max_logic_function_loc` correctly points at the logic function.
        let blob = (0..20)
            .map(|i| format!("    a{i} = {i}\n"))
            .collect::<String>();
        let source = format!(
            "def blob():\n{blob}    return None\n\n\ndef logic(xs):\n    for x in xs:\n        if x > 0:\n            while x:\n                x -= 1\n    return 0\n"
        );
        let repo = aggregate(&[metrics(&source)]);
        // blob: def + 20 assigns + return = 22 lines, cognitive 0 → excluded.
        assert_eq!(repo.max_function_loc, 22, "blob is the longest by raw LoC");
        // logic: def + for + if + while + x-=1 + return = 6 lines, cognitive 6 (≥5) → counted.
        assert_eq!(
            repo.max_logic_function_loc, 6,
            "longest *logic* function is the 6-line one, not the 22-line blob"
        );
    }

    #[test]
    fn exception_stats_classify_bare_broad_and_swallow() {
        let exc = metrics(
            "\
def a():
    try:
        risky()
    except Exception:          # broad
        log()

def b():
    try:
        risky()
    except ValueError:         # narrow — neither broad nor swallow
        handle()

def c():
    try:
        risky()
    except:                    # bare + swallow
        pass

def d():
    try:
        risky()
    except (KeyError, BaseException):  # broad via tuple + swallow via ...
        ...

for item in items:
    try:
        top()
    except builtins.Exception:  # broad via attribute trailing name; continue is swallow
        continue
",
        )
        .exception;
        assert_eq!(
            exc.handlers, 5,
            "every except clause, module-level included"
        );
        assert_eq!(exc.bare, 1, "only `except:`");
        assert_eq!(
            exc.broad, 3,
            "Exception, (…, BaseException), builtins.Exception"
        );
        assert_eq!(exc.swallow, 3, "pass, ..., continue");
    }

    #[test]
    fn exception_rates_are_fraction_of_handlers() {
        // 2 handlers: one broad+swallow, one narrow with a real body.
        let repo = aggregate(&[metrics(
            "\
def f():
    try:
        x()
    except Exception:
        pass
    try:
        y()
    except ValueError:
        recover()
",
        )]);
        assert_eq!(repo.exception.handlers, 2);
        assert_eq!(repo.exception.broad, 1);
        assert_eq!(repo.exception.swallow, 1);
        assert!((repo.broad_except_rate - 0.5).abs() < 1e-9);
        assert!((repo.swallow_except_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn exception_rates_zero_without_handlers() {
        let repo = aggregate(&[metrics("def f():\n    return 1\n")]);
        assert_eq!(repo.exception, ExceptionStats::default());
        assert_eq!(repo.broad_except_rate, 0.0);
        assert_eq!(repo.swallow_except_rate, 0.0);
    }

    #[test]
    fn class_metrics() {
        // Per-class size/cohesion/weight over the fixture (single-file, so cross-file
        // dit/noc/cbo are 0 and omitted here). Pins methods/attributes/lcom4/wmc/loc: Counter is
        // cohesive (lcom4 1), Utils splits (lcom4 2), WmcDemo sums method cyclomatic to 4, Empty
        // has no weight, and WmcNested counts only m's own-body `if` (wmc 2, nested helper
        // excluded).
        use std::fmt::Write;
        let source = fixture_source("complexity/class_metrics.py");
        let mut out = String::new();
        for c in &metrics(&source).classes {
            writeln!(
                out,
                "{}: methods={} attributes={} lcom4={} wmc={} loc={}",
                c.name, c.methods, c.attributes, c.lcom4, c.wmc, c.loc
            )
            .unwrap();
        }
        insta::assert_snapshot!(out);
    }

    #[test]
    fn aggregate_reports_wmc_distribution_not_just_max() {
        // Three classes WMC 0 (low), 21 (moderate — 21 trivial methods), and 51 (high — 51
        // trivial methods). The histogram must show the spread; max alone would hide the two
        // smaller classes. Bands: ≤20 low, 21–50 moderate, 51–200 high, >200 very high.
        let body = |name: &str, methods: usize| {
            let mut s = format!("class {name}:\n");
            if methods == 0 {
                s.push_str("    pass\n");
            }
            for i in 0..methods {
                s.push_str(&format!("    def m{i}(self):\n        return {i}\n"));
            }
            s
        };
        let source = format!("{}{}{}", body("Low", 0), body("Mid", 21), body("Big", 51));
        let repo = aggregate(&[metrics(&source)]);

        assert_eq!(repo.classes, 3);
        assert_eq!(repo.max_wmc, 51);
        assert_eq!(repo.wmc_risk.low, 1, "Low (wmc 0)");
        assert_eq!(repo.wmc_risk.moderate, 1, "Mid (wmc 21)");
        assert_eq!(repo.wmc_risk.high, 1, "Big (wmc 51)");
        assert_eq!(repo.wmc_risk.very_high, 0);
        // p95 (nearest-rank over [0, 21, 51]) lands on the heaviest class.
        assert_eq!(repo.p95_wmc, 51);
    }

    #[test]
    fn wmc_distribution_is_empty_without_classes() {
        let repo = aggregate(&[metrics("def f():\n    return 1\n")]);
        assert_eq!(repo.classes, 0);
        assert_eq!(repo.wmc_risk, RiskHistogram::default());
        assert_eq!(repo.p95_wmc, 0);
    }

    /// DIT resolves over the whole project by class name: a chain `Grandchild -> Child -> Root`
    /// split across two files gives depths 2/1/0, and bases that don't resolve to a first-party
    /// class (`object`, a third-party import) terminate at 0.
    #[test]
    fn dit_resolves_first_party_chain_across_files() {
        let mut base = metrics("class Root(object):\n    pass\n");
        let mut derived = metrics(
            "\
from base import Root
from third_party import Plugin


class Child(Root):
    pass

class Grandchild(Child):
    pass

class External(Plugin):
    pass
",
        );
        resolve_inheritance(&mut [&mut base, &mut derived]);

        let dit = |file: &FileMetrics, name: &str| {
            file.classes.iter().find(|c| c.name == name).unwrap().dit
        };
        assert_eq!(dit(&base, "Root"), 0, "object is external → root");
        assert_eq!(dit(&derived, "Child"), 1, "Root is first-party");
        assert_eq!(dit(&derived, "Grandchild"), 2, "Child -> Root");
        assert_eq!(
            dit(&derived, "External"),
            0,
            "Plugin is third-party → invisible"
        );
    }

    #[test]
    fn noc_counts_direct_first_party_children_across_files() {
        let mut base = metrics(
            "\
from third_party import Plugin


class Base:
    pass

class A(Base):
    pass

class B(Base):
    pass

class Ext(Plugin):
    pass
",
        );
        // A third child of Base, defined in another file — NOC must see across the project.
        let mut more = metrics("from base import Base\n\nclass C(Base):\n    pass\n");
        resolve_inheritance(&mut [&mut base, &mut more]);

        let noc = |file: &FileMetrics, name: &str| {
            file.classes.iter().find(|c| c.name == name).unwrap().noc
        };
        assert_eq!(noc(&base, "Base"), 3, "A, B (same file) + C (cross-file)");
        assert_eq!(noc(&base, "A"), 0, "a leaf has no children");
        assert_eq!(
            noc(&base, "Ext"),
            0,
            "Ext has no children; its third-party base doesn't make it one"
        );
        // A grandchild does not count toward the grandparent's NOC — breadth is one level only.
        let mut chain = metrics(
            "class Root:\n    pass\n\nclass Mid(Root):\n    pass\n\nclass Leaf(Mid):\n    pass\n",
        );
        resolve_inheritance(&mut [&mut chain]);
        let n = |name: &str| chain.classes.iter().find(|c| c.name == name).unwrap().noc;
        assert_eq!(n("Root"), 1, "only Mid is a direct child, not Leaf");
        assert_eq!(n("Mid"), 1);
        assert_eq!(n("Leaf"), 0);
    }

    #[test]
    fn cbo_tier_boundaries() {
        assert_eq!(CboTier::from_cbo(0), CboTier::Low);
        assert_eq!(CboTier::from_cbo(4), CboTier::Low);
        assert_eq!(CboTier::from_cbo(5), CboTier::Moderate);
        assert_eq!(CboTier::from_cbo(9), CboTier::Moderate);
        assert_eq!(CboTier::from_cbo(10), CboTier::High);
        assert_eq!(CboTier::from_cbo(20), CboTier::High);
        assert_eq!(CboTier::from_cbo(21), CboTier::VeryHigh);
    }

    #[test]
    fn cbo_counts_distinct_first_party_classes_via_all_sources() {
        // `Hub` couples to first-party classes via: base (Base), annotation (Widget on a param +
        // Result return), instantiation (Engine()), and isinstance (Plugin). `int`/`list` are not
        // first-party → dropped. Self-references and third-party names don't count.
        let mut file = metrics(
            "\
class Base:
    pass

class Widget:
    pass

class Engine:
    pass

class Result:
    pass

class Plugin:
    pass

class Hub(Base):
    def run(self, w: Widget, n: int) -> Result:
        items: list = []
        e = Engine()
        if isinstance(w, Plugin):
            return Result()
        return Hub()
",
        );
        resolve_inheritance(&mut [&mut file]);
        let hub = file.classes.iter().find(|c| c.name == "Hub").unwrap();
        // Base, Widget, Engine, Result, Plugin = 5 distinct first-party classes. `int`/`list`
        // dropped (not first-party); `Hub` (self) and `Result()` counted once via the annotation.
        assert_eq!(hub.cbo, 5, "coupled: {:?}", hub.coupled);
    }

    #[test]
    fn cbo_resolves_across_files_and_excludes_self_and_external() {
        let mut a = metrics(
            "from third_party import External\n\nclass Service:\n    def make(self) -> 'Helper':\n        return Helper()\n",
        );
        // Helper lives in another file — cross-file resolution must see it.
        let mut b = metrics("class Helper:\n    pass\n");
        resolve_inheritance(&mut [&mut a, &mut b]);
        let service = a.classes.iter().find(|c| c.name == "Service").unwrap();
        // Helper() instantiation resolves first-party; External is third-party (dropped); the
        // 'Helper' string forward-ref in the return annotation is NOT counted (documented lower
        // bound) but the Helper() call is, so cbo = 1.
        assert_eq!(service.cbo, 1, "coupled: {:?}", service.coupled);
    }

    #[test]
    fn aggregate_reports_cbo_distribution() {
        // Hub couples to 2 others (low band); the two leaves couple to nothing.
        let mut file = metrics(
            "\
class A:
    pass

class B:
    pass

class Hub:
    def f(self, a: A) -> B:
        return B()
",
        );
        resolve_inheritance(&mut [&mut file]);
        let repo = aggregate(&[file]);
        assert_eq!(repo.classes, 3);
        assert_eq!(repo.max_cbo, 2, "Hub couples to A and B");
        assert!((repo.avg_cbo - 2.0 / 3.0).abs() < 1e-9, "mean of [0,0,2]");
        assert_eq!(repo.p95_cbo, 2);
        assert_eq!(repo.cbo_risk.low, 3, "all three are ≤4");
    }

    #[test]
    fn cbo_does_not_descend_into_nested_classes() {
        // A nested class is its own unit; its coupling (to Target) belongs to Inner, not Outer.
        // Outer couples only to what it uses directly (Other, via instantiation in its own method).
        let mut file = metrics(
            "\
class Target:
    pass

class Other:
    pass

class Outer:
    class Inner:
        def use(self, x: Target) -> Target:
            return Target()

    def f(self):
        return Other()
",
        );
        resolve_inheritance(&mut [&mut file]);
        let cbo = |name: &str| file.classes.iter().find(|c| c.name == name).unwrap().cbo;
        assert_eq!(cbo("Outer"), 1, "Other only — Target belongs to Inner");
        assert_eq!(cbo("Inner"), 1, "Target");
    }

    #[test]
    fn cbo_is_zero_before_resolution() {
        // Like DIT/NOC, CBO needs the project-wide pass; a bare file_metrics leaves it 0.
        let file =
            metrics("class A:\n    pass\n\nclass B:\n    def f(self) -> A:\n        return A()\n");
        assert_eq!(file.classes.iter().find(|c| c.name == "B").unwrap().cbo, 0);
    }

    #[test]
    fn aggregate_reports_noc_distribution() {
        // Hub with 3 children (moderate band) + the 3 leaves (low). max 3, p95 3.
        let mut file = metrics(
            "\
class Hub:
    pass

class A(Hub):
    pass

class B(Hub):
    pass

class C(Hub):
    pass
",
        );
        resolve_inheritance(&mut [&mut file]);
        let repo = aggregate(&[file]);
        assert_eq!(repo.classes, 4);
        assert_eq!(repo.max_noc, 3, "Hub");
        assert_eq!(repo.noc_risk.low, 3, "the three leaves (NOC 0)");
        assert_eq!(repo.noc_risk.moderate, 1, "Hub (NOC 3)");
        assert_eq!(repo.noc_risk.high, 0);
        assert_eq!(repo.p95_noc, 3);
    }

    #[test]
    fn dit_takes_longest_path_and_survives_name_cycles() {
        // Multiple inheritance: D(B, C), B(A), C(A), A. Longest path D->B->A (or D->C->A) = 2.
        let mut multi = metrics(
            "\
class A:
    pass

class B(A):
    pass

class C(A):
    pass

class D(B, C):
    pass
",
        );
        resolve_inheritance(&mut [&mut multi]);
        let dit = |name: &str| multi.classes.iter().find(|c| c.name == name).unwrap().dit;
        assert_eq!(dit("D"), 2, "longest path to a root is two hops");

        // A name collision can synthesize a cycle (X(Y), Y(X)); resolution must still halt
        // rather than recurse forever.
        let mut cyclic = metrics("class X(Y):\n    pass\n\nclass Y(X):\n    pass\n");
        resolve_inheritance(&mut [&mut cyclic]);
        // No assertion on the (ill-defined) depth — the contract is that the pass terminates.
    }

    #[test]
    fn docstring_detected_on_functions_and_classes() {
        // A function and a class, each with a multi-line docstring as the first statement.
        let file = metrics(
            "\
def documented():
    \"\"\"A docstring
    spanning three
    lines.\"\"\"
    return 1

def bare():
    return 2

class Doc:
    \"\"\"One-line class docstring.\"\"\"
    def m(self):
        return 1
",
        );
        let documented = file
            .functions
            .iter()
            .find(|f| f.name == "documented")
            .unwrap();
        assert!(documented.has_docstring);
        assert_eq!(documented.docstring_lines, 3);

        let bare = file.functions.iter().find(|f| f.name == "bare").unwrap();
        assert!(!bare.has_docstring);
        assert_eq!(bare.docstring_lines, 0);

        let class = &file.classes[0];
        assert!(class.has_docstring);
        assert_eq!(class.docstring_lines, 1);
        // The method `m` has no docstring; the class docstring is not attributed to it.
        let m = file.functions.iter().find(|f| f.name == "m").unwrap();
        assert!(!m.has_docstring);
    }

    #[test]
    fn first_statement_must_be_a_bare_string_to_count() {
        // A string literal that is *not* the first statement is not a docstring.
        let file = metrics("def f():\n    x = 1\n    \"not a docstring\"\n    return x\n");
        assert!(!file.functions[0].has_docstring);
        // A string used in an assignment is not a bare expression statement, so not a docstring.
        let file = metrics("def g():\n    s = \"hi\"\n    return s\n");
        assert!(!file.functions[0].has_docstring);
    }

    #[test]
    fn docstring_coverage_counts_only_public_units() {
        // Public: `pub_fn` (documented), `Public` (documented). Non-public, excluded from the
        // denominator: `_private` and `__init__` (both `_`-prefixed), regardless of docstrings.
        let repo = aggregate(&[metrics(
            "\
def pub_fn():
    \"\"\"documented public function.\"\"\"
    return 1

def _private():
    return 2

class Public:
    \"\"\"documented public class.\"\"\"
    def __init__(self):
        \"\"\"dunder, not public.\"\"\"
        self.x = 1
",
        )]);
        // 2 public units, both documented -> 100%.
        assert!((repo.docstring_coverage - 1.0).abs() < 1e-9);

        // Now an undocumented public function drags coverage to 2/3.
        let repo = aggregate(&[metrics(
            "\
def a():
    \"\"\"doc.\"\"\"
    return 1

def b():
    \"\"\"doc.\"\"\"
    return 2

def c():
    return 3
",
        )]);
        assert!((repo.docstring_coverage - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn class_docstrings_drive_coverage_not_the_ratio() {
        // A documented class with no methods: it counts as a documented public unit (coverage
        // 100%), but contributes no function NCSS, so the function-scoped ratio is 0.0 — not a
        // div-by-zero, and the class docstring is not smuggled into the numerator.
        let repo = aggregate(&[metrics(
            "class Doc:\n    \"\"\"A documented class.\"\"\"\n    x = 1\n",
        )]);
        assert!((repo.docstring_coverage - 1.0).abs() < 1e-9);
        assert_eq!(repo.docstring_code_ratio, 0.0);
    }

    #[test]
    fn docstring_coverage_is_zero_with_no_public_units() {
        // Only a `_`-prefixed def: no public units, so coverage is 0.0 (not NaN).
        let repo = aggregate(&[metrics("def _hidden():\n    return 1\n")]);
        assert_eq!(repo.docstring_coverage, 0.0);
        assert_eq!(repo.docstring_code_ratio, 0.0);
    }

    #[test]
    fn docstring_code_ratio_flags_over_documentation() {
        // A trivial getter with a 4-line docstring. NCSS counts every statement, including the
        // docstring expression statement, so ncss = 2 (docstring + return); ratio = 4 / 2 = 2.0.
        // Still the over-documentation signal a `has_docstring` boolean can't express: a verbose
        // docstring stacked onto a one-line body pushes the ratio up.
        let repo = aggregate(&[metrics(
            "\
def getter(self):
    \"\"\"Return the value.
    This getter returns the value.
    It really just returns the value.
    \"\"\"
    return self.value
",
        )]);
        assert_eq!(repo.docstring_code_ratio, 2.0);
    }

    /// Every documented abstractness signal is recognized, and a plain concrete class is
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
            // A whole-class stub is a concrete leaf/marker, not an abstraction. A stub body
            // has no `def`, so these can never be interfaces.
            ("empty exception subclass", "class E(ValueError): ...\n"),
            ("ellipsis marker, no base", "class Marker:\n    ...\n"),
            ("pass marker, no base", "class Marker:\n    pass\n"),
            (
                "docstring-only marker",
                "class Marker:\n    \"\"\"just a marker\"\"\"\n",
            ),
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

    /// An exception-heavy module (the httpx idiom) must not read as nearly-all-abstract. Only
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

    /// Type-hint coverage on a single signature — the receiver is excluded, `*args`/`**kwargs`
    /// don't count, and the return annotation is reported separately.
    #[test]
    fn type_hint_coverage_counts_annotatable_params() {
        let f = |src: &str| {
            let m = &metrics(src).functions[0];
            (
                m.typed_params,
                m.annotatable_params,
                m.has_return_annotation,
            )
        };

        assert_eq!(f("def g(a, b): ...\n"), (0, 2, false), "no hints");
        assert_eq!(
            f("def g(a: int, b) -> str: ...\n"),
            (1, 2, true),
            "partial + return"
        );
        assert_eq!(
            f("def g(a: int, b: str) -> None: ...\n"),
            (2, 2, true),
            "full"
        );
        // `self` is excluded from the denominator; `cls` likewise.
        assert_eq!(
            f("class C:\n    def m(self, a: int): ...\n"),
            (1, 1, false),
            "self excluded"
        );
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
        assert_eq!(
            f("def g(a: int, *args, **kwargs): ...\n"),
            (1, 1, false),
            "variadics ignored"
        );
        // A positional-only receiver (`self, /`) is still the receiver and is excluded.
        assert_eq!(
            f("class C:\n    def m(self, /, a: int): ...\n"),
            (1, 1, false),
            "positional-only self excluded"
        );
        // Keyword-only params (after a bare `*`) count toward the denominator.
        assert_eq!(
            f("def g(*, a: int, b): ...\n"),
            (1, 2, false),
            "keyword-only counted"
        );
        // `async def` is a function definition too — same treatment.
        assert_eq!(
            f("async def g(a: int) -> str: ...\n"),
            (1, 1, true),
            "async"
        );
        // No annotatable params: a zero denominator, return type tracked independently.
        assert_eq!(
            f("def g() -> int: ...\n"),
            (0, 0, true),
            "nullary with return"
        );
    }

    /// Project aggregates — coverage is over the param pool, and the fully-annotated rate
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
