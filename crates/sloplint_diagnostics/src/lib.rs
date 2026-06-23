//! Rule-independent diagnostic abstractions.
//!
//! Mirrors Ruff's `ruff_diagnostics` split: rules produce [`Diagnostic`]s without knowing
//! how they're rendered (text / JSON / SARIF / PR comment). Keeping these abstractions in a
//! seam crate lets every downstream crate depend on the diagnostic model without pulling in
//! the rule engine.

pub mod fix;
pub mod render;

pub use fix::{Applicability, Applied, Edit, Fix};

/// Ruff-style rule metadata, derived from a rule's doc-comment by
/// `#[derive(ViolationMetadata)]` (see the `sloplint_macros` crate). Mirrors ruff's
/// `ViolationMetadata`: `rule_name` is the rule type's name; `explanation` is its rendered
/// `## What it does` / `## Why is this bad?` doc block (the single source of truth for a rule's
/// prose), or `None` when the rule carries no doc-comment.
pub trait ViolationMetadata {
    fn rule_name(&self) -> &'static str;
    fn explanation(&self) -> Option<&'static str>;
}

use ruff_text_size::TextRange;

/// How serious a finding is. Drives exit codes and badge colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Reported, but does not by itself fail the run.
    Warning,
    /// Fails the run (non-zero exit).
    Error,
}

/// A single finding at a source location.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Stable rule code, e.g. `"SLP001"`.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Source range the finding refers to.
    pub range: TextRange,
    /// Severity of this finding.
    pub severity: Severity,
    /// An optional autofix. `None` when the rule can't mechanically fix the finding; rules that
    /// can attach one via [`Diagnostic::with_fix`]. Applied by the CLI's `--fix` mode.
    pub fix: Option<Fix>,
}

impl Diagnostic {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        range: TextRange,
        severity: Severity,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            range,
            severity,
            fix: None,
        }
    }

    /// Attach an autofix to this diagnostic (builder style).
    #[must_use]
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}
