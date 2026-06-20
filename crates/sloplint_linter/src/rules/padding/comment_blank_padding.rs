//! SLP150: comment/blank padding — functions whose physical size is mostly narration and
//! whitespace, not code (preview).
//!
//! Padded code — real logic wrapped in line-by-line explanatory comments and generous
//! blank-line spacing — inflates every physical-line metric while adding nothing a reader can
//! use. NCSS (non-commenting source statements) strips that varnish; here the signal is the
//! same divergence expressed per function: the fraction of a function's own lines that are
//! comment-only or blank. Above a threshold, the function is mostly padding.
//!
//! Distinct from `SLP003` (the "deodorant" smell — dense comments over a *complex* function):
//! this keys on size distortion, not complexity, so it catches a heavily-commented *simple*
//! function that `SLP003` leaves alone. Both thresholds are configurable under `[limits]`;
//! ships Preview.
//!
//! Measured over each function's **own** lines — the spans of nested `def`s are excluded, so a
//! heavily-commented helper is judged on its own and never inflates its parent's ratio.

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TextRange, TokenKind};

use crate::ast_util::collect_functions;
use crate::lint::{FileContext, Rule};

pub struct CommentBlankPadding;

impl Rule for CommentBlankPadding {
    fn code(&self) -> &'static str {
        "SLP150"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let lines: Vec<&str> = ctx.source.lines().collect();
        // Lines whose content lives inside a string literal (docstrings, multi-line strings):
        // their text is data, not comment/blank padding, even if a line reads `# heading` or is
        // blank. A `#` *outside* a string is always a real comment, so the textual check below
        // is safe once these are excluded.
        let string_lines = string_lines(ctx);
        let mut functions = Vec::new();
        collect_functions(&ctx.parsed.syntax().body, &mut functions);
        let spans: Vec<(usize, usize)> = functions
            .iter()
            .map(|function| line_span(ctx.source, function.range()))
            .collect();

        for (i, function) in functions.iter().enumerate() {
            let (first, last) = spans[i];
            // Line spans of functions nested inside this one — excluded from its own lines.
            let nested: Vec<(usize, usize)> = spans
                .iter()
                .enumerate()
                .filter(|&(j, &(gf, gl))| j != i && gf >= first && gl <= last)
                .map(|(_, &span)| span)
                .collect();

            let mut own_lines = 0usize;
            let mut padding = 0usize;
            for line_no in first..=last {
                if nested
                    .iter()
                    .any(|&(gf, gl)| line_no >= gf && line_no <= gl)
                {
                    continue;
                }
                own_lines += 1;
                if string_lines.contains(&line_no) {
                    continue; // string content — counts as code, not padding.
                }
                let trimmed = lines.get(line_no - 1).copied().unwrap_or("").trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    padding += 1;
                }
            }

            if own_lines < ctx.limits.padding_min_lines {
                continue;
            }
            let ratio = padding as f64 / own_lines as f64;
            if ratio >= ctx.limits.padding_max_ratio {
                diagnostics.push(Diagnostic::new(
                    "SLP150",
                    format!(
                        "function `{}`: {:.0}% of its {own_lines} lines are comments or blanks \
                         — the code is padded with narration and spacing, not logic",
                        function.name,
                        ratio * 100.0,
                    ),
                    function.name.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// The set of 1-based line numbers any of whose content lies inside a string-literal token
/// (including multi-line strings / docstrings). Such lines are string data, not padding.
fn string_lines(ctx: &FileContext) -> HashSet<usize> {
    let mut set = HashSet::new();
    for token in ctx.parsed.tokens().iter() {
        if !is_string_token(token.kind()) {
            continue;
        }
        let (first, last) = line_span(ctx.source, token.range());
        for line_no in first..=last {
            set.insert(line_no);
        }
    }
    set
}

fn is_string_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::String
            | TokenKind::FStringStart
            | TokenKind::FStringMiddle
            | TokenKind::FStringEnd
            | TokenKind::TStringStart
            | TokenKind::TStringMiddle
            | TokenKind::TStringEnd
    )
}

/// Inclusive 1-based `(first_line, last_line)` of `range` within `source`.
fn line_span(source: &str, range: TextRange) -> (usize, usize) {
    let start = u32::from(range.start());
    let end = u32::from(range.end()).saturating_sub(1).max(start);
    (line_of(source, start), line_of(source, end))
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

    fn findings(source: &str) -> usize {
        findings_with(source, Limits::default())
    }

    fn findings_with(source: &str, limits: Limits) -> usize {
        let parsed = parse(source).expect("valid python");
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            limits,
        };
        let mut diagnostics = Vec::new();
        CommentBlankPadding.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    // 11 own lines: 6 code, 4 comment, 2 blank... actually 5 code / 4 comment / 2 blank = 11.
    const PADDED: &str = "\
def process(items):
    # initialize the running total
    total = 0

    # walk over every item we were given
    for item in items:
        # add this item's value to the total
        total += item.value

    # hand the computed total back
    return total
";

    #[test]
    fn flags_a_comment_and_blank_padded_function() {
        assert_eq!(findings(PADDED), 1);
    }

    #[test]
    fn dense_real_code_is_not_flagged() {
        let dense = "\
def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    scaled = [v / total for v in values]
    rounded = [round(x, 3) for x in scaled]
    capped = [min(x, 1.0) for x in rounded]
    floored = [max(x, 0.0) for x in capped]
    return floored
";
        assert_eq!(findings(dense), 0);
    }

    #[test]
    fn small_heavily_commented_function_is_below_the_line_floor() {
        // High padding ratio, but too few lines to judge.
        let small = "def add(a, b):\n    # sum the two inputs\n    return a + b\n";
        assert_eq!(findings(small), 0);
    }

    #[test]
    fn docstring_lines_including_hashes_and_blanks_are_not_padding() {
        // Regression: lines inside the docstring that are blank or start with `#` (a code
        // sample / reST heading) are string content, NOT comment/blank padding.
        let documented = "\
def transform(data):
    \"\"\"Transform the data.

    Example output looks like::

        # heading one
        # heading two

    It is documentation, not comment/blank padding, so it must not trip this rule.
    \"\"\"
    cleaned = [d.strip() for d in data]
    deduped = list(dict.fromkeys(cleaned))
    return sorted(deduped)
";
        assert_eq!(findings(documented), 0);
    }

    #[test]
    fn nested_comments_do_not_inflate_the_parent() {
        // `outer`'s own body is dense; only the nested `helper` is comment-padded. `outer`
        // must not be flagged for `helper`'s padding (helper itself is below the line floor).
        let source = "\
def outer(xs):
    def helper(x):
        # double it
        # then add one
        return x * 2 + 1
    a = helper(xs[0])
    b = helper(xs[1])
    c = helper(xs[2])
    d = a + b + c
    return d
";
        assert_eq!(findings(source), 0);
    }

    #[test]
    fn threshold_is_configurable() {
        // The padded fixture passes a 0.7 ratio floor.
        let strict = Limits {
            padding_max_ratio: 0.7,
            ..Limits::default()
        };
        assert_eq!(findings_with(PADDED, strict), 0);
    }
}
