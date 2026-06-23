//! SLP220 metadata — corrupted / truncated AI output.
//!
//! Unlike the other whole-tree rules, SLP220's detection lives in the binary
//! (`sloplint/src/corrupted.rs`), not here: its headline case is a file that *fails to parse*,
//! which never reaches the per-file rules (they only run on a parsed tree). This struct carries
//! only the rule's code and documentation, so SLP220 still appears in the catalog,
//! `sloplint rule`, and the docs guard like every other shipped rule.

use sloplint_macros::ViolationMetadata;

use crate::registry::WholeProjectRule;

/// ## What it does
/// Flags signs that an LLM's output was written back imperfectly: a leftover Markdown code fence,
/// a merge-conflict marker, or a stray `<file …>` scaffolding tag sitting in executable space; a
/// `.py` file that does not parse at all; or a high fraction of natural-language prose lines.
///
/// ## Why is this bad?
/// These are near-unambiguous corruption signals — the file is broken or truncated, not merely
/// stylistically off. A file that does not parse would otherwise be silently skipped; SLP220 turns
/// it into an explicit finding that says *why* it looks like corrupted AI output.
///
/// ## Example
/// ```python
/// def handler(request):
/// <<<<<<< HEAD          # a leftover merge-conflict marker in executable space
///     return process(request)
/// ```
#[derive(ViolationMetadata)]
pub struct Corrupted;

impl WholeProjectRule for Corrupted {
    fn code(&self) -> &'static str {
        "SLP220"
    }
}
