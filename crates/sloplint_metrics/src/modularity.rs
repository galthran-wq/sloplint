//! Newman–Girvan modularity Q of the module dependency graph (issue #69).
//!
//! Two numbers, both computed over the **undirected** projection of the import graph (modularity
//! is defined for undirected graphs; we symmetrize by summing the two directed weights of each
//! pair, so a mutual import counts double — documented choice):
//!
//! - **declared Q** — the modularity of the *declared* partition, treating each directory/package
//!   as one community. High when intra-package coupling dominates inter-package coupling; this is
//!   "do the package boundaries actually capture the structure?".
//! - **detected Q** — the modularity of the partition found by [Louvain](https://en.wikipedia.org/wiki/Louvain_method)
//!   community detection (a high-Q clustering ignoring the declared boundaries).
//!
//! The **gap** (detected − declared) is the slop signal the issue calls out: a large gap means
//! the natural community structure does not line up with the declared packages — flat
//! dumping-grounds and packages-in-name-only, common in vibe-coded repos.
//!
//! Louvain is normally randomized (random node order / tie-breaking); here it is made fully
//! deterministic — nodes are visited in index order and ties resolve to the lowest community id —
//! so the reported Q is reproducible, matching the rest of sloplint. (We avoid an external
//! community-detection crate precisely because those introduce nondeterminism.)

use std::collections::HashMap;

/// The modularity rollup for a project's import graph.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModularityReport {
    /// Q of the declared partition (one community per package/directory).
    pub q_declared: f64,
    /// Number of communities in the declared partition (= number of packages).
    pub communities_declared: usize,
    /// Q of the Louvain-detected partition.
    pub q_detected: f64,
    /// Number of communities Louvain found.
    pub communities_detected: usize,
}

impl ModularityReport {
    /// `q_detected − q_declared`: how much better the natural clustering scores than the declared
    /// package boundaries. A large positive gap flags "packages in name only".
    pub fn gap(&self) -> f64 {
        self.q_detected - self.q_declared
    }
}

/// Compute the modularity report from the directed module edges and the declared partition.
///
/// `n` is the module count; `edges` are directed `(from, to)` module indices in `0..n` (self-edges
/// are ignored); `declared[i]` is the community id (e.g. package id) of module `i`.
pub fn analyze(n: usize, edges: &[(usize, usize)], declared: &[usize]) -> ModularityReport {
    let g = Undirected::build(n, edges);
    let detected = g.louvain();
    ModularityReport {
        q_declared: g.modularity(declared),
        communities_declared: distinct(declared),
        q_detected: g.modularity(&detected),
        communities_detected: distinct(&detected),
    }
}

