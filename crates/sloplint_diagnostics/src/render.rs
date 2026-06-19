//! Deterministic, snapshot-friendly rendering of diagnostics.
//!
//! Stable output is what makes `insta` snapshot tests trustworthy: the same diagnostics
//! must always render byte-for-byte identically, regardless of the order rules produced
//! them. Findings are therefore sorted by position, then code.

use crate::{Diagnostic, Severity};

/// Render diagnostics as a stable text block, one finding per line:
/// `line:col CODE [severity] message`.
///
/// Returns a fixed `"No diagnostics.\n"` sentinel when there are none, so "clean" fixtures
/// produce a meaningful snapshot rather than an empty file.
pub fn render_diagnostics(source: &str, diagnostics: &[Diagnostic]) -> String {
    if diagnostics.is_empty() {
        return "No diagnostics.\n".to_string();
    }

    let mut ordered: Vec<&Diagnostic> = diagnostics.iter().collect();
    ordered.sort_by(|a, b| {
        u32::from(a.range.start())
            .cmp(&u32::from(b.range.start()))
            .then_with(|| a.code.cmp(&b.code))
    });

    let mut out = String::new();
    for diagnostic in ordered {
        let (line, col) = line_col(source, u32::from(diagnostic.range.start()) as usize);
        let severity = match diagnostic.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        out.push_str(&format!(
            "{line}:{col} {} [{severity}] {}\n",
            diagnostic.code, diagnostic.message
        ));
    }
    out
}

/// 1-based `(line, column)` for a byte offset. Columns count UTF-8 characters, not bytes,
/// so non-ASCII source still reports sensible positions.
fn line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let offset = byte_offset.min(source.len());
    let mut line = 1;
    let mut col = 1;
    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_text_size::{TextRange, TextSize};

    fn diag(code: &str, start: u32, end: u32) -> Diagnostic {
        Diagnostic::new(
            code,
            "msg",
            TextRange::new(TextSize::from(start), TextSize::from(end)),
            Severity::Warning,
        )
    }

    #[test]
    fn empty_is_sentinel() {
        assert_eq!(render_diagnostics("x = 1\n", &[]), "No diagnostics.\n");
    }

    #[test]
    fn sorts_by_position_then_code() {
        let source = "aaa\nbbb\nccc\n";
        // Provided out of order; render must sort by offset then code.
        let out = render_diagnostics(source, &[diag("SLP002", 8, 9), diag("SLP001", 0, 1)]);
        assert_eq!(out, "1:1 SLP001 [warning] msg\n3:1 SLP002 [warning] msg\n");
    }
}
