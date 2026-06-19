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

## Status

Early scaffolding. See `crates/` for the workspace layout (mirrors Ruff's `ruff_linter`).

```bash
cargo run -p sloplint -- parse path/to/file.py   # dump AST + tokens (debug aid)
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