/// Number of distinct community ids in a partition.
fn distinct(partition: &[usize]) -> usize {
    partition
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// An undirected, weighted graph: the symmetrized projection of the import graph. Self-loops are
/// supported (Louvain's aggregation step creates them); the original graph has none.
struct Undirected {
    n: usize,
    /// `adj[u]` = `(v, weight)` for each neighbor `v != u`. Symmetric.
    adj: Vec<Vec<(usize, f64)>>,
    /// Weight of the self-loop at each node (intra-community weight after aggregation).
    self_loop: Vec<f64>,
    /// Weighted degree, self-loops counted twice (the standard convention).
    degree: Vec<f64>,
    /// `2m` — the sum of all degrees; invariant under Louvain aggregation.
    m2: f64,
}

impl Undirected {
    /// Symmetrize the directed edges: each directed `(u, v)` adds 1 to the undirected weight of
    /// the pair `{u, v}`, so a mutual import yields weight 2.
    fn build(n: usize, edges: &[(usize, usize)]) -> Undirected {
        let mut weight: HashMap<(usize, usize), f64> = HashMap::new();
        for &(a, b) in edges {
            if a == b {
                continue;
            }
            let key = if a < b { (a, b) } else { (b, a) };
            *weight.entry(key).or_default() += 1.0;
        }
        let mut pairs: Vec<((usize, usize), f64)> = weight.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0)); // determinism

        let mut adj = vec![Vec::new(); n];
        let mut degree = vec![0.0; n];
        for ((u, v), w) in pairs {
            adj[u].push((v, w));
            adj[v].push((u, w));
            degree[u] += w;
            degree[v] += w;
        }
        let m2 = degree.iter().sum();
        Undirected {
            n,
            adj,
            self_loop: vec![0.0; n],
            degree,
            m2,
        }
    }

    /// Newman–Girvan modularity of a partition: `Q = Σ_c [ Σ_in(c)/2m − (Σ_tot(c)/2m)² ]`, where
    /// `Σ_in(c)` is the ordered sum of intra-community edge weights (each undirected edge twice,
    /// plus self-loops twice) and `Σ_tot(c)` the summed degree. Returns `0.0` for an edgeless
    /// graph (Q is undefined there; 0 is the natural "no structure" value).
    fn modularity(&self, comm: &[usize]) -> f64 {
        if self.m2 == 0.0 {
            return 0.0;
        }
        let mut internal: HashMap<usize, f64> = HashMap::new();
        let mut tot: HashMap<usize, f64> = HashMap::new();
        for u in 0..self.n {
            *tot.entry(comm[u]).or_default() += self.degree[u];
            // Self-loops are intra-community by definition; count both directions.
            *internal.entry(comm[u]).or_default() += 2.0 * self.self_loop[u];
            for &(v, w) in &self.adj[u] {
                if comm[u] == comm[v] {
                    *internal.entry(comm[u]).or_default() += w;
                }
            }
        }
        tot.iter()
            .map(|(c, &t)| {
                let i = internal.get(c).copied().unwrap_or(0.0);
                i / self.m2 - (t / self.m2).powi(2)
            })
            .sum()
    }

    /// Louvain community detection, returning a `node -> community id` partition over the original
    /// `0..n` nodes. Deterministic: each level visits nodes in index order and breaks ties toward
    /// the lower community id / staying put.
    fn louvain(&self) -> Vec<usize> {
        // Which current working-graph node each original node belongs to.
        let mut origin_to_node: Vec<usize> = (0..self.n).collect();
        let mut level = Level::from(self);

        loop {
            let comm = level.local_move();
            // Renumber the communities this level produced into a dense `0..k`.
            let (renumbered, k) = renumber(&comm);
            // Compose: push every original node through this level's community assignment.
            for slot in origin_to_node.iter_mut() {
                *slot = renumbered[*slot];
            }
            if k == level.n {
                break; // no node changed community — converged
            }
            level = level.aggregate(&renumbered, k);
        }
        let (final_partition, _) = renumber(&origin_to_node);
        final_partition
    }
}

/// One Louvain level: a weighted graph (with self-loops) plus the bookkeeping for local moving.
struct Level {
    n: usize,
    adj: Vec<Vec<(usize, f64)>>,
    self_loop: Vec<f64>,
    degree: Vec<f64>,
    m2: f64,
}

impl Level {
    fn from(g: &Undirected) -> Level {
        Level {
            n: g.n,
            adj: g.adj.clone(),
            self_loop: g.self_loop.clone(),
            degree: g.degree.clone(),
            m2: g.m2,
        }
    }

