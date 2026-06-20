//! Inline `# sloplint: allow <CODE> [reason]` suppression (#94).
//!
//! sloplint disallows duplication — and everything else — by default; this is the honest
//! per-site opt-out. A trailing or standalone comment acknowledges a specific finding at the
//! construct it sits on, the maintainer's "I understand, and here's why." It mirrors Ruff's
//! `# noqa`, but is a *general* mechanism that works for every rule, and the optional free-text
//! reason is encouraged (it is parsed only to delimit the codes — never surfaced as an error).
//!
//! ```text
//! def request(self, ...):   # sloplint: allow SLP020  (sync/async mirror of AsyncClient.request)
//!     ...
//! ```
//!
//! Forms:
//! - `# sloplint: allow SLP020` — suppress SLP020 on this construct.
//! - `# sloplint: allow SLP020, SLP030 reason text` — several codes, then a reason.
//! - `# sloplint: allow` (bare) — suppress *every* rule on this construct.
//!
//! A directive suppresses a finding when the directive's line falls within the finding's line
//! span (start line through end line, inclusive). The obvious placement is the `def`/statement
//! line; because a whole-tree finding like SLP020 spans an entire function, the directive may sit
//! anywhere inside that function. A clone is reported at *each* end independently, so silencing a
//! whole pair takes a directive in both functions — by design (acknowledge each site).

use sloplint_diagnostics::Diagnostic;
use sloplint_python::ast::ModModule;
use sloplint_python::parser::Parsed;
use sloplint_python::{LineIndex, Ranged, TextSize, TokenKind};

/// The rule codes a single directive acknowledges.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Allow {
    /// Bare `allow` (or `allow` with only a reason) — every rule.
    All,
    /// Specific rule codes, matched exactly.
    Codes(Vec<String>),
}

impl Allow {
    fn allows(&self, code: &str) -> bool {
        match self {
            Allow::All => true,
            Allow::Codes(codes) => codes.iter().any(|c| c == code),
        }
    }
}

/// A parsed inline directive and the 1-based line it sits on. The optional reason is parsed only
/// to delimit the codes (see [`parse_codes_and_reason`]) and then dropped — it is encouraged for
/// the human reader but, by design, surfaced nowhere as an error or in output.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Directive {
    line: usize,
    allow: Allow,
}

/// Every inline `# sloplint: allow` directive in one file, plus the line index used to map a
/// finding's byte range to its line span.
pub struct Suppressions {
    directives: Vec<Directive>,
    line_index: LineIndex,
}

impl Suppressions {
    /// A suppressor with no directives — suppresses nothing. Handy for files with no inline
    /// allows and as a default in tests.
    pub fn empty() -> Self {
        Self {
            directives: Vec::new(),
            line_index: LineIndex::from_source_text(""),
        }
    }

    /// Parse every `# sloplint: allow ...` comment in the file. Reuses Ruff's token stream
    /// (`TokenKind::Comment`) so a `#` inside a string is never mistaken for a comment, and
    /// Ruff's [`LineIndex`] for the byte-offset → line mapping.
    pub fn parse(source: &str, parsed: &Parsed<ModModule>) -> Self {
        let line_index = LineIndex::from_source_text(source);
        let mut directives = Vec::new();
        for token in parsed.tokens().iter() {
            if token.kind() != TokenKind::Comment {
                continue;
            }
            if let Some((allow, _reason)) = parse_directive(&source[token.range()]) {
                let line = line_index.line_index(token.range().start()).get();
                directives.push(Directive { line, allow });
            }
        }
        Self {
            directives,
            line_index,
        }
    }

    /// Drop every suppressed finding in place, returning how many were removed.
    pub fn filter(&self, diagnostics: &mut Vec<Diagnostic>) -> usize {
        if self.directives.is_empty() {
            return 0;
        }
        let before = diagnostics.len();
        diagnostics.retain(|diagnostic| !self.is_suppressed(diagnostic));
        before - diagnostics.len()
    }

    fn is_suppressed(&self, diagnostic: &Diagnostic) -> bool {
        let start = self.line_of(diagnostic.range.start());
        // `range.end()` is exclusive: step back one byte so a span ending exactly at a line break
        // doesn't claim the following line. Empty/one-byte ranges keep the start line.
        let end_byte = u32::from(diagnostic.range.end());
        let start_byte = u32::from(diagnostic.range.start());
        let end_offset = if end_byte > start_byte {
            TextSize::from(end_byte - 1)
        } else {
            diagnostic.range.start()
        };
        let end = self.line_of(end_offset);
        self.directives
            .iter()
            .any(|d| d.line >= start && d.line <= end && d.allow.allows(&diagnostic.code))
    }

    fn line_of(&self, offset: TextSize) -> usize {
        self.line_index.line_index(offset).get()
    }
}

/// Parse a comment's raw text (leading `#` included) into `(allowed codes, reason)`, or `None`
/// when it isn't a `# sloplint: allow` directive.
fn parse_directive(comment: &str) -> Option<(Allow, Option<String>)> {
    let body = comment.trim_start_matches('#').trim_start();
    let rest = body.strip_prefix("sloplint:")?.trim_start();
    let after = rest.strip_prefix("allow")?;
    // `allow` must be a whole word: end of comment or a following separator — so `allowlist`
    // (or any word merely starting with "allow") is not a directive.
    match after.chars().next() {
        None => Some((Allow::All, None)),
        Some(c) if c.is_whitespace() => Some(parse_codes_and_reason(after.trim_start())),
        Some(_) => None,
    }
}

