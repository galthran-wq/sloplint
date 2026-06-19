# sloplint

A fast, deterministic, **no-LLM** linter that counters AI slop in Python — a deliberately
nitpicking, opinionated layer that runs **right after [Ruff](https://docs.astral.sh/ruff/)**
in the same CI job. Ruff handles standard linting; sloplint adds the strict, slop-specific
judgments Ruff intentionally won't ship, and **never re-checks anything Ruff already covers**.

Written in Rust, reusing Ruff's own parser crates for a full-fidelity AST + token stream.

## What it targets

Patterns that no mainstream linter flags today:

- Redundant "what" comments & docstrings that just restate the code (default: comments are
  **banned**, configurable per-path).
- **Cross-file duplicated / near-duplicate functions** — copy-paste *and* "same logic,
  slightly different" (the flagship clone engine).
- Redundant type hints, overly defensive `try/except`, verbose mechanical naming.
- ASCII-only enforcement (no emoji), deep-nesting caps, oversized files, flat-directory fanout.
- Software-quality-metric **badges** + a per-PR summary, via a GitHub Action.

## Usage

```bash
cargo run -p sloplint -- check path/to/code            # lint (exit 1 on findings)
cargo run -p sloplint -- check src --format sarif       # SARIF / json / github / text
cargo run -p sloplint -- metrics src                    # software-quality metrics table
cargo run -p sloplint -- metrics src --badges badges/   # emit SVG + shields-endpoint badges
cargo run -p sloplint -- parse file.py                  # dump AST + tokens (debug aid)
```

Comments are banned by default; relax per-path in `sloplint.toml`. Heuristic rules
(`SLP001/002/040/060`) are preview — enable with `--preview`.

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
release tag like `v0.1.0`, or `latest`); if none is available it builds from source. Cut a
release to publish binaries:

```bash
git tag v0.1.0 && git push origin v0.1.0   # triggers .github/workflows/release.yml
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