    /// Greedily move each node into the neighboring community that most increases modularity,
    /// repeating full passes until none moves. Returns the `node -> community` assignment (community
    /// ids are node ids, not yet densified).
    fn local_move(&self) -> Vec<usize> {
        let mut comm: Vec<usize> = (0..self.n).collect();
        let mut tot: Vec<f64> = self.degree.clone();
        if self.m2 == 0.0 {
            return comm; // no edges: every node is its own community
        }
        loop {
            let mut moved = false;
            for u in 0..self.n {
                let cu = comm[u];
                tot[cu] -= self.degree[u]; // tentatively remove u from its community

                // Weight from u to each neighboring community.
                let mut w_to: HashMap<usize, f64> = HashMap::new();
                for &(v, w) in &self.adj[u] {
                    *w_to.entry(comm[v]).or_default() += w;
                }

                // Gain of placing u in community c (dropping the constant 1/m2 factor):
                //   w_to(c) - tot[c] * degree[u] / m2
                let ku = self.degree[u];
                let mut best_c = cu;
                let mut best_gain = w_to.get(&cu).copied().unwrap_or(0.0) - tot[cu] * ku / self.m2;

                let mut candidates: Vec<(usize, f64)> = w_to.into_iter().collect();
                candidates.sort_by(|a, b| a.0.cmp(&b.0)); // determinism
                for (c, w) in candidates {
                    let gain = w - tot[c] * ku / self.m2;
                    if gain > best_gain + 1e-12 {
                        best_gain = gain;
                        best_c = c;
                    }
                }

                tot[best_c] += ku;
                if best_c != cu {
                    comm[u] = best_c;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        comm
    }

    /// Collapse each community (already densified to `0..k` via `renumbered`) into a single node,
    /// summing inter-community weights into edges and intra-community weights into self-loops.
    fn aggregate(&self, renumbered: &[usize], k: usize) -> Level {
        let mut self_loop = vec![0.0; k];
        for u in 0..self.n {
            self_loop[renumbered[u]] += self.self_loop[u];
        }
        // Accumulate undirected weights between the new super-nodes.
        let mut between: HashMap<(usize, usize), f64> = HashMap::new();
        for u in 0..self.n {
            let cu = renumbered[u];
            for &(v, w) in &self.adj[u] {
                if v < u {
                    continue; // each undirected edge once
                }
                let cv = renumbered[v];
                if cu == cv {
                    self_loop[cu] += w; // becomes intra-community weight
                } else {
                    let key = if cu < cv { (cu, cv) } else { (cv, cu) };
                    *between.entry(key).or_default() += w;
                }
            }
        }
        let mut adj = vec![Vec::new(); k];
        for (&(a, b), &w) in &between {
            adj[a].push((b, w));
            adj[b].push((a, w));
        }
        let mut degree = vec![0.0; k];
        for c in 0..k {
            degree[c] = 2.0 * self_loop[c] + adj[c].iter().map(|&(_, w)| w).sum::<f64>();
        }
        Level {
            n: k,
            adj,
            self_loop,
            degree,
            m2: self.m2,
        }
    }
}

/// Densify community ids into `0..k`, preserving first-seen order, and return the count.
fn renumber(comm: &[usize]) -> (Vec<usize>, usize) {
    let mut map: HashMap<usize, usize> = HashMap::new();
    let mut out = Vec::with_capacity(comm.len());
    for &c in comm {
        let next = map.len();
        let id = *map.entry(c).or_insert(next);
        out.push(id);
    }
    let k = map.len();
    (out, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_is_zero() {
        let r = analyze(0, &[], &[]);
        assert_eq!(r.q_declared, 0.0);
        assert_eq!(r.q_detected, 0.0);
    }

    #[test]
    fn no_edges_has_zero_modularity() {
        // Three isolated modules: any partition has Q = 0.
        let r = analyze(3, &[], &[0, 1, 2]);
        assert_eq!(r.q_declared, 0.0);
        assert_eq!(r.q_detected, 0.0);
    }

    #[test]
    fn two_clear_clusters_have_positive_modularity() {
        // Two triangles {0,1,2} and {3,4,5} joined by a single bridge edge 2-3.
        let edges = [
            (0, 1),
            (1, 2),
            (2, 0),
            (3, 4),
            (4, 5),
            (5, 3),
            (2, 3), // bridge
        ];
        let declared = [0, 0, 0, 1, 1, 1];
        let r = analyze(6, &edges, &declared);
        // Q = 2 * (6/14 - (7/14)^2) = 5/14 for two triangles joined by one bridge edge.
        assert!(
            (r.q_declared - 5.0 / 14.0).abs() < 1e-9,
            "q_declared = {}",
            r.q_declared
        );
        // Louvain finds the same two clusters (or better), so detected >= declared.
        assert!(
            r.q_detected >= r.q_declared - 1e-9,
            "detected {} should be >= declared {}",
            r.q_detected,
            r.q_declared
        );
        assert_eq!(r.communities_detected, 2);
    }

    #[test]
    fn declared_partition_can_lag_detected() {
        // Real structure is two triangles, but the declared partition lumps everything into one
        // package -> declared Q = 0, detected Q > 0, so the gap exposes the mismatch.
        let edges = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (2, 3)];
        let one_package = [0, 0, 0, 0, 0, 0];
        let r = analyze(6, &edges, &one_package);
        assert_eq!(r.q_declared, 0.0, "everything in one community scores 0");
        assert!(
            r.gap() > 0.3,
            "gap should expose the hidden structure: {}",
            r.gap()
        );
        assert_eq!(r.communities_declared, 1);
    }

    #[test]
    fn modularity_of_a_single_clique_partition() {
        // A triangle, all in one community: Σ_in = 6 (3 edges × 2), 2m = 6, Σ_tot = 6.
        // Q = 6/6 - (6/6)^2 = 1 - 1 = 0.
        let edges = [(0, 1), (1, 2), (2, 0)];
        let r = analyze(3, &edges, &[0, 0, 0]);
        assert!(r.q_declared.abs() < 1e-9, "q = {}", r.q_declared);
    }

    #[test]
    fn detection_is_deterministic() {
        let edges = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (2, 3)];
        let declared = [0, 0, 0, 1, 1, 1];
        let a = analyze(6, &edges, &declared);
        let b = analyze(6, &edges, &declared);
        assert_eq!(a, b);
    }
}
