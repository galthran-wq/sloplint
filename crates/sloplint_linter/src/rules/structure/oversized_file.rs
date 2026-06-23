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
                self.code(),
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
