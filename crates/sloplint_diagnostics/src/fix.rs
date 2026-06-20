//! Autofix model and application engine — rule-independent, mirroring Ruff's `ruff_diagnostics`.
//!
//! A [`Fix`] is a set of [`Edit`]s (a source range to replace with a string) plus an
//! [`Applicability`] saying whether it's safe to apply automatically. Rules attach an optional
//! `Fix` to a [`Diagnostic`]; the CLI's `--fix` mode runs [`apply`] to rewrite the file.
//!
//! Like Ruff, application is conservative: edits are applied right-to-left so earlier offsets
//! stay valid, and a fix whose edits would overlap an already-accepted edit is skipped (left for
//! a later run or a human) rather than producing a corrupt splice.

use ruff_text_size::TextRange;

use crate::Diagnostic;

/// Whether a fix is safe to apply without human review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Preserves program behavior — applied by `--fix`.
    Safe,
    /// Might change behavior or intent — only applied with `--unsafe-fixes`.
    Unsafe,
}

/// A single source edit: replace `range` with `content` (an empty `content` is a deletion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The byte range to replace.
    pub range: TextRange,
    /// The replacement text (empty = delete the range).
    pub content: String,
}

impl Edit {
    /// Delete the given range.
    pub fn deletion(range: TextRange) -> Self {
        Self {
            range,
            content: String::new(),
        }
    }

    /// Replace the given range with `content`.
    pub fn replacement(range: TextRange, content: impl Into<String>) -> Self {
        Self {
            range,
            content: content.into(),
        }
    }
}

/// A suggested fix for a diagnostic: one or more edits applied together, with an applicability.
#[derive(Debug, Clone)]
pub struct Fix {
    /// The edits, assumed non-overlapping within the fix.
    pub edits: Vec<Edit>,
    /// Whether it's safe to apply automatically.
    pub applicability: Applicability,
}

impl Fix {
    /// A safe fix made of a single edit.
    pub fn safe_edit(edit: Edit) -> Self {
        Self {
            edits: vec![edit],
            applicability: Applicability::Safe,
        }
    }

    /// An unsafe fix made of a single edit.
    pub fn unsafe_edit(edit: Edit) -> Self {
        Self {
            edits: vec![edit],
            applicability: Applicability::Unsafe,
        }
    }

    /// The lowest start offset across the fix's edits (its anchor for ordering / overlap checks).
    fn min_start(&self) -> u32 {
        self.edits
            .iter()
            .map(|e| u32::from(e.range.start()))
            .min()
            .unwrap_or(0)
    }

    /// The highest end offset across the fix's edits.
    fn max_end(&self) -> u32 {
        self.edits
            .iter()
            .map(|e| u32::from(e.range.end()))
            .max()
            .unwrap_or(0)
    }

    /// Whether the fix has at least one edit and its edits don't overlap each other — the
    /// precondition the right-to-left splice in [`apply`] relies on. A fix that's empty (a no-op
    /// that would still be counted as "fixed") or whose own edits overlap (which would corrupt the
    /// splice) is skipped rather than applied. Single-edit fixes always pass.
    fn is_well_formed(&self) -> bool {
        if self.edits.is_empty() {
            return false;
        }
        let mut sorted: Vec<&Edit> = self.edits.iter().collect();
        sorted.sort_by_key(|e| u32::from(e.range.start()));
        sorted
            .windows(2)
            .all(|w| u32::from(w[0].range.end()) <= u32::from(w[1].range.start()))
    }
}

/// The result of applying fixes to a source string.
pub struct Applied {
    /// The rewritten source (equal to the input when nothing applied).
    pub output: String,
    /// Indices (into the `diagnostics` slice passed to [`apply`]) whose fix was applied.
    pub fixed: Vec<usize>,
}

impl Applied {
    /// Whether any fix was applied.
    pub fn changed(&self) -> bool {
        !self.fixed.is_empty()
    }
}

