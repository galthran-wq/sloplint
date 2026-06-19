//! Rule-independent diagnostic abstractions.
//!
//! Mirrors Ruff's `ruff_diagnostics` split: rules produce [`Diagnostic`]s without knowing
//! how they're rendered (text / JSON / SARIF / PR comment). The full model — fix edits,
//! severity policy, suppression — is fleshed out in the diagnostics/registry PR. This is
//! the seam crate so downstream crates can already depend on it.

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
        }
    }
}
