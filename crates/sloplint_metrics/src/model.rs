//! The metric data model: per-function, per-class, and per-file metric records produced by
//! `file_metrics`. Plain data — the computation lives in the sibling modules, the aggregation in
//! `aggregate`, and the risk-tier classification in `risk`.

use sloplint_python::TextRange;

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
    /// Caller-facing arity: [`Self::params`] minus the `self`/`cls` receiver — the
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
    /// Type-hint coverage: parameters carrying an annotation, out of [`Self::annotatable_params`].
    pub typed_params: usize,
    /// Parameters eligible for an annotation — positional and keyword params, excluding the
    /// `self`/`cls` receiver and `*args`/`**kwargs`. The denominator for parameter annotation
    /// coverage; `0` for a function with no annotatable params (e.g. `def f(self): ...`).
    pub annotatable_params: usize,
    /// Whether the function declares a return-type annotation (`-> T`).
    pub has_return_annotation: bool,
    /// Whether the function's first body statement is a bare string literal (a docstring). A
    /// `StringLiteral` token, not a `Comment`, so this is orthogonal to `comment_density`.
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
    /// is a conservative under-count of the true Python MRO depth.
    pub dit: usize,
    /// NOC — Number of Children (Chidamber & Kemerer 1994): how many **direct** subclasses this
    /// class has within first-party code — the inheritance *breadth* that pairs with [`Self::dit`]
    /// depth. The in-degree of the same class graph, resolved project-wide by
    /// [`resolve_inheritance`] (0 until that pass runs). A high-NOC base is a change-amplifier
    /// (fragile-base-class risk); often it's a well-used abstraction.
    pub noc: usize,
    /// Trailing identifiers of this class's base expressions (`Base` from `pkg.mod.Base`), in
    /// source order — the raw input to [`resolve_inheritance`]. Unresolved here; whether a base is
    /// first-party is decided project-wide against the full class set.
    pub bases: Vec<String>,
    /// CBO — Coupling Between Objects (Chidamber & Kemerer 1994): the number of **distinct
    /// first-party classes** this class is coupled to. Resolved project-wide by
    /// [`resolve_inheritance`] against the first-party class set (0 until that pass runs).
    ///
    /// A class-level coupling measure — "how central is this class" — distinct from WMC (size) and
    /// DIT/NOC (inheritance): a small class wired to 30 collaborators is a fragile hub a change
    /// ripples out from. Python has no static types, so this is an **approximation, biased low** —
    /// it counts coupling via base classes, instantiations (`ClassName(...)`), `isinstance`/
    /// `issubclass` checks, and type annotations, but **misses duck-typed** coupling
    /// (`self.axes.foo()` where `axes` is unannotated). Resolution is scope-unaware, so a local or
    /// parameter shadowing a class name can occasionally overcount. Most reliable on well-typed code
    /// (matplotlib's `Axes` is the textbook hub it can only partly see).
    pub cbo: usize,
    /// Distinct trailing identifiers this class references as a *coupling candidate* — base class
    /// names, instantiation/`isinstance`/`issubclass` callees, and type-annotation names — sorted,
    /// deduped. The raw input to [`Self::cbo`], resolved against the first-party class set
    /// project-wide (a candidate that no first-party class claims, e.g. `int`/`list`, is dropped).
    pub coupled: Vec<String>,
    /// Whether this class counts as "abstract" for Martin's package abstractness ratio.
    /// A documented heuristic ([`class_is_abstract`]), since Python has no interface keyword.
    pub is_abstract: bool,
    /// Whether the class's first body statement is a bare string literal (a docstring). See
    /// [`FunctionMetrics::has_docstring`] — same rule, applied to the class body.
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
    /// content), i.e. excluding blank and comment-only lines. The module-size measure;
    /// distinct from the comment-inclusive physical [`Self::loc`].
    pub nloc: usize,
    /// Exception-handling hygiene counts for this file: total/`bare`/`broad`/`swallow`
    /// `except` handlers, anywhere in the file (module level or nested).
    pub exception: ExceptionStats,
    /// Executable-logic statements at **module scope** — not inside any function/method, and
    /// excluding imports, the module docstring, the `if __name__ == "__main__":` guard, class-body
    /// declarations, and pure constant assignments. The "logic dumped at top level" count.
    pub top_level_code: usize,
    /// Executable-logic statements **inside** functions/methods. With [`Self::top_level_code`]
    /// these give the top-level-code ratio = `top_level_code / (top_level_code + function_code)` —
    /// how much of a module's logic lives at module scope vs. organized into functions.
    pub function_code: usize,
}

/// Exception-handling hygiene counts — broad-except and silent-swallow are reliable
/// low-effort / "make-it-work" smells (wrap it in `except Exception` or `except: pass` so the
/// error disappears). Counted by AST over every `except` handler; aggregated into a *rate* the
/// per-site lints (Ruff `E722`/`BLE001`) can't express. Descriptive — broad except is sometimes
/// correct (top-level daemon loops, plugin boundaries) — so it's read as a cohort rate, never a
/// gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExceptionStats {
    /// Total `except` clauses.
    pub handlers: usize,
    /// Bare `except:` (no exception type). Near-extinct in practice (Ruff `E722` catches it).
    pub bare: usize,
    /// Broad `except Exception` / `except BaseException` (or a tuple containing one) — the real
    /// signal default Ruff doesn't aggregate.
    pub broad: usize,
    /// Silent-swallow handlers whose body is exactly `pass`, `continue`, or `...` — discarding the
    /// error with no handling. The strongest sub-signal; rarely justified.
    pub swallow: usize,
}
