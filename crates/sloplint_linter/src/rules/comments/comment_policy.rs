//! SLP010: comments are banned by default.

use sloplint_diagnostics::{Diagnostic, Edit, Fix, Severity};
use sloplint_python::{TextRange, TextSize};

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags prose comments. sloplint's opinionated default is that production code carries no
/// comments; functional comments are exempt — tool directives (`# noqa`, `# type:`), the
/// shebang, encoding declarations, and TODO/FIXME that reference a ticket.
///
/// ## Why is this bad?
/// Comments drift from the code they describe and are a common vehicle for AI narration; code
/// should be self-explanatory. Paths that legitimately want comments (migrations, some tests)
/// opt back in via `allow_comments` in config.
#[derive(ViolationMetadata)]
pub struct CommentPolicy;

impl Rule for CommentPolicy {
    fn code(&self) -> &'static str {
        "SLP010"
    }

    fn check_comment(
        &self,
        ctx: &FileContext,
        range: TextRange,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let body = comment_body(&ctx.source[range]);
        if is_directive(body) || is_ticketed_todo(body) {
            return;
        }
        diagnostics.push(
            Diagnostic::new(
                self.code(),
                "comment is not allowed (comments are banned by default; allow specific paths in config)",
                range,
                Severity::Warning,
            )
            // Deleting a prose comment never changes runtime behavior, so the fix is Safe.
            .with_fix(Fix::safe_edit(deletion_edit(ctx.source, range))),
        );
    }
}

/// Build the edit that removes a banned comment.
///
/// An *own-line* comment (nothing but whitespace before the `#`) takes its whole physical line —
/// indentation and the trailing line terminator — so no blank, indented stub is left behind. A
/// *trailing* comment (code precedes the `#`) deletes only the run of whitespace before the `#`
/// and the comment itself, preserving the code and the newline (`x = 1  # c` -> `x = 1`).
fn deletion_edit(source: &str, comment: TextRange) -> Edit {
    let bytes = source.as_bytes();
    let start = usize::from(comment.start());
    let end = usize::from(comment.end());

    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    // A leading UTF-8 BOM (U+FEFF) isn't whitespace to `trim`, so strip it before classifying —
    // otherwise a BOM-prefixed first-line comment looks "trailing" and we'd leave a stray BOM line.
    // The own-line branch deletes from `line_start`, so the BOM is removed along with the line.
    let own_line = source[line_start..start]
        .trim_start_matches('\u{feff}')
        .trim()
        .is_empty();

    if own_line {
        // Extend past the line terminator (handles "\n", "\r\n", and a lone "\r"; EOF has none).
        let mut del_end = end;
        if del_end < source.len() && bytes[del_end] == b'\r' {
            del_end += 1;
        }
        if del_end < source.len() && bytes[del_end] == b'\n' {
            del_end += 1;
        }
        Edit::deletion(TextRange::new(
            TextSize::from(line_start as u32),
            TextSize::from(del_end as u32),
        ))
    } else {
        // Trailing comment: also remove the whitespace separating it from the code.
        let mut ws_start = start;
        while ws_start > line_start && matches!(bytes[ws_start - 1], b' ' | b'\t') {
            ws_start -= 1;
        }
        Edit::deletion(TextRange::new(
            TextSize::from(ws_start as u32),
            TextSize::from(end as u32),
        ))
    }
}

/// Strip the leading `#`(s) and surrounding whitespace from a raw comment token.
pub fn comment_body(raw: &str) -> &str {
    raw.trim_start_matches('#').trim()
}

/// Functional comments that must never be banned: tool directives, shebang, encoding.
pub fn is_directive(body: &str) -> bool {
    // Shebang: the comment body after `#` begins with `!`.
    if body.starts_with('!') {
        return true;
    }
    let lower = body.to_ascii_lowercase();

    // Colon-delimited tool directives: the colon is the word boundary, so a plain
    // `starts_with` can't be fooled by a prose word that merely begins the same way.
    const COLON_DIRECTIVES: &[&str] = &[
        "type:",
        "mypy:",
        "pyright:",
        "ruff:",
        "sloplint:",
        "pylint:",
        "pyre:",
        "isort:",
        "fmt:",
        "yapf:",
        "pragma:",
    ];
    if COLON_DIRECTIVES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }

    // Bare keyword directives must end at a word boundary, so `# noqaX` / `# nosecret`
    // are still treated as prose (and banned), not mistaken for `# noqa` / `# nosec`.
    const BARE_DIRECTIVES: &[&str] = &["noqa", "nosec"];
    if BARE_DIRECTIVES
        .iter()
        .any(|keyword| starts_with_word(&lower, keyword))
    {
        return true;
    }

    // Encoding declaration (PEP 263), e.g. `# -*- coding: utf-8 -*-`.
    lower.contains("coding:") || lower.contains("coding=")
}

