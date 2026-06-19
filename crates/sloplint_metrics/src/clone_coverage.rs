//! Project-level **clone coverage**: how much of the codebase participates in a near-duplicate.
//!
//! `SLP020` answers "*which* functions are clones?" — a per-finding view. The headline,
//! trend-worthy question for a duplication-heavy repo is the aggregate one: *how much* of it
//! is duplicated? Clone coverage is the fraction of functions (primary) and of function lines
//! (secondary) that participate in at least one detected clone pair. The clone engine already
//! computes the clusters; this turns them into a single reported number.
//!
//! The number is reproducible from the `[clone]` config alone, so it is reported *with* that
//! config (`min_statements`, `similarity`) — a badge/PR-summary figure means nothing without
//! the knobs it was produced under.
//!
//! Coverage is a *project-wide* figure: clone detection runs once over every function in the
//! tree, so it counts cross-file duplicates that the per-file `SLP020` finding does not — the
//! headline percentage will not equal a count of `SLP020` findings, by design.

use std::collections::HashMap;

use sloplint_clone::{find_clones, CloneConfig, FunctionUnit};

use crate::badge::{Badge, Color};

/// Project-level clone-coverage figures over one analyzed function population.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloneCoverage {
    /// Total functions considered (the clone engine's unit population: every function,
    /// methods and nested defs included).
    pub total_functions: usize,
    /// Functions involved in at least one clone pair.
    pub clone_functions: usize,
    /// Distinct physical lines spanned by all considered functions (per file; overlap from
    /// nested functions is counted once).
    pub total_function_lines: usize,
    /// Distinct physical lines spanned by clone-involved functions (same de-duplication).
    pub clone_function_lines: usize,
    /// Number of confirmed clone pairs (context for the coverage figure).
    pub clone_pairs: usize,
    /// `[clone].min_statements` the figure was produced with (reproducibility).
    pub min_statements: usize,
    /// `[clone].similarity` the figure was produced with (reproducibility).
    pub similarity: f64,
}

impl CloneCoverage {
    /// Fraction of functions (0.0–1.0) involved in a clone — the headline metric.
    pub fn coverage_funcs(&self) -> f64 {
        ratio(self.clone_functions, self.total_functions)
    }

    /// Fraction of function *lines* (0.0–1.0) involved in a clone — the secondary metric, which
    /// weights big duplicated functions more heavily than tiny ones.
    pub fn coverage_lines(&self) -> f64 {
        ratio(self.clone_function_lines, self.total_function_lines)
    }

    /// Badge for the headline (function) coverage. "Lower is better": green below 10%, yellow
    /// below 25%, red at/above — the thresholds from issue #9.
    pub fn badge(&self) -> Badge {
        let pct = self.coverage_funcs() * 100.0;
        Badge::new(
            "clone coverage",
            format!("{pct:.1}%"),
            Color::for_value(pct, 10.0, 25.0),
        )
    }

    /// A one-line markdown summary for the PR comment, including the `[clone]` config so the
    /// number is reproducible by a reader.
    pub fn markdown(&self) -> String {
        format!(
            "**Clone coverage** — {:.1}% of functions ({}/{}) and {:.1}% of function lines \
             ({}/{}) participate in a near-duplicate ({} clone pair(s); \
             `min_statements = {}`, `similarity = {:.2}`).\n",
            self.coverage_funcs() * 100.0,
            self.clone_functions,
            self.total_functions,
            self.coverage_lines() * 100.0,
            self.clone_function_lines,
            self.total_function_lines,
            self.clone_pairs,
            self.min_statements,
            self.similarity,
        )
    }
}

