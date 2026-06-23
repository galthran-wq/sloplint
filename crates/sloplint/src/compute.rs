//! CLI-side metric computation for the `metrics` command: per-profile module-count concentration
//! over the package graph, and SLP020 clone density (the share of a profile's functions that
//! participate in a clone pair, plus the largest clone cluster via union-find).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use sloplint_clone::ClonePair;
use sloplint_metrics::graph;

use crate::{module_name, CloneStats, MeasuredFile};

/// The package module-count concentration for one profile's files. Edge-free — it needs
/// only each module's package, so the text view computes it without building the import graph
/// (which would require an extra import-scan pass per file).
///
/// Modules are deduplicated by dotted name (last writer wins), exactly as `ImportGraph::build`
/// populates its node index: two files resolving to the same dotted name (e.g. `a.py` beside a
/// package `a/`) are one node there and must be one module here too — otherwise the text view would
/// disagree with the JSON feed and `--format packages`.
pub(crate) fn concentration_for(per_file: &[MeasuredFile], profile: &str) -> graph::Concentration {
    let mut modules: BTreeMap<String, bool> = BTreeMap::new();
    for file in per_file
        .iter()
        .filter(|f| f.profiles.iter().any(|p| p == profile))
    {
        if let Some(name) = module_name(Path::new(&file.path)) {
            modules.insert(name.dotted, name.is_package);
        }
    }
    let packages: Vec<String> = modules
        .into_iter()
        .map(|(dotted, is_package)| graph::package_of(&dotted, is_package))
        .collect();
    graph::concentration(&packages)
}

/// Compute the clone density for `profile` from the project-wide SLP020 `pairs`, keeping only pairs
/// whose both functions belong to the profile (duplication internal to it). `largest_cluster` is
/// the biggest connected component of those pairs, via union-find.
pub(crate) fn clone_stats_for(
    profile: &str,
    unit_profiles: &[Vec<String>],
    pairs: &[ClonePair],
) -> CloneStats {
    let in_profile = |idx: usize| unit_profiles[idx].iter().any(|p| p == profile);
    let total_functions = (0..unit_profiles.len()).filter(|&i| in_profile(i)).count();

    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut in_clones: HashSet<usize> = HashSet::new();
    let mut pair_count = 0usize;
    for pair in pairs {
        if in_profile(pair.a) && in_profile(pair.b) {
            pair_count += 1;
            in_clones.insert(pair.a);
            in_clones.insert(pair.b);
            let ra = dsu_find(&mut parent, pair.a);
            let rb = dsu_find(&mut parent, pair.b);
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    // Largest cluster = the biggest union-find component among clone members.
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for &node in &in_clones {
        let root = dsu_find(&mut parent, node);
        *sizes.entry(root).or_insert(0) += 1;
    }
    CloneStats {
        pairs: pair_count,
        functions_in_clones: in_clones.len(),
        total_functions,
        largest_cluster: sizes.values().copied().max().unwrap_or(0),
    }
}

/// Union-find root of `x` with path compression; inserts `x` (as its own root) on first touch.
fn dsu_find(parent: &mut HashMap<usize, usize>, x: usize) -> usize {
    let p = *parent.entry(x).or_insert(x);
    if p == x {
        return x;
    }
    let root = dsu_find(parent, p);
    parent.insert(x, root);
    root
}
