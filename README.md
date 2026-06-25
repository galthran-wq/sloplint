# sloplint

A fast, deterministic, **no-LLM** linter that counters AI slop in Python — a deliberately
nitpicking, opinionated layer that runs **right after [Ruff](https://docs.astral.sh/ruff/)**
in the same CI job. Ruff handles standard linting; sloplint adds the strict, slop-specific
judgments Ruff intentionally won't ship, and **never re-checks anything Ruff already covers**.

Written in Rust, reusing Ruff's own parser crates for a full-fidelity AST + token stream.

sloplint has two halves:

- **Software-quality metrics** — a deterministic, research-backed measurement layer over your
  code (complexity, cohesion, coupling, architecture, duplication, test substance). This is the
  foundation: the rules below are increasingly built on top of it. **[Jump to the reference
  ↓](#software-quality-metrics)**
- **Lint rules** — strict `SLP*` rules that flag slop patterns no mainstream linter covers.

## Installation

sloplint ships on PyPI as **`sloplintpy`** (the wheel bundles the native binary — no Rust
toolchain needed). The installed command is **`sloplint`**.

Run it directly with [uvx](https://docs.astral.sh/uv/) (the package and command differ, so use
`--from`):

```bash
uvx --from sloplintpy sloplint check    # Lint all files in the current directory.
uvx --from sloplintpy sloplint metrics  # Report software-quality metrics.
```

Or install `sloplintpy` with uv (recommended), pip, or pipx — then run `sloplint`:

```bash
uv tool install sloplintpy@latest   # uv: install the `sloplint` command globally
uv add --dev sloplintpy             # uv: or add it to your project
pip install sloplintpy              # pip
pipx install sloplintpy             # pipx
```

From a clone, run it through cargo instead (`cargo run -p sloplint -- check path/to/code`), or
build a wheel locally with [maturin](https://www.maturin.rs/) (`maturin build --release`).

## Usage

```bash
sloplint check path/to/code              # lint (exit 1 on findings)
sloplint check src --fix                 # auto-fix findings that have a safe fix
sloplint check src --format sarif        # SARIF / json / github / text / agent
sloplint metrics src                     # software-quality metrics table (production code)
sloplint metrics src --scope all         # a panel for every profile (default: production only)
sloplint metrics src --format github     # PR-summary markdown (CC risk tiers)
sloplint metrics src --format packages   # per-package feed: coupling, cycles, abstractness (JSONL)
sloplint metrics src --max-cyclomatic 10 # CI gate: exit 1 over McCabe's ceiling
sloplint metrics src --badges badges/    # emit SVG + shields-endpoint badges
sloplint init                            # wire sloplint into your AI coding tool
sloplint rule SLP030                     # explain a rule (or `sloplint rule` to list all)
sloplint rule --format json              # machine-readable rule metadata
sloplint parse file.py                   # dump AST + tokens (debug aid)
```

Comments are banned by default; relax per-path (see [Configuration](#configuration)). Preview
rules need `--preview`.

More, on the [wiki](https://github.com/galthran-wq/sloplint/wiki):
**[Autofix](https://github.com/galthran-wq/sloplint/wiki/Autofix)** ·
**[Agent-loop integration](https://github.com/galthran-wq/sloplint/wiki/Agent-loop-integration)**
(run sloplint inside your AI coding tool's edit loop) ·
**[GitHub Action](https://github.com/galthran-wq/sloplint/wiki/GitHub-Action)**.

## Software-quality metrics

`sloplint metrics` reports a deterministic, reproducible measurement layer over your code — **no
LLM, no randomness**. These are **measured, not linted**, so they never duplicate Ruff. They're
descriptive signals for tracking a repo over time or comparing across a cohort, and the foundation
the lint rules increasingly build on.

The guiding principle: **no single metric orders code by quality.** You read the whole panel
(function → class → module → package), in context. Most distributions are reported as **risk-tier
histograms** (`low` / `moderate` / `high` / `very_high`), because an `avg`/`max` collapses the
distribution — the same `max` could be one justified hub or fifty god-units. Where a metric has no
canonical hard threshold, the bands are **descriptive, never a gate**: high `high`/`very_high`
counts flag units to *read*, not defects.

Gate the ones with a canonical ceiling in CI by exit code (each names the offending units and exits 1):

```bash
sloplint metrics src --max-cyclomatic 10   # fail if any function's cyclomatic complexity > 10
sloplint metrics src --max-cognitive 15    # ditto for SonarSource cognitive complexity
```

`--format json` emits the full rollup; `--format functions` / `classes` / `packages` are per-unit
JSONL discovery feeds; the text table and `--format github` PR summary show the headline figures.

### Metric reference

| Metric | Level | What it measures |
| --- | --- | --- |
| [Cyclomatic complexity](#cyclomatic--cognitive-complexity) | function | McCabe branch count, with risk tiers |
| [Cognitive complexity](#cyclomatic--cognitive-complexity) | function | SonarSource readability-weighted complexity |
| [Function length](#function-length) | function | physical LoC + longest *logic* function |
| [Parameter count / arity](#parameter-count) | function | Long-Parameter-List smell (caller-facing arity) |
| [Max nesting](#cyclomatic--cognitive-complexity) | function | deepest control-flow nesting |
| [Type-hint coverage](#type-hint-coverage) | function | under-annotation as a quality concern |
| [WMC / DIT / NOC / CBO / LCOM4](#class-metrics) | class | CK class metrics + cohesion |
| [Module size (NLOC)](#module-size) | module | god-module detection |
| [Top-level / undecomposed code](#top-level--undecomposed-code) | module | logic dumped at module scope vs. in functions |
| [Package & module architecture](#package--module-architecture) | project | coupling, cycles, propagation cost, modularity, concentration |
| [Duplication density](#duplication-density) | project | clone ratio (the SLP020 engine as a cohort aggregate) |
| [Comment & docstring density](#comment--docstring-coverage) | project | comment density, docstring coverage, docstring/code ratio |
| [Exception-handling hygiene](#exception-handling-hygiene) | project | broad / swallowed exception rates |
| [Static test proxies](#static-test-proxies-not-coverage) | project | test:code, assertion density, assertion-free rate, doctest coverage |
| [God-unit tail](#god-unit-tail) | cross-cutting | how many units land in the worst band of each distribution |

### Per-profile metric panels

Different parts of a codebase have different healthy norms — test code is legitimately longer,
more repetitive, and less type-annotated; generated code and examples differ again — so collapsing
them into one set of aggregates misleads in either direction (a heavy test-support class can
dominate the "worst class", a thin test suite can drag down the averages). `sloplint metrics`
reports a panel **per profile** (see [Configuration](#configuration)), in **one run**. With zero
config that's the built-in `tests` / `generated` / `production` split.

**Machine-generated code** is detected and segregated automatically into a built-in `generated`
profile, kept out of the `production` aggregates by default — exactly as tests are. A generated
`models/` dump or a 34k-line OpenAPI client manufactures "god-classes", "god-modules", and
sync/async clones that are codegen artifacts, not maintainability signal. This is **not**
"generated code is slop" (provenance isn't badness): the files are regenerated, never hand-edited,
so their numbers are *noise* in a human-code signal. Detection is a cheap, high-precision header
scan for the markers generators emit (`@generated`, `DO NOT EDIT`, `openapi-generator`,
`swagger-codegen`) plus the protobuf `*_pb2.py` / `*_pb2_grpc.py` filename convention. The panel is
still *reported* (a file that was *supposed* to be generated but got hand-edited is exactly what to
surface) — just not folded into production. Setting `generated = true` on any profile extends the
same content detection.

- **`--scope <profile>`** (default: `production`) selects which profile the text view and the
  per-unit feeds report; `--scope all` prints a panel for every profile. The **packages graph is
  built from the scoped profile's modules only**, so a file in one profile importing another can't
  manufacture cycles or coupling in the first profile's architecture metrics.
- **`--format json`** ignores `--scope` and is always comprehensive: a panel for every profile
  under **`profiles`** (keyed by name), plus the project-wide **`test_proxies`** split. One
  invocation yields every view.

### Cyclomatic & cognitive complexity

Both are measured **per function**, each reported with mean / p95 / max and a risk-tier histogram:

- **Cyclomatic complexity** — McCabe's branch count. The `--max-cyclomatic` gate uses McCabe's
  canonical 10/function ceiling.
- **Cognitive complexity** — SonarSource's readability-weighted variant (nesting costs more than
  breadth); its bands are anchored on SonarSource's 15/function guidance, and it's the better
  *readability* signal. Gated via `--max-cognitive`.
- **Max nesting** — the deepest control-flow nesting in a function, a third structural angle.

```jsonc
"cyclomatic_risk": { "low": 7423, "moderate": 492, "high": 285, "very_high": 142 }
```

### Function length

Average and max physical lines per function, plus **`max_logic_function_loc`** — the longest
function that's actually *logic*, not a straight-line data/config-init blob. A 600-line dict
literal and a 600-line branchy function are very different maintainability stories; the logic
variant separates them.

### Parameter count

The **Long Parameter List** smell (Fowler) — too many arguments, a sign of a missing abstraction
or data clump. Per profile:

```jsonc
"params": { "avg": 3.1, "max": 26, "p95": 8 },
"param_count_risk": { "low": 7423, "moderate": 492, "high": 285, "very_high": 142 }
// bands by arity:  low ≤4   moderate 5–6   high 7–10   very_high >10
```

Counts **caller-facing arity**: the `self`/`cls` receiver is excluded, and `*args`/`**kwargs` count
once each (a `**kwargs` sink is the *opposite* of a long parameter list, so matplotlib-style APIs
don't false-positive). It's distinct from complexity — a CC-3 wrapper threading 25 options is
invisible to every other metric. Arity has no canonical hard threshold (Fowler/Martin suggest
≤3–4), so the bands are **descriptive, never a gate**.

### Type-hint coverage

`--format json` rolls up `param_annotation_coverage` (annotated ÷ annotatable params) and
`fully_annotated_function_rate` (functions with every param **and** the return type annotated).
Annotatable params exclude the `self`/`cls` receiver and `*args`/`**kwargs`. This measures
**under**-annotation as a quality concern (missing types are harder to read and refactor, and
weaken tooling) — the bad direction is *low* coverage only. Fully-typed code is neutral-to-good and
is never itself a slop signal. The `--format functions` feed carries `typed_params`,
`annotatable_params`, and `has_return_annotation` per function.

### Class metrics

`--format classes` emits one JSONL row per class — `loc`, `methods`, `attributes`, **`lcom4`**
cohesion, `is_abstract`, and the CK class metrics ([Chidamber & Kemerer 1994][ck]):

- **`wmc`** — Weighted Methods per Class: the sum of the cyclomatic complexity of the class's
  direct methods. A class-*weight* measure that separates 40 trivial accessors from 40 branchy
  ones, where a raw method count can't.
- **`dit`** — Depth of Inheritance Tree: the longest path up to a root through **first-party**
  bases. Bases that resolve to `object`, the stdlib, or a third party end the chain, so `dit` is a
  deliberate, conservative under-count of the true Python MRO depth.
- **`noc`** — Number of Children: how many **direct** first-party subclasses the class has — the
  inheritance *breadth* that pairs with `dit` depth. A high-NOC base is a change-amplifier
  (fragile-base-class risk). Often that's good design (a well-used abstraction — yt-dlp's
  `InfoExtractor` is subclassed by ~965 extractors), so it flags bases to *review carefully before
  changing*, not defects.
- **`cbo`** — Coupling Between Objects: the number of **distinct first-party classes** the class is
  coupled to (via bases, instantiations, `isinstance`/`issubclass`, and type annotations) — the
  class-level counterpart to package `ce`/`ca`. **Caveat:** Python has no static types, so `cbo` is
  an **approximation, biased low** — duck-typed coupling and string forward-refs aren't counted,
  while name resolution is scope-unaware, so a local shadowing a class name can occasionally
  overcount. Most reliable on well-typed codebases.
- **`lcom4`** — cohesion: how many disconnected method/attribute groups the class splits into
  (powers the SLP120 god-class rule). `> 1` means the class is doing unrelated jobs.

`--format json` adds the matching aggregates and the band histograms **`wmc_risk`**, **`noc_risk`**,
and **`cbo_risk`**, mirroring the function `cyclomatic_risk` tiers:

```jsonc
"wmc_risk": { "low": 451, "moderate": 23, "high": 15, "very_high": 5 },
// bands by WMC:  low ≤20   moderate 21–50   high 51–200   very_high >200
"noc_risk": { "low": 480, "moderate": 9, "high": 4, "very_high": 1 },
// bands by NOC:  low ≤1   moderate 2–5   high 6–20   very_high >20
"cbo_risk": { "low": 470, "moderate": 18, "high": 5, "very_high": 1 }
// bands by CBO:  low ≤4   moderate 5–9   high 10–20   very_high >20  (lower bound — see caveat)
```

`max_wmc` could be **one** justified hub (a wide-API dataframe class) or **fifty** god-classes —
very different stories that `wmc_risk` tells apart. WMC has no McCabe-equivalent canonical
threshold, so the bands are **descriptive, never a gate**.

[ck]: https://doi.org/10.1109/32.295895

### Module size

The third leg of the size triad: oversized **functions** (cyclomatic tiers) and **classes**
(`wmc_risk`) have reporting; oversized **modules** are the file-level counterpart.

```jsonc
"module_nloc": { "avg": 88.4, "max": 6531, "p95": 412 },
"module_size_risk": { "low": 980, "moderate": 47, "high": 19, "very_high": 11 }
// bands by NLOC:  low ≤250   moderate 251–500   high 501–1000   very_high >1000
```

`nloc` is **non-comment, non-blank** lines (string-literal/docstring content counts; blank and
comment-only lines don't). A **god-module** — a single multi-thousand-line file — otherwise
vanishes into `total_loc` and the average. NLOC has no canonical hard threshold (SonarQube's
~750–1000-line guidance is the starting point), so the bands are **descriptive, never a gate**.

### Top-level / undecomposed code

Complexity is measured **per function**, so a procedural script with no functions — a Streamlit
dashboard, a notebook export, a "write me a script" one-shot — scores near-pristine while being
untestable, unreusable, and unrefactorable. The **top-level-code ratio** catches it: the fraction
of a module's executable logic at module scope vs. inside functions.

```jsonc
"top_level_code": { "avg_ratio": 0.18, "max_ratio": 0.95, "undecomposed_modules": 2 }
```

`undecomposed_modules` counts non-trivial modules (≥ 15 logic statements) whose ratio ≥ 0.6 — the
script-dumps. The numerator excludes imports, the module docstring, the
`if __name__ == "__main__":` guard, class-body declarations, and pure constant assignments, so
config/`__main__`/library modules don't false-fire. Orthogonal to complexity (the code is linear)
and module-size (it's often only moderate). Descriptive, never a gate.

### Package & module architecture

`sloplint metrics` analyzes the project's **first-party import graph** — the metrics the literature
ties most directly to architectural decay, and the ones AI-generated codebases tend to do worst
(circular imports, god-modules, flat dumping-grounds, hidden coupling). Two feeds:

- **`--format packages`** — one JSONL row per package: `modules`, `loc`, efferent / afferent
  coupling (`ce` / `ca`) and Martin **`instability`**, **`abstractness`** + **`distance`** from the
  main sequence, whether it sits in a dependency cycle (`in_cycle`), and the first-party packages
  it `imports` / is `imported_by`.
- **`--format json`** — a per-project `packages` rollup alongside the complexity figures:

  ```jsonc
  "packages": {
    "modules": 412, "packages": 37, "module_edges": 689, "package_edges": 81,
    "cycles": {            // cyclic dependency tangles (Tarjan SCC) — 2–11× defect density
      "tangles": 3, "largest_tangle": 9, "modules_in_cycles": 21, "pct_modules_in_cycles": 0.051,
      "runtime_tangles": 2,      // dropping `if TYPE_CHECKING:`-only edges (benign at runtime)
      "load_bearing_tangles": 1, // also dropping deferred/function-local imports — hard cycles only
      "members": [["pkg.a", "pkg.b", "pkg.c"]]
    },
    "propagation_cost": 0.18, // how far a change ripples (DSM transitive-closure density)
    "modularity": {           // Newman–Girvan Q: declared packages vs. detected communities
      "q_declared": 0.41, "communities_declared": 37,
      "q_detected": 0.55, "communities_detected": 29,
      "gap": 0.14             // large positive gap ⇒ "packages in name only"
    },
    "concentration": {        // node distribution: god-package / flat dumping-ground (not edges)
      "max_package_share": 0.21,  // biggest package's share of all modules
      "module_count_gini": 0.38,  // inequality of modules-per-package (0 = even, →1 = one pile)
      "largest_package": { "package": "pkg.io", "modules": 86 }
    }
  }
  ```

  The `concentration` block is the one architecture metric over **nodes** rather than **edges**: a
  flat directory accreting hundreds of independent files (the classic god-package) has near-zero
  coupling, so propagation cost / cycles / modularity all read it as healthy — only the
  module-count distribution exposes it.

These are research-backed structural signals (Martin's package metrics; MacCormack's propagation
cost; Newman–Girvan modularity; Melton & Tempero on cyclic dependencies) — descriptive measures,
not pass/fail gates.

### Duplication density

`--format json` surfaces the SLP020 clone engine as a cohort aggregate — duplication is
**disallowed-by-default** and one of the clearest vibe-slop tells ("write a scraper per site" →
copy-paste). Per profile:

```jsonc
"duplication": {
  "clone_ratio": 0.41,          // fraction of the profile's functions in ≥1 clone pair
  "functions_in_clones": 38, "functions": 92,
  "clone_pairs": 40,            // confirmed SLP020 pairs internal to the profile
  "largest_clone_cluster": 9    // a helper duplicated across N functions
}
```

`clone_ratio` is near 0 for clean libraries and high for copy-paste codebases. It also explains a
subtlety the panel otherwise hides: **low propagation cost / zero cycles can be a *symptom* of
copy-paste** — self-contained duplicated modules don't import each other, so a high clone ratio
shows the low coupling is duplication, not modularity.

### Comment & docstring coverage

- **Comment density** — `#`-comment lines as a fraction of code.
- **Docstring coverage** — public defs/classes with a docstring ÷ all public defs/classes
  ("public" = not `_`-prefixed). Tracked separately from comment density because many codebases
  document almost entirely via docstrings (a `StringLiteral`, not a `Comment`). Low coverage flags
  an under-documented public API.
- **`docstring_code_ratio`** — function docstring lines ÷ function NCSS. A *high* ratio flags AI
  **over-documentation** — a verbose docstring stacked onto a one-line body.

### Exception-handling hygiene

Over-broad and silently-swallowed exception handling — `except Exception` / `except: pass` to make
the error disappear — is a reliable "make-it-work" smell and one of the sharpest cohort
discriminators. Ruff flags individual sites (`E722`/`BLE001`/`S110`); this is the *rate* those
per-site lints can't express (and which survives blanket `# noqa`/`# pylint: disable`).

```jsonc
"exception_handling": {
  "handlers": 412, "bare": 1, "broad": 18, "swallow": 9,
  "broad_rate": 0.044,   // broad / handlers   (Exception / BaseException, incl. in a tuple)
  "swallow_rate": 0.022  // swallow / handlers (body is exactly pass / continue / ...)
}
```

Clean libraries cluster low; low-discipline apps run 15–40× higher. The **`swallow_rate`** is the
strongest sub-signal — silently discarding errors is rarely justified — while broad except is
*sometimes* correct (top-level daemon loops, plugin boundaries), so it's read as a rate in context,
never a gate.

### Static test proxies (NOT coverage)

> [!IMPORTANT]
> **This is not test coverage.** Real coverage requires *executing* the tests, which a static
> linter cannot do. These are *proxies*: a low test:code ratio + low assertion density *suggest*
> under-testing, a high assertion-free rate *suggests* test theater — but they cannot tell a
> shallow test from a thorough one. Reported as descriptive cohort statistics, **never** a
> pass/fail gate. Their value is across a *cohort* (slop tends to ship far less test code with
> shallower assertions), not as a per-repo verdict.

```jsonc
"test_proxies": {
  "test_files": 12, "production_files": 48,
  "test_code_ratio": 0.353,    // test LoC / production LoC
  "test_functions": 96, "assertions": 311,
  "assertion_density": 3.24,   // assertions per test function (asserts + self.assertX + raises)
  "assertion_free_rate": 0.09, // fraction of test fns whose body asserts nothing ("test theater")
  "doctest_coverage": 0.589    // production functions whose docstring carries a `>>>` example
}
```

- **assertion-free-test rate** is a *test-substance* counterweight: `test:code` and
  `assertion_density` both reward volume, so a suite can read as "well-tested" while individual
  tests verify nothing. A rate near 1.0 next to a high test:code ratio flags a suite that *looks*
  tested but isn't.
- **doctest coverage** captures a testing style the path-based ratio is blind to: doctests live in
  the docstrings of *production* files (common in scientific/educational libraries), so a codebase
  tested primarily via doctests otherwise reads as untested.

### God-unit tail

Per-unit **averages** wash out the worst outliers: a repo can hold a dozen god-modules and a
cognitive-172 god-function yet show a clean `avg_cognitive`, because they're diluted across
thousands of units. The **god-unit tail** counts how many units land in the worst (`very_high`)
band of each distribution, so the outliers stay visible:

```jsonc
"god_units": {
  "very_high_cognitive_functions": 1, "very_high_cyclomatic_functions": 1,
  "very_high_wmc_classes": 0, "very_high_size_modules": 12, "total": 14
}
```

> [!NOTE]
> **Over-engineering is a documented limitation.** Detecting that a codebase is *disproportionate
> to its purpose* (a 100K-LOC 4D-tetris) needs the problem's intrinsic complexity — semantic, not
> statically computable, the same ceiling as AI-slop detection generally. sloplint does **not**
> ship an "over-engineering score." The god-unit tail surfaces the extreme outliers averages hide,
> but a clean tail is not proof a codebase is right-sized.

### Badges

`--badges badges/` writes an SVG + a shields.io [endpoint](https://shields.io/endpoint) JSON for
each metric (`cyclomatic-risk`, `max-cognitive`, `cognitive-risk`, `avg-function-loc`,
`max-nesting`, `comment-density`, `docstring-coverage`, …):

![cyclomatic-risk](https://img.shields.io/badge/cyclomatic--risk-moderate-yellow)
![max cognitive](https://img.shields.io/badge/max%20cognitive-14-yellow)
![avg function loc](https://img.shields.io/badge/avg%20function%20loc-22-brightgreen)

Choose which via `[badges]` in `sloplint.toml`: `include` picks the per-metric badges (omit the key
for all, `[]` for none), and `summary` folds a list of metrics into one combined `sloplint` badge
colored by the worst tier:

![sloplint](https://img.shields.io/badge/sloplint-CC%208%20·%20CoCo%2014%20·%20density%2018%25-yellow)

Commit the SVGs, or host the `*.json` and point a shields URL at it for a self-updating badge. The
[GitHub Action](https://github.com/galthran-wq/sloplint/wiki/GitHub-Action) writes them when you set
its `badges-dir` input.

See the [**case studies**](https://github.com/galthran-wq/sloplint/wiki/cases) for these metrics
run on 140 real projects — clean libraries, large frameworks, god-class codebases, and vibe-coded
repos — each validated against the source.

## Rules

Rules that flag slop patterns no mainstream linter covers today. **Stable** rules run by default;
**preview** rules are heuristic — enable them with `--preview`. Explain any rule with
`sloplint rule SLP030` (or `sloplint rule` to list all).

| Rule | Stability | What it flags |
| --- | --- | --- |
| `SLP010` | stable | Comments — **banned by default** (relax per-path in `sloplint.toml`) |
| `SLP020` | stable | Cross-file duplicate / near-duplicate functions — copy-paste *and* "same logic, slightly different" |
| `SLP030` | stable | Overly defensive `try`/`except` |
| `SLP050` | stable | Non-ASCII source (e.g. emoji) |
| `SLP080` | stable | Oversized files (default: > 400 lines, via `file_max_lines`) |
| `SLP082` | stable | Deep control-flow nesting inside a function (default: > 4 levels, via `nesting_max_depth`) |
| `SLP090` | stable | Flat-directory fanout — too many `.py` modules in one directory (default: > 15, via `dir_max_modules`) |
| `SLP001` | preview | Redundant "what" comments that just restate the code |
| `SLP002` | preview | Redundant docstrings that just restate the code |
| `SLP004` | preview | AI-narration comment tells — deferral/incompleteness (`for now`, `in production this would`; **error**), hedging (`should work`, `probably`), and structural noise (step narration, ASCII dividers) |
| `SLP040` | preview | Redundant type hints |
| `SLP060` | preview | Verbose, mechanical identifier naming |
| `SLP084` | preview | Deeply nested data-structure literals (a dict-of-lists-of-dicts blob past a depth — model it with a named type) |
| `SLP120` | preview | Low-cohesion "god classes" via LCOM4 (methods that split into unrelated groups) |
| `SLP130` | preview | Literal-dispatch & isinstance ladders — a long `if`/`elif` chain testing the same value against literals or types past a branch count (default: > 3, via `dispatch_max_branches`) |
| `SLP180` | preview | Undeclared third-party imports — a module imported but missing from `pyproject.toml`/`requirements*.txt` (broken on a clean install) |
| `SLP210` | preview | Phantom security guards — a call to / decorator of a known security-guard name (`validate_token`, `@requires_auth`, …) that is never defined or imported (fake security control — CWE-693) |
| `SLP220` | preview | Corrupted / truncated AI output — a leftover ```` ``` ```` fence, merge-conflict marker or `<file …>` tag in code, a file that fails to parse, or a prose-heavy paste |
| `SLP230` | preview | Mock / placeholder data in production code — `@example.com` emails, fake phone numbers, low-entropy/nil UUIDs, weak credentials (`changeme`), dummy returns; excludes test paths |
| `SLP240` | preview | Ghost scaffolding — a top-level class/function defined but **never referenced anywhere** in the project, or a `settings.ENABLE_X` flag read but defined nowhere |
| `SLP250` | preview | Cross-language pollution — wrong-language idioms in Python: camelCase methods (`.toString()`), foreign attributes (`.length`), `console.log`, foreign builtins (`array_push`). Narrow + allow-listed |

Several rules have a mechanical **[autofix](https://github.com/galthran-wq/sloplint/wiki/Autofix)**
(`check --fix`); single intentional cases are acknowledged in-line with Ruff-style
**[`# noqa`](https://github.com/galthran-wq/sloplint/wiki/Inline-suppression)**.

## Configuration

sloplint reads `sloplint.toml`, discovered from the working directory upward (or pass
`--config <path>`). Every key is optional — the defaults are shown below.

```toml
ignore = ["SLP040"]           # turn specific rules/prefixes off
select = []                   # force-enable rules/prefixes
preview = false               # enable preview rules (same as --preview)

[limits]                      # thresholds for the size/structure rules
file_max_lines = 400          # SLP080
nesting_max_depth = 4         # SLP082
data_nesting_max_depth = 3    # SLP084
max_identifier_words = 4      # SLP060
dir_max_modules = 15          # SLP090
lcom4_max_components = 1       # SLP120 — flag a class that splits into > 1 cohesion group
lcom4_min_methods = 3         # SLP120 — skip classes smaller than this
corrupted_prose_ratio = 0.5   # SLP220 — flag a file this fraction natural-language prose lines

[clone]                       # SLP020 near-duplicate detection
min_statements = 3            # ignore tiny functions
similarity = 0.85             # Jaccard similarity at/above which a pair is reported

[imports]                     # SLP180 undeclared third-party import
extra = []                    # extra distribution names to treat as declared (suppress FPs)

[security]                    # SLP210 phantom security guard
extra = []                    # extra security-guard names beyond the built-in catalog

[placeholders]                # SLP230 mock/placeholder data
extra = []                    # extra placeholder literal values beyond the built-in sets

[comments]                    # SLP004 hedging/narration comment tells
extra = []                    # extra hedging/deferral comment phrases beyond the built-in lexicon

[crosslang]                   # SLP250 cross-language pollution
allow = []                    # extra names to treat as legitimate Python (suppress false positives)

[badges]                      # which `metrics --badges` files to emit
# include = ["cyclomatic-risk"]   # per-metric badges; omit = all, [] = none
summary = []                  # metrics to fold into one combined `sloplint` badge

# Profiles: named, path-matched slices of the tree. Each carries its own rule deltas over the
# global config AND defines a metrics panel. Omit the section entirely to get the built-in
# `tests` / `generated` / `production` trio (generated code is content-detected). A profile's
# `limits` overrides only the per-file thresholds it sets (the cross-file SLP020/SLP090 thresholds
# and `[clone]` stay global). The name `all` is reserved (it's the every-profile scope).
[[profiles]]
name = "tests"                # matched first; a file belongs to every profile whose globs hit it
match = ["tests/**", "test_*.py", "*_test.py", "conftest.py"]
# exclude = ["tests/fixtures/**"]   # the "not" pattern — carve paths back out of `match`
ignore = ["SLP010"]           # rule deltas for this profile (accumulate across matches)
allow_comments = true         # permit comments here (otherwise banned)
limits = { file_max_lines = 1000 }   # threshold deltas (only the keys set here change)

[[profiles]]
name = "production"
default = true                # the catch-all: claims every file no other profile matched
```

For a file in more than one profile, rule `ignore`s accumulate and threshold overrides resolve in
declaration order (last writer wins). `metrics` reports a panel per profile; `check` lints each
file with its profile's effective config. Cross-file/directory rules (SLP020 clones, SLP090
fanout) use the global thresholds, since their unit of analysis spans profiles. Keep a `default`
profile unless you mean it — a file that matches no profile is linted with the global config but is
omitted from every metrics panel.

For single intentional findings, acknowledge them at the site with Ruff-style
**[`# noqa`](https://github.com/galthran-wq/sloplint/wiki/Inline-suppression)** — `# noqa: SLP020`
suppresses one code on the line, a bare `# noqa` suppresses every sloplint rule on it.

## More

- **[Autofix](https://github.com/galthran-wq/sloplint/wiki/Autofix)** — `check --fix` mechanics, safe vs. unsafe fixes.
- **[Agent-loop integration](https://github.com/galthran-wq/sloplint/wiki/Agent-loop-integration)** — `sloplint init`; run sloplint inside Claude Code / Cursor / Aider's edit loop.
- **[Inline suppression (`# noqa`)](https://github.com/galthran-wq/sloplint/wiki/Inline-suppression)** — per-site suppression, and running alongside Ruff.
- **[GitHub Action](https://github.com/galthran-wq/sloplint/wiki/GitHub-Action)** — SARIF annotations, PR summary, metric badges.
- **[Architecture / layout](https://github.com/galthran-wq/sloplint/wiki/Architecture)** — the crate map.
- **[Case studies](https://github.com/galthran-wq/sloplint/wiki/cases)** — metrics on 140 real projects.
