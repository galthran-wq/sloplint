# sloplint

A fast, deterministic, **no-LLM** linter that counters AI slop in Python — a deliberately
nitpicking, opinionated layer that runs **right after [Ruff](https://docs.astral.sh/ruff/)**
in the same CI job. Ruff handles standard linting; sloplint adds the strict, slop-specific
judgments Ruff intentionally won't ship, and **never re-checks anything Ruff already covers**.

Written in Rust, reusing Ruff's own parser crates for a full-fidelity AST + token stream.

## Features

Rules that flag slop patterns no mainstream linter covers today. **Stable** rules run by
default; **preview** rules are heuristic — enable them with `--preview`.

| Rule | Stability | What it flags |
| --- | --- | --- |
| `SLP010` | stable | Comments — **banned by default** (relax per-path in `sloplint.toml`) |
| `SLP020` | stable | Cross-file duplicate / near-duplicate functions — copy-paste *and* "same logic, slightly different" |
| `SLP030` | stable | Overly defensive `try`/`except` |
| `SLP050` | stable | Non-ASCII source (e.g. emoji) |
| `SLP080` | stable | Oversized files (default: > 400 lines, configurable via `file_max_lines`) |
| `SLP082` | stable | Deep control-flow nesting inside a function (default: > 4 levels, via `nesting_max_depth`) |
| `SLP090` | stable | Flat-directory fanout — too many `.py` modules in one directory (default: > 15, via `dir_max_modules`) |
| `SLP001` | preview | Redundant "what" comments that just restate the code |
| `SLP002` | preview | Redundant docstrings that just restate the code |
| `SLP040` | preview | Redundant type hints |
| `SLP060` | preview | Verbose, mechanical identifier naming |
| `SLP084` | preview | Deeply nested data-structure literals (a dict-of-lists-of-dicts blob past a depth — model it with a named type) |
| `SLP120` | preview | Low-cohesion "god classes" via LCOM4 (methods that split into unrelated groups) |
| `SLP180` | preview | Undeclared third-party imports — a module imported but missing from the project's `pyproject.toml`/`requirements*.txt` (broken on a clean install) |

Plus software-quality **metrics** (cyclomatic + cognitive complexity, LCOM4 cohesion) with
McCabe risk tiers, shields **badges**, and a per-PR summary — and **package/module architecture
metrics** over the import graph (dependency cycles, coupling/instability, propagation cost,
modularity) — via the `metrics` command and the GitHub Action.

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
# With uv.
uv tool install sloplintpy@latest   # Install the `sloplint` command globally.
uv add --dev sloplintpy             # Or add it to your project.

# With pip.
pip install sloplintpy

# With pipx.
pipx install sloplintpy
```

## Usage

Once installed, `sloplint` is a native binary on your `PATH`:

```bash
sloplint check path/to/code              # lint (exit 1 on findings)
sloplint check src --fix                 # auto-fix findings that have a safe fix (e.g. delete banned comments)
sloplint check src --format sarif        # SARIF / json / github / text
sloplint metrics src                     # software-quality metrics table (production code)
sloplint metrics src --scope all         # a panel for every profile (default: production only)
sloplint metrics src --format github     # PR-summary markdown (CC risk tiers)
sloplint metrics src --format packages   # per-package feed: coupling, cycles, abstractness (JSONL)
sloplint metrics src --max-cyclomatic 10 # CI gate: exit 1 over McCabe's ceiling
sloplint metrics src --badges badges/    # emit SVG + shields-endpoint badges
sloplint init                            # wire sloplint into your AI coding tool (see below)
sloplint parse file.py                   # dump AST + tokens (debug aid)
```

From a clone, run it through cargo instead (`cargo run -p sloplint -- check path/to/code`), or
build a wheel locally with [maturin](https://www.maturin.rs/) (`maturin build --release`).

Comments are banned by default; relax per-path (see [Configuration](#configuration)). Preview
rules need `--preview`.

### Autofix

Like Ruff, `sloplint check --fix` automatically resolves findings that have a mechanical fix,
rewriting files in place. Not every rule is fixable — near-duplicate functions (SLP020) or a
low-cohesion god class (SLP120) need human judgment — but several are. The flagship example is
**SLP010**: where comments are banned, `--fix` simply deletes them (own-line comments take their
whole line; inline `code  # …` comments lose just the trailing comment).

```bash
sloplint check src --fix            # apply safe fixes, rewrite files, report what remains
sloplint check src --fix --unsafe-fixes   # also apply fixes that might change behavior/intent
```

Fixes run **after** per-path rule selection and inline `# noqa` suppression, so a path that opts
back into comments (a profile that ignores `SLP010`) and any `# noqa`-suppressed finding are never
touched. Only `Safe` fixes apply by default; `--unsafe-fixes` opts into the rest. Findings without
a fix are still reported.

## Agent-loop integration

sloplint is fast, deterministic and reproducible — so instead of only catching slop in CI,
after the code has landed, you can run it *inside* your AI coding tool's edit loop. The tool
fires a hook after every file edit, sloplint checks the just-edited file, and any findings go
straight back to the agent so it self-corrects in the same turn — a guardrail, not just a gate.

```bash
sloplint init                 # detect the tools in this repo and wire them up
sloplint init --tool claude   # or target one: claude | cursor | aider | all
sloplint init --dry-run       # preview the config changes without writing
```

`init` writes (merging into any existing config, never clobbering it):

| Tool | Config | Mechanism |
| --- | --- | --- |
| Claude Code | `.claude/settings.json` | `PostToolUse` hook → `sloplint check --hook --format agent` |
| Cursor | `.cursor/hooks.json` | `afterFileEdit` hook → `sloplint check --hook --format agent` |
| Aider | `.aider.conf.yml` | `lint-cmd: "python: sloplint check --format agent"` |

The Claude Code and Cursor hooks pass the edited path as JSON on stdin; `check --hook` reads it
(no `jq` needed), lints just that file with the fast per-file rules, prints any findings to
stderr in the terse `path:line:col: CODE message` agent format, and exits 2 so the agent sees
them. A clean edit exits 0 silently. Whole-project rules (clone detection, dir fanout,
undeclared imports) still belong in the CI run — they need the whole tree, not one edit.

You can use the agent format anywhere, not just in hooks: `sloplint check src --format agent`.

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

[clone]                       # SLP020 near-duplicate detection
min_statements = 3            # ignore tiny functions
similarity = 0.85             # Jaccard similarity at/above which a pair is reported

[imports]                     # SLP180 undeclared third-party import
extra = []                    # extra distribution names to treat as declared (suppress FPs)

[badges]                      # which `metrics --badges` files to emit (see Metrics & badges)
# include = ["cyclomatic-risk"]   # per-metric badges; omit = all, [] = none
summary = []                  # metrics to fold into one combined `sloplint` badge

# Profiles (#96): named, path-matched slices of the tree. Each carries its own rule deltas over
# the global config AND defines a metrics panel. Omit the section entirely to get the built-in
# `tests` + `production` pair. The `[limits]` above are the global defaults a profile inherits; a
# profile's `limits` overrides only the per-file thresholds it sets (the cross-file SLP020/SLP090
# thresholds and `[clone]` stay global). The name `all` is reserved (it's the every-profile scope).
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

Profiles replace the old `[[overrides]]`. For a file in more than one profile, rule `ignore`s
accumulate and threshold overrides resolve in declaration order (last writer wins). `metrics`
reports a panel per profile (see below); `check` lints each file with its profile's effective
config. Cross-file/directory rules (SLP020 clones, SLP090 fanout) use the global thresholds, since
their unit of analysis spans profiles. Keep a `default` profile unless you mean it — a file that
matches no profile is linted with the global config but is omitted from every metrics panel.

### Inline suppression (`# noqa`)

A profile's `ignore` mutes a rule across a whole path slice; for a single intentional case,
acknowledge it **at the site** with Ruff's familiar `# noqa` — sloplint reads it exactly as Ruff
does:

```python
def request(self, ...):   # noqa: SLP020  (sync/async mirror of AsyncClient.request)
    ...
```

- `# noqa: SLP020` suppresses that code on the line; list several with `# noqa: SLP020, SLP082`.
- A bare `# noqa` suppresses every sloplint rule on that line.
- The trailing free-text reason is just a normal comment — encouraged ("I understand, and here's
  why"), never itself reported.

A `# noqa` is scoped to its line — the finding's reported line (the `line:col` shown in output), so
for a whole-function finding it goes on the `def` line. This is line-level only, like Ruff;
broad/file/directory suppression stays in config (global `ignore` and per-profile `ignore`).
Duplication is the motivating case: SLP020 is on by default ("no un-acknowledged duplication"), and
a clone is reported at *each* end — so silencing a whole pair takes a `# noqa` at each end, each
documenting why that twin is intentional.

**Running alongside Ruff:** Ruff reads the same `# noqa` comments, and since `SLP*` aren't Ruff
codes, its RUF100 (unused-noqa) would otherwise flag `# noqa: SLP020` as unnecessary. Tell Ruff to
preserve them:

```toml
# ruff.toml / pyproject.toml [tool.ruff.lint]
external = ["SLP"]
```

Symmetrically, sloplint only ever acts on its own `SLP*` codes and never reports on Ruff directives
like `# noqa: E501`.

## Metrics & badges

Beyond the lint rules, `sloplint metrics` reports software-quality metrics — cyclomatic and
cognitive complexity (each with mean / p95 / max and risk-tier histograms; cognitive's bands are
anchored on SonarSource's 15/function guidance and are the better *readability* signal), average
function length, max nesting, comment density, type-hint coverage, and **docstring coverage**.
These are **measured, not linted**, so
they never duplicate Ruff. Gate them in CI by exit code (each names the offending functions and
exits 1):

```bash
sloplint metrics src --max-cyclomatic 10   # fail if any function's cyclomatic complexity > 10
sloplint metrics src --max-cognitive 15    # ditto for SonarSource cognitive complexity
```

### Per-profile metric panels

Different parts of a codebase have different healthy norms — test code is legitimately longer,
more repetitive, and less type-annotated; generated code and examples differ again — so collapsing
them into one set of aggregates misleads in either direction (a heavy test-support class can
dominate the "worst class", a thin test suite can drag down the averages). `sloplint metrics`
reports a panel **per profile** (see [Configuration](#configuration)), in **one run** (#96). With
zero config that's the built-in `tests` vs `production` split.

- **`--scope <profile>`** (default: the `default` profile, `production` out of the box) selects
  which profile the text view and the per-unit feeds (`--format functions`/`classes`/`packages`)
  report; `--scope all` prints a panel for every profile. The **packages graph is built from the
  scoped profile's modules only**, so a file in one profile importing another can't manufacture
  cycles or coupling in the first profile's architecture metrics.
- **`--format json`** ignores `--scope` and is always comprehensive: a panel for every profile
  under **`profiles`** (keyed by name), plus the project-wide **`test_proxies`** split (always
  over all files, bound to the `tests` profile). One invocation yields every view — no more
  pointing at the package dir, `rsync --exclude tests`, and a second whole-repo pass just to
  recover the test figures.

**Docstring coverage** is tracked separately from comment density, because the two measure
different things: comment density counts `#`-comments, while many codebases document almost
entirely via docstrings (a `StringLiteral`, not a `Comment`). The `--format json` rollup reports
`docstring_coverage` (public defs/classes with a docstring ÷ all public defs/classes — "public" =
not `_`-prefixed) and `docstring_code_ratio` (function docstring lines ÷ function NCSS). Low
coverage flags an under-documented public API; a high ratio flags AI **over-documentation** — a
verbose docstring stacked onto a one-line body. The `--format functions` / `--format classes` feeds carry
`has_docstring` + `docstring_lines` per unit.

`--badges badges/` writes an SVG + a shields.io [endpoint](https://shields.io/endpoint) JSON for
each metric (`cyclomatic-risk`, `max-cognitive`, `cognitive-risk`, `avg-function-loc`,
`max-nesting`, `comment-density`, `docstring-coverage`, …) — for example:

![cyclomatic-risk](https://img.shields.io/badge/cyclomatic--risk-moderate-yellow)
![max cognitive](https://img.shields.io/badge/max%20cognitive-14-yellow)
![avg function loc](https://img.shields.io/badge/avg%20function%20loc-22-brightgreen)

Choose which badges via `[badges]` in `sloplint.toml`: `include` picks the per-metric badges
(omit the key for all, `[]` for none), and `summary` folds a list of metrics into one combined
`sloplint` badge colored by the worst tier — e.g. `include = []` + `summary = [...]` emits *only*:

![sloplint](https://img.shields.io/badge/sloplint-CC%208%20·%20CoCo%2014%20·%20density%2018%25-yellow)

Commit the SVGs, or host the `*.json` and point a shields URL at it for a badge that updates
itself. The GitHub Action writes them when you set its `badges-dir` input.

### Type-hint coverage

`--format functions` rows carry per-function annotation counts (`typed_params`,
`annotatable_params`, `has_return_annotation`), and `--format json` rolls them up into
`param_annotation_coverage` (annotated ÷ annotatable params) and `fully_annotated_function_rate`
(functions with every param **and** the return type annotated). Annotatable params exclude the
`self`/`cls` receiver and `*args`/`**kwargs`. This measures **under**-annotation as a quality
concern (missing types are harder to read and refactor, and weaken tooling) — the bad direction is
*low* coverage only. Fully-typed code is neutral-to-good and is never itself a slop signal.

### Parameter count

The **Long Parameter List** smell (Fowler) — too many arguments, a sign of a missing abstraction
or data clump (#108). `--format json` emits, per profile:

```jsonc
"params": { "avg": 3.1, "max": 26, "p95": 8 },
"param_count_risk": { "low": 7423, "moderate": 492, "high": 285, "very_high": 142 }
// bands by arity:  low ≤4   moderate 5–6   high 7–10   very_high >10
```

Counts **caller-facing arity**: the `self`/`cls` receiver is excluded, and `*args`/`**kwargs` count
once each (a `**kwargs` sink is the *opposite* of a long parameter list, so matplotlib-style APIs
don't false-positive). The per-function `--format functions` feed carries `arity` alongside the raw
`params`. It's distinct from complexity — a CC-3 wrapper threading 25 options is invisible to every
other metric. As with the other tiers, arity has no canonical hard threshold (Fowler/Martin suggest
≤3–4), so the bands are **descriptive, never a gate** — high `high`/`very_high` counts flag
functions to *read* (numeric solvers genuinely take many knobs), not defects.

### Class metrics

`--format classes` emits one JSONL row per class — the class-level discovery feed: `loc`,
`methods`, `attributes`, **`lcom4`** cohesion (SLP120), `is_abstract`, and the CK class metrics
([Chidamber & Kemerer 1994][ck]):

- **`wmc`** — Weighted Methods per Class: the sum of the cyclomatic complexity of the class's
  direct methods. A class-*weight* measure that separates 40 trivial accessors from 40 branchy
  ones, where a raw method count can't.
- **`dit`** — Depth of Inheritance Tree: the longest path up to a root through **first-party**
  bases. Bases that resolve to `object`, the stdlib, or a third party are invisible and end the
  chain, so `dit` is a deliberate, conservative under-count of the true Python MRO depth.
- **`noc`** — Number of Children: how many **direct** first-party subclasses the class has — the
  inheritance *breadth* that pairs with `dit` depth (#113). It's the in-degree of the same class
  graph. A high-NOC base is a change-amplifier (fragile-base-class risk): every change ripples to
  its children. Often that's good design (a well-used abstraction — yt-dlp's `InfoExtractor` is
  subclassed by ~965 extractors), so it flags bases to *review carefully before changing*, not
  defects.
- **`cbo`** — Coupling Between Objects: the number of **distinct first-party classes** the class is
  coupled to — the class-level coupling counterpart to package `ce`/`ca` (#116). Counts coupling via
  base classes, instantiations (`ClassName(...)`), `isinstance`/`issubclass` checks, and type
  annotations, resolved against the project's first-party class set. A small class wired to dozens of
  collaborators is a fragile hub WMC/DIT/NOC don't see. **Caveat:** Python has no static types, so
  `cbo` is an **approximation, biased low** — duck-typed coupling (`self.axes.foo()` with no
  annotation) and string forward-refs are *not* counted (an undercount), while name resolution is
  scope-unaware, so a local/parameter shadowing a class name can occasionally overcount. It's most
  reliable on well-typed codebases. Flags hubs to *review before changing*, never defects.

`--format json` adds the matching aggregates next to the complexity figures: `classes`,
`max_wmc`, `avg_wmc`, `p95_wmc`, `max_dit`, `avg_dit`, `max_noc`, `avg_noc`, `p95_noc`,
`max_cbo`, `avg_cbo`, `p95_cbo`, and the band histograms **`wmc_risk`**, **`noc_risk`**, and
**`cbo_risk`**, mirroring the function `cyclomatic_risk` tiers:

```jsonc
"wmc_risk": { "low": 451, "moderate": 23, "high": 15, "very_high": 5 },
// bands by WMC:  low ≤20   moderate 21–50   high 51–200   very_high >200
"noc_risk": { "low": 480, "moderate": 9, "high": 4, "very_high": 1 },
// bands by NOC:  low ≤1   moderate 2–5   high 6–20   very_high >20
"cbo_risk": { "low": 470, "moderate": 18, "high": 5, "very_high": 1 }
// bands by CBO:  low ≤4   moderate 5–9   high 10–20   very_high >20  (lower bound — see caveat)
```

This is the point of the histogram (#104): `avg`/`max` collapse the distribution, so the same
`max_wmc` could be **one** justified hub (e.g. a wide-API dataframe class) or **fifty** god-classes
— very different maintainability stories that `wmc_risk` tells apart. WMC has no McCabe-equivalent
canonical threshold, so the bands are **descriptive, calibrated against the cohort, never a gate** —
high `high`/`very_high` counts flag *candidates to read*, not defects. Like the rest, these are
descriptive distributions for tracking a repo over time.

[ck]: https://doi.org/10.1109/32.295895

### Module size

The third leg of the size triad (#107): oversized **functions** (cyclomatic tiers) and **classes**
(`wmc_risk`) have reporting; oversized **modules** are the file-level counterpart. `--format json`
emits, per profile:

```jsonc
"module_nloc": { "avg": 88.4, "max": 6531, "p95": 412 },
"module_size_risk": { "low": 980, "moderate": 47, "high": 19, "very_high": 11 }
// bands by NLOC:  low ≤250   moderate 251–500   high 501–1000   very_high >1000
```

`nloc` is **non-comment, non-blank** lines (string-literal/docstring content counts; blank and
comment-only lines don't). A **god-module** — a single multi-thousand-line file — is otherwise
invisible: its lines vanish into `total_loc` and the average. The histogram tells "47 files over
1000 NLOC" apart from "one big generated file", which `max`/`avg` collapse. As with the function
and class tiers, NLOC has no canonical hard threshold (SonarQube's ~750–1000-line guidance is the
starting point), so the bands are **descriptive, never a gate** — high `high`/`very_high` counts
flag files to *read*, not defects.

### Package & module architecture metrics

`sloplint metrics` also analyzes the project's **first-party import graph** — the metrics the
literature ties most directly to architectural decay, and the ones AI-generated codebases tend to
do worst (circular imports, god-modules, flat dumping-grounds, hidden coupling). All deterministic
and reproducible — no LLM, no randomness. Two feeds:

- **`--format packages`** — one JSONL row per package (directory): `modules`, `loc`, efferent /
  afferent coupling (`ce` / `ca`) and Martin **`instability`**, **`abstractness`** + **`distance`**
  from the main sequence, whether it sits in a dependency cycle (`in_cycle`), and the first-party
  packages it `imports` / is `imported_by`. The per-package discovery feed, mirroring
  `--format functions` / `--format classes`.
- **`--format json`** — a per-project `packages` rollup alongside the complexity figures:

  ```jsonc
  "packages": {
    "modules": 412, "packages": 37, "module_edges": 689, "package_edges": 81,
    "cycles": {            // cyclic dependency tangles (Tarjan SCC) — 2–11× defect density
      "tangles": 3, "largest_tangle": 9, "modules_in_cycles": 21,
      "pct_modules_in_cycles": 0.051,
      "runtime_tangles": 2,   // dropping `if TYPE_CHECKING:`-only edges (benign at runtime)
      "load_bearing_tangles": 1, // also dropping function-local/deferred imports — hard load-time
                                 // cycles only (0 ⇒ every cycle was deferred on purpose; not a
                                 // strict subset of `tangles` — dropping edges can split an SCC)
      "members": [["pkg.a", "pkg.b", "pkg.c"]]
    },
    "propagation_cost": 0.18, // how far a change ripples (DSM transitive-closure density)
    "modularity": {           // Newman–Girvan Q: declared packages vs. detected communities
      "q_declared": 0.41, "communities_declared": 37,
      "q_detected": 0.55, "communities_detected": 29,
      "gap": 0.14             // large positive gap ⇒ "packages in name only"
    },
    "concentration": {        // node distribution: god-package / flat dumping-ground (not edges)
      "total_modules": 412, "packages": 37,
      "max_package_share": 0.21,  // biggest package's share of all modules
      "module_count_gini": 0.38,  // inequality of modules-per-package (0 = even, →1 = one pile)
      "largest_package": { "package": "pkg.io", "modules": 86 }
    }
  }
  ```

  The `concentration` block is the one architecture metric over **nodes** rather than **edges**: a
  flat directory accreting hundreds of independent files (the classic god-package) has near-zero
  coupling, so propagation cost / cycles / modularity all read it as healthy — only the module-count
  distribution exposes it. The `text` view prints `max package share` / `module-count gini` under
  each profile's panel and names the offending package.

These are research-backed structural signals (Martin's package metrics; MacCormack's propagation
cost; Newman–Girvan modularity; Melton & Tempero on cyclic dependencies) — descriptive measures
for tracking a repo over time or comparing across codebases, not pass/fail gates. (Published
clean-vs-slop reference distributions are the job of the benchmark harness, [#55][bench].)

[bench]: https://github.com/galthran-wq/sloplint/issues/55

### Duplication density

`--format json` surfaces the SLP020 clone engine as a cohort aggregate (#123) — duplication is
**disallowed-by-default** and one of the clearest vibe-slop tells ("write a scraper per site" →
copy-paste), but it was previously only a per-finding lint, invisible in the metrics panel. Per
profile:

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
shows the low coupling is duplication, not modularity. Pairs are scoped per profile (production
duplication is counted over production functions). It reuses the existing detector — descriptive
cohort signal, never a per-repo gate.

### Exception-handling hygiene

Over-broad and silently-swallowed exception handling — `except Exception` / `except: pass` to make
the error disappear — is a reliable "make-it-work" smell, and one of the sharpest cohort
discriminators (#117). Ruff flags individual sites (`E722`/`BLE001`/`S110`); this is the *rate*
those per-site lints can't express (and which survives blanket `# noqa`/`# pylint: disable`).
`--format json` emits, per profile:

```jsonc
"exception_handling": {
  "handlers": 412, "bare": 1, "broad": 18, "swallow": 9,
  "broad_rate": 0.044,   // broad / handlers   (Exception / BaseException, incl. in a tuple)
  "swallow_rate": 0.022  // swallow / handlers (body is exactly pass / continue / ...)
}
```

Measured by AST over every `except` handler (module level or nested). Clean libraries cluster low;
low-discipline apps run 15–40× higher. The **`swallow_rate`** is the strongest sub-signal —
silently discarding errors is rarely justified — while broad except is *sometimes* correct
(top-level daemon loops, plugin boundaries), so like the rest it's read as a rate in context, never
a gate. Per-1k-LOC rates are derivable from the counts and the panel's `total_loc`.

### Static test proxies (NOT coverage)

`--format json` also reports a `test_proxies` block — *static* signals of how (un)tested a
codebase is, computed without running anything:

```jsonc
"test_proxies": {
  "_note": "Static proxies, NOT coverage. Descriptive cohort statistics only — never a pass/fail gate. ...",
  "test_files": 12, "production_files": 48,
  "test_loc": 1840, "production_loc": 5210,
  "test_code_ratio": 0.353,    // test LoC / production LoC
  "test_functions": 96, "assertions": 311,
  "assertion_density": 3.24,   // assertions per test function (asserts + self.assertX +
                               // pytest.raises + self.fail), null when there are no test fns
  "assertion_free_tests": 9,
  "assertion_free_rate": 0.09  // fraction of test fns whose body asserts nothing ("test
                               // theater"); null when there are no test fns
}
```

The **assertion-free-test rate** is a *test-substance* counterweight: `test:code` and
`assertion_density` both reward volume, so a suite can read as "well-tested" while individual tests
verify nothing. It is the fraction of test functions whose body contains **no assertion at all** —
the shape "test theater" actually takes (print-spam "tests" that exercise code but check nothing,
assertion-free stubs). A rate near 1.0 next to a high test:code ratio flags a suite that *looks*
tested but isn't. (The complementary *empty test scaffolding* form — `tests/` dirs holding only
empty `__init__.py` — is already caught by `test_code_ratio` = 0.0.)

> Earlier releases keyed this on *cognitive complexity* (a "trivial-test rate"), which was
> backwards — a disciplined arrange-act-assert test is deliberately branch-free, so good tests
> scored as trivial while assertion-free loops scored as substantive. Fixed in
> [#127](https://github.com/galthran-wq/sloplint/issues/127): the quality signal is whether a test
> **asserts**, not whether it **branches**.

Test files are identified by path (`test_*.py`, `*_test.py`, a `tests/` segment, `conftest.py`);
the figures also appear in the text table and the `--format github` PR summary.

> [!IMPORTANT]
> **This is not test coverage.** Real coverage requires *executing* the tests, which a static
> linter cannot do. These are *proxies*: a low test:code ratio + low assertion density *suggest*
> under-testing, and a high assertion-free rate *suggests* test theater — but they cannot tell a
> shallow test from a thorough one — a test can carry many asserts and verify nothing, or assert
> through a helper and look assertion-free. So they are reported as descriptive
> cohort statistics and are **never** a pass/fail gate. Their value is across a *cohort* (slop
> tends to ship far less test code with shallower assertions), not as a per-repo verdict. They
> are the cohort-level counterpart to the per-file `SLP070` (assertion-free tests) and `SLP160`
> (test mirroring) *rules*.

## GitHub Action

Run sloplint on every PR — it uploads SARIF (inline annotations), posts a findings
summary comment, and can emit metric badges:

```yaml
permissions:
  contents: read
  security-events: write
  pull-requests: write
jobs:
  sloplint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: galthran-wq/sloplint@main
        with:
          paths: src
          badges-dir: .sloplint-badges   # optional
```

Intended to run **after** Ruff in the same job. See [`action.yml`](action.yml) for all inputs.

Required permissions: `security-events: write` (SARIF upload) and `pull-requests: write`
(PR comment) — both shown above. Without them the action degrades gracefully (it warns
rather than failing).

By default the action downloads a **prebuilt binary** for the runner (set `version:` to a
release tag like `v0.2.0`, or `latest`); if none is available it builds from source. Cut a
release to publish binaries:

```bash
git tag v0.2.0 && git push origin v0.2.0   # triggers .github/workflows/release.yml
```

## Layout

| Crate | Role |
| --- | --- |
| `sloplint` | CLI binary |
| `sloplint_linter` | all rules + core run logic (cf. `ruff_linter`) |
| `sloplint_python` | parser seam over the pinned `ruff_*` crates |
| `sloplint_diagnostics` | rule-independent diagnostic model |
| `sloplint_clone` | near-duplicate function detection |
| `sloplint_metrics` | quality metrics, import-graph architecture metrics, badges |
| `sloplint_report` | output formatters (text/JSON/SARIF/markdown) |
| `sloplint_dev` | development utilities (cf. `ruff_dev`) |
