//! SLP080: oversized file.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::TextRange;

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags a file longer than the configured line limit (`file_max_lines`).
///
/// ## Why is this bad?
/// AI tends to dump everything into one enormous module, and Ruff has no file-length gate.
/// Long modules are hard to navigate and signal missing decomposition.
#[derive(ViolationMetadata)]
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
