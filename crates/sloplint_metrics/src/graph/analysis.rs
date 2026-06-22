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
