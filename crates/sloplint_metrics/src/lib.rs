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
pub mod test_proxies;

use badge::{Badge, Color};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{
    Comprehension, ExceptHandler, Expr, ModModule, Parameters, Stmt, StmtClassDef, StmtFunctionDef,
};
use sloplint_python::parser::Parsed;
use sloplint_python::{LineIndex, Ranged, TextRange, TextSize, TokenKind};

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

/// Cognitive-complexity bands (#110), anchored on SonarSource's per-function guidance of **15**.
/// Cognitive complexity is the better *readability* signal than cyclomatic — it adds a nesting
/// penalty and charges for breaks in linear flow — so these bands track "how hard is this to read".
/// Boundaries (inclusive): **≤5 low** (trivial), **6–15 moderate** (SonarSource's ceiling), **16–40
/// high** (hard to follow), **>40 very high** (effectively unreadable). Descriptive bands calibrated
/// against the cohort, never a pass/fail gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CognitiveTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl CognitiveTier {
    /// Classify a function's cognitive complexity into its readability band.
    pub fn from_cognitive(cognitive: usize) -> Self {
        match cognitive {
            0..=5 => CognitiveTier::Low,
            6..=15 => CognitiveTier::Moderate,
            16..=40 => CognitiveTier::High,
            _ => CognitiveTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables, JSON, and badges.
    pub fn label(self) -> &'static str {
        match self {
            CognitiveTier::Low => "low",
            CognitiveTier::Moderate => "moderate",
            CognitiveTier::High => "high",
            CognitiveTier::VeryHigh => "very high",
        }
    }

    /// Badge color keyed to the band: low green, moderate yellow, high/very-high red (both exceed
    /// SonarSource's recommended ceiling of 15).
    pub fn color(self) -> Color {
        match self {
            CognitiveTier::Low => Color::Green,
            CognitiveTier::Moderate => Color::Yellow,
            CognitiveTier::High | CognitiveTier::VeryHigh => Color::Red,
        }
    }
}

/// WMC (Weighted Methods per Class) size bands for god-class prevalence (#104). Unlike the
/// cyclomatic [`RiskTier`], WMC has **no** McCabe-equivalent canonical threshold, so these are
/// **descriptive** bands calibrated against the cohort, never a pass/fail standard. Boundaries
/// (inclusive): **≤20 low** (ordinary class), **21–50 moderate** (large but fine), **51–200
/// high** (god-class candidate), **>200 very high** (god-class). WMC is the sum of the cyclomatic
/// complexity of a class's methods, so these run higher than the per-function CC bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WmcTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl WmcTier {
    /// Classify a class's WMC into its size band.
    pub fn from_wmc(wmc: usize) -> Self {
        match wmc {
            0..=20 => WmcTier::Low,
            21..=50 => WmcTier::Moderate,
            51..=200 => WmcTier::High,
            _ => WmcTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            WmcTier::Low => "low",
            WmcTier::Moderate => "moderate",
            WmcTier::High => "high",
            WmcTier::VeryHigh => "very high",
        }
    }
}

/// NOC (Number of Children) breadth bands for fragile-base-class risk (#113) — how many direct
/// first-party subclasses a class has. No canonical CK threshold, so **descriptive** bands
/// calibrated against the cohort, never a pass/fail standard. Boundaries (inclusive): **≤1 low**
/// (a leaf or lightly-extended class), **2–5 moderate**, **6–20 high** (a well-used base),
/// **>20 very high** (a high-leverage hub — every change ripples widely; review carefully).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NocTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl NocTier {
    /// Classify a class's NOC into its breadth band.
    pub fn from_noc(noc: usize) -> Self {
        match noc {
            0..=1 => NocTier::Low,
            2..=5 => NocTier::Moderate,
            6..=20 => NocTier::High,
            _ => NocTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            NocTier::Low => "low",
            NocTier::Moderate => "moderate",
            NocTier::High => "high",
            NocTier::VeryHigh => "very high",
        }
    }
}