/// Apply the diagnostics' fixes to `source`.
///
/// Only fixes whose [`Applicability`] is permitted are considered: `Safe` always; `Unsafe` only
/// when `allow_unsafe` is set. Fixes are ordered by their first edit; a fix that overlaps an
/// already-accepted one is skipped (it'll be caught on a re-run after the conflicting region
/// settles). Accepted edits are spliced right-to-left so earlier offsets stay valid.
///
/// Returns the rewritten source and the indices of the diagnostics that were fixed, so the caller
/// can drop them from its report.
pub fn apply(source: &str, diagnostics: &[Diagnostic], allow_unsafe: bool) -> Applied {
    // (original index, &Fix) for every diagnostic with an applicable fix.
    let mut candidates: Vec<(usize, &Fix)> = diagnostics
        .iter()
        .enumerate()
        .filter_map(|(i, d)| d.fix.as_ref().map(|f| (i, f)))
        .filter(|(_, f)| allow_unsafe || f.applicability == Applicability::Safe)
        .filter(|(_, f)| f.is_well_formed())
        .collect();

    // Deterministic order: by first edit start, then original index.
    candidates.sort_by(|a, b| a.1.min_start().cmp(&b.1.min_start()).then(a.0.cmp(&b.0)));

    let mut accepted_edits: Vec<&Edit> = Vec::new();
    let mut fixed: Vec<usize> = Vec::new();
    // Watermark: the highest end offset accepted so far. A fix starting before it would overlap an
    // already-accepted edit, so it's skipped (caught on a re-run). Candidates are sorted by start
    // and `watermark` begins at 0, so the first one is always accepted (`min_start < 0` is false).
    let mut watermark: u32 = 0;
    for (index, fix) in candidates {
        if fix.min_start() < watermark {
            continue; // overlaps an accepted edit — leave it for a later pass.
        }
        watermark = watermark.max(fix.max_end());
        accepted_edits.extend(fix.edits.iter());
        fixed.push(index);
    }

    if accepted_edits.is_empty() {
        return Applied {
            output: source.to_string(),
            fixed,
        };
    }

    // Splice right-to-left so each edit's offsets remain valid against the un-spliced prefix.
    accepted_edits.sort_by_key(|e| std::cmp::Reverse(u32::from(e.range.start())));
    let mut output = source.to_string();
    for edit in accepted_edits {
        let start = u32::from(edit.range.start()) as usize;
        let end = (u32::from(edit.range.end()) as usize).min(output.len());
        if start > end || end > output.len() {
            continue; // defensively skip an out-of-bounds edit rather than panic.
        }
        output.replace_range(start..end, &edit.content);
    }

    Applied { output, fixed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;
    use ruff_text_size::TextSize;

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::from(start), TextSize::from(end))
    }

    fn diag_with(code: &str, r: TextRange, fix: Option<Fix>) -> Diagnostic {
        let mut d = Diagnostic::new(code, "msg", r, Severity::Warning);
        d.fix = fix;
        d
    }

    #[test]
    fn applies_non_overlapping_deletions() {
        // "abXcdYe" -> delete X (2..3) and Y (5..6) -> "abcde".
        let source = "abXcdYe";
        let diags = [
            diag_with("A", range(2, 3), Some(Fix::safe_edit(Edit::deletion(range(2, 3))))),
            diag_with("B", range(5, 6), Some(Fix::safe_edit(Edit::deletion(range(5, 6))))),
        ];
        let applied = apply(source, &diags, false);
        assert_eq!(applied.output, "abcde");
        assert_eq!(applied.fixed, vec![0, 1]);
    }

    #[test]
    fn skips_overlapping_fix() {
        // Two fixes touching overlapping ranges: only the first (by start) is applied.
        let source = "0123456789";
        let diags = [
            diag_with("A", range(2, 6), Some(Fix::safe_edit(Edit::deletion(range(2, 6))))),
            diag_with("B", range(4, 8), Some(Fix::safe_edit(Edit::deletion(range(4, 8))))),
        ];
        let applied = apply(source, &diags, false);
        assert_eq!(applied.fixed, vec![0]);
        assert_eq!(applied.output, "016789");
    }

    #[test]
    fn unsafe_fix_gated_by_flag() {
        let source = "keep me";
        let diags = [diag_with(
            "A",
            range(0, 4),
            Some(Fix::unsafe_edit(Edit::deletion(range(0, 4)))),
        )];
        // Default: unsafe fixes are not applied.
        let applied = apply(source, &diags, false);
        assert!(!applied.changed());
        assert_eq!(applied.output, source);
        // With the flag, it applies.
        let applied = apply(source, &diags, true);
        assert_eq!(applied.output, " me");
        assert_eq!(applied.fixed, vec![0]);
    }

    #[test]
    fn replacement_edit() {
        let source = "a = 1";
        let diags = [diag_with(
            "A",
            range(4, 5),
            Some(Fix::safe_edit(Edit::replacement(range(4, 5), "2"))),
        )];
        assert_eq!(apply(source, &diags, false).output, "a = 2");
    }

    #[test]
    fn empty_fix_is_not_counted_or_applied() {
        let source = "abc";
        let diags = [diag_with(
            "A",
            range(0, 1),
            Some(Fix {
                edits: vec![],
                applicability: Applicability::Safe,
            }),
        )];
        let applied = apply(source, &diags, false);
        assert!(!applied.changed());
        assert_eq!(applied.output, source);
    }

    #[test]
    fn fix_with_self_overlapping_edits_is_skipped() {
        // A malformed multi-edit fix whose own edits overlap is dropped, not spliced.
        let source = "0123456789";
        let diags = [diag_with(
            "A",
            range(0, 1),
            Some(Fix {
                edits: vec![Edit::deletion(range(2, 6)), Edit::deletion(range(4, 8))],
                applicability: Applicability::Safe,
            }),
        )];
        let applied = apply(source, &diags, false);
        assert!(!applied.changed());
        assert_eq!(applied.output, source);
    }

    #[test]
    fn well_formed_multi_edit_fix_applies() {
        // Two non-overlapping edits in one fix both apply.
        let source = "0123456789";
        let diags = [diag_with(
            "A",
            range(0, 1),
            Some(Fix {
                edits: vec![Edit::deletion(range(1, 3)), Edit::deletion(range(6, 8))],
                applicability: Applicability::Safe,
            }),
        )];
        let applied = apply(source, &diags, false);
        assert_eq!(applied.fixed, vec![0]);
        assert_eq!(applied.output, "034589");
    }

    #[test]
    fn diagnostics_without_fix_are_untouched() {
        let source = "x";
        let diags = [diag_with("A", range(0, 1), None)];
        let applied = apply(source, &diags, false);
        assert!(!applied.changed());
        assert_eq!(applied.output, source);
    }
}
