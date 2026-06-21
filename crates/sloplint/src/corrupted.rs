//! SLP220: corrupted / truncated LLM output leaked into a `.py` file.
//!
//! Three near-zero-ambiguity signals that an agent's output was pasted/written back imperfectly:
//! 1. **Leftover markdown / scaffolding artifacts** — a ```` ``` ```` fence, a merge-conflict
//!    marker, or a stray `<file …>` / closing tag sitting in executable space.
//! 2. **Syntax-error-as-finding** — a `.py` file that doesn't parse is reported as a corrupted/
//!    truncated paste rather than silently skipped. In Python a bare fence/marker/tag *cannot* be
//!    valid syntax, so when the file fails to parse we classify the wreckage from the raw text.
//! 3. **Prose density** — a high fraction of natural-language lines (no code punctuation, several
//!    words) means a pasted explanation rather than code.
//!
//! This lives in the CLI, not the registry, because the headline case (a file that fails to parse)
//! never reaches the registry rules — they only run on a parsed tree. On a file that *does* parse,
//! artifact markers are matched only **outside** string/comment tokens, so a Markdown code block
//! inside a docstring is never flagged.

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::ModModule;
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange, TextSize, TokenKind};

/// Minimum code-ish lines before the prose-density ratio is judged — tiny files are too noisy.
const PROSE_MIN_LINES: usize = 8;

/// A leftover scaffolding artifact and how to describe it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Artifact {
    /// A Markdown code fence (```` ``` ````, ```` ```python ````).
    Fence,
    /// A git merge-conflict marker (`<<<<<<<`, `=======`, `>>>>>>>`, `|||||||`).
    Conflict,
    /// A stray XML/HTML-ish scaffolding tag (`<file path=…>`, `</file>`).
    Tag,
}

impl Artifact {
    fn describe(self) -> &'static str {
        match self {
            Artifact::Fence => "a leftover Markdown code fence (```)",
            Artifact::Conflict => "a merge-conflict marker",
            Artifact::Tag => "a stray scaffolding tag (e.g. `<file …>`)",
        }
    }
}

/// Build an SLP220 diagnostic for a file that **failed to parse**: classify the wreckage from the
/// raw text (artifact marker, else prose-heavy, else generic truncation) so the message says *why*
/// it looks like corrupted AI output rather than just "syntax error".
pub fn on_parse_error(source: &str, prose_ratio: f64) -> Diagnostic {
    if let Some((range, artifact)) = find_artifact(source, None) {
        return diag(
            range,
            format!(
                "{} in a Python file that does not parse — corrupted or truncated AI output",
                cap(artifact.describe())
            ),
        );
    }
    // No token stream on an unparseable file, so prose counting can't mask string content — a
    // truncated file whose tail is a big English docstring may read as "prose"; acceptable, since
    // it's corrupt either way and the alternative message is the generic truncation one below.
    let (prose, total) = prose_stats(source, None);
    if total >= PROSE_MIN_LINES && prose as f64 / total as f64 >= prose_ratio {
        return diag(
            first_line(source),
            format!(
                "file does not parse and {prose}/{total} lines look like natural-language prose — \
                 likely pasted LLM explanation, not code"
            ),
        );
    }
    diag(
        first_line(source),
        "file does not parse — likely truncated or corrupted AI output".to_string(),
    )
}

/// Scan a file that **parsed** for the same signals: artifact markers outside string/comment tokens
/// (a fence inside a docstring is content, not an artifact), and prose density over code lines.
pub fn scan_parsed(source: &str, parsed: &Parsed<ModModule>, prose_ratio: f64) -> Vec<Diagnostic> {
    let masked = masked_lines(source, parsed);
    let mut out = Vec::new();
    if let Some((range, artifact)) = find_artifact(source, Some(&masked)) {
        out.push(diag(
            range,
            format!(
                "{} outside any string or comment — corrupted or truncated AI output",
                cap(artifact.describe())
            ),
        ));
    }
    let (prose, total) = prose_stats(source, Some(&masked));
    if total >= PROSE_MIN_LINES && prose as f64 / total as f64 >= prose_ratio {
        out.push(diag(
            first_line(source),
            format!(
                "{prose}/{total} code lines look like natural-language prose — likely pasted LLM \
                 explanation left in executable space"
            ),
        ));
    }
    out
}

