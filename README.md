# sloplint

A fast, deterministic, **no-LLM** linter that counters AI slop in Python — a deliberately
nitpicking, opinionated layer that runs **right after [Ruff](https://docs.astral.sh/ruff/)**
in the same CI job. Ruff handles standard linting; sloplint adds the strict, slop-specific
judgments Ruff intentionally won't ship, and **never re-checks anything Ruff already covers**.

Written in Rust, reusing Ruff's own parser crates for a full-fidelity AST + token stream.

sloplint has two halves:

- **[Software-quality metrics](#software-quality-metrics)** — a deterministic, research-backed
  measurement layer over your code (complexity, cohesion, coupling, architecture, duplication,
  test substance). This is the foundation: the rules are increasingly built on top of it.
- **[Lint rules](#rules)** — strict `SLP*` rules that flag slop patterns no mainstream linter covers.

## Software-quality metrics

`sloplint metrics` reports a deterministic, reproducible measurement layer over your code — **no
LLM, no randomness**. These are **measured, not linted**, so they never duplicate Ruff, and they're
the foundation the lint rules increasingly build on. The guiding principle: **no single metric
orders code by quality** — you read the whole panel (function → class → module → package), in
context, mostly as risk-tier histograms (`low`/`moderate`/`high`/`very_high`) rather than a
pass/fail score.

```bash
sloplint metrics src                       # metrics table (production code)
sloplint metrics src --format json         # full rollup; --format functions/classes/packages for per-unit feeds
sloplint metrics src --max-cyclomatic 10   # CI gate: exit 1 over McCabe's ceiling (also --max-cognitive)
sloplint metrics src --badges badges/      # emit SVG + shields-endpoint badges (see Badges below)
```

**[Full metric reference → wiki](https://github.com/galthran-wq/sloplint/wiki/Metrics)** — what each
metric measures, how it's computed, how to read its bands, plus the per-profile panels:

| Metric | Level | What it measures |
| --- | --- | --- |
| [Cyclomatic & cognitive complexity](https://github.com/galthran-wq/sloplint/wiki/Metrics#cyclomatic--cognitive-complexity) | function | McCabe branch count + SonarSource readability-weighted complexity, with risk tiers |
| [Function length](https://github.com/galthran-wq/sloplint/wiki/Metrics#function-length) | function | physical LoC + longest *logic* function |
| [Parameter count / arity](https://github.com/galthran-wq/sloplint/wiki/Metrics#parameter-count) | function | Long-Parameter-List smell (caller-facing arity) |
| [Type-hint coverage](https://github.com/galthran-wq/sloplint/wiki/Metrics#type-hint-coverage) | function | under-annotation as a quality concern |
| [WMC / DIT / NOC / CBO / fan-in/out / RFC / NOSI / LCOM4 / LCOM\* / TCC / LCC](https://github.com/galthran-wq/sloplint/wiki/Metrics#class-metrics) | class | CK class metrics + cohesion |
| [Module size (NLOC)](https://github.com/galthran-wq/sloplint/wiki/Metrics#module-size) | module | god-module detection |
| [Top-level / undecomposed code](https://github.com/galthran-wq/sloplint/wiki/Metrics#top-level--undecomposed-code) | module | logic dumped at module scope vs. in functions |
| [Package & module architecture](https://github.com/galthran-wq/sloplint/wiki/Metrics#package--module-architecture) | project | coupling, cycles, propagation cost, modularity, concentration |
| [Duplication density](https://github.com/galthran-wq/sloplint/wiki/Metrics#duplication-density) | project | clone ratio (the SLP020 engine as a cohort aggregate) |
| [Comment & docstring density](https://github.com/galthran-wq/sloplint/wiki/Metrics#comment--docstring-coverage) | project | comment density, docstring coverage, docstring/code ratio |
| [Exception-handling hygiene](https://github.com/galthran-wq/sloplint/wiki/Metrics#exception-handling-hygiene) | project | broad / swallowed exception rates |
| [Static test proxies](https://github.com/galthran-wq/sloplint/wiki/Metrics#static-test-proxies-not-coverage) | project | test:code, assertion density, assertion-free rate, doctest coverage |
| [God-unit tail](https://github.com/galthran-wq/sloplint/wiki/Metrics#god-unit-tail) | cross-cutting | how many units land in the worst band of each distribution |

See the [**case studies**](https://github.com/galthran-wq/sloplint/wiki/cases) for these metrics run
on 140 real projects — clean libraries, large frameworks, god-class codebases, and vibe-coded repos.

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
pipx install sloplintpy              # pipx
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
rules need `--preview`. More on the [wiki](https://github.com/galthran-wq/sloplint/wiki):
[Autofix](https://github.com/galthran-wq/sloplint/wiki/Autofix),
[agent-loop integration](https://github.com/galthran-wq/sloplint/wiki/Agent-loop-integration) (run
sloplint inside your AI coding tool's edit loop), and the
[GitHub Action](https://github.com/galthran-wq/sloplint/wiki/GitHub-Action).

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

## Badges

`metrics --badges badges/` writes an SVG + a shields.io [endpoint](https://shields.io/endpoint) JSON
for each metric (`cyclomatic-risk`, `max-cognitive`, `cognitive-risk`, `avg-function-loc`,
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

## More

- **[Full metric reference](https://github.com/galthran-wq/sloplint/wiki/Metrics)** — every metric, how it's computed, how to read its bands.
- **[Autofix](https://github.com/galthran-wq/sloplint/wiki/Autofix)** — `check --fix` mechanics, safe vs. unsafe fixes.
- **[Agent-loop integration](https://github.com/galthran-wq/sloplint/wiki/Agent-loop-integration)** — `sloplint init`; run sloplint inside Claude Code / Cursor / Aider's edit loop.
- **[Inline suppression (`# noqa`)](https://github.com/galthran-wq/sloplint/wiki/Inline-suppression)** — per-site suppression, and running alongside Ruff.
- **[GitHub Action](https://github.com/galthran-wq/sloplint/wiki/GitHub-Action)** — SARIF annotations, PR summary, metric badges.
- **[Architecture / layout](https://github.com/galthran-wq/sloplint/wiki/Architecture)** — the crate map.
- **[Case studies](https://github.com/galthran-wq/sloplint/wiki/cases)** — metrics on 140 real projects.
