//! SLP050: ASCII-only source.

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{TextRange, TextSize};

use crate::lint::{FileContext, Rule};

/// ## What it does
/// Flags any non-ASCII character anywhere in the source — emoji, accented letters, smart
/// quotes — in code, comments, or string literals alike (one finding per contiguous run).
///
/// ## Why is this bad?
/// Non-ASCII in source is a strong AI tell and a portability hazard: encoding-dependent
/// behavior and invisible look-alike characters that pass review but break tooling.
pub struct AsciiOnly;

impl Rule for AsciiOnly {
    fn code(&self) -> &'static str {
        "SLP050"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let mut run_start: Option<usize> = None;
        for (idx, ch) in ctx.source.char_indices() {
            if ch.is_ascii() {
                if let Some(start) = run_start.take() {
                    push_run(self.code(), diagnostics, start, idx);
                }
            } else if run_start.is_none() {
                run_start = Some(idx);
            }
        }
        if let Some(start) = run_start {
            push_run(self.code(), diagnostics, start, ctx.source.len());
        }
    }
}

fn push_run(code: &'static str, diagnostics: &mut Vec<Diagnostic>, start: usize, end: usize) {
    diagnostics.push(Diagnostic::new(
        code,
        "non-ASCII character; sloplint enforces ASCII-only source",
        TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32)),
        Severity::Warning,
    ));
}