fn diag(range: TextRange, message: String) -> Diagnostic {
    Diagnostic::new("SLP220", message, range, Severity::Warning)
}

/// Capitalize the first ASCII letter of a description for sentence start.
fn cap(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The range of the first physical line (for findings with no specific marker location).
fn first_line(source: &str) -> TextRange {
    let end = source.find('\n').unwrap_or(source.len());
    TextRange::new(TextSize::from(0), TextSize::from(end as u32))
}

/// The first artifact marker line, with its range. `masked` (when given) holds 0-based line indices
/// covered by string/comment tokens, which are skipped — a fence/marker inside a docstring is
/// legitimate content, not corrupted output.
fn find_artifact(source: &str, masked: Option<&HashSet<usize>>) -> Option<(TextRange, Artifact)> {
    for (index, range, text) in lines(source) {
        if masked.is_some_and(|m| m.contains(&index)) {
            continue;
        }
        if let Some(artifact) = classify_line(text) {
            return Some((range, artifact));
        }
    }
    None
}

/// Classify a single line's trimmed text as an artifact marker, if it is one.
fn classify_line(text: &str) -> Option<Artifact> {
    let trimmed = text.trim();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        return Some(Artifact::Fence);
    }
    // Conflict markers: a run of >= 7 of the marker char (git uses exactly 7). `=======` alone is
    // ambiguous (reST underline), so the unambiguous `<<<`/`>>>`/`|||` markers carry it.
    for marker in ["<<<<<<<", ">>>>>>>", "|||||||"] {
        if trimmed.starts_with(marker) {
            return Some(Artifact::Conflict);
        }
    }
    if looks_like_tag(trimmed) {
        return Some(Artifact::Tag);
    }
    None
}

/// A whole-line XML/HTML-ish tag: `<file path="…">`, `</file>`, `<source>` — the scaffolding
/// wrappers agents emit. Requires the line to be *only* the tag (`<…>`) so an in-code comparison
/// like `a < b` (which has no trailing `>`) or `x > 0` is never matched.
fn looks_like_tag(trimmed: &str) -> bool {
    let bytes = trimmed.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'<' || !trimmed.ends_with('>') {
        return false;
    }
    // Second char starts a tag name or a closing slash.
    let second = bytes[1];
    (second == b'/' || second.is_ascii_alphabetic())
        // Reject comparison/shift operators that happen to end in '>' is impossible here (must end
        // with '>'); also require an alphabetic tag name somewhere.
        && trimmed[1..].chars().any(|c| c.is_ascii_alphabetic())
}

/// `(prose_lines, total_code_ish_lines)`. A code-ish line is non-blank, not a `#` comment, and
/// (when `masked` is given) not inside a string token. A prose line additionally has no code
/// punctuation and >= 4 words — natural language, not Python.
fn prose_stats(source: &str, masked: Option<&HashSet<usize>>) -> (usize, usize) {
    let mut prose = 0;
    let mut total = 0;
    for (index, _range, text) in lines(source) {
        if masked.is_some_and(|m| m.contains(&index)) {
            continue;
        }
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        total += 1;
        if is_prose_line(trimmed) {
            prose += 1;
        }
    }
    (prose, total)
}

/// Whether a trimmed line reads as natural-language prose: no code punctuation, several words, and
/// not led by a Python statement keyword. Most code carries `=`/`(`/`:` punctuation, but a handful
/// of keyword statements don't — `assert user is not None`, `raise Error from cause`, `from x import
/// y` — and have ≥4 "words"; keying out a leading keyword keeps those (common in real test/validation
/// code) from reading as prose, while genuine sentences ("Here is how this works") still count.
fn is_prose_line(trimmed: &str) -> bool {
    const CODE_PUNCT: &str = "=(){}[]:;@%<>/\\|*+~`\"";
    if trimmed.chars().any(|c| CODE_PUNCT.contains(c)) {
        return false;
    }
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() < 4 {
        return false;
    }
    !PYTHON_STMT_KEYWORDS.contains(&words[0])
}