/// Split the text after `allow` into a leading run of rule codes and a trailing free-text reason.
/// A "code" is an all-uppercase-and-digit word with at least one letter and one digit (`SLP020`);
/// the reason is everything from the first word that isn't a code. No codes ⇒ [`Allow::All`].
fn parse_codes_and_reason(rest: &str) -> (Allow, Option<String>) {
    let mut codes = Vec::new();
    let mut chars = rest.char_indices().peekable();
    let mut reason: Option<String> = None;

    loop {
        // Skip separators between codes: whitespace and commas.
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(word_start, _)) = chars.peek() else {
            break;
        };
        // Read a word up to the next separator (without consuming it).
        let mut word_end = rest.len();
        while let Some(&(idx, c)) = chars.peek() {
            if c.is_whitespace() || c == ',' {
                word_end = idx;
                break;
            }
            chars.next();
        }
        let word = &rest[word_start..word_end];
        if is_code(word) {
            codes.push(word.to_string());
        } else {
            // First non-code word begins the reason — keep the original text verbatim.
            reason = Some(rest[word_start..].trim().to_string());
            break;
        }
    }

    let allow = if codes.is_empty() {
        Allow::All
    } else {
        Allow::Codes(codes)
    };
    (allow, reason)
}

/// A rule code: ASCII uppercase letters and digits only, with at least one of each (e.g. `SLP020`).
fn is_code(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && word.bytes().any(|b| b.is_ascii_uppercase())
        && word.bytes().any(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_diagnostics::Severity;
    use sloplint_python::{parse, TextRange};

    fn directive(comment: &str) -> Option<(Allow, Option<String>)> {
        parse_directive(comment)
    }

    #[test]
    fn parses_the_directive_forms() {
        // Single code, no reason.
        assert_eq!(
            directive("# sloplint: allow SLP020"),
            Some((Allow::Codes(vec!["SLP020".into()]), None))
        );
        // Code + reason; the reason is preserved verbatim.
        assert_eq!(
            directive("# sloplint: allow SLP020  (sync/async mirror)"),
            Some((
                Allow::Codes(vec!["SLP020".into()]),
                Some("(sync/async mirror)".into())
            ))
        );
        // Several codes, comma- and/or space-separated, then a reason.
        assert_eq!(
            directive("# sloplint: allow SLP020, SLP030 intentional here"),
            Some((
                Allow::Codes(vec!["SLP020".into(), "SLP030".into()]),
                Some("intentional here".into())
            ))
        );
        // Bare allow = every rule.
        assert_eq!(directive("# sloplint: allow"), Some((Allow::All, None)));
        // Allow with only a reason (no code) is still "all".
        assert_eq!(
            directive("# sloplint: allow generated file"),
            Some((Allow::All, Some("generated file".into())))
        );
    }

    #[test]
    fn rejects_non_directives() {
        assert_eq!(directive("# a normal comment"), None);
        assert_eq!(directive("# sloplint: ignore SLP020"), None); // wrong verb
        assert_eq!(directive("# sloplint: allowlist everything"), None); // not a whole word
        assert_eq!(directive("# noqa: F401"), None);
    }

    fn diag(code: &str, range: std::ops::Range<u32>) -> Diagnostic {
        Diagnostic::new(
            code,
            "msg",
            TextRange::new(TextSize::from(range.start), TextSize::from(range.end)),
            Severity::Warning,
        )
    }

    fn suppressions(source: &str) -> Suppressions {
        Suppressions::parse(source, &parse(source).expect("fixture parses"))
    }

    #[test]
    fn suppresses_only_the_named_code_on_the_directive_line() {
        // `x = 1  # sloplint: allow SLP030` — directive on line 1.
        let source = "x = 1  # sloplint: allow SLP030\ny = 2\n";
        let supp = suppressions(source);

        let mut on_line_1 = vec![diag("SLP030", 0..5), diag("SLP010", 0..5)];
        let removed = supp.filter(&mut on_line_1);
        assert_eq!(removed, 1, "only the named SLP030 is dropped");
        assert_eq!(on_line_1.len(), 1);
        assert_eq!(on_line_1[0].code, "SLP010", "an unnamed code survives");

        // A finding on line 2 is untouched even though it shares the code.
        let mut on_line_2 = vec![diag("SLP030", 32..37)];
        assert_eq!(supp.filter(&mut on_line_2), 0, "different line, not suppressed");
    }

    #[test]
    fn bare_allow_suppresses_every_code_on_the_line() {
        let source = "x = 1  # sloplint: allow\n";
        let supp = suppressions(source);
        let mut diags = vec![diag("SLP030", 0..5), diag("SLP010", 0..5)];
        assert_eq!(supp.filter(&mut diags), 2);
        assert!(diags.is_empty());
    }

    #[test]
    fn directive_inside_a_multi_line_construct_suppresses_the_whole_span() {
        // The finding spans both lines; the directive sits on the second. A whole-tree finding
        // (SLP020) whose range is an entire function is suppressed by a directive anywhere inside.
        let source = "def f():\n    return 1  # sloplint: allow SLP020\n";
        let supp = suppressions(source);
        let whole_function = 0..u32::try_from(source.len()).unwrap();
        let mut diags = vec![diag("SLP020", whole_function)];
        assert_eq!(supp.filter(&mut diags), 1);
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_directive() {
        // Token-based parsing means this `#` is part of a string literal, not a comment.
        let source = "x = \"# sloplint: allow SLP030\"\n";
        let supp = suppressions(source);
        let mut diags = vec![diag("SLP030", 0..1)];
        assert_eq!(supp.filter(&mut diags), 0, "string content is not a directive");
    }
}