/// True if `text` begins with `word` followed by a word boundary (end of string or a
/// non-identifier character).
fn starts_with_word(text: &str, word: &str) -> bool {
    match text.strip_prefix(word) {
        Some(rest) => rest
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_'),
        None => false,
    }
}

/// A TODO/FIXME/XXX/HACK that references a ticket in parentheses, e.g. `TODO(PROJ-12): ...`.
/// Bare `# TODO` is still banned — a tracked follow-up is allowed, an untracked one isn't.
pub fn is_ticketed_todo(body: &str) -> bool {
    const TAGS: &[&str] = &["TODO", "FIXME", "XXX", "HACK"];
    for tag in TAGS {
        if let Some(rest) = body.strip_prefix(tag) {
            if let Some(after_open) = rest.strip_prefix('(') {
                if let Some(close) = after_open.find(')') {
                    if close > 0 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directives_are_recognized() {
        assert!(is_directive("noqa"));
        assert!(is_directive("noqa: F401"));
        assert!(is_directive("type: ignore"));
        assert!(is_directive("!/usr/bin/env python")); // shebang body
        assert!(is_directive("-*- coding: utf-8 -*-"));
        // A `# noqa` inline suppression is a tool directive, so the comment carrying it is
        // never itself banned by SLP010.
        assert!(is_directive("noqa: SLP020"));
    }

    #[test]
    fn prose_that_merely_starts_like_a_directive_is_not_a_directive() {
        // Regression: bare keywords must end at a word boundary.
        assert!(!is_directive("noqaX should still be banned"));
        assert!(!is_directive("nosecret handling here"));
        assert!(!is_directive("type annotations are great")); // no colon
    }

    /// Apply SLP010's deletion edit to the first comment in `source` and return the result.
    fn fix_first_comment(source: &str) -> String {
        use sloplint_python::{parse, Ranged, TokenKind};
        let parsed = parse(source).expect("test source parses");
        let token = parsed
            .tokens()
            .iter()
            .find(|t| t.kind() == TokenKind::Comment)
            .expect("source has a comment");
        let edit = deletion_edit(source, token.range());
        let mut out = source.to_string();
        out.replace_range(
            usize::from(edit.range.start())..usize::from(edit.range.end()),
            &edit.content,
        );
        out
    }

    #[test]
    fn own_line_comment_deletes_whole_line() {
        assert_eq!(fix_first_comment("a = 1\n# c\nb = 2\n"), "a = 1\nb = 2\n");
    }

    #[test]
    fn indented_own_line_comment_deletes_indentation_too() {
        assert_eq!(
            fix_first_comment("def f():\n    # c\n    return 1\n"),
            "def f():\n    return 1\n"
        );
    }

    #[test]
    fn trailing_comment_keeps_code_and_newline() {
        assert_eq!(fix_first_comment("x = 1  # c\n"), "x = 1\n");
    }

    #[test]
    fn own_line_comment_at_eof_without_newline() {
        assert_eq!(fix_first_comment("x = 1\n# c"), "x = 1\n");
    }

    #[test]
    fn bom_prefixed_comment_line_is_deleted_whole() {
        // A file starting with a BOM then a comment: the whole first line (BOM included) goes.
        assert_eq!(fix_first_comment("\u{feff}# c\nx = 1\n"), "x = 1\n");
    }

    #[test]
    fn only_ticketed_todos_are_allowed() {
        assert!(is_ticketed_todo("TODO(PROJ-123): wire this up"));
        assert!(is_ticketed_todo("FIXME(bug-7) flaky"));
        assert!(!is_ticketed_todo("TODO figure this out"));
        assert!(!is_ticketed_todo("TODO(): empty"));
    }
}
