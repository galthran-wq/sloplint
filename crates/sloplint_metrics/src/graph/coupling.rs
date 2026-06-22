//! Pure package-coupling formulas over the resolved import graph: Martin's instability,
//! abstractness, and distance-from-main-sequence, plus the module-across-package concentration
//! (a Gini-based spread measure). No graph state — just arithmetic on counts.

use std::collections::BTreeMap;

/// Martin's instability `I = Ce / (Ce + Ca)`, defined as `0.0` when `Ce + Ca == 0` (matching
/// JDepend, which returns `0` for an uncoupled package rather than dividing by zero).
pub fn instability(ce: usize, ca: usize) -> f64 {
    let total = ce + ca;
    if total == 0 {
        0.0
    } else {
        ce as f64 / total as f64
    }
}

/// Martin's abstractness `A = abstract_classes / classes`, defined as `0.0` when `classes == 0`
/// (matching JDepend, which returns `0` for a class-less package rather than dividing by zero).
pub fn abstractness(abstract_classes: usize, classes: usize) -> f64 {
    if classes == 0 {
        0.0
    } else {
        abstract_classes as f64 / classes as f64
    }
}

/// Distance from the main sequence `D = |A + I − 1|` ∈ [0, 1] — how far a package sits from the
/// ideal balance where abstractness and instability sum to one (JDepend's `distance()` with the
/// default volatility of 1).
pub fn distance(abstractness: f64, instability: f64) -> f64 {
    (abstractness + instability - 1.0).abs()
}

/// Node-distribution concentration of modules across packages — the first
/// architecture metric over *nodes* rather than *edges*. It surfaces a "god-package" / flat
/// dumping-ground (one directory holding most of the repo), a shape the coupling metrics
/// structurally cannot see: independent leaf modules have near-zero coupling no matter how many
/// pile up in one directory (yt-dlp's `extractor` holds 90% of the repo at propagation cost 0.07).
///
/// Descriptive only, like every metric here: high concentration is a small repo's one main package
/// as readily as it is a slop pile. Read it in cohort context; never a pass/fail gate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Concentration {
    /// Total first-party modules counted.
    pub total_modules: usize,
    /// Packages (directories holding ≥1 module) the modules are distributed over.
    pub packages: usize,
    /// `max(modules in a package) / total_modules` ∈ [0, 1]. `0.0` when there are no modules.
    pub max_package_share: f64,
    /// Population Gini coefficient of the modules-per-package distribution ∈ [0, 1 − 1/packages].
    /// `0.0` when every package is the same size (or there is ≤1 package); approaches 1 as a single
    /// package dominates.
    pub module_count_gini: f64,
    /// The package holding the most modules, as `(name, module count)` — names the offender for the
    /// human view. Ties broken by package name (smallest first) for determinism. `None` when there
    /// are no packages.
    pub largest_package: Option<(String, usize)>,
}

/// Compute [`Concentration`] from the package each module belongs to — one entry per module
/// (repeats are the point: a package with N modules appears N times).
pub fn concentration(module_packages: &[String]) -> Concentration {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for pkg in module_packages {
        *counts.entry(pkg.as_str()).or_default() += 1;
    }
    let total = module_packages.len();
    // Largest package: highest count, ties broken by the (BTreeMap-sorted) name — replace only on a
    // strictly greater count, so the first (smallest) name among equals wins, deterministically.
    let mut largest: Option<(String, usize)> = None;
    for (pkg, &n) in &counts {
        if largest.as_ref().is_none_or(|(_, best)| n > *best) {
            largest = Some(((*pkg).to_string(), n));
        }
    }
    let max_package_share = match &largest {
        Some((_, n)) if total > 0 => *n as f64 / total as f64,
        _ => 0.0,
    };
    let sizes: Vec<usize> = counts.values().copied().collect();
    Concentration {
        total_modules: total,
        packages: counts.len(),
        max_package_share,
        module_count_gini: gini(&sizes),
        largest_package: largest,
    }
}

/// Population Gini coefficient of a non-negative distribution, in [0, 1 − 1/n]. `0.0` for an empty,
/// all-zero, or all-equal (including single-element) distribution. Sorted-rank formula, O(n log n):
/// `G = (2·Σ i·xᵢ) / (n·Σxᵢ) − (n+1)/n` over 1-based ranks of the ascending-sorted values.
fn gini(values: &[usize]) -> f64 {
    let n = values.len();
    let total: usize = values.iter().sum();
    if n == 0 || total == 0 {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let weighted: f64 = sorted
        .iter()
        .enumerate()
        .map(|(i, &x)| (i as f64 + 1.0) * x as f64)
        .sum();
    let n = n as f64;
    let g = (2.0 * weighted) / (n * total as f64) - (n + 1.0) / n;
    // Guard a tiny negative from floating-point round-off on perfectly-equal distributions.
    g.max(0.0)
}
