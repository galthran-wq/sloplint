//! SLP003: the "comment deodorant" smell (preview).
//!
//! Fowler's *Refactoring*: "comments are often used as a deodorant … you look at thickly
//! commented code and notice that the comments are there because the code is bad." This rule
//! keys on exactly that — but only on the **composite**: a function flagged solely when it is
//! *both* densely commented *and* genuinely hard (high cognitive complexity). Either signal
//! alone is noise; together they are a high-value, low-false-positive smell that says "the
//! comments may be masking code that should have been made clear instead."
//!
//! Distinct from `SLP001` (one comment restating one line, lexical and per-comment): this is a
//! per-function *statistical* smell built on the metrics crate. Both thresholds are
//! configurable under `[limits]`; ships Preview.
//!
//! Comment density is `# comment lines / lines` measured over the function's **own** body —
//! lines (and comments) belonging to a nested `def` are excluded, mirroring how the reused
//! cognitive-complexity score excludes nested functions. So a complex but uncommented parent
//! is never flagged for comments that actually live in a nested helper. Docstrings are string
//! literals, not comments, so they never count here (they are `SLP002`'s concern).

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_metrics::file_metrics;
use sloplint_python::{Ranged, TextRange, TokenKind};

use crate::lint::{FileContext, Rule};

pub struct CommentDeodorant;

impl Rule for CommentDeodorant {
    fn code(&self) -> &'static str {
        "SLP003"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let min_density = ctx.limits.comment_deodorant_density;
        let min_cognitive = ctx.limits.comment_deodorant_cognitive;
        // Reuse the metrics crate for complexity rather than re-deriving it here.
        let metrics = file_metrics(ctx.source, ctx.parsed);
        let ranges: Vec<TextRange> = metrics.functions.iter().map(|f| f.range).collect();

        for function in &metrics.functions {
            // Cheap gate first: the "is this code actually hard?" axis.
            if function.cognitive < min_cognitive {
                continue;
            }
            // The ranges of functions nested inside this one — excluded from its density on
            // both sides of the ratio, so a nested helper's comments aren't attributed here
            // (`cognitive` already excludes them, so the two axes share one scope).
            let nested: Vec<TextRange> = ranges
                .iter()
                .copied()
                .filter(|r| {
                    *r != function.range
                        && r.start() >= function.range.start()
                        && r.end() <= function.range.end()
                })
                .collect();

            let comment_lines = own_comment_lines(ctx, function.range, &nested);
            if comment_lines == 0 {
                continue; // never report "0% comment density"
            }
            let own_lines = own_line_count(ctx.source, function.range, &nested);
            let density = comment_lines as f64 / own_lines.max(1) as f64;
            if density < min_density {
                continue;
            }
            diagnostics.push(Diagnostic::new(
                "SLP003",
                format!(
                    "function `{}`: {:.0}% comment density over cognitive complexity {} — \
                     comments may be masking unclear code; clarify the code instead",
                    function.name,
                    density * 100.0,
                    function.cognitive,
                ),
                function.range,
                Severity::Warning,
            ));
        }
    }
}

/// `#` comment lines inside `range` but not inside any `nested` function range. A `#` comment
/// runs to end of line, so at most one comment token exists per physical line — the token
/// count is the comment-line count.
fn own_comment_lines(ctx: &FileContext, range: TextRange, nested: &[TextRange]) -> usize {
    ctx.parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .filter(|token| {
            let r = token.range();
            r.start() >= range.start() && r.end() <= range.end()
        })
        .filter(|token| {
            let r = token.range();
            !nested
                .iter()
                .any(|n| r.start() >= n.start() && r.end() <= n.end())
        })
        .count()
}

/// Physical lines spanned by `range` minus the distinct lines belonging to `nested` functions.
fn own_line_count(source: &str, range: TextRange, nested: &[TextRange]) -> usize {
    let span = |r: TextRange| {
        let first = line_of(source, r.start().into());
        let last = line_of(
            source,
            u32::from(r.end()).saturating_sub(1).max(r.start().into()),
        );
        (first, last)
    };
    let (first, last) = span(range);
    let total = last - first + 1;
    let mut intervals: Vec<(usize, usize)> = nested.iter().map(|&r| span(r)).collect();
    total.saturating_sub(distinct_lines(&mut intervals))
}

