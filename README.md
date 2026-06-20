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
sloplint check src --format sarif        # SARIF / json / github / text
sloplint metrics src                     # software-quality metrics table
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

[[overrides]]                 # relax rules for matching paths (gitignore-style globs)
path = "tests/**"
ignore = ["SLP010"]
allow_comments = true         # permit comments here (otherwise banned)
```

## Metrics & badges

Beyond the lint rules, `sloplint metrics` reports software-quality metrics — cyclomatic and
cognitive complexity (with McCabe risk tiers), average function length, max nesting, comment
density, and type-hint coverage. These are **measured, not linted**, so they never duplicate Ruff.
Gate them in CI by exit code (each names the offending functions and exits 1):

```bash
sloplint metrics src --max-cyclomatic 10   # fail if any function's cyclomatic complexity > 10
sloplint metrics src --max-cognitive 15    # ditto for SonarSource cognitive complexity
```

`--badges badges/` writes an SVG + a shields.io [endpoint](https://shields.io/endpoint) JSON for
each metric (`cyclomatic-risk`, `max-cognitive`, `avg-function-loc`, `max-nesting`,
`comment-density`, …) — for example:

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
      "members": [["pkg.a", "pkg.b", "pkg.c"]]
    },
    "propagation_cost": 0.18, // how far a change ripples (DSM transitive-closure density)
    "modularity": {           // Newman–Girvan Q: declared packages vs. detected communities
      "q_declared": 0.41, "communities_declared": 37,
      "q_detected": 0.55, "communities_detected": 29,
      "gap": 0.14             // large positive gap ⇒ "packages in name only"
    }
  }
  ```

These are research-backed structural signals (Martin's package metrics; MacCormack's propagation
cost; Newman–Girvan modularity; Melton & Tempero on cyclic dependencies) — descriptive measures
for tracking a repo over time or comparing across codebases, not pass/fail gates. (Published
clean-vs-slop reference distributions are the job of the benchmark harness, [#55][bench].)

[bench]: https://github.com/galthran-wq/sloplint/issues/55

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
