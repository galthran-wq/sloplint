//! Whole-graph analysis over the built [`super::ImportGraph`]: dependency-cycle tangles (Tarjan
//! SCC at three edge filters — full / runtime / load-bearing), propagation cost (density of the
//! module reachability matrix), and Newman–Girvan modularity of the package partition.

use std::collections::HashMap;

use petgraph::visit::EdgeRef;

use super::{CycleReport, EdgeKind, ImportGraph};

impl ImportGraph {
    /// Cyclic-dependency tangles over the **full** module graph (every resolved import,
    /// including `if TYPE_CHECKING:`-only and function-local edges).
    pub fn cycles(&self) -> CycleReport {
        self.scc(|_| true)
    }

    /// Cyclic-dependency tangles over the **runtime** graph — edges that exist only under an
    /// `if TYPE_CHECKING:` guard are dropped, since they never execute at runtime. A tangle that
    /// survives this is a real circular dependency; one that disappears was benign type-checking
    /// coupling. (Function-local imports are kept: they are deferred but do run.)
    pub fn runtime_cycles(&self) -> CycleReport {
        self.scc(|kind| kind.runtime || kind.local)
    }

    /// Cyclic-dependency tangles over the **load-bearing** graph — only module-top-level runtime
    /// edges. Drops `if TYPE_CHECKING:` edges (like [`Self::runtime_cycles`]) *and*
    /// function-local/deferred imports, which are written inside function bodies precisely to defer
    /// a back-edge past module load. A tangle surviving here is a genuine load-time circular
    /// dependency that *can* raise `ImportError` at load (notably `from x import name` against a
    /// partially-initialized module); one that disappears was worked around with deferred imports —
    /// the milder smell. `kind.runtime` is exactly "at least one contributing import is neither
    /// type-checking nor function-local".
    ///
    /// Note: this is **not** a strict subset of [`Self::cycles`] by *count*. Removing edges can
    /// split one large SCC into several smaller non-trivial ones, so `load_bearing_cycles().len()`
    /// can exceed `cycles().len()`. The participating-module *set* only shrinks; the tangle count
    /// need not. The robust signal is `== 0` (no hard cycles) vs `> 0` (some).
    pub fn load_bearing_cycles(&self) -> CycleReport {
        self.scc(|kind| kind.runtime)
    }