/// Distinct lines covered by inclusive `[start, end]` line intervals, merging overlaps (a
/// function nested inside another shares its lines and must be counted once). Sorts in place.
fn distinct_lines(intervals: &mut [(usize, usize)]) -> usize {
    intervals.sort_unstable();
    let mut total = 0;
    let mut current: Option<(usize, usize)> = None;
    for &(start, end) in intervals.iter() {
        match current {
            Some((cs, ce)) if start <= ce => current = Some((cs, ce.max(end))),
            Some((cs, ce)) => {
                total += ce - cs + 1;
                current = Some((start, end));
            }
            None => current = Some((start, end)),
        }
    }
    if let Some((cs, ce)) = current {
        total += ce - cs + 1;
    }
    total
}

/// 1-based line number of a byte offset.
fn line_of(source: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(source.len());
    source.as_bytes()[..offset]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Limits;
    use sloplint_python::parse;

    /// Findings over `source` with explicit deodorant thresholds.
    fn findings(source: &str, min_density: f64, min_cognitive: usize) -> usize {
        let parsed = parse(source).expect("valid python");
        let limits = Limits {
            comment_deodorant_density: min_density,
            comment_deodorant_cognitive: min_cognitive,
            ..Limits::default()
        };
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            limits,
        };
        let mut diagnostics = Vec::new();
        CommentDeodorant.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    // A branchy (cognitive > 0) function that is also heavily commented.
    const HARD_AND_COMMENTED: &str = "\
def f(xs):
    total = 0
    for x in xs:        # walk the items
        if x > 0:       # keep positives
            total += x  # accumulate
    return total
";

    #[test]
    fn fires_only_on_the_composite() {
        // Hard + densely commented -> fires.
        assert_eq!(findings(HARD_AND_COMMENTED, 0.3, 1), 1);
    }

    #[test]
    fn complex_but_clean_code_is_not_flagged() {
        let clean = "\
def f(xs):
    total = 0
    for x in xs:
        if x > 0:
            total += x
    return total
";
        assert_eq!(findings(clean, 0.3, 1), 0, "no comments -> never fires");
    }

    #[test]
    fn trivial_but_commented_code_is_not_flagged() {
        // High comment density but zero cognitive complexity -> not the smell.
        let trivial = "\
def g(a):
    # explain the obvious
    # and again
    return a
";
        assert_eq!(findings(trivial, 0.3, 1), 0);
    }

    #[test]
    fn lightly_commented_hard_code_is_not_flagged() {
        // Hard, but only one comment over six lines (~17%) — below the density floor.
        let light = "\
def f(xs):
    total = 0
    for x in xs:
        if x > 0:       # the only comment
            total += x
    return total
";
        assert_eq!(findings(light, 0.3, 1), 0);
    }

    #[test]
    fn cognitive_threshold_gates_the_smell() {
        // Same dense comments; fires under a reachable complexity floor, not an absurd one.
        assert_eq!(findings(HARD_AND_COMMENTED, 0.3, 1), 1);
        assert_eq!(findings(HARD_AND_COMMENTED, 0.3, 99), 0);
    }

    #[test]
    fn nested_helper_comments_do_not_inflate_the_parent() {
        // Regression: `outer` is complex (cognitive 10) but carries NO comments of its own —
        // all six live in the trivial nested `inner`. The parent must NOT be flagged for them.
        let source = "\
def outer(xs):
    total = 0
    for x in xs:
        if x > 0:
            if x > 1:
                if x > 2:
                    total += x
    def inner(y):
        # c1
        # c2
        # c3
        # c4
        # c5
        # c6
        return y
    return total
";
        // `outer` own-density is 0 (no fire); `inner` is trivial (cognitive 0, no fire).
        assert_eq!(findings(source, 0.3, 1), 0);
    }

    #[test]
    fn each_function_assessed_independently() {
        let source = format!(
            "{HARD_AND_COMMENTED}\n{}",
            "def clean(n):\n    return n + 1\n"
        );
        // Only the hard+commented function fires; the trivial clean one doesn't.
        assert_eq!(findings(&source, 0.3, 1), 1);
    }
}
