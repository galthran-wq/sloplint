//! SLP020: cross-file duplicate / near-duplicate functions.
//!
//! Whole-tree: the clone engine (`sloplint_clone`) compares every function against every other.
//! The pure attribution logic lives here in the linter; the binary collects the function units
//! during its per-file scan and drives this, attaching one finding per clone-involved function.

use std::collections::HashMap;

use sloplint_clone::{find_clones, CloneConfig, FunctionUnit};
use sloplint_macros::ViolationMetadata;
use sloplint_python::TextRange;

use crate::registry::WholeProjectRule;

/// ## What it does
/// Flags functions that are exact or near-duplicate copies of another function anywhere in the
/// project — verbatim copy-paste, and "same logic, slightly different" (renamed identifiers,
/// reordered or lightly edited bodies).
///
/// ## Why is this bad?
/// Duplicated logic multiplies the cost of every future change: a bug fix or behavior tweak has
/// to be found and applied in each copy, and the copies silently drift when one is updated and
/// the others are forgotten. Near-duplicates are a common tell of generated code pasted in
/// repeatedly instead of being factored into a shared helper.
///
/// ## Example
/// ```python
/// def total_with_tax(items):
///     total = sum(i.price for i in items)
///     return total * 1.2
///
/// def grand_total(products):  # same logic, renamed — a near-duplicate of total_with_tax
///     subtotal = sum(p.price for p in products)
///     return subtotal * 1.2
/// ```
#[derive(ViolationMetadata)]
pub struct Clones;

impl WholeProjectRule for Clones {
    fn code(&self) -> &'static str {
        "SLP020"
    }
}

/// One SLP020 finding: a clone-involved function pointing at its lowest-index partner.
pub struct Finding {
    /// File index (into the caller's file list, via `unit_file`) this finding attaches to.
    pub file: usize,
    /// Range of the duplicated function — where the diagnostic points.
    pub range: TextRange,
    /// The rendered finding message.
    pub message: String,
}

/// Find clones among `units` and emit one finding per clone-involved function, pointing at its
/// lowest-index partner. `unit_file[i]` is the file index unit `i` came from; `sources[file]` is
/// that file's source (used only to render the partner's line number). Deterministic: lowest-index
/// partner wins ties, findings come in ascending unit order.
pub fn findings(
    units: &[FunctionUnit],
    unit_file: &[usize],
    sources: &[&str],
    config: &CloneConfig,
) -> Vec<Finding> {
    // For each clone-involved function, keep its lowest-index partner (deterministic).
    let mut partner: HashMap<usize, (usize, f64)> = HashMap::new();
    let mut record = |from: usize, to: usize, similarity: f64| {
        partner
            .entry(from)
            .and_modify(|best| {
                if to < best.0 {
                    *best = (to, similarity);
                }
            })
            .or_insert((to, similarity));
    };
    for pair in find_clones(units, config) {
        record(pair.a, pair.b, pair.similarity);
        record(pair.b, pair.a, pair.similarity);
    }

    let mut involved: Vec<usize> = partner.keys().copied().collect();
    involved.sort_unstable();
    let mut out = Vec::with_capacity(involved.len());
    for unit_index in involved {
        let (partner_index, similarity) = partner[&unit_index];
        let unit = &units[unit_index];
        let partner_unit = &units[partner_index];
        let percent = (similarity * 100.0).round() as u32;
        let partner_line = line_of(
            sources[unit_file[partner_index]],
            partner_unit.range.start().into(),
        );
        out.push(Finding {
            file: unit_file[unit_index],
            range: unit.range,
            message: format!(
                "duplicate of {}:{partner_line} (function `{}`, {percent}% similar)",
                partner_unit.file, partner_unit.name
            ),
        });
    }
    out
}

/// 1-based line number of byte `offset` in `source`.
fn line_of(source: &str, offset: u32) -> usize {
    let offset = (offset as usize).min(source.len());
    source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_clone::extract_functions;
    use sloplint_python::parse;

    const DUP: &str = "\
def alpha(a, b):
    total = a + b
    total = total * 2
    total = total - 1
    result = total + a
    return result


def beta(x, y):
    total = x + y
    total = total * 2
    total = total - 1
    result = total + x
    return result
";

    fn units(src: &str) -> (Vec<FunctionUnit>, CloneConfig) {
        let parsed = parse(src).unwrap();
        let config = CloneConfig::default();
        let units = extract_functions("dup.py", src, &parsed, config.shingle_k);
        (units, config)
    }

    #[test]
    fn flags_duplicate_functions_at_their_lowest_index_partner() {
        let (units, config) = units(DUP);
        let unit_file = vec![0usize; units.len()];
        let found = findings(&units, &unit_file, &[DUP], &config);
        assert_eq!(found.len(), 2, "both functions are clones of each other");
        // The later function points back at the lowest-index partner, `alpha`.
        assert!(
            found.iter().any(|f| f.message.contains("function `alpha`")),
            "{:?}",
            found.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
        assert!(found[0].message.contains("% similar"));
        assert!(found[0].message.starts_with("duplicate of dup.py:"));
    }

    #[test]
    fn distinct_functions_are_not_flagged() {
        let src = "\
def alpha(a, b):
    return a + b


def gamma(items):
    out = []
    for item in items:
        out.append(item * 2)
    return out
";
        let (units, config) = units(src);
        let unit_file = vec![0usize; units.len()];
        assert!(findings(&units, &unit_file, &[src], &config).is_empty());
    }
}