    /// Run Tarjan SCC over the subgraph whose edges satisfy `keep`, returning the non-trivial
    /// components as a deterministic [`CycleReport`].
    fn scc(&self, keep: impl Fn(&EdgeKind) -> bool) -> CycleReport {
        let filtered =
            petgraph::visit::EdgeFiltered::from_fn(&self.graph, |edge| keep(edge.weight()));
        let mut tangles: Vec<Vec<String>> = petgraph::algo::tarjan_scc(&filtered)
            .into_iter()
            .filter(|component| component.len() > 1)
            .map(|component| {
                let mut names: Vec<String> =
                    component.iter().map(|&n| self.graph[n].clone()).collect();
                names.sort();
                names
            })
            .collect();
        // Largest first, then by first member — stable, reproducible ordering.
        tangles.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));
        CycleReport { tangles }
    }

    /// Propagation cost (MacCormack, Rusnak & Baldwin 2006, *Exploring the Structure of Complex
    /// Software Designs*): the **density of the module reachability matrix** — the average
    /// fraction of the system reachable, directly or transitively, from a module:
    ///
    /// ```text
    /// propagation_cost = |{(a, b) : a can reach b}| / N^2
    /// ```
    ///
    /// `1.0` means every module can reach every other (maximally brittle — a change anywhere can
    /// ripple everywhere); low values mean changes stay local. Cycles inflate it, so it pairs
    /// with the SCC metric. The **diagonal is included** (a module reaches itself), following
    /// MacCormack — so a lone module scores `1.0` and a fully disconnected set of `N` modules
    /// scores `1/N`.
    ///
    /// Computed by a DFS from each node over the full import graph — cheaper than Floyd–Warshall
    /// on a sparse graph. Returns `0.0` for an empty graph.
    ///
    /// Unlike the cycle metric there is intentionally no runtime-only variant: an
    /// `if TYPE_CHECKING:` import is still a real source-level coupling a developer must reason
    /// about when changing an interface, which is exactly the change-impact surface this measures.
    pub fn propagation_cost(&self) -> f64 {
        let n = self.graph.node_count();
        if n == 0 {
            return 0.0;
        }
        let mut reachable_pairs = 0usize;
        for start in self.graph.node_indices() {
            // `Dfs` yields `start` first, so the count includes the diagonal entry.
            let mut dfs = petgraph::visit::Dfs::new(&self.graph, start);
            while dfs.next(&self.graph).is_some() {
                reachable_pairs += 1;
            }
        }
        reachable_pairs as f64 / (n as f64 * n as f64)
    }

    /// Newman–Girvan modularity: Q of the declared package partition vs. Q of the
    /// Louvain-detected community structure, over the undirected projection of the module graph.
    /// The gap between them flags packages-in-name-only. See [`crate::modularity`].
    pub fn modularity(&self) -> crate::modularity::ModularityReport {
        // Declared partition: map each module's package (directory) to a dense community id, in
        // node-index order so the assignment is deterministic.
        let mut package_id: HashMap<String, usize> = HashMap::new();
        let mut declared = vec![0usize; self.graph.node_count()];
        for node in self.graph.node_indices() {
            let pkg = self.package_of_node(&self.graph[node]);
            let next = package_id.len();
            declared[node.index()] = *package_id.entry(pkg).or_insert(next);
        }
        let edges: Vec<(usize, usize)> = self
            .graph
            .edge_references()
            .map(|e| (e.source().index(), e.target().index()))
            .collect();
        crate::modularity::analyze(self.graph.node_count(), &edges, &declared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::graph_of;

    #[test]
    fn propagation_cost_empty_and_single() {
        // No modules -> 0.0 (degenerate, defined to avoid NaN).
        assert_eq!(ImportGraph::build(Vec::new()).propagation_cost(), 0.0);
        // A lone module reaches only itself: 1/1^2 = 1.0 (the diagonal is counted).
        let g = graph_of(&[("solo.py", "")]);
        assert_eq!(g.propagation_cost(), 1.0);
    }

    #[test]
    fn propagation_cost_disconnected_is_one_over_n() {
        // Two modules, no first-party edges: each reaches only itself -> 2/4 = 0.5 = 1/N.
        let g = graph_of(&[("a.py", "import os\n"), ("b.py", "import sys\n")]);
        assert!((g.propagation_cost() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn propagation_cost_linear_chain() {
        // a -> b -> c. Reachable (incl. self): a={a,b,c}=3, b={b,c}=2, c={c}=1 -> 6/9.
        let g = graph_of(&[("a.py", "import b\n"), ("b.py", "import c\n"), ("c.py", "")]);
        assert!((g.propagation_cost() - 6.0 / 9.0).abs() < 1e-9);
    }

    #[test]
    fn propagation_cost_is_one_for_a_full_cycle() {
        // a <-> b: each reaches both -> 4/4 = 1.0. Cycles maximize propagation cost.
        let g = graph_of(&[("a.py", "import b\n"), ("b.py", "import a\n")]);
        assert_eq!(g.propagation_cost(), 1.0);
    }

    #[test]
    fn no_cycles_in_an_acyclic_graph() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            ("pkg/b.py", ""),
        ]);
        let report = g.cycles();
        assert_eq!(report.tangle_count(), 0);
        assert_eq!(report.largest_tangle(), 0);
        assert_eq!(report.modules_in_cycles(), 0);
        assert!(g.package_rows().iter().all(|r| !r.in_cycle));
    }

    #[test]
    fn detects_a_two_module_mutual_import() {
        // The minimal cycle: a <-> b.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            ("pkg/b.py", "from pkg import a\n"),
        ]);
        let report = g.cycles();
        assert_eq!(
            report.tangles,
            vec![vec!["pkg.a".to_string(), "pkg.b".to_string()]]
        );
        assert_eq!(report.largest_tangle(), 2);
        assert_eq!(report.modules_in_cycles(), 2);
    }

    #[test]
    fn larger_tangles_sort_first_and_members_are_sorted() {
        // One 3-cycle (x<->y<->z) and one 2-cycle (p<->q); the 3-cycle must come first.
        let g = graph_of(&[
            ("p.py", "import q\n"),
            ("q.py", "import p\n"),
            ("x.py", "import y\n"),
            ("y.py", "import z\n"),
            ("z.py", "import x\n"),
        ]);
        let report = g.cycles();
        assert_eq!(
            report.tangles,
            vec![
                vec!["x".to_string(), "y".to_string(), "z".to_string()],
                vec!["p".to_string(), "q".to_string()],
            ]
        );
    }

    #[test]
    fn type_checking_only_cycle_is_runtime_benign() {
        // a imports b normally; b imports a only under TYPE_CHECKING. The cycle exists in the
        // full graph but vanishes at runtime.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            (
                "pkg/b.py",
                "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from pkg import a\n",
            ),
        ]);
        assert_eq!(g.cycles().tangle_count(), 1, "full graph has the cycle");
        assert_eq!(
            g.runtime_cycles().tangle_count(),
            0,
            "the cycle only closes via a TYPE_CHECKING edge"
        );
    }

    #[test]
    fn function_local_back_edge_still_closes_a_runtime_cycle() {
        // A function-local import is deferred but does execute, so it keeps the runtime cycle.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            (
                "pkg/b.py",
                "def f():\n    from pkg import a\n    return a\n",
            ),
        ]);
        assert_eq!(g.runtime_cycles().tangle_count(), 1);
        // ...but it's deferred, so it is NOT a load-bearing (load-time) cycle.
        assert_eq!(
            g.load_bearing_cycles().tangle_count(),
            0,
            "the back-edge is a function-local import, deferred past module load"
        );
    }

    #[test]
    fn load_bearing_cycle_is_a_hard_top_level_cycle() {
        // Both directions are module-top-level runtime imports → a real load-time cycle.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            ("pkg/b.py", "from pkg import a\n"),
        ]);
        assert_eq!(g.cycles().tangle_count(), 1);
        assert_eq!(g.runtime_cycles().tangle_count(), 1);
        assert_eq!(
            g.load_bearing_cycles().tangle_count(),
            1,
            "both edges run at module load → a hard cycle"
        );
    }

    #[test]
    fn load_bearing_count_is_not_a_subset_of_full_tangles() {
        // Two hard 2-cycles (a<->b, c<->d) bridged into one big SCC by a top-level b->c and a
        // function-local-only d->a. The full graph is a single tangle; dropping the deferred bridge
        // SPLITS it into two hard cycles — so load_bearing_tangles (2) > tangles (1). The count is
        // not a strict subset; only the participating-module set shrinks.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            ("pkg/b.py", "from pkg import a\nfrom pkg import c\n"),
            ("pkg/c.py", "from pkg import d\n"),
            (
                "pkg/d.py",
                "from pkg import c\ndef f():\n    from pkg import a\n    return a\n",
            ),
        ]);
        assert_eq!(g.cycles().tangle_count(), 1, "one big SCC over all edges");
        assert_eq!(
            g.load_bearing_cycles().tangle_count(),
            2,
            "dropping the deferred d->a splits it into two hard cycles"
        );
    }
}