/// Module (file) NLOC size bands for god-module prevalence (#107). Like [`WmcTier`], file size has
/// **no** canonical hard threshold, so these are **descriptive** bands calibrated against the
/// cohort (SonarQube's ~750–1000-line guidance is the starting point), never a pass/fail standard.
/// Boundaries (inclusive), in NLOC (non-comment, non-blank lines): **≤250 low** (ordinary module),
/// **251–500 moderate**, **501–1000 high** (god-module candidate), **>1000 very high**
/// (god-module — a dumping-ground smell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleSizeTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl ModuleSizeTier {
    /// Classify a module's NLOC into its size band.
    pub fn from_nloc(nloc: usize) -> Self {
        match nloc {
            0..=250 => ModuleSizeTier::Low,
            251..=500 => ModuleSizeTier::Moderate,
            501..=1000 => ModuleSizeTier::High,
            _ => ModuleSizeTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            ModuleSizeTier::Low => "low",
            ModuleSizeTier::Moderate => "moderate",
            ModuleSizeTier::High => "high",
            ModuleSizeTier::VeryHigh => "very high",
        }
    }
}

/// Function-arity bands for the Long Parameter List smell (#108) — Fowler's canonical signal that
/// parameters want bundling into an object. Counts caller-facing [`FunctionMetrics::arity`], not
/// raw params. No canonical hard threshold (Fowler/Martin suggest keeping arguments ≤3–4), so
/// **descriptive** bands, never a pass/fail standard. Boundaries (inclusive): **≤4 low**,
/// **5–6 moderate**, **7–10 high**, **>10 very high**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParamCountTier {
    Low,
    Moderate,
    High,
    VeryHigh,
}

impl ParamCountTier {
    /// Classify a function's caller-facing arity into its band.
    pub fn from_arity(arity: usize) -> Self {
        match arity {
            0..=4 => ParamCountTier::Low,
            5..=6 => ParamCountTier::Moderate,
            7..=10 => ParamCountTier::High,
            _ => ParamCountTier::VeryHigh,
        }
    }

    /// Short, stable label used in tables and JSON.
    pub fn label(self) -> &'static str {
        match self {
            ParamCountTier::Low => "low",
            ParamCountTier::Moderate => "moderate",
            ParamCountTier::High => "high",
            ParamCountTier::VeryHigh => "very high",
        }
    }
}

