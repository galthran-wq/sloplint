# Labeled corpus

A permanent, rule-agnostic dataset used as an integration/regression suite and to tune
thresholds. It is labeled by **directory**, not by any individual rule, so it stays valid
as rules come and go.

- `slop/` — real-world-style AI-generated / sloppy Python. Each file exhibits one or more
  slop traits (redundant "what" comments, redundant type hints, over-defensive
  `try/except`, etc.). A correct linter should flag **at least one** finding per file.
- `clean/` — idiomatic, human-style Python that a correct linter must leave **alone**
  (zero findings). These are the false-positive guards — e.g. docstrings that introduce
  genuine domain concepts and must not be treated as redundant.
- `duplicates/<pair>/` — pairs (`a.py`, `b.py`) of near-duplicate functions for the clone
  engine: same logic, renamed identifiers / minor edits. Used by the clone slice.

The corpus runner (`crates/sloplint_linter/tests/corpus.rs`) computes precision/recall of
the **shipped** rule set (`lint::all_rules`) over `slop/` vs `clean/`:

- **Precision bar = 1.0 today**: shipped rules must never fire on `clean/`.
- **Recall bar** starts at 0.0 and is raised as real rules land, so we never claim
  coverage we don't yet have.

Add files freely; richer corpora make the gates stronger. Keep every file valid Python.
