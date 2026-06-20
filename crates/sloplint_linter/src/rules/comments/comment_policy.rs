//! SLP010: comments are banned by default.
//!
//! sloplint's opinionated default is that production code shouldn't carry prose comments.
//! Functional comments are exempt — tool directives (`# noqa`, `# type:`), the shebang,
//! encoding declarations, and TODO/FIXME that reference a ticket. Paths that legitimately
//! want comments (migrations, some tests) opt back in via `allow_comments` in config; the
//! CLI passes that through by simply not selecting SLP010 for those files.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TokenKind};

use crate::lint::{FileContext, Rule};

pub struct CommentPolicy;

impl Rule for CommentPolicy {
    fn code(&self) -> &'static str {
        "SLP010"
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
            diagnostics.push(Diagnostic::new(
                "SLP010",
                "comment is not allowed (comments are banned by default; allow specific paths in config)",
                token.range(),
                Severity::Warning,
            ));
        }
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
        // An inline-suppression directive (#94) is a tool directive, so the comment carrying it is
        // never itself banned by SLP010.
        assert!(is_directive("sloplint: allow SLP020"));
        assert!(is_directive("sloplint: allow"));
    }

    #[test]
    fn prose_that_merely_starts_like_a_directive_is_not_a_directive() {
        // Regression: bare keywords must end at a word boundary.
        assert!(!is_directive("noqaX should still be banned"));
        assert!(!is_directive("nosecret handling here"));
        assert!(!is_directive("type annotations are great")); // no colon
    }

    #[test]
    fn only_ticketed_todos_are_allowed() {
        assert!(is_ticketed_todo("TODO(PROJ-123): wire this up"));
        assert!(is_ticketed_todo("FIXME(bug-7) flaky"));
        assert!(!is_ticketed_todo("TODO figure this out"));
        assert!(!is_ticketed_todo("TODO(): empty"));
    }
}
