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
McCabe risk tiers, shields **badges**, and a per-PR summary — via the `metrics` command and the
GitHub Action.

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
sloplint metrics src --max-cyclomatic 10 # CI gate: exit 1 over McCabe's ceiling
sloplint metrics src --badges badges/    # emit SVG + shields-endpoint badges
sloplint parse file.py                   # dump AST + tokens (debug aid)
```

From a clone, run it through cargo instead (`cargo run -p sloplint -- check path/to/code`), or
build a wheel locally with [maturin](https://www.maturin.rs/) (`maturin build --release`).

Comments are banned by default; relax per-path (see [Configuration](#configuration)). Preview
rules need `--preview`.

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
cognitive complexity (with McCabe risk tiers), average function length, max nesting, and comment
density. These are **measured, not linted**, so they never duplicate Ruff. Gate them in CI by
exit code (each names the offending functions and exits 1):

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
| `sloplint_metrics` | quality metrics + badges |
| `sloplint_report` | output formatters (text/JSON/SARIF/markdown) |
| `sloplint_dev` | development utilities (cf. `ruff_dev`) |
