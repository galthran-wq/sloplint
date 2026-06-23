//! SLP001: redundant "what" comment.

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TextRange, TokenKind};

use crate::lint::{FileContext, Rule};
use crate::rules::comments::comment_policy::{comment_body, is_directive, is_ticketed_todo};
use crate::words::{content_words, overlap_ratio};
use sloplint_macros::ViolationMetadata;

/// Share of a comment's content words that must also appear in the associated code before
/// it's considered a restatement.
const OVERLAP_THRESHOLD: f64 = 0.5;
/// Only short comments are judged; long prose is more likely to add real context.
const MAX_COMMENT_WORDS: usize = 6;

/// ## What it does
/// Flags an inline or standalone comment whose words mostly restate the code on (or right
/// below) its line — `# compute total` next to `total = compute_total()`.
///
/// ## Why is this bad?
/// A "what" comment that restates the code adds no information and rots as the code changes.
/// Directives and ticketed TODOs are exempt (they aren't prose). Heuristic, so preview.
#[derive(ViolationMetadata)]
pub struct RedundantComment;

impl Rule for RedundantComment {
    fn code(&self) -> &'static str {
        "SLP001"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        for token in ctx.parsed.tokens().iter() {
            if token.kind() != TokenKind::Comment {
                continue;
            }
            let body = comment_body(&ctx.source[token.range()]);
            if is_directive(body) || is_ticketed_todo(body) {
                continue;
            }
            let comment_words = content_words(body);
            if comment_words.is_empty() || comment_words.len() > MAX_COMMENT_WORDS {
                continue;
            }
            let code_words = associated_code_words(ctx.source, token.range());
            if overlap_ratio(&comment_words, &code_words) >= OVERLAP_THRESHOLD {
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    "comment restates the code (redundant 'what' comment)",
                    token.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Words of the code a comment describes: the code before it on the same line, or — for a
/// standalone comment — the next non-blank line.
fn associated_code_words(source: &str, range: TextRange) -> HashSet<String> {
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let before = &source[line_start..start];
    if before.trim().is_empty() {
        let next_line = source[end..]
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("");
        content_words(next_line)
    } else {
        content_words(before)
    }
}