/// Statement-leading Python keywords. A code line beginning with one of these (`assert`, `raise`,
/// `from`, …) is a statement, not prose, even when it carries no code punctuation.
const PYTHON_STMT_KEYWORDS: &[&str] = &[
    "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else",
    "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal",
    "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "and",
];

/// 0-based line indices covered by string-literal or comment tokens — the ranges where a fence or
/// prose-like text is legitimate content, not a leaked artifact.
fn masked_lines(source: &str, parsed: &Parsed<ModModule>) -> HashSet<usize> {
    let mut masked = HashSet::new();
    for token in parsed.tokens() {
        let is_text = matches!(
            token.kind(),
            TokenKind::String
                | TokenKind::FStringStart
                | TokenKind::FStringMiddle
                | TokenKind::FStringEnd
                | TokenKind::Comment
        );
        if !is_text {
            continue;
        }
        let start = usize::from(token.range().start());
        let end = usize::from(token.range().end()).min(source.len());
        let first = line_of(source, start);
        let last = line_of(source, end);
        for line in first..=last {
            masked.insert(line);
        }
    }
    masked
}

/// 0-based line index of a byte offset.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

/// Iterate the physical lines as `(0-based index, content range, content text)`. The range/text
/// exclude the trailing newline.
fn lines(source: &str) -> impl Iterator<Item = (usize, TextRange, &str)> {
    let mut offset = 0usize;
    source.split('\n').enumerate().map(move |(index, line)| {
        // `split('\n')` drops the `\n`; strip a trailing `\r` so CRLF lines measure cleanly.
        let content = line.strip_suffix('\r').unwrap_or(line);
        let start = offset;
        let range = TextRange::new(
            TextSize::from(start as u32),
            TextSize::from((start + content.len()) as u32),
        );
        offset += line.len() + 1; // + the consumed '\n'
        (index, range, content)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_artifacts() {
        assert_eq!(classify_line("```python"), Some(Artifact::Fence));
        assert_eq!(classify_line("    ```"), Some(Artifact::Fence));
        assert_eq!(classify_line("<<<<<<< HEAD"), Some(Artifact::Conflict));
        assert_eq!(classify_line(">>>>>>> branch"), Some(Artifact::Conflict));
        assert_eq!(classify_line("<file path=\"a.py\">"), Some(Artifact::Tag));
        assert_eq!(classify_line("</file>"), Some(Artifact::Tag));
        // Real code is not an artifact.
        assert_eq!(classify_line("x = compute(a, b)"), None);
        assert_eq!(classify_line("if a < b and c > d:"), None); // not a whole-line tag
        assert_eq!(classify_line("return value"), None);
    }

    #[test]
    fn prose_lines_need_words_and_no_code_punct() {
        assert!(is_prose_line("Here is how this function works"));
        assert!(is_prose_line("This handles the edge case correctly today"));
        assert!(!is_prose_line("return self.value")); // < 4 words
        assert!(!is_prose_line("process all the items now()")); // has code punct
        assert!(!is_prose_line("x = 1")); // code punct
    }

    #[test]
    fn keyword_statements_are_not_prose() {
        // Punctuation-free keyword statements common in real test/validation code must not read as
        // prose (regression: a run of these is not "pasted explanation").
        assert!(!is_prose_line("assert user is not None"));
        assert!(!is_prose_line("raise ValueError from original"));
        assert!(!is_prose_line("from package import a b c"));
        let asserts = "def t():\n    assert a is not None\n    assert b is not None\n    \
                       assert c is not None\n    assert d is not None\n";
        let (prose, _total) = prose_stats(asserts, None);
        assert_eq!(prose, 0, "assert statements are code, not prose");
    }

    #[test]
    fn prose_stats_counts_code_ish_lines() {
        let src = "Here is the explanation of the code\nx = 1\n\n# a comment\nIt loops over each item slowly\n";
        // code-ish lines: line0 (prose), line1 (code), line4 (prose). blank + comment excluded.
        let (prose, total) = prose_stats(src, None);
        assert_eq!(total, 3);
        assert_eq!(prose, 2);
    }
}
