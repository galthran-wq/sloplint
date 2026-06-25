# CLAUDE.md

Guidance for AI agents (Claude Code and friends) working in this repo. Human-facing contribution
rules live in [`CONTRIBUTING.md`](CONTRIBUTING.md) — read both; this file adds the project context,
architecture map, and the workflow an agent should follow.

## What this is

**sloplint** is a fast, deterministic, **no-LLM** linter that counters AI slop in Python. It runs
*after* [Ruff](https://docs.astral.sh/ruff/) in the same CI job and **never re-checks what Ruff
already covers**. It has two halves:

- **Software-quality metrics** (`sloplint metrics`) — a deterministic measurement layer
  (complexity, cohesion, coupling, architecture, duplication, test substance). This is the
  foundation; the rules increasingly build on it.
- **Lint rules** (`sloplint check`) — strict `SLP*` rules for slop patterns no mainstream linter flags.

It's a Rust workspace that **reuses Ruff's own parser crates** (pinned to tag `0.15.18`) for a
full-fidelity AST + token stream.

## Core principles

1. **Mirror Ruff.** When designing anything at the rule/engine/crate-structure layer, copy how Ruff
   does it rather than inventing. Ruff `0.15.18` is vendored locally at
   `~/.cargo/git/checkouts/ruff-*/<rev>/crates` — read it before making a design decision. Don't
   ask the user to choose a design that Ruff has already settled; match Ruff and say so.
2. **Slop is badness, not provenance.** Rules target *bad code*, never "this looks AI-written."
   Reject any heuristic that flags authorship rather than a concrete defect. (E.g. generated code
   is segregated from metrics because its numbers are *noise*, not because it's "slop".)
3. **Determinism.** No LLM, no randomness, reproducible output. Everything is static analysis over
   the AST/token stream.
4. **Hold our own source to the bar we enforce** — see `CONTRIBUTING.md` (comment policy, tests).

## Repo map

Eight crates, mirroring Ruff's split. Keep `lib.rs`/`main.rs` roots **thin facades** (module decls
+ re-exports); real logic lives in focused submodules.

| Crate | Role |
| --- | --- |
| `sloplint` | CLI binary (`main.rs` is a dispatcher; commands live in `commands/`) |
| `sloplint_linter` | all `SLP*` rules + the single-pass `Checker` engine (cf. `ruff_linter`) |
| `sloplint_python` | **the seam** — the *only* crate that names the pinned `ruff_*` crates; re-exports the AST/token/text-size types and shared AST helpers everyone else uses |
| `sloplint_diagnostics` | rule-independent diagnostic model + text rendering |
| `sloplint_clone` | near-duplicate function detection (SLP020) |
| `sloplint_metrics` | quality metrics, import-graph architecture metrics, badges |
| `sloplint_report` | output formatters (text / JSON / SARIF / markdown) |
| `sloplint_macros` | proc-macros: the rule catalog (`map_codes!`) + `ViolationMetadata` derive |

**Seam discipline:** only `sloplint_python` (and the blessed, documented `ruff_text_size` use in
`sloplint_diagnostics`) may depend on `ruff_*` directly. Shared AST walks (e.g. `collect_functions`,
docstring helpers) live in `sloplint_python` so they can't drift between crates.

## Conventions

- **Gates (must pass — same as CI `ci.yml`):**
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings   # warnings are hard errors
  cargo build --workspace
  cargo test --workspace
  ```
  Run `clippy --all-targets` (not just `build`) — it's the gate that catches unused imports after a
  refactor. Check fmt's exit code directly (`cargo fmt --all --check; echo $?`); don't pipe it
  through `tail`, which masks the exit code.
- **Lints** are centralized: `[workspace.lints]` in the root `Cargo.toml`, every crate opts in with
  `[lints] workspace = true`.
- **Errors:** `thiserror` enums in library crates; `anyhow` only at the binary edge. Prefer the bare
  `use anyhow::Result` import (Ruff's dominant style) and `.context(...)?` / `.with_context(|| …)?`
  over `.map_err(|e| anyhow!("…{e}"))`. **Caveat:** `.context()` only matches the old inline `: {e}`
  output for *depth-1* sources; an error that itself carries a `source()` (e.g. a `#[error(transparent)]`
  wrapper) will double-print under `{err:#}` — keep the inline form there.
