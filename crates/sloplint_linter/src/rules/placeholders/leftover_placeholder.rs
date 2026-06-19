//! SLP100: leftover template placeholders.
//!
//! Fill-in-the-blank scaffolding ships with literal placeholder strings a developer is meant
//! to replace — `your_api_key_here`, `# Insert your code here`, `# Replace with your ...`.
//! When that scaffolding is pasted and committed unchanged, the placeholder survives into
//! production: `your_api_key_here` as a real credential, an "add error handling here" comment
//! where there is none. The leftover string is a high-precision marker that the code was
//! never finished.
//!
//! This rule deliberately leads with the *verbatim template phrases*, never the bare tags
//! `TODO`/`FIXME`/`XXX` — those are Ruff's `flake8-todos` (`TD*`) / `flake8-fixme` (`FIX*`)
//! territory, and re-flagging them would just duplicate Ruff. It also self-exempts test,
//! example, and docs files, where placeholder-looking strings are legitimate sample data.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TokenKind};

use crate::lint::{FileContext, Rule};

pub struct LeftoverPlaceholder;

/// Which kind of fill-in-the-blank a phrase represents — surfaced in the message so a leaked
/// secret stub reads differently from an unimplemented body.
#[derive(Clone, Copy)]
enum Category {
    /// A stand-in for a real credential, e.g. `your_api_key_here`.
    Secret,
    /// A marker where an implementation is expected, e.g. `insert your code here`.
    Implement,
    /// A "replace this with your own ..." instruction left in place.
    Replace,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Category::Secret => "secret placeholder",
            Category::Implement => "unimplemented stub",
            Category::Replace => "replace-with-your",
        }
    }
}

/// The curated, AI-specific placeholder lexicon. Every entry is a verbatim template phrase
/// (lowercase; matched case-insensitively as a word-bounded substring — see
/// [`contains_phrase`]) that should never survive into shipped code. Deliberately excludes
/// bare `TODO`/`FIXME`/`XXX` — see the module docs.
const LEXICON: &[(&str, Category)] = &[
    // --- secret / credential placeholders ---
    // Specific, fully-qualified forms only: bare `your_api_key` is a plausible real field
    // name, so it's excluded — every entry here ends in `_here`/`here` or is an explicit
    // "insert/enter your ... key" instruction.
    ("your_api_key_here", Category::Secret),
    ("your-api-key-here", Category::Secret),
    ("your api key here", Category::Secret),
    ("api_key_here", Category::Secret),
    ("your_secret_key_here", Category::Secret),
    ("your_secret_here", Category::Secret),
    ("your_client_secret_here", Category::Secret),
    ("your_token_here", Category::Secret),
    ("your-token-here", Category::Secret),
    ("your token here", Category::Secret),
    ("your_access_token_here", Category::Secret),
    ("your_password_here", Category::Secret),
    ("insert_your_api_key", Category::Secret),
    ("enter_your_api_key", Category::Secret),
    ("put_your_api_key_here", Category::Secret),
    // --- "implementation goes here" markers ---
    ("insert your code here", Category::Implement),
    ("insert code here", Category::Implement),
    ("your code here", Category::Implement),
    ("add your code here", Category::Implement),
    ("write your code here", Category::Implement),
    ("code goes here", Category::Implement),
    ("implementation goes here", Category::Implement),
    ("your implementation here", Category::Implement),
    ("add your implementation here", Category::Implement),
    ("add error handling here", Category::Implement),
    ("add logic here", Category::Implement),
    ("your logic here", Category::Implement),
    ("rest of your code here", Category::Implement),
    ("implement me", Category::Implement),
    // --- "replace this with your own ..." instructions ---
    // Bare "replace with your" is excluded: it also appears in legitimate docs prose
    // ("replace with your preferred backend"). The templated forms below are unambiguous.
    ("replace_with_your", Category::Replace),
    ("replace this with your", Category::Replace),
    ("replace with the actual", Category::Replace),
    ("replace this with the actual", Category::Replace),
];

