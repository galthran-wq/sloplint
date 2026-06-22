# Contributing to sloplint

sloplint is a slop linter, so we hold its own source to the bar it enforces. Two
conventions matter most here.

## Comment policy

Comments must earn their place. The bar: **would a competent reader who knows the domain
learn something they can't get from the code and names alone?** If not, delete it.

- **No WHAT-comments.** Don't restate what the code plainly does (`// increment counter`
  above `i += 1`, `// sort` above a `.sort()`). Name things well instead.
- **No provenance.** Source must not cite PR or issue numbers (`(#96)`, `issue #69`,
  "replaces the old X", "implemented in the … PR"). History lives in git and the tracker.
- **No scaffolding / future-tense notes.** "Filled in by a later PR", "empty until X lands",
  "the PRs that follow" go stale the moment the code lands.
- **Doc-comments are terse by default.** A `///` / `//!` states what the item is plus the one
  non-obvious *why*. Multi-paragraph algorithm narration belongs in a module-level `//!` once,
  or in design docs — not repeated across functions.
- **Keep** the comments that explain a non-obvious rationale, a subtle invariant, a workaround
  reason, a determinism guarantee, or a real caveat.

## Tests: fixtures + snapshots, not inline assertions

Like Ruff, rule tests are thin. A rule's test is a `test_rule!` entry that runs the rule over a
fixture under `crates/sloplint_linter/resources/test/fixtures/<category>/<CODE>.py` and snapshots
the rendered diagnostics; the fixture carries both violations and non-violations. Pass a custom
`Limits` as the fifth `test_rule!` argument for threshold-sensitive rules. Regenerate snapshots
with `cargo insta review` (or `INSTA_UPDATE=always cargo test`). Don't hand-write large
assertion blocks inside source modules — push the expected output into the reviewable `.snap`.

## Before opening a PR

```
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