- **Dependencies** shared by 2+ crates go in `[workspace.dependencies]`; crates inherit with
  `dep.workspace = true`. Crate manifests inherit the workspace package keys uniformly.
- **Comments & tests:** see `CONTRIBUTING.md`. In short — no WHAT-comments, no provenance/issue
  citations in source; rule tests are `test_rule!` + a fixture + an insta `.snap`, never large
  inline assertion blocks (`INSTA_UPDATE=always cargo test` or `cargo insta review` to regenerate).

### Adding / changing a rule

- The rule catalog is centralized in `sloplint_linter/src/codes.rs` via `map_codes!`; rules
  register through the `registry`. Whole-tree rules (clones, fanout, ghost, imports, corrupted)
  implement `WholeProjectRule`.
- Every rule derives `ViolationMetadata` and **must** carry the full ruff-style doc triad:
  `## What it does`, `## Why is this bad?`, `## Example`. A doc-guard test
  (`registry.rs::every_shipped_rule_is_documented`) enforces all three headings — it will fail CI
  if you omit one.
- Per-file rules hook the single-pass `Checker` (`check_stmt` / `check_source` / `check_names`);
  don't add bespoke full-tree walks unless the rule genuinely needs two passes.

## How to land a change (agent workflow)

1. Branch off the latest `origin/main` (often in a `git worktree` so the main checkout is
   untouched). **Fast-forward your local `main` to `origin/main` first** — squash-merges mean a
   local checkout drifts behind quickly, and auditing stale code wastes a lot of effort.
2. Implement. For a **refactor**, the bar is *behavior-preserving*: verify the change is
   byte-for-byte equivalent (diff moved bodies; compare the new binary's output against the
   `origin/main` binary on a sample / the test corpus). State that you verified it.
3. Run all four gates above; smoke-test the affected command.
4. Open a PR against `main` with a descriptive body. Squash-merge. End commit messages with:
   ```
   Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
   ```
5. After merge, delete the branch and clean up the worktree.

Keep PRs small and single-purpose (one rule, one refactor, one dedup). The git history here is a
long series of focused squash-merges, not big mixed commits.

## Releases

Version lives **only** in the root `Cargo.toml` `[workspace.package] version` (pyproject uses
`dynamic = ["version"]` via maturin). To cut a release:

1. Branch `release/x.y.z`, bump the version, `cargo build` to update `Cargo.lock`, commit
   `Release x.y.z`, PR, squash-merge.
2. Tag the merge commit `vX.Y.Z` and push the tag. **The tag push is irreversible and
   outward-facing** — it triggers `release.yml` (prebuilt binaries + the GitHub Release) and
   `wheels.yml` (publishes to **PyPI as `sloplintpy`**, a version that can never be reused). Confirm
   before pushing a release tag.

## Docs

- The **README is metrics-centric**: it leads with the metrics section and an index table, then
  rules, install, usage, config, badges. It must **not** carry the long per-metric descriptions or
  internal issue-number citations.
- The detailed **per-metric reference and operational guides live in the GitHub wiki** (`Metrics`,
  `Autofix`, `Agent-loop-integration`, `Inline-suppression`, `GitHub-Action`, `Architecture`, plus
  the `cases` case studies). The wiki is a separate git repo (`…/sloplint.wiki.git`).
- Wiki gotcha: pushes need `git -c lfs.locksverify=false push` (the lock-verify pre-push step 401s
  otherwise; credentials come from the normal helper — never embed a token).