/// Compute clone coverage over `units` (the clone engine's whole-tree function population).
/// `unit_spans[i]` is the inclusive `(first_line, last_line)` (1-based) of `units[i]` in its
/// own file, supplied by the caller (which holds the source). Runs [`find_clones`] internally,
/// so the result is fully determined by `units` + `config`.
///
/// Line counts use a **per-file interval union**: a nested function's lines lie inside its
/// parent's span, and the parent also appears as its own unit, so a naive sum would count
/// those lines twice — the union collapses the overlap. Files are unioned separately so the
/// same line number in two files stays distinct.
///
/// `unit_spans` shorter than `units` treats the missing tail as empty (defensive; callers
/// always pass a parallel array).
pub fn clone_coverage(
    units: &[FunctionUnit],
    unit_spans: &[(usize, usize)],
    config: &CloneConfig,
) -> CloneCoverage {
    let pairs = find_clones(units, config);

    let mut involved = vec![false; units.len()];
    for pair in &pairs {
        involved[pair.a] = true;
        involved[pair.b] = true;
    }
    let clone_functions = involved.iter().filter(|&&hit| hit).count();

    // Per file: all function spans, and the clone-involved subset.
    let mut by_file: HashMap<&str, FileSpans> = HashMap::new();
    for (i, unit) in units.iter().enumerate() {
        let span = unit_spans.get(i).copied().unwrap_or((1, 0));
        let entry = by_file.entry(unit.file.as_str()).or_default();
        entry.0.push(span);
        if involved[i] {
            entry.1.push(span);
        }
    }
    let mut total_function_lines = 0;
    let mut clone_function_lines = 0;
    for (mut all, mut clones) in by_file.into_values() {
        total_function_lines += distinct_lines(&mut all);
        clone_function_lines += distinct_lines(&mut clones);
    }

    CloneCoverage {
        total_functions: units.len(),
        clone_functions,
        total_function_lines,
        clone_function_lines,
        clone_pairs: pairs.len(),
        min_statements: config.min_statements,
        similarity: config.similarity,
    }
}

/// Per-file line intervals: `(all function spans, clone-involved spans)`.
type FileSpans = (Vec<(usize, usize)>, Vec<(usize, usize)>);

/// Distinct lines covered by a set of inclusive `[start, end]` line intervals, merging
/// overlaps (so a nested function inside its parent isn't counted twice). Mutates `intervals`
/// (sorts in place). Intervals with `end < start` are treated as empty.
fn distinct_lines(intervals: &mut [(usize, usize)]) -> usize {
    intervals.sort_unstable();
    let mut total = 0;
    let mut current: Option<(usize, usize)> = None;
    for &(start, end) in intervals.iter() {
        if end < start {
            continue;
        }
        match current {
            // Overlapping (shared endpoint counts as overlap) — extend the run.
            Some((cs, ce)) if start <= ce => current = Some((cs, ce.max(end))),
            Some((cs, ce)) => {
                total += ce - cs + 1;
                current = Some((start, end));
            }
            None => current = Some((start, end)),
        }
    }
    if let Some((cs, ce)) = current {
        total += ce - cs + 1;
    }
    total
}

/// `num / den` as a fraction, 0.0 when `den == 0`.
fn ratio(num: usize, den: usize) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    /// Build clone units + their `(first_line, last_line)` spans from one source file, the way
    /// the CLI does.
    fn units_and_spans(
        source: &str,
        config: &CloneConfig,
    ) -> (Vec<FunctionUnit>, Vec<(usize, usize)>) {
        let parsed = parse(source).expect("valid python");
        let units = sloplint_clone::extract_functions("t.py", source, &parsed, config.shingle_k);
        // 1-based line of a byte offset, matching the CLI's `line_of`.
        let line_of = |offset: usize| {
            source.as_bytes()[..offset.min(source.len())]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1
        };
        let spans = units
            .iter()
            .map(|unit| {
                let start = u32::from(unit.range.start()) as usize;
                let end = u32::from(unit.range.end()) as usize;
                (line_of(start), line_of(end.saturating_sub(1).max(start)))
            })
            .collect();
        (units, spans)
    }

    // Two structurally-identical functions (renamed identifiers) plus one unrelated function:
    // coverage should be 2/3 of functions.
    const TWO_CLONES_ONE_UNIQUE: &str = "\
def total_price(items):
    total = 0
    for item in items:
        total += item.price * item.quantity
    return total

