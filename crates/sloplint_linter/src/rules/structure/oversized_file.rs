//! SLP080: oversized file.
//!
//! AI tends to dump everything into one enormous module. Ruff has no file-length gate, so
//! this is genuinely ours: flag files longer than the configured line limit.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::TextRange;

use crate::lint::{FileContext, Rule};

pub struct OversizedFile;

impl Rule for OversizedFile {
    fn code(&self) -> &'static str {
        "SLP080"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let lines = ctx.source.lines().count();
        if lines > ctx.limits.file_max_lines {
            diagnostics.push(Diagnostic::new(
                "SLP080",
                format!(
                    "file has {lines} lines (max {}); split it into smaller modules",
                    ctx.limits.file_max_lines
                ),
                TextRange::default(),
                Severity::Warning,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Limits;
    use crate::lint::FileContext;
    use sloplint_python::parse;

    fn count_with_limit(source: &str, file_max_lines: usize) -> usize {
        let parsed = parse(source).unwrap();
        let ctx = FileContext {
            path: "t.py",
            source,
            parsed: &parsed,
            security_extra: &[],
            placeholders_extra: &[],
            comment_phrases_extra: &[],
            limits: Limits {
                file_max_lines,
                ..Limits::default()
            },
        };
        let mut diagnostics = Vec::new();
        OversizedFile.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    #[test]
    fn flags_files_over_the_limit_only() {
        let source = "a = 1\nb = 2\nc = 3\nd = 4\n";
        assert_eq!(count_with_limit(source, 3), 1, "4 lines over a limit of 3");
        assert_eq!(
            count_with_limit(source, 4),
            0,
            "exactly at the limit is fine"
        );
        assert_eq!(count_with_limit(source, 10), 0, "under the limit is fine");
    }
}
