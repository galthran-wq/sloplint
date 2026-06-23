//! SLP004: AI-narration comment tells.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::TextRange;

use crate::lint::{FileContext, Rule};
use crate::rules::comments::comment_policy::{comment_body, is_directive, is_ticketed_todo};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Where comments are allowed (so SLP010 doesn't already ban them), flags three text-only
/// classes of AI narration: deferral/incompleteness (`for now`, `in production this would`;
/// **error**), hedging (`should work`, `probably`; **warning**), and structural noise — step
/// narration (`# Step 1:`), ASCII dividers (`# ======`), and narrator comments (**warning**).
///
/// ## Why is this bad?
/// These phrasings are strong LLM tells — the model narrating rather than documenting, or
/// admitting an unfinished corner (a semantic-incompleteness signal). WHY-comments, license
/// headers, directives, and ticketed TODOs are exempt; the lexicon extends via `[comments] extra`.
///
/// ## Example
/// ```python
/// # Step 1: parse the input
/// data = parse(raw)  # this should work for now
/// ```
#[derive(ViolationMetadata)]
pub struct CommentTells;

impl Rule for CommentTells {
    fn code(&self) -> &'static str {
        "SLP004"
    }

    fn check_comment(
        &self,
        ctx: &FileContext,
        range: TextRange,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let body = comment_body(&ctx.source[range]);
        if body.is_empty()
            || is_directive(body)
            || is_ticketed_todo(body)
            || is_license_or_why(body)
        {
            return;
        }
        if let Some((severity, message)) = classify(body, ctx.comment_phrases_extra) {
            diagnostics.push(Diagnostic::new(self.code(), message, range, severity));
        }
    }
}

/// Classify a comment body as an AI tell, highest-signal first (deferral → hedging → structural).
fn classify(body: &str, extra: &[String]) -> Option<(Severity, String)> {
    let lower = body.to_ascii_lowercase();

    if let Some(phrase) = DEFERRAL.iter().find(|p| contains_phrase(&lower, p)) {
        return Some((
            Severity::Error,
            format!(
                "deferral/incompleteness language in a comment (\"{phrase}\") — signals a corner \
                 was cut, not finished work"
            ),
        ));
    }
    let hedge = HEDGING
        .iter()
        .copied()
        .find(|p| contains_phrase(&lower, p))
        .or_else(|| {
            extra
                .iter()
                .map(String::as_str)
                .find(|p| contains_phrase(&lower, &p.to_ascii_lowercase()))
        });
    if let Some(phrase) = hedge {
        return Some((
            Severity::Warning,
            format!("hedging/uncertainty language in a comment (\"{phrase}\")"),
        ));
    }
    if is_step_narration(&lower) {
        return Some((
            Severity::Warning,
            "step-narration comment (`Step N:` / `Phase N`) — narrating the code, not documenting it"
                .to_string(),
        ));
    }
    if is_divider(body) {
        return Some((
            Severity::Warning,
            "ASCII section-divider comment — structural noise".to_string(),
        ));
    }
    if let Some(prefix) = NARRATOR.iter().find(|p| lower.starts_with(**p)) {
        return Some((
            Severity::Warning,
            format!("narrator comment (\"{prefix}…\") — describes what the code is, not why"),
        ));
    }
    None
}

/// A comment that is part of a license header or an obvious WHY rationale — never a tell.
fn is_license_or_why(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    [
        "copyright",
        "spdx",
        "licensed under",
        "all rights reserved",
        "(c)",
    ]
    .iter()
    .any(|m| lower.contains(m))
}

/// `Step 1:` / `Phase 2` / `Part 3` narration: the body starts with one of those words (at a word
/// boundary) and carries a digit or colon shortly after.
fn is_step_narration(lower: &str) -> bool {
    for kw in ["step", "phase", "part"] {
        if let Some(rest) = lower.strip_prefix(kw) {
            let boundary = rest
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            let has_marker = rest.chars().take(8).any(|c| c.is_ascii_digit() || c == ':');
            if boundary && has_marker {
                return true;
            }
        }
    }
    false
}

/// A pure ASCII divider: the body, whitespace removed, is ≥4 chars all drawn from the divider set.
fn is_divider(body: &str) -> bool {
    let stripped: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    // `<`/`>` are excluded so arrow comments (`# --->`) and doctest-ish `# >>>` aren't mistaken for
    // dividers (and merge markers are SLP220's job).
    stripped.len() >= 4 && stripped.chars().all(|c| "=-*~#_+".contains(c))
}

