//! CI complexity gates for the `metrics` command: fail the run (exit 1) when any function's
//! cyclomatic/cognitive complexity exceeds a configured ceiling. A gate is *not* a diagnostic —
//! it never emits an SLPxxx finding, so it doesn't duplicate Ruff's C901.

use sloplint_metrics::FunctionMetrics;

use crate::{line_of, MeasuredFile};

/// One complexity gate: report every function whose `metric` exceeds `ceiling` and return
/// whether any did. A `None` ceiling is a no-op (returns `false`).
pub(crate) fn gate(
    per_file: &[MeasuredFile],
    ceiling: Option<usize>,
    noun: &str,
    metric: impl Fn(&FunctionMetrics) -> usize,
) -> bool {
    let Some(ceiling) = ceiling else {
        return false;
    };
    let offenders = gate_offenders(per_file, ceiling, metric);
    if offenders.is_empty() {
        return false;
    }
    eprintln!(
        "sloplint: {} function(s) over the {noun} ceiling of {ceiling}:",
        offenders.len()
    );
    for offender in &offenders {
        eprintln!(
            "  {}: `{}` has {noun} complexity {}",
            offender.location, offender.name, offender.value
        );
    }
    true
}

/// A function whose `metric` value exceeds the configured ceiling.
struct GateOffender {
    /// `path:line` of the function's `def` line (its name, not the first decorator).
    location: String,
    name: String,
    value: usize,
}

/// Collect every function whose `metric` exceeds `ceiling`, in file then source order
/// (deterministic).
fn gate_offenders(
    per_file: &[MeasuredFile],
    ceiling: usize,
    metric: impl Fn(&FunctionMetrics) -> usize,
) -> Vec<GateOffender> {
    let mut offenders = Vec::new();
    for file in per_file {
        for function in &file.metrics.functions {
            let value = metric(function);
            if value > ceiling {
                // Locate the `def` line via the name span — `range` would point at the first
                // decorator on a decorated function.
                let line = line_of(&file.source, function.name_range.start().into());
                offenders.push(GateOffender {
                    location: format!("{}:{line}", file.path),
                    name: function.name.clone(),
                    value,
                });
            }
        }
    }
    offenders
}