def sum_costs(products):
    acc = 0
    for product in products:
        acc += product.price * product.quantity
    return acc

def parse_config(path):
    with open(path) as handle:
        data = handle.read()
    return data.strip().splitlines()
";

    #[test]
    fn measures_function_and_line_coverage() {
        let config = CloneConfig::default();
        let (units, spans) = units_and_spans(TWO_CLONES_ONE_UNIQUE, &config);
        let coverage = clone_coverage(&units, &spans, &config);

        assert_eq!(coverage.total_functions, 3);
        assert_eq!(coverage.clone_functions, 2);
        assert_eq!(coverage.clone_pairs, 1);
        assert!((coverage.coverage_funcs() - 2.0 / 3.0).abs() < 1e-9);
        // The two clones span 5 lines each (no nesting) -> 10 clone lines; the unique function
        // is 4 lines -> 14 total.
        assert_eq!(coverage.clone_function_lines, 10);
        assert_eq!(coverage.total_function_lines, 14);
        // Config is echoed for reproducibility.
        assert_eq!(coverage.min_statements, config.min_statements);
        assert_eq!(coverage.similarity, config.similarity);
    }

    #[test]
    fn nested_function_lines_are_not_double_counted() {
        // `outer` (a clone of `outer_twin`) contains a nested `helper`. `helper` is its own
        // unit whose lines lie inside `outer`, so a naive line sum would count them twice.
        let config = CloneConfig::default();
        let source = "\
def outer(items):
    def helper(value):
        scaled = value * 2
        return scaled + 1
    total = 0
    for item in items:
        total += helper(item)
    return total

def outer_twin(rows):
    def worker(entry):
        boosted = entry * 2
        return boosted + 1
    acc = 0
    for row in rows:
        acc += worker(row)
    return acc
";
        let (units, spans) = units_and_spans(source, &config);
        let coverage = clone_coverage(&units, &spans, &config);
        // 4 functions total (2 outer + 2 nested), and the two outers form a clone pair.
        assert_eq!(coverage.total_functions, 4);
        // The two outers span lines 1-8 and 10-17 = 8 each = 16 distinct lines. The nested
        // helpers lie WITHIN those spans, so the union must stay 16, not 16 + (nested lines).
        assert_eq!(coverage.total_function_lines, 16);
        // Whichever of {outer, helper} the engine pairs, involved lines must not exceed the
        // 16 distinct function lines (the bug would push this over 16).
        assert!(coverage.clone_function_lines <= 16);
        assert!(coverage.coverage_lines() <= 1.0);
    }

    #[test]
    fn no_clones_is_zero_coverage() {
        let config = CloneConfig::default();
        let source = "\
def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    return [v / total for v in values]

def parse_config(path):
    with open(path) as handle:
        return handle.read().splitlines()
";
        let (units, spans) = units_and_spans(source, &config);
        let coverage = clone_coverage(&units, &spans, &config);
        assert_eq!(coverage.clone_functions, 0);
        assert_eq!(coverage.coverage_funcs(), 0.0);
        assert_eq!(coverage.coverage_lines(), 0.0);
    }

    #[test]
    fn empty_population_is_zero_not_nan() {
        let config = CloneConfig::default();
        let coverage = clone_coverage(&[], &[], &config);
        assert_eq!(coverage.total_functions, 0);
        assert_eq!(coverage.coverage_funcs(), 0.0);
        assert_eq!(coverage.coverage_lines(), 0.0);
        assert!(coverage.badge().message.starts_with('0'));
    }

    #[test]
    fn badge_color_tracks_thresholds() {
        let mut coverage = clone_coverage(&[], &[], &CloneConfig::default());
        // 0% -> green
        assert_eq!(coverage.badge().color, Color::Green);
        // Force 50% by hand: red.
        coverage.total_functions = 2;
        coverage.clone_functions = 1;
        assert_eq!(coverage.badge().color, Color::Red);
    }
}
