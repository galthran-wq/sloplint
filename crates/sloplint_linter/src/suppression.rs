//! Inline suppression via Ruff's `# noqa` convention (#94).
//!
//! sloplint mirrors Ruff's select/ignore model and runs right next to Ruff, so suppression uses
//! Ruff's familiar `# noqa` — nothing bespoke. Duplication, and every rule, is disallowed by
//! default; a `# noqa` is the maintainer's explicit, per-site acknowledgement: the honest "I
//! understand, and here's why."
//!
//! Line-level only, exactly like Ruff (broad/file/directory suppression stays in config — `ignore`
//! and per-path `overrides`):
//! - `# noqa` — suppress every sloplint rule on that line.
//! - `# noqa: SLP020` — suppress just that code; `# noqa: SLP020, SLP082` for several.
//! - A trailing reason is just a normal comment: `# noqa: SLP020  (sync/async mirror)`.
//!
//! A `# noqa` matches a finding whose **reported line** (its range start — the `line:col` shown in
//! output) equals the comment's line, exactly the line Ruff scopes a `# noqa` to. A whole-tree
//! clone (SLP020) is reported at each end, so silencing a whole pair takes a `# noqa` at each end.
//!
//! Interop: Ruff reads the *same* `# noqa` comments. Since `SLP*` aren't Ruff codes, set
//! `external = ["SLP"]` in your Ruff config so RUF100 (unused-noqa) preserves them. Symmetrically,
//! sloplint only looks at its own `SLP*` codes here and never reports on Ruff directives like
//! `# noqa: E501`.

use sloplint_diagnostics::Diagnostic;
use sloplint_python::ast::ModModule;
use sloplint_python::parser::Parsed;
use sloplint_python::{LineIndex, Ranged, TokenKind};

/// The codes a `# noqa` acknowledges.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Codes {
    /// Blanket `# noqa` (no codes) — every rule.
    All,
    /// `# noqa: SLP020[, ...]` — specific codes, matched exactly.
    Only(Vec<String>),
}

impl Codes {
    fn allows(&self, code: &str) -> bool {
        match self {
            Codes::All => true,
            Codes::Only(codes) => codes.iter().any(|c| c == code),
        }
    }
}

/// Every inline `# noqa` in one file, each paired with the 1-based line it sits on, plus the line
/// index used to map a finding's reported (range-start) byte offset to its line.
pub struct Suppressions {
    directives: Vec<(usize, Codes)>,
    line_index: LineIndex,
}

impl Suppressions {
    /// A suppressor with no directives — suppresses nothing. Handy for files with no `# noqa` and
    /// as a default in tests.
    pub fn empty() -> Self {
        Self {
            directives: Vec::new(),
            line_index: LineIndex::from_source_text(""),
        }
    }