/// Word-boundary substring match (so `for now` doesn't fire inside `before nowhere`). Both inputs
/// are ASCII-lowercased; boundaries are non-`[A-Za-z0-9_]` bytes or string ends.
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    for (i, _) in haystack.match_indices(needle) {
        let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        let after = i + needle.len();
        let after_ok = after >= bytes.len() || !is_word_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Deferral / semantic-incompleteness phrases (error severity).
const DEFERRAL: &[&str] = &[
    "for now",
    "in production this would",
    "in production, this would",
    "in a real world",
    "in the real world",
    "real-world",
    "this would be",
    "would be implemented",
    "would go here",
    "in practice you would",
    // "placeholder" is heavily overloaded (HTML/form/image/SQL placeholders), so match only the
    // phrase forms that carry deferral intent — not the bare noun.
    "placeholder implementation",
    "placeholder value",
    "placeholder for now",
    "just a placeholder",
    "return a placeholder",
    "placeholder until",
    "not implemented yet",
    "left as an exercise",
    "in a real app",
    "in real code",
    "naive implementation",
    "simplified version",
];

/// Hedging / uncertainty phrases (warning severity). Includes "simplicity" rationales, which are
/// often a legitimate design note rather than an unfinished corner — kept at warning, not error.
const HEDGING: &[&str] = &[
    "for simplicity",
    "to keep it simple",
    "should work",
    "hopefully",
    "not sure if",
    "i think",
    "i guess",
    "probably",
    "might need",
    "might want",
    "replace this with your",
    "your actual",
    "adjust as needed",
    "tweak as needed",
    "or something",
    "feel free to",
];

/// Narrator-comment prefixes (warning severity). Deliberately the unambiguous "narrating the unit"
/// openers — vaguer leads like `this is the …` / `the following …` / `here we …` are dropped, since
/// they frequently begin legitimate clarifying WHY-comments.
const NARRATOR: &[&str] = &[
    "this function",
    "this method",
    "this class",
    "this module",
    "this script",
    "we are going to",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn class(body: &str) -> Option<(Severity, String)> {
        classify(body, &[])
    }

    #[test]
    fn deferral_is_error() {
        let (sev, msg) = class("compute the average for now").unwrap();
        assert_eq!(sev, Severity::Error);
        assert!(msg.contains("for now"));
        assert_eq!(
            class("in production this would call the API").unwrap().0,
            Severity::Error
        );
    }

    #[test]
    fn hedging_is_warning() {
        assert_eq!(class("this should work").unwrap().0, Severity::Warning);
        assert!(class("probably fine").is_some());
        assert!(class("replace this with your actual key").is_some());
    }

    #[test]
    fn structural_noise() {
        assert!(class("Step 1: load the data").is_some());
        assert!(class("Phase 2 - transform").is_some());
        assert!(class("==========").is_some());
        assert!(class("- - - - -").is_some());
        assert!(class("This function handles the request").is_some());
        assert!(class("we are going to load the data").is_some());
    }

    #[test]
    fn deferral_placeholder_needs_intent_phrase() {
        // Bare "placeholder" (UI/SQL/image) is NOT a tell …
        assert!(class("set the placeholder attribute on the input").is_none());
        assert!(class("render a placeholder image while loading").is_none());
        // … but the deferral phrase forms are (error).
        assert_eq!(
            class("this is a placeholder until the real impl lands")
                .unwrap()
                .0,
            Severity::Error
        );
        assert_eq!(class("just a placeholder").unwrap().0, Severity::Error);
    }

    #[test]
    fn legitimate_comments_are_clean() {
        assert!(class("because the upstream API rejects empty bodies").is_none());
        assert!(class("see RFC 2606 for reserved domains").is_none());
        assert!(class("e.g. a UTF-8 BOM").is_none());
        // "i think" inside another word must not match (word boundaries).
        assert!(class("multithink is unaffected").is_none());
        // A short divider-ish but real comment isn't a divider.
        assert!(class("x += 1").is_none());
        // Dropped over-broad phrases: a real temporary var, a clarifying WHY, an arrow comment.
        assert!(class("temporary buffer reused across calls").is_none());
        assert!(class("the following must stay sorted").is_none());
        assert!(class("this is the fast path, the slow one is below").is_none());
        assert!(class("---> see the helper below").is_none());
        // Dropped narrator over-matches: "this code" / "we will" begin legitimate WHY-notes.
        assert!(class("this code path is hot, avoid allocations").is_none());
        assert!(class("we will need to revisit when load increases").is_none());
    }

    #[test]
    fn extra_phrases_extend_hedging() {
        assert!(class("revisit later").is_none());
        let extra = vec!["revisit later".to_string()];
        assert!(classify("we should revisit later", &extra).is_some());
    }
}