/// A four-band tier histogram: how many units fall into each `{low, moderate, high, very_high}`
/// band. Shared by the function cyclomatic tiers (#10), the class WMC tiers (#104), the module
/// NLOC tiers (#107), and the function-arity tiers (#108) — the bands differ per metric (see
/// [`RiskTier`] / [`WmcTier`] / [`ModuleSizeTier`] / [`ParamCountTier`]); the bucket shape does
/// not.
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

    /// Record a function by its cognitive band (#110) — the readability counterpart to
    /// [`Self::record`] (which buckets by cyclomatic).
    fn record_cognitive(&mut self, cognitive: usize) {
        match CognitiveTier::from_cognitive(cognitive) {
            CognitiveTier::Low => self.low += 1,
            CognitiveTier::Moderate => self.moderate += 1,
            CognitiveTier::High => self.high += 1,
            CognitiveTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its WMC band (#104) — the class-side counterpart to [`Self::record`].
    fn record_wmc(&mut self, wmc: usize) {
        match WmcTier::from_wmc(wmc) {
            WmcTier::Low => self.low += 1,
            WmcTier::Moderate => self.moderate += 1,
            WmcTier::High => self.high += 1,
            WmcTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a class by its NOC band (#113) — inheritance breadth (fragile-base-class risk).
    fn record_noc(&mut self, noc: usize) {
        match NocTier::from_noc(noc) {
            NocTier::Low => self.low += 1,
            NocTier::Moderate => self.moderate += 1,
            NocTier::High => self.high += 1,
            NocTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a module by its NLOC band (#107) — the file-side counterpart to [`Self::record`].
    fn record_module_size(&mut self, nloc: usize) {
        match ModuleSizeTier::from_nloc(nloc) {
            ModuleSizeTier::Low => self.low += 1,
            ModuleSizeTier::Moderate => self.moderate += 1,
            ModuleSizeTier::High => self.high += 1,
            ModuleSizeTier::VeryHigh => self.very_high += 1,
        }
    }

    /// Record a function by its arity band (#108) — the Long-Parameter-List counterpart to
    /// [`Self::record`].
    fn record_arity(&mut self, arity: usize) {
        match ParamCountTier::from_arity(arity) {
            ParamCountTier::Low => self.low += 1,
            ParamCountTier::Moderate => self.moderate += 1,
            ParamCountTier::High => self.high += 1,
            ParamCountTier::VeryHigh => self.very_high += 1,
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

    /// The worst occupied band as a [`CognitiveTier`] — the cognitive counterpart to
    /// [`Self::worst_tier`], for the cognitive badge/markdown (#110). `None` only when empty.
    pub fn worst_cognitive_tier(self) -> Option<CognitiveTier> {
        if self.very_high > 0 {
            Some(CognitiveTier::VeryHigh)
        } else if self.high > 0 {
            Some(CognitiveTier::High)
        } else if self.moderate > 0 {
            Some(CognitiveTier::Moderate)
        } else if self.low > 0 {
            Some(CognitiveTier::Low)
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
    /// Number of declared parameters, including the `self`/`cls` receiver and `*args`/`**kwargs`
    /// (each variadic counted once). The raw signature width; see [`Self::arity`] for the
    /// caller-facing count the long-parameter-list metric uses.
    pub params: usize,
    /// Caller-facing arity (#108): [`Self::params`] minus the `self`/`cls` receiver — the
    /// parameters a caller actually passes. `*args`/`**kwargs` each count once (a `**kwargs` sink
    /// is the *opposite* of a long parameter list, so it must not inflate the count). The input to
    /// the Long-Parameter-List bands ([`ParamCountTier`]).
    pub arity: usize,
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
    /// Whether the function's first body statement is a bare string literal (a docstring). A
    /// `StringLiteral` token, not a `Comment`, so this is orthogonal to `comment_density` (#83).
    pub has_docstring: bool,
    /// Physical lines spanned by the docstring, or 0 if there is none. A verbose docstring on a
    /// trivial (low-`ncss`) function is the AI **over-documentation** signal that a bare
    /// `has_docstring` boolean can't capture.
    pub docstring_lines: usize,
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
    /// WMC — Weighted Methods per Class (Chidamber & Kemerer 1994): the sum of the cyclomatic
    /// complexity of the class's **direct** methods. A class-weight measure — "how heavy is this
    /// class" — that distinguishes 40 trivial accessors from 40 branchy ones, unlike a raw
    /// method count. Each method's complexity is its own-body cyclomatic (nested defs excluded,
    /// as in [`FunctionMetrics::cyclomatic`]).
    pub wmc: usize,
    /// DIT — Depth of Inheritance Tree (Chidamber & Kemerer 1994): the longest path from this
    /// class to a root through its bases, counting **first-party** bases only. Resolved
    /// project-wide by [`resolve_inheritance`] (0 until that pass runs). Bases that resolve
    /// to `object`, the stdlib, or a third party are invisible and terminate the chain, so this
    /// is a conservative under-count of the true Python MRO depth (#84).
    pub dit: usize,
    /// NOC — Number of Children (Chidamber & Kemerer 1994): how many **direct** subclasses this
    /// class has within first-party code — the inheritance *breadth* that pairs with [`Self::dit`]
    /// depth. The in-degree of the same class graph, resolved project-wide by
    /// [`resolve_inheritance`] (0 until that pass runs). A high-NOC base is a change-amplifier
    /// (fragile-base-class risk); often it's a well-used abstraction (#113).
    pub noc: usize,
    /// Trailing identifiers of this class's base expressions (`Base` from `pkg.mod.Base`), in
    /// source order — the raw input to [`resolve_inheritance`]. Unresolved here; whether a base is
    /// first-party is decided project-wide against the full class set.
    pub bases: Vec<String>,
    /// Whether this class counts as "abstract" for Martin's package abstractness ratio (#70).
    /// A documented heuristic ([`class_is_abstract`]), since Python has no interface keyword.
    pub is_abstract: bool,
    /// Whether the class's first body statement is a bare string literal (a docstring). See
    /// [`FunctionMetrics::has_docstring`] — same rule, applied to the class body (#83).
    pub has_docstring: bool,
    /// Physical lines spanned by the class docstring, or 0 if there is none.
    pub docstring_lines: usize,
}

/// Metrics for a single file.
#[derive(Debug, Clone)]
pub struct FileMetrics {
    pub functions: Vec<FunctionMetrics>,
    pub classes: Vec<ClassMetrics>,
    pub loc: usize,
    pub comment_lines: usize,
    /// NLOC — physical lines bearing a non-comment, non-trivia token (code or string-literal
    /// content), i.e. excluding blank and comment-only lines (#107). The module-size measure;
    /// distinct from the comment-inclusive physical [`Self::loc`].
    pub nloc: usize,
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
    /// Mean caller-facing arity across all functions ([`FunctionMetrics::arity`]) (#108).
    pub avg_params: f64,
    /// Highest caller-facing arity — the worst Long Parameter List, which the mean hides.
    pub max_params: usize,
    /// 95th-percentile caller-facing arity (nearest-rank) — the heavy tail, mirroring
    /// [`Self::p95_cyclomatic`].
    pub p95_params: usize,
    /// Count of functions in each arity band (#108) — Long-Parameter-List *prevalence*. Counts
    /// caller-facing arity (`self`/`cls` excluded, `*args`/`**kwargs` once). Descriptive bands
    /// ([`ParamCountTier`]), never a gate.
    pub param_count_risk: RiskHistogram,
    pub max_cognitive: usize,
    /// Mean cognitive complexity across all functions (#110).
    pub avg_cognitive: f64,
    /// 95th-percentile cognitive complexity (nearest-rank) — the "hard-to-read tail", mirroring
    /// [`Self::p95_cyclomatic`] (#110).
    pub p95_cognitive: usize,
    /// Count of functions in each cognitive readability band ([`CognitiveTier`], #110). Brings
    /// cognitive to parity with cyclomatic, which already has full distribution + tiers; cognitive
    /// is the better readability signal, so its distribution (not just the max) is the one to watch.
    pub cognitive_risk: RiskHistogram,
    pub max_nesting: usize,
    /// Comment lines as a fraction of total lines (0.0–1.0).
    pub comment_density: f64,
    /// Type-hint coverage (#85): annotated params / annotatable params across all functions
    /// (0.0–1.0). Low coverage flags under-annotation; high coverage is neutral, never a smell.
    pub param_annotation_coverage: f64,
    /// Fraction of functions that are fully annotated — every annotatable param plus the return
    /// type (0.0–1.0).
    pub fully_annotated_function_rate: f64,
    /// Mean module NLOC across all files — the size triad's third leg (#107).
    pub avg_module_nloc: f64,
    /// Largest module by NLOC. The single god-module the repo sum/`avg` would otherwise hide.
    pub max_module_nloc: usize,
    /// 95th-percentile module NLOC (nearest-rank) — the heavy tail, mirroring
    /// [`Self::p95_cyclomatic`]/[`Self::p95_wmc`].
    pub p95_module_nloc: usize,
    /// Count of files in each module-size band (#107) — god-module *prevalence*, which the repo
    /// `total_loc` sum and the `avg` collapse. Descriptive bands ([`ModuleSizeTier`]), never a
    /// gate.
    pub module_size_risk: RiskHistogram,
    /// Number of classes across all files — the denominator for the WMC/DIT averages.
    pub classes: usize,
    /// Heaviest class by WMC (sum of its methods' cyclomatic complexity).
    pub max_wmc: usize,
    /// Mean WMC across all classes.
    pub avg_wmc: f64,
    /// 95th-percentile class WMC (nearest-rank). Surfaces the heavy tail even when the mean is
    /// pulled down by many tiny classes — the WMC counterpart to [`Self::p95_cyclomatic`] (#104).
    pub p95_wmc: usize,
    /// Count of classes in each WMC size band (#104) — god-class *prevalence*, which `avg`/`max`
    /// alone hide: the same `max_wmc` can come from one justified hub or many. Descriptive bands
    /// ([`WmcTier`]), never a gate.
    pub wmc_risk: RiskHistogram,
    /// Deepest first-party inheritance chain (DIT). Requires [`resolve_inheritance`] to have run
    /// over the file set first; otherwise every `dit` is 0.
    pub max_dit: usize,
    /// Mean DIT across all classes.
    pub avg_dit: f64,
    /// Most direct first-party subclasses any class has (NOC) — the worst fragile-base-class
    /// blast radius. Requires [`resolve_inheritance`] to have run (#113).
    pub max_noc: usize,
    /// Mean NOC across all classes.
    pub avg_noc: f64,
    /// 95th-percentile class NOC (nearest-rank) — the breadth tail; most classes are leaves
    /// (NOC 0), so p95 surfaces the hubs the mean buries.
    pub p95_noc: usize,
    /// Count of classes in each NOC breadth band (#113) — fragile-base-class *prevalence*.
    /// Descriptive bands ([`NocTier`]), never a gate.
    pub noc_risk: RiskHistogram,
    /// Docstring coverage: public defs/classes carrying a docstring, as a fraction of all public
    /// defs/classes (0.0–1.0). "Public" = a name not `_`-prefixed. Distinct from
    /// `comment_density` (which counts `#`-comments, not docstrings) — low coverage flags an
    /// under-documented public API. 0.0 when there are no public units (#83).
    pub docstring_coverage: f64,
    /// Docstring-to-code ratio: total **function** docstring lines over total **function** NCSS
    /// (which counts the docstring's own expression statement). Function-scoped on both sides so
    /// the ratio has one unit — class docstrings count toward [`Self::docstring_coverage`], not
    /// here. A high ratio flags AI **over-documentation** — verbose docstrings piled onto trivial
    /// code. 0.0 when there are no functions (#83).
    pub docstring_code_ratio: f64,
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

    /// The arity counterpart to [`Self::cyclomatic_markdown`] (#108): mean/p95/max parameters plus
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

    /// A badge summarizing cognitive-complexity risk (#110): the worst occupied band plus the peak
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

    /// The cognitive counterpart to [`Self::cyclomatic_markdown`] (#110): mean/p95/max cognitive plus
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

    /// The class-size counterpart to [`Self::cyclomatic_markdown`] (#104): mean/p95/max WMC plus
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

    /// The inheritance-breadth counterpart to [`Self::cyclomatic_markdown`] (#113): mean/p95/max
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

    /// The module-size counterpart to [`Self::cyclomatic_markdown`] (#107): mean/p95/max NLOC plus
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
    }
}

/// NLOC for a file (#107): the count of physical lines that carry at least one non-comment,
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
    let mut module_nloc_sum = 0usize;
    let mut module_nloc_values: Vec<usize> = Vec::new();
    // Docstring coverage (#83): every public def/class (functions *and* classes) is in the
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
        module_nloc_sum += file.nloc;
        module_nloc_values.push(file.nloc);
        repo.max_module_nloc = repo.max_module_nloc.max(file.nloc);
        repo.module_size_risk.record_module_size(file.nloc);
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
            repo.max_wmc = repo.max_wmc.max(class.wmc);
            repo.max_dit = repo.max_dit.max(class.dit);
            repo.max_noc = repo.max_noc.max(class.noc);
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
    repo
}

/// Whether a def/class name is "public" for docstring coverage — i.e. not `_`-prefixed (#83).
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
/// `comment_density` and this metric is purely additive (#83).
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

/// Fill in [`ClassMetrics::dit`] and [`ClassMetrics::noc`] for every class across the project —
/// the CK inheritance pair (#84 depth, #113 breadth). Both resolve bases by **trailing class
/// name** against the set of first-party classes in `files`; a base that doesn't resolve —
/// `object`, the stdlib, a third party, or any name no first-party class claims — is invisible.
///
/// - **DIT** (depth): the longest path from a class up to a root via its bases. An external base
///   terminates the chain, so this is a conservative under-count; a class with no first-party base
///   has DIT 0.
/// - **NOC** (breadth): the number of **direct** subclass *definitions* that name this class as a
///   base — the in-degree of the same graph. A class no first-party class extends has NOC 0.
///
/// When a class name is defined more than once, depth uses the first definition's bases and both
/// figures are assigned by name (every class of that name gets the same DIT/NOC); names are sorted
/// so the result is deterministic. Real Python inheritance is acyclic, but name collisions could
/// synthesize a cycle; a name already on the current resolution path terminates it, so the pass
/// always halts.
pub fn resolve_inheritance(files: &mut [&mut FileMetrics]) {
    use std::collections::HashMap;

    let mut bases_of: HashMap<&str, &[String]> = HashMap::new();
    for file in files.iter() {
        for class in &file.classes {
            bases_of
                .entry(class.name.as_str())
                .or_insert(class.bases.as_slice());
        }
    }

    let mut cache: HashMap<&str, usize> = HashMap::new();
    let mut names: Vec<&str> = bases_of.keys().copied().collect();
    names.sort_unstable();
    for name in names {
        dit_of(name, &bases_of, &mut cache, &mut Vec::new());
    }
    // NOC (breadth): the in-degree of the inheritance graph. Count, per first-party class name,
    // every class *definition* that lists it as a direct base (so two distinct subclasses of the
    // same base count twice, even if the base is defined once). A single child's bases are deduped
    // by name first, so a class is counted once per base even if it names that base twice (e.g.
    // `class X(a.Base, b.Base)` where both trail to `Base`) — it's still one child of `Base`.
    let mut children: HashMap<&str, usize> = HashMap::new();
    for file in files.iter() {
        for class in &file.classes {
            let mut counted: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for base in &class.bases {
                if bases_of.contains_key(base.as_str()) && counted.insert(base.as_str()) {
                    *children.entry(base.as_str()).or_insert(0) += 1;
                }
            }
        }
    }

    // Detach both maps from `bases_of`'s borrow of `files` so we can write them back.
    let depths: HashMap<String, usize> = cache.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let noc: HashMap<String, usize> = children.iter().map(|(k, v)| (k.to_string(), *v)).collect();

    for file in files.iter_mut() {
        for class in &mut file.classes {
            class.dit = depths.get(&class.name).copied().unwrap_or(0);
            class.noc = noc.get(&class.name).copied().unwrap_or(0);
        }
    }
}

/// Longest first-party base chain above `name`, memoized in `cache`. `path` holds the names on
/// the current DFS branch; revisiting one means a (collision-induced) cycle, severed by
/// returning 0 there without caching. Depths *on or just above* such a cycle are then
/// ill-defined — they reflect where the back-edge happened to be cut — but the cut point is
/// fixed (names are resolved in sorted order), so the result is at least deterministic, and the
/// only contract for the cyclic case is that the pass halts. Acyclic inheritance (i.e. all real
/// Python) memoizes exactly.
fn dit_of<'a>(
    name: &'a str,
    bases_of: &std::collections::HashMap<&'a str, &'a [String]>,
    cache: &mut std::collections::HashMap<&'a str, usize>,
    path: &mut Vec<&'a str>,
) -> usize {
    if let Some(depth) = cache.get(name) {
        return *depth;
    }
    if path.contains(&name) {
        return 0;
    }
    let Some(bases) = bases_of.get(name) else {
        return 0;
    };
    path.push(name);
    let mut best = 0;
    for base in bases.iter() {
        // Resolve by name against the first-party class set; only a base another first-party
        // class claims extends the chain.
        if let Some((first_party_name, _)) = bases_of.get_key_value(base.as_str()) {
            best = best.max(1 + dit_of(first_party_name, bases_of, cache, path));
        }
    }
    path.pop();
    cache.insert(name, best);
    best
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

/// Whether the function's first positional parameter is a `self`/`cls` receiver (`1`) or not
/// (`0`). A non-static method whose first parameter (positional-only first, else the first regular
/// arg) is named `self`/`cls` carries one. Caller-invisible, so it counts toward neither annotation
/// coverage (#85) nor arity (#108).
fn receiver_count(function: &StmtFunctionDef) -> usize {
    let params = &function.parameters;
    usize::from(
        !is_staticmethod(function)
            && params
                .posonlyargs
                .first()
                .or_else(|| params.args.first())
                .is_some_and(|param| matches!(param.parameter.name.as_str(), "self" | "cls")),
    )
}

/// Caller-facing arity (#108): every declared parameter a caller passes — positional-only,
/// regular, keyword-only, and `*args`/`**kwargs` (each variadic once) — minus the `self`/`cls`
/// receiver. The input to the Long-Parameter-List bands.
fn caller_arity(function: &StmtFunctionDef) -> usize {
    param_count(&function.parameters) - receiver_count(function)
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
    // Drop exactly one leading positional for a non-static method whose first parameter is the
    // `self`/`cls` receiver (see [`receiver_count`]).
    let skip_receiver = receiver_count(function);

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
///
/// `pub(crate)` so [`test_proxies`] can score a test function's own body for the trivial-test
/// signal (#121) using the same definition the function panel uses.
pub(crate) fn cognitive(body: &[Stmt]) -> usize {
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
    fn wmc_tier_boundaries() {
        // Descriptive bands (#104): ≤20 low, 21–50 moderate, 51–200 high, >200 very high.
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
        // Descriptive breadth bands (#113): ≤1 low, 2–5 moderate, 6–20 high, >20 very high.
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
        // Descriptive NLOC bands (#107): ≤250 low, 251–500 moderate, 501–1000 high, >1000 very high.
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
        // Descriptive arity bands (#108): ≤4 low, 5–6 moderate, 7–10 high, >10 very high.
        assert_eq!(ParamCountTier::from_arity(0), ParamCountTier::Low);
        assert_eq!(ParamCountTier::from_arity(4), ParamCountTier::Low);
        assert_eq!(ParamCountTier::from_arity(5), ParamCountTier::Moderate);
        assert_eq!(ParamCountTier::from_arity(6), ParamCountTier::Moderate);
        assert_eq!(ParamCountTier::from_arity(7), ParamCountTier::High);
        assert_eq!(ParamCountTier::from_arity(10), ParamCountTier::High);
        assert_eq!(ParamCountTier::from_arity(11), ParamCountTier::VeryHigh);
    }

    #[test]
    fn arity_excludes_receiver_and_counts_variadics_once() {
        let by_name = |src: &str| {
            metrics(src)
                .functions
                .into_iter()
                .map(|f| (f.name, f.params, f.arity))
                .collect::<Vec<_>>()
        };
        let fns = by_name(
            "\
def free(a, b, c):
    return a

class C:
    def method(self, x, y):
        return x

    @staticmethod
    def stat(self, z):
        return z

    def variadic(self, *args, **kwargs):
        return args
",
        );
        // (name, params incl. receiver/variadics, arity caller-facing)
        assert_eq!(fns[0], ("free".into(), 3, 3), "no receiver");
        assert_eq!(fns[1], ("method".into(), 3, 2), "self excluded from arity");
        assert_eq!(
            fns[2],
            ("stat".into(), 2, 2),
            "@staticmethod: the first param is a real arg"
        );
        assert_eq!(
            fns[3],
            ("variadic".into(), 3, 2),
            "self excluded; *args + **kwargs count once each"
        );
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

    #[test]
    fn wmc_sums_method_cyclomatic() {
        // calc: CC 1; check: `if` (+1) + `and` (+1) over base 1 = 3. WMC = 1 + 3 = 4. The bare
        // `Empty` class contributes 0 (no methods).
        let file = metrics(
            "\
class C:
    def calc(self):
        return 1
    def check(self, x):
        if x and x > 0:
            return True
        return False

class Empty:
    pass
",
        );
        let c = &file.classes[0];
        assert_eq!(c.wmc, 4, "1 (calc) + 3 (check: if + and)");
        assert_eq!(file.classes[1].wmc, 0, "no methods → no weight");
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

    #[test]
    fn wmc_excludes_nested_helper_complexity() {
        // A method's WMC contribution is its *own-body* cyclomatic, like FunctionMetrics: the
        // nested `inner`'s branch belongs to `inner`, not to `m`. m's own body: just the `if`
        // guarding the def → CC 2. inner's `for`+`if` are excluded.
        let file = metrics(
            "\
class C:
    def m(self, flag):
        def inner(xs):
            for x in xs:
                if x:
                    return x
        if flag:
            return inner([])
        return None
",
        );
        assert_eq!(
            file.classes[0].wmc, 2,
            "only m's own `if` counts, not inner's"
        );
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