    /// Parse every `# noqa` comment in the file. Reuses Ruff's token stream (`TokenKind::Comment`)
    /// so a `#` inside a string is never mistaken for a comment, and Ruff's [`LineIndex`] for the
    /// byte-offset → line mapping.
    pub fn parse(source: &str, parsed: &Parsed<ModModule>) -> Self {
        let line_index = LineIndex::from_source_text(source);
        let mut directives = Vec::new();
        for token in parsed.tokens().iter() {
            if token.kind() != TokenKind::Comment {
                continue;
            }
            if let Some(codes) = parse_noqa(&source[token.range()]) {
                let line = line_index.line_index(token.range().start()).get();
                directives.push((line, codes));
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
        let reported_line = self.line_index.line_index(diagnostic.range.start()).get();
        self.directives
            .iter()
            .any(|(line, codes)| *line == reported_line && codes.allows(&diagnostic.code))
    }
}

/// Parse a `# noqa[: CODES]` directive from a comment's raw text, or `None` if it isn't one.
fn parse_noqa(comment: &str) -> Option<Codes> {
    let body = comment.trim_start_matches('#').trim_start();
    let rest = body.strip_prefix("noqa")?;
    // Interpret the text immediately after the `noqa` keyword, following Ruff:
    // - end of comment, whitespace, or other punctuation → blanket `Codes::All`;
    // - `: CODES [reason]` → those codes (an empty list ⇒ blanket, as in Ruff);
    // - an identifier char (`noqaX`) → not a directive (word boundary).
    match rest.chars().next() {
        None => Some(Codes::All),
        Some(':') => Some(parse_codes(rest[1..].trim_start())),
        Some(c) if c.is_alphanumeric() || c == '_' => None,
        Some(_) => Some(Codes::All),
    }
}

/// Read the leading run of rule codes from `# noqa:`'s tail, stopping at the first word that isn't
/// a code (the rest is a free-text reason). A "code" is all-uppercase-and-digits with at least one
/// of each (`SLP020`). No codes ⇒ blanket [`Codes::All`].
fn parse_codes(rest: &str) -> Codes {
    let mut codes = Vec::new();
    let mut chars = rest.char_indices().peekable();

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
        if is_code(&rest[word_start..word_end]) {
            codes.push(rest[word_start..word_end].to_string());
        } else {
            // First non-code word begins the reason — stop reading codes.
            break;
        }
    }

    if codes.is_empty() {
        Codes::All
    } else {
        Codes::Only(codes)
    }
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
    use sloplint_python::{parse, TextRange, TextSize};

    #[test]
    fn parses_noqa_forms() {
        assert_eq!(parse_noqa("# noqa"), Some(Codes::All));
        assert_eq!(
            parse_noqa("# noqa: SLP020"),
            Some(Codes::Only(vec!["SLP020".into()]))
        );
        assert_eq!(
            parse_noqa("# noqa: SLP020, SLP082"),
            Some(Codes::Only(vec!["SLP020".into(), "SLP082".into()]))
        );
        // Trailing reason after the codes is ignored.
        assert_eq!(
            parse_noqa("# noqa: SLP020  (sync/async mirror)"),
            Some(Codes::Only(vec!["SLP020".into()]))
        );
        // Blanket noqa with a trailing reason.
        assert_eq!(parse_noqa("# noqa  generated"), Some(Codes::All));
        // A Ruff code parses fine (and will simply match no SLP finding).
        assert_eq!(
            parse_noqa("# noqa: E501"),
            Some(Codes::Only(vec!["E501".into()]))
        );
    }

    #[test]
    fn rejects_non_directives() {
        assert_eq!(parse_noqa("# a normal comment"), None);
        assert_eq!(parse_noqa("# noqaX still prose"), None); // word boundary
        assert_eq!(parse_noqa("# type: ignore"), None);
        assert_eq!(parse_noqa("# ruff: noqa"), None); // Ruff's own directive, not a bare noqa
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
    fn noqa_suppresses_only_the_named_code_on_the_reported_line() {
        // `x = 1  # noqa: SLP030` — directive on line 1.
        let source = "x = 1  # noqa: SLP030\ny = 2\n";
        let supp = suppressions(source);

        let mut on_line_1 = vec![diag("SLP030", 0..5), diag("SLP010", 0..5)];
        assert_eq!(
            supp.filter(&mut on_line_1),
            1,
            "only the named SLP030 is dropped"
        );
        assert_eq!(on_line_1[0].code, "SLP010", "an unnamed code survives");

        // A finding on line 2 is untouched even though it shares the code.
        let mut on_line_2 = vec![diag("SLP030", 22..27)];
        assert_eq!(
            supp.filter(&mut on_line_2),
            0,
            "different line, not suppressed"
        );
    }

    #[test]
    fn blanket_noqa_suppresses_every_code_on_the_line() {
        let supp = suppressions("x = 1  # noqa\n");
        let mut diags = vec![diag("SLP030", 0..5), diag("SLP010", 0..5)];
        assert_eq!(supp.filter(&mut diags), 2);
        assert!(diags.is_empty());
    }

    #[test]
    fn noqa_matches_the_reported_line_of_a_multi_line_finding() {
        // A whole-function finding (SLP020) is reported at its first line; a `# noqa` there clears
        // it, but a `# noqa` deeper in the body does not (Ruff scopes noqa to the reported line).
        let on_def = "def f():  # noqa: SLP020\n    return 1\n";
        let supp = suppressions(on_def);
        let mut diags = vec![diag("SLP020", 0..u32::try_from(on_def.len()).unwrap())];
        assert_eq!(
            supp.filter(&mut diags),
            1,
            "noqa on the reported line clears it"
        );

        let in_body = "def f():\n    return 1  # noqa: SLP020\n";
        let supp = suppressions(in_body);
        let mut diags = vec![diag("SLP020", 0..u32::try_from(in_body.len()).unwrap())];
        assert_eq!(
            supp.filter(&mut diags),
            0,
            "noqa off the reported line does not match"
        );
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_directive() {
        let supp = suppressions("x = \"# noqa: SLP030\"\n");
        let mut diags = vec![diag("SLP030", 0..1)];
        assert_eq!(
            supp.filter(&mut diags),
            0,
            "string content is not a directive"
        );
    }
}
