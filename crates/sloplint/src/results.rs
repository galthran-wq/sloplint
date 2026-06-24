//! Shared result types produced by the command pipelines and consumed across the binary:
//! a checked file's findings (`FileResult`), a measured file's metrics (`MeasuredFile`), and a
//! profile's duplication aggregate (`CloneStats`).

use sloplint_diagnostics::Diagnostic;
use sloplint_linter::suppression::Suppressions;
use sloplint_metrics::FileMetrics;

/// One file's parsed source and accumulated diagnostics.
pub(crate) struct FileResult {
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Inline `# sloplint: allow` directives for this file. Parsed up front while the tree
    /// is in scope, then applied once at the end so it filters whole-tree findings (SLP020) too.
    pub(crate) suppressions: Suppressions,
}

/// A measured file: its display path, source, per-function metrics, and the names of the profiles
/// its path belongs to (used to place it into one or more metric panels).
pub(crate) struct MeasuredFile {
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) metrics: FileMetrics,
    pub(crate) profiles: Vec<String>,
}

/// Production duplication aggregate: SLP020 clone density for one profile's functions —
/// surfacing the existing clone engine as a descriptive cohort metric, not new detection.
pub(crate) struct CloneStats {
    /// Confirmed SLP020 clone pairs whose *both* functions are in the profile.
    pub(crate) pairs: usize,
    /// Distinct functions appearing in at least one such pair.
    pub(crate) functions_in_clones: usize,
    /// Functions the clone engine considered for the profile — the ratio denominator.
    pub(crate) total_functions: usize,
    /// Functions in the largest connected clone cluster (a helper duplicated across N functions);
    /// 0 when there are no clones.
    pub(crate) largest_cluster: usize,
}

impl CloneStats {
    /// Fraction of the profile's functions that participate in at least one clone pair (0.0 when
    /// there are none). The headline duplication ratio — high for copy-paste codebases, ≈0 for
    /// clean ones.
    pub(crate) fn ratio(&self) -> f64 {
        if self.total_functions == 0 {
            0.0
        } else {
            self.functions_in_clones as f64 / self.total_functions as f64
        }
    }
}