impl Rule for LeftoverPlaceholder {
    fn code(&self) -> &'static str {
        "SLP100"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        // Test/example/docs files carry placeholder-looking sample data by design.
        if is_exempt_path(ctx.path) {
            return;
        }
        // Lowercase the user's extra phrases once, not once per token.
        let extra: Vec<String> = ctx
            .placeholders
            .iter()
            .map(|phrase| phrase.to_ascii_lowercase())
            .filter(|phrase| !phrase.is_empty())
            .collect();

        for token in ctx.parsed.tokens().iter() {
            if !is_text_token(token.kind()) {
                continue;
            }
            let lower = ctx.source[token.range()].to_ascii_lowercase();
            if let Some((phrase, label)) = match_placeholder(&lower, &extra) {
                diagnostics.push(Diagnostic::new(
                    "SLP100",
                    format!(
                        "leftover template placeholder ({label}): `{phrase}` — finish or remove it"
                    ),
                    token.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Comments and string literals (including the text spans of f-strings) — the tokens where
/// fill-in-the-blank residue lands. Identifiers are intentionally not scanned: the residue is
/// template *text*, and scanning names would invite false positives on ordinary code.
fn is_text_token(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Comment | TokenKind::String | TokenKind::FStringMiddle
    )
}

/// The first placeholder phrase found in `lower` (already lowercased), built-in lexicon
/// before user `extra`. Returns the matched phrase and its category label.
fn match_placeholder(lower: &str, extra: &[String]) -> Option<(String, &'static str)> {
    for (phrase, category) in LEXICON {
        if contains_phrase(lower, phrase) {
            return Some((phrase.to_string(), category.label()));
        }
    }
    for phrase in extra {
        if contains_phrase(lower, phrase) {
            return Some((phrase.clone(), "configured placeholder"));
        }
    }
    None
}

/// Whether `haystack` contains `needle` bounded by non-word characters (or the ends of the
/// string) on both sides — both already lowercased. The boundary check is what keeps
/// `implement me` from firing inside `implement memoization`: a plain substring search would
/// match, so every phrase is required to stand as its own run of word characters.
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bytes = haystack.as_bytes();
    // `match_indices` yields byte offsets on valid char boundaries, so this is UTF-8 safe
    // even when comments/strings contain non-ASCII text.
    for (start, matched) in haystack.match_indices(needle) {
        let end = start + matched.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// A word character for boundary purposes: ASCII alphanumeric or underscore. (Phrases and the
/// source we scan are ASCII-relevant here; non-ASCII bytes count as boundaries, which is
/// fine — a placeholder phrase never abuts one meaningfully.)
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Whether `path` is a test, example, or docs file — where placeholder-looking strings are
/// legitimate sample data, not residue. Conservative on two axes: it keys off the *plural*
/// directory names (`tests`/`examples`/`docs`) so a lone `test`/`doc` segment (as in
/// `resources/test/...`, or a real `doc/` package) doesn't blanket-exempt source, and it
/// honors the conventional `test_*`/`*_test.py`/`conftest.py` filenames. Windows `\` is
/// normalized; matching is case-insensitive.
fn is_exempt_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if file.starts_with("test_") || file.ends_with("_test.py") || file == "conftest.py" {
        return true;
    }
    lower
        .split('/')
        .any(|segment| matches!(segment, "tests" | "examples" | "docs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    /// Run SLP100 over `source` at `path` and return the finding count.
    fn findings(path: &str, source: &str, extra: &[String]) -> usize {
        let parsed = parse(source).expect("source must parse");
        let ctx = FileContext {
            path,
            source,
            parsed: &parsed,
            limits: Default::default(),
            placeholders: extra,
        };
        let mut diagnostics = Vec::new();
        LeftoverPlaceholder.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    #[test]
    fn flags_secret_placeholder_in_a_string() {
        assert_eq!(
            findings("src/app.py", "api_key = \"your_api_key_here\"\n", &[]),
            1
        );
    }

    #[test]
    fn flags_implement_and_replace_in_comments() {
        assert_eq!(
            findings("src/a.py", "# Insert your code here\nx = 1\n", &[]),
            1
        );
        assert_eq!(
            findings("src/a.py", "# Replace this with your handler\nx = 1\n", &[]),
            1
        );
    }

    #[test]
    fn phrase_matching_respects_word_boundaries() {
        // Regression (reviewer): `implement me` must NOT match `implement memoization` /
        // `implement memory` — only the standalone phrase.
        assert_eq!(
            findings(
                "src/a.py",
                "# helpers to implement memoization\nx = 1\n",
                &[]
            ),
            0
        );
        assert_eq!(
            findings("src/a.py", "# implement memory limits later\nx = 1\n", &[]),
            0
        );
        assert_eq!(findings("src/a.py", "# implement me\nx = 1\n", &[]), 1);
    }

    #[test]
    fn dropped_broad_phrases_do_not_fire() {
        // These over-broad forms were removed for precision; ensure they stay silent.
        assert_eq!(
            findings(
                "src/a.py",
                "# replace with your preferred backend\nx = 1\n",
                &[]
            ),
            0
        );
        assert_eq!(
            findings(
                "src/a.py",
                "doc = \"pass your_api_key to authenticate\"\n",
                &[]
            ),
            0,
            "bare your_api_key (no _here) is a plausible field name, not residue"
        );
    }

    #[test]
    fn flags_placeholder_inside_fstring() {
        assert_eq!(
            findings("src/a.py", "msg = f\"token is your_token_here now\"\n", &[]),
            1
        );
    }

    #[test]
    fn bare_todo_is_not_flagged() {
        // Ruff's TD*/FIX* territory — must not overlap.
        assert_eq!(
            findings("src/a.py", "# TODO: refactor this later\nx = 1\n", &[]),
            0
        );
        assert_eq!(findings("src/a.py", "# FIXME the parser\nx = 1\n", &[]), 0);
    }

    #[test]
    fn ordinary_prose_and_strings_are_not_flagged() {
        assert_eq!(
            findings(
                "src/a.py",
                "# compute the running total\ntotal = a + b\n",
                &[]
            ),
            0
        );
        assert_eq!(
            findings("src/a.py", "greeting = \"Welcome to your account\"\n", &[]),
            0
        );
    }

    #[test]
    fn test_and_example_and_docs_paths_are_exempt() {
        let src = "api_key = \"your_api_key_here\"\n";
        assert_eq!(findings("tests/test_client.py", src, &[]), 0);
        assert_eq!(findings("tests/data/sample.py", src, &[]), 0);
        assert_eq!(findings("examples/quickstart.py", src, &[]), 0);
        assert_eq!(findings("docs/snippets/demo.py", src, &[]), 0);
        assert_eq!(findings("pkg/conftest.py", src, &[]), 0);
        // A lone `test` directory segment (as in our own fixture path) is NOT exempt.
        assert_eq!(findings("resources/test/fixtures/x.py", src, &[]), 1);
    }

    #[test]
    fn user_extra_phrases_extend_the_lexicon() {
        let extra = vec!["fill_me_in".to_string()];
        assert_eq!(findings("src/a.py", "value = \"FILL_ME_IN\"\n", &extra), 1);
        // Without the config entry, the same string is clean.
        assert_eq!(findings("src/a.py", "value = \"FILL_ME_IN\"\n", &[]), 0);
    }

    #[test]
    fn one_finding_per_token_even_with_multiple_matches() {
        // The comment matches both "insert your code here" and "your code here": one finding.
        assert_eq!(
            findings("src/a.py", "# Insert your code here please\nx = 1\n", &[]),
            1
        );
    }

    #[test]
    fn empty_extra_phrase_does_not_flag_everything() {
        // A blank config entry must be ignored, not matched against every token.
        let extra = vec![String::new()];
        assert_eq!(
            findings(
                "src/a.py",
                "# a normal comment\nvalue = \"plain text\"\n",
                &extra
            ),
            0
        );
    }
}
