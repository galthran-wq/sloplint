//! First-party module import dependency graph (issue #65).
//!
//! Builds the project's own import graph — nodes are modules (`.py` files), edges are imports
//! that resolve to another first-party module — then collapses module→module edges to
//! **package (directory) → package** edges so per-package coupling can be computed. This is
//! the foundation the rest of the package-metrics epic (cycles, instability, propagation cost,
//! modularity) builds on; no metric is *interpreted* here, only the structure is produced.
//!
//! Resolution mirrors [grimp](https://github.com/seddonym/grimp), the reference Python
//! import-graph library. The two rules worth stating explicitly, because the behavior must be
//! reproducible:
//!
//! 1. **Name-vs-submodule** ([`resolve_internal`]): a candidate `a.b.c` resolves to the module
//!    `a.b.c` if one exists, else to its parent `a.b` if *that* is a module (the name `c` is an
//!    attribute re-exported by the package `a.b`), else it is third-party / unresolved. Only
//!    one trailing component is stripped — never deeper. This single rule transparently handles
//!    `import a.b.c`, `from a.b import c` (submodule or name), `from x import *`, and
//!    `__init__.py` re-exports.
//! 2. **Relative anchoring** ([`relative_anchor`]): the leading-dot count maps to an anchor
//!    package, with the off-by-one between a regular module and an `__init__.py` package — for
//!    `a.b.c` a single `.` anchors to its parent `a.b`, but for the package `a.b` a single `.`
//!    anchors to *itself*.
//!
//! Marked-but-counted edges: imports under `if TYPE_CHECKING:` (real coupling for cycles, not
//! runtime) and function-local imports (often added to *break* a cycle) are recorded with a
//! flag rather than dropped, so later metrics can include or exclude them deliberately.
//!
//! Out of scope (documented, not handled): dynamic `importlib.import_module` / `__import__`
//! calls — they are ordinary call expressions, not `import` statements, so no edge is recorded.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use sloplint_python::ast::{Expr, ModModule, Stmt};
use sloplint_python::parser::Parsed;

/// The dotted name of a discovered module, plus whether the file is a package `__init__.py`
/// (which is the package node itself, not a `…__init__` module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleName {
    /// Dotted importable name, e.g. `pkg.sub.mod` or `pkg.sub` for `pkg/sub/__init__.py`.
    pub dotted: String,
    /// `true` for an `__init__.py` (and PEP 420 namespace dirs) — the node represents the
    /// package itself. Relative-import anchoring differs for packages (see [`relative_anchor`]).
    pub is_package: bool,
}

/// A single `import` / `from … import …` statement found in a file, before resolution.
///
/// One [`RawImport`] is produced per `import a` alias and per `from a.b import x` statement
/// (the imported names are kept together so the candidates can be generated against the
/// importer's context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImport {
    /// Leading-dot count: `0` for an absolute import, `1` for `from . import`, `2` for `..`, …
    pub level: u32,
    /// The dotted module part: `a.b.c` for `import a.b.c`, `a.b` for `from a.b import x`, and
    /// `None` for `from . import x` (a bare relative import with no module component).
    pub module: Option<String>,
    /// Imported names — `x` in `from a.b import x`. `*` is recorded literally; empty for a
    /// plain `import a.b.c`.
    pub names: Vec<String>,
    /// Recorded under an `if TYPE_CHECKING:` guard (real coupling for cycles, not runtime).
    pub type_checking: bool,
    /// Recorded inside a function body (a local import, often added to break a cycle).
    pub local: bool,
}

/// A discovered module and the imports scanned from its source — the input to [`ImportGraph`].
#[derive(Debug, Clone)]
pub struct ModuleInput {
    pub name: ModuleName,
    pub imports: Vec<RawImport>,
    /// Physical lines of code in the module file, summed into the owning package's `loc` (#67).
    pub loc: usize,
    /// Classes defined in this module, summed into the owning package's `classes` (#70).
    pub classes: usize,
    /// Of those, the ones counted as abstract (see `class_is_abstract`), summed per package (#70).
    pub abstract_classes: usize,
}

/// Merged flags on a resolved module→module edge. An edge can be produced by several import
/// statements at once (e.g. a runtime import *and* a `TYPE_CHECKING` one), so each flag is the
/// OR across every contributing import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EdgeKind {
    /// At least one contributing import is neither type-checking-only nor function-local —
    /// i.e. a genuine module-top-level runtime dependency.
    pub runtime: bool,
    /// At least one contributing import is under `if TYPE_CHECKING:`.
    pub type_checking: bool,
    /// At least one contributing import is function-local.
    pub local: bool,
}

impl EdgeKind {
    fn merge(&mut self, import: &RawImport) {
        self.type_checking |= import.type_checking;
        self.local |= import.local;
        self.runtime |= !import.type_checking && !import.local;
    }
}

/// One row of the `metrics --format packages` feed: a package (directory) and its first-party
/// coupling. `imports`/`imported_by` are the distinct *packages* this one depends on / is
/// depended on by; their sizes are the efferent/afferent coupling that drive `instability` (#67).
///
/// Counting *distinct packages* (not individual module-to-module dependencies) for Ce/Ca mirrors
/// JDepend's `efferentCoupling()`/`afferentCoupling()` (`efferents.size()`/`afferents.size()`),
/// the reference implementation of Martin's package metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageRow {
    /// Dotted package name, or `.` for the project root (top-level modules).
    pub package: String,
    /// Number of modules (`.py` files) directly in this package.
    pub modules: usize,
    /// Physical lines of code summed across this package's modules.
    pub loc: usize,
    /// Distinct first-party packages this package imports (efferent), sorted. `len()` is Ce.
    pub imports: Vec<String>,
    /// Distinct first-party packages that import this one (afferent), sorted. `len()` is Ca.
    pub imported_by: Vec<String>,
    /// Whether any module in this package participates in a module-level dependency cycle
    /// (a non-trivial SCC of the full import graph, see [`ImportGraph::cycles`]).
    pub in_cycle: bool,
    /// Martin's instability `I = Ce / (Ce + Ca)` ∈ [0, 1], or `0.0` when `Ce + Ca == 0`
    /// (a package with no first-party coupling is treated as maximally stable, as in JDepend).
    pub instability: f64,
    /// Total classes defined across this package's modules (#70).
    pub classes: usize,
    /// Of those, the ones counted as abstract by the heuristic (#70).
    pub abstract_classes: usize,
    /// Martin's abstractness `A = abstract_classes / classes` ∈ [0, 1], or `0.0` when there are
    /// no classes (matching JDepend). A *heuristic* in Python — see `class_is_abstract`.
    pub abstractness: f64,
    /// Distance from the main sequence `D = |A + I − 1|` ∈ [0, 1] (#70). High `D` flags the
    /// "zone of pain" (concrete + heavily depended on) or "zone of uselessness" (abstract +
    /// unused). Weakly validated — design guidance more than a defect signal.
    pub distance: f64,
}

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

/// Node-distribution concentration of modules across packages (issue #103) — the first
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

/// Cyclic-dependency tangles found by running Tarjan's SCC over the module import graph
/// (issue #66) — being inside a large cycle is one of the best-validated module-level defect
/// predictors. Each tangle is a strongly-connected component of size > 1 (a 2-module mutual
/// import is the minimal cycle); a lone module is not a tangle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReport {
    /// The tangles. Each is a sorted list of member module names; the list as a whole is sorted
    /// by size (largest first) then by first member, for deterministic output.
    pub tangles: Vec<Vec<String>>,
}

impl CycleReport {
    /// Number of non-trivial cycles.
    pub fn tangle_count(&self) -> usize {
        self.tangles.len()
    }

    /// Size of the largest tangle (0 when there are no cycles).
    pub fn largest_tangle(&self) -> usize {
        self.tangles.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Total number of modules participating in any cycle. SCCs are disjoint, so this is just
    /// the sum of the tangle sizes.
    pub fn modules_in_cycles(&self) -> usize {
        self.tangles.iter().map(Vec::len).sum()
    }
}

/// Per-project rollup placed in `metrics --format json` — the foundation figures. Cycles,
/// propagation cost, and modularity (issues #66–#69) will extend this struct.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectImportSummary {
    /// First-party modules (`.py` files) discovered.
    pub modules: usize,
    /// Packages (directories holding ≥1 module), including the root.
    pub packages: usize,
    /// First-party module→module edges (deduplicated, self-edges excluded).
    pub module_edges: usize,
    /// First-party package→package edges (cross-package, deduplicated).
    pub package_edges: usize,
}

/// The first-party import graph: a module-level directed graph, from which package-level
/// coupling is derived on demand.
pub struct ImportGraph {
    graph: DiGraph<String, EdgeKind>,
    /// `dotted name -> node index`, and the package flag for each module.
    index: HashMap<String, NodeIndex>,
    is_package: HashMap<String, bool>,
    /// `dotted name -> physical lines of code`, summed per package for `PackageRow::loc`.
    loc: HashMap<String, usize>,
    /// `dotted name -> (total classes, abstract classes)`, summed per package for abstractness.
    class_counts: HashMap<String, (usize, usize)>,
}

impl ImportGraph {
    /// Build the graph from the project's modules. The full module set is fixed first (an
    /// import only becomes an edge if it resolves into this set), then each module's imports
    /// are resolved and added as edges.
    pub fn build(modules: Vec<ModuleInput>) -> Self {
        let module_set: HashSet<String> = modules.iter().map(|m| m.name.dotted.clone()).collect();

        let mut graph = DiGraph::new();
        let mut index = HashMap::new();
        let mut is_package = HashMap::new();
        let mut loc = HashMap::new();
        let mut class_counts = HashMap::new();
        for module in &modules {
            let node = graph.add_node(module.name.dotted.clone());
            index.insert(module.name.dotted.clone(), node);
            is_package.insert(module.name.dotted.clone(), module.name.is_package);
            loc.insert(module.name.dotted.clone(), module.loc);
            class_counts.insert(
                module.name.dotted.clone(),
                (module.classes, module.abstract_classes),
            );
        }

        for module in &modules {
            let from = index[&module.name.dotted];
            // Merge multiple imports that resolve to the same target into one edge.
            let mut edges: BTreeMap<String, EdgeKind> = BTreeMap::new();
            for import in &module.imports {
                for candidate in candidates(&module.name, import) {
                    if let Some(target) = resolve_internal(&candidate, &module_set) {
                        if target != module.name.dotted {
                            edges.entry(target).or_default().merge(import);
                        }
                    }
                }
            }
            for (target, kind) in edges {
                let to = index[&target];
                graph.add_edge(from, to, kind);
            }
        }

        ImportGraph {
            graph,
            index,
            is_package,
            loc,
            class_counts,
        }
    }

    /// The resolved first-party edges as `(importer, imported, kind)`, sorted — the raw graph,
    /// exposed for tests and for the package-level metrics built on top.
    pub fn module_edges(&self) -> Vec<(String, String, EdgeKind)> {
        let mut out: Vec<(String, String, EdgeKind)> = self
            .graph
            .edge_references()
            .map(|edge| {
                (
                    self.graph[edge.source()].clone(),
                    self.graph[edge.target()].clone(),
                    *edge.weight(),
                )
            })
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        out
    }

    /// The package (directory) of a module node by dotted name.
    fn package_of_node(&self, module: &str) -> String {
        package_of(
            module,
            self.is_package.get(module).copied().unwrap_or(false),
        )
    }

    /// One row per package, sorted by package name. Every package with ≥1 module gets a row,
    /// even one with no first-party imports (so the feed mirrors the module set).
    pub fn package_rows(&self) -> Vec<PackageRow> {
        // A package is "in a cycle" if any of its modules is in a non-trivial SCC.
        let report = self.cycles();
        let cycle_modules: HashSet<&str> = report
            .tangles
            .iter()
            .flatten()
            .map(String::as_str)
            .collect();

        let mut module_count: BTreeMap<String, usize> = BTreeMap::new();
        let mut in_cycle: BTreeMap<String, bool> = BTreeMap::new();
        let mut loc: BTreeMap<String, usize> = BTreeMap::new();
        let mut classes: BTreeMap<String, usize> = BTreeMap::new();
        let mut abstract_classes: BTreeMap<String, usize> = BTreeMap::new();
        let mut efferent: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut afferent: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for name in self.index.keys() {
            let pkg = self.package_of_node(name);
            *module_count.entry(pkg.clone()).or_default() += 1;
            *in_cycle.entry(pkg.clone()).or_default() |= cycle_modules.contains(name.as_str());
            *loc.entry(pkg.clone()).or_default() += self.loc.get(name).copied().unwrap_or(0);
            let (total, abstract_) = self.class_counts.get(name).copied().unwrap_or((0, 0));
            *classes.entry(pkg.clone()).or_default() += total;
            *abstract_classes.entry(pkg.clone()).or_default() += abstract_;
            efferent.entry(pkg.clone()).or_default();
            afferent.entry(pkg).or_default();
        }

        for edge in self.graph.edge_references() {
            let from_pkg = self.package_of_node(&self.graph[edge.source()]);
            let to_pkg = self.package_of_node(&self.graph[edge.target()]);
            if from_pkg != to_pkg {
                efferent.get_mut(&from_pkg).unwrap().insert(to_pkg.clone());
                afferent.get_mut(&to_pkg).unwrap().insert(from_pkg);
            }
        }

        module_count
            .into_iter()
            .map(|(package, modules)| {
                let imports: Vec<String> = efferent[&package].iter().cloned().collect();
                let imported_by: Vec<String> = afferent[&package].iter().cloned().collect();
                let instability = instability(imports.len(), imported_by.len());
                let total_classes = classes[&package];
                let abstract_count = abstract_classes[&package];
                let abstractness = abstractness(abstract_count, total_classes);
                PackageRow {
                    distance: distance(abstractness, instability),
                    abstractness,
                    classes: total_classes,
                    abstract_classes: abstract_count,
                    instability,
                    loc: loc[&package],
                    in_cycle: in_cycle[&package],
                    imports,
                    imported_by,
                    package,
                    modules,
                }
            })
            .collect()
    }

    /// The per-project rollup for the JSON feed.
    pub fn summary(&self) -> ProjectImportSummary {
        let mut packages: BTreeSet<String> = BTreeSet::new();
        for name in self.index.keys() {
            packages.insert(self.package_of_node(name));
        }
        let mut package_edges: BTreeSet<(String, String)> = BTreeSet::new();
        for edge in self.graph.edge_references() {
            let from_pkg = self.package_of_node(&self.graph[edge.source()]);
            let to_pkg = self.package_of_node(&self.graph[edge.target()]);
            if from_pkg != to_pkg {
                package_edges.insert((from_pkg, to_pkg));
            }
        }
        ProjectImportSummary {
            modules: self.graph.node_count(),
            packages: packages.len(),
            module_edges: self.graph.edge_count(),
            package_edges: package_edges.len(),
        }
    }

    /// Module-count [`Concentration`] of this graph's modules across their packages (issue #103).
    /// Edge-free — it reads only the node→package mapping, so it's meaningful even on a graph with
    /// no resolved imports.
    pub fn concentration(&self) -> Concentration {
        let module_packages: Vec<String> = self
            .index
            .keys()
            .map(|name| self.package_of_node(name))
            .collect();
        concentration(&module_packages)
    }

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

    /// Newman–Girvan modularity (issue #69): Q of the declared package partition vs. Q of the
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

/// Derive a module's dotted name from a file path *relative to its source root*: an
/// `__init__.py` collapses to the package itself, and the remaining path becomes a dotted name.
/// Returns `None` for paths that don't name an importable module (e.g. a bare `__init__.py` at
/// the root).
///
/// The CLI feeds this a source-root-relative path (computed by an `__init__.py` walk-up that
/// already handles `src/` layout). The leading-`src/` strip below is a belt-and-suspenders
/// fallback for direct callers that pass a full path without doing that walk-up — it never
/// fires on the CLI path, so the two layers don't double-strip.
pub fn module_from_path(path: &str) -> Option<ModuleName> {
    let mut segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.first() == Some(&"src") && segs.len() > 1 {
        segs.remove(0);
    }
    let last = segs.pop()?;
    if last == "__init__.py" {
        // The package itself; `pkg/sub/__init__.py` -> `pkg.sub`.
        if segs.is_empty() {
            return None; // a top-level `__init__.py` names no importable package
        }
        return Some(ModuleName {
            dotted: segs.join("."),
            is_package: true,
        });
    }
    let stem = last.strip_suffix(".py")?;
    if stem.is_empty() {
        return None;
    }
    segs.push(stem);
    Some(ModuleName {
        dotted: segs.join("."),
        is_package: false,
    })
}

/// The package (directory) that owns a module: a package module (`__init__.py`) *is* its own
/// package; a regular module `a.b.c` belongs to `a.b`; a top-level module belongs to the root
/// package, written `.`.
pub fn package_of(module: &str, is_package: bool) -> String {
    if is_package {
        return module.to_string();
    }
    match module.rsplit_once('.') {
        Some((parent, _)) => parent.to_string(),
        None => ".".to_string(),
    }
}

/// Resolve an absolute candidate dotted name against the fixed module set, applying grimp's
/// name-vs-submodule rule: exact module, else the parent (one component stripped) if *that* is a
/// module, else `None` (third-party or unresolved). Handles `from x import *` (the `*` strips to
/// `x`) and `__init__.py` re-exports for free.
pub fn resolve_internal(candidate: &str, modules: &HashSet<String>) -> Option<String> {
    if candidate.is_empty() {
        return None;
    }
    if modules.contains(candidate) {
        return Some(candidate.to_string());
    }
    if let Some((parent, _)) = candidate.rsplit_once('.') {
        if modules.contains(parent) {
            return Some(parent.to_string());
        }
    }
    None
}

/// The anchor (base) package for a relative import, or `None` if the dots reach above the
/// project root. The leading-dot count `level` maps differently for a package vs a regular
/// module: for the package `a.b`, one dot anchors to `a.b` itself; for the regular module
/// `a.b.c`, one dot anchors to its parent `a.b`. Returned as path components.
pub fn relative_anchor(importer: &ModuleName, level: u32) -> Option<Vec<String>> {
    let comps: Vec<&str> = importer.dotted.split('.').collect();
    // The package the importer lives in: the module itself if it's a package, else its parent.
    let pkg_len = if importer.is_package {
        comps.len()
    } else {
        comps.len().saturating_sub(1)
    };
    let drop = (level - 1) as usize; // one dot stays at the importer's own package
    let keep = pkg_len.checked_sub(drop)?;
    Some(comps[..keep].iter().map(|s| s.to_string()).collect())
}

/// All absolute candidate dotted names an import contributes, in the importer's context.
/// Each candidate is then run through [`resolve_internal`].
fn candidates(importer: &ModuleName, import: &RawImport) -> Vec<String> {
    // Build the base path components: the module part for an absolute import, or the relative
    // anchor plus module part for a relative one.
    let mut base: Vec<String> = if import.level == 0 {
        Vec::new()
    } else {
        match relative_anchor(importer, import.level) {
            Some(anchor) => anchor,
            None => return Vec::new(), // relative import escapes the project root
        }
    };
    if let Some(module) = &import.module {
        base.extend(module.split('.').map(|s| s.to_string()));
    }

    if import.names.is_empty() {
        // A plain `import a.b.c` (or a relative import with no names, which is invalid Python).
        let joined = base.join(".");
        return if joined.is_empty() {
            Vec::new()
        } else {
            vec![joined]
        };
    }

    import
        .names
        .iter()
        .map(|name| {
            let mut full = base.clone();
            full.push(name.clone());
            full.join(".")
        })
        .collect()
}

/// Scan a parsed module for every `import` / `from … import …` statement, recording the
/// `TYPE_CHECKING` and function-local context of each (see [`RawImport`]).
pub fn scan_module_imports(parsed: &Parsed<ModModule>) -> Vec<RawImport> {
    let mut out = Vec::new();
    collect_imports(&parsed.syntax().body, Ctx::default(), &mut out);
    out
}

/// Walk context: whether we are inside a function body (local) or an `if TYPE_CHECKING:` block.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    local: bool,
    type_checking: bool,
}

fn collect_imports(body: &[Stmt], ctx: Ctx, out: &mut Vec<RawImport>) {
    for stmt in body {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    out.push(RawImport {
                        level: 0,
                        module: Some(alias.name.to_string()),
                        names: Vec::new(),
                        type_checking: ctx.type_checking,
                        local: ctx.local,
                    });
                }
            }
            Stmt::ImportFrom(from) => {
                out.push(RawImport {
                    level: from.level,
                    module: from.module.as_ref().map(|m| m.to_string()),
                    names: from.names.iter().map(|a| a.name.to_string()).collect(),
                    type_checking: ctx.type_checking,
                    local: ctx.local,
                });
            }
            // A function body is a local scope: imports inside it are function-local.
            Stmt::FunctionDef(node) => {
                collect_imports(&node.body, ctx.into_local(), out);
            }
            Stmt::ClassDef(node) => collect_imports(&node.body, ctx, out),
            Stmt::If(node) => {
                // Only the `if TYPE_CHECKING:` body gets the flag; elif/else keep the context.
                let tc = ctx.type_checking || is_type_checking_test(&node.test);
                collect_imports(&node.body, ctx.with_type_checking(tc), out);
                for clause in &node.elif_else_clauses {
                    collect_imports(&clause.body, ctx, out);
                }
            }
            Stmt::For(node) => {
                collect_imports(&node.body, ctx, out);
                collect_imports(&node.orelse, ctx, out);
            }
            Stmt::While(node) => {
                collect_imports(&node.body, ctx, out);
                collect_imports(&node.orelse, ctx, out);
            }
            Stmt::With(node) => collect_imports(&node.body, ctx, out),
            Stmt::Try(node) => {
                collect_imports(&node.body, ctx, out);
                for handler in &node.handlers {
                    let sloplint_python::ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_imports(&handler.body, ctx, out);
                }
                collect_imports(&node.orelse, ctx, out);
                collect_imports(&node.finalbody, ctx, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_imports(&case.body, ctx, out);
                }
            }
            _ => {}
        }
    }
}

impl Ctx {
    fn into_local(mut self) -> Self {
        self.local = true;
        self
    }
    fn with_type_checking(mut self, value: bool) -> Self {
        self.type_checking = value;
        self
    }
}

/// Whether an `if` test is a `TYPE_CHECKING` guard: a bare `TYPE_CHECKING` name or any
/// attribute access ending in `.TYPE_CHECKING` (e.g. `typing.TYPE_CHECKING`). Purely syntactic,
/// matching grimp — it does not verify the name was imported from `typing`.
fn is_type_checking_test(test: &Expr) -> bool {
    match test {
        Expr::Name(name) => name.id.as_str() == "TYPE_CHECKING",
        Expr::Attribute(attr) => attr.attr.as_str() == "TYPE_CHECKING",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn module(path: &str) -> ModuleName {
        module_from_path(path).unwrap()
    }

    /// Build a graph from `(path, source)` pairs, the way the CLI does.
    fn graph_of(files: &[(&str, &str)]) -> ImportGraph {
        let inputs = files
            .iter()
            .filter_map(|(path, src)| {
                let name = module_from_path(path)?;
                let parsed = parse(src).unwrap();
                Some(ModuleInput {
                    name,
                    imports: scan_module_imports(&parsed),
                    loc: src.lines().count(),
                    // Class counts are exercised separately (see the abstractness tests, which
                    // build inputs directly); the import-graph tests don't need them.
                    classes: 0,
                    abstract_classes: 0,
                })
            })
            .collect();
        ImportGraph::build(inputs)
    }

    fn imports_of(src: &str) -> Vec<RawImport> {
        scan_module_imports(&parse(src).unwrap())
    }

    #[test]
    fn module_names_from_paths() {
        assert_eq!(module("pkg/sub/mod.py").dotted, "pkg.sub.mod");
        assert!(!module("pkg/sub/mod.py").is_package);
        // __init__.py collapses to the package itself and is flagged.
        let init = module("pkg/sub/__init__.py");
        assert_eq!(init.dotted, "pkg.sub");
        assert!(init.is_package);
        // src-layout: a leading src/ is stripped.
        assert_eq!(module("src/pkg/mod.py").dotted, "pkg.mod");
        // top-level module.
        assert_eq!(module("mod.py").dotted, "mod");
        // non-Python and a bare root __init__ are not modules.
        assert!(module_from_path("README.md").is_none());
        assert!(module_from_path("__init__.py").is_none());
    }

    #[test]
    fn package_of_module() {
        assert_eq!(package_of("pkg.sub", true), "pkg.sub"); // a package is its own package
        assert_eq!(package_of("pkg.sub.mod", false), "pkg.sub"); // regular module -> parent dir
        assert_eq!(package_of("mod", false), "."); // top-level module -> root
    }

    #[test]
    fn resolve_internal_name_vs_submodule() {
        let modules: HashSet<String> = ["a.b", "a.b.c"].iter().map(|s| s.to_string()).collect();
        // exact module wins.
        assert_eq!(
            resolve_internal("a.b.c", &modules).as_deref(),
            Some("a.b.c")
        );
        // a name re-exported by a package falls back to the package (strip one component).
        assert_eq!(
            resolve_internal("a.b.name", &modules).as_deref(),
            Some("a.b")
        );
        // `from a.b import *` -> `a.b.*` -> strip -> `a.b`.
        assert_eq!(resolve_internal("a.b.*", &modules).as_deref(), Some("a.b"));
        // third-party / unresolved.
        assert_eq!(resolve_internal("requests", &modules), None);
        // only one component is ever stripped: `a.b.c.deep` -> `a.b.c` (a module) wins, but
        // `a.x.y` strips to `a.x` which is not a module -> unresolved.
        assert_eq!(resolve_internal("a.x.y", &modules), None);
    }

    #[test]
    fn relative_anchor_off_by_one_for_packages() {
        // regular module a.b.c: one dot anchors to its parent a.b, two dots to a.
        let regular = ModuleName {
            dotted: "a.b.c".into(),
            is_package: false,
        };
        assert_eq!(relative_anchor(&regular, 1).unwrap(), ["a", "b"]);
        assert_eq!(relative_anchor(&regular, 2).unwrap(), ["a"]);
        // package a.b: one dot anchors to itself, two dots to a.
        let package = ModuleName {
            dotted: "a.b".into(),
            is_package: true,
        };
        assert_eq!(relative_anchor(&package, 1).unwrap(), ["a", "b"]);
        assert_eq!(relative_anchor(&package, 2).unwrap(), ["a"]);
        // dots that escape the project root.
        assert!(relative_anchor(&regular, 5).is_none());
    }

    #[test]
    fn scan_marks_type_checking_and_local_imports() {
        let src = "\
import os
from a import b

if TYPE_CHECKING:
    from c import d

def f():
    import e
";
        let imports = imports_of(src);
        let find = |module: &str| {
            imports
                .iter()
                .find(|i| i.module.as_deref() == Some(module))
                .unwrap()
                .clone()
        };
        let os = find("os");
        assert!(!os.type_checking && !os.local);
        let c = find("c");
        assert!(c.type_checking, "import under TYPE_CHECKING is marked");
        let e = find("e");
        assert!(e.local, "import inside a function body is marked local");
    }

    #[test]
    fn scan_records_try_except_and_star_imports() {
        // try/except ImportError fallbacks are recorded (both branches), and `*` is literal.
        let imports = imports_of(
            "\
try:
    from fast import thing
except ImportError:
    from slow import thing

from pkg import *
",
        );
        assert!(imports.iter().any(|i| i.module.as_deref() == Some("fast")));
        assert!(imports.iter().any(|i| i.module.as_deref() == Some("slow")));
        let star = imports
            .iter()
            .find(|i| i.module.as_deref() == Some("pkg"))
            .unwrap();
        assert_eq!(star.names, vec!["*".to_string()]);
    }

    #[test]
    fn typing_dot_type_checking_guard_is_recognized() {
        // `if typing.TYPE_CHECKING:` (attribute form) is detected like the bare name.
        let imports = imports_of("if typing.TYPE_CHECKING:\n    from c import d\n");
        assert!(imports[0].type_checking);
    }

    #[test]
    fn absolute_import_resolves_to_submodule_or_package() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "import pkg.b\nfrom pkg import c\n"),
            ("pkg/b.py", ""),
            ("pkg/c.py", ""),
        ]);
        let edges = g.module_edges();
        let targets: Vec<&str> = edges
            .iter()
            .filter(|(from, _, _)| from == "pkg.a")
            .map(|(_, to, _)| to.as_str())
            .collect();
        // `import pkg.b` -> module pkg.b; `from pkg import c` -> submodule pkg.c.
        assert!(targets.contains(&"pkg.b"));
        assert!(targets.contains(&"pkg.c"));
    }

    #[test]
    fn from_package_import_name_points_at_the_package() {
        // `from pkg import helper` where `helper` is a *name* (not a submodule) -> edge to pkg.
        let g = graph_of(&[
            ("pkg/__init__.py", "helper = 1\n"),
            ("app.py", "from pkg import helper\n"),
        ]);
        let edges = g.module_edges();
        assert_eq!(edges, vec![("app".into(), "pkg".into(), edges[0].2)]);
        assert!(edges[0].2.runtime);
    }

    #[test]
    fn relative_imports_resolve_against_the_importer() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/sub/__init__.py", ""),
            (
                "pkg/sub/a.py",
                "from . import b\nfrom ..util import helper\n",
            ),
            ("pkg/sub/b.py", ""),
            ("pkg/util.py", ""),
        ]);
        let edges = g.module_edges();
        let from_a: Vec<&str> = edges
            .iter()
            .filter(|(from, _, _)| from == "pkg.sub.a")
            .map(|(_, to, _)| to.as_str())
            .collect();
        // `from . import b` -> pkg.sub.b; `from ..util import helper` -> pkg.util.
        assert!(from_a.contains(&"pkg.sub.b"), "got {from_a:?}");
        assert!(from_a.contains(&"pkg.util"), "got {from_a:?}");
    }

    #[test]
    fn third_party_and_self_imports_are_excluded() {
        let g = graph_of(&[("app.py", "import os\nimport requests\nimport app\n")]);
        // os/requests are not first-party; a self-import resolves to itself and is dropped.
        assert!(g.module_edges().is_empty());
    }

    #[test]
    fn type_checking_edge_is_marked_but_kept() {
        let g = graph_of(&[
            ("app.py", "if TYPE_CHECKING:\n    from pkg import thing\n"),
            ("pkg/__init__.py", "thing = 1\n"),
        ]);
        let edges = g.module_edges();
        assert_eq!(edges.len(), 1);
        let kind = edges[0].2;
        assert!(kind.type_checking, "the edge is marked type-checking");
        assert!(!kind.runtime, "and has no runtime contributor");
    }

    #[test]
    fn merged_edge_is_runtime_when_any_import_is() {
        // A runtime import and a function-local import to the same target merge into one edge
        // that is both runtime and local.
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "import pkg.b\n\ndef f():\n    import pkg.b\n"),
            ("pkg/b.py", ""),
        ]);
        let edges = g.module_edges();
        let edge = edges.iter().find(|(f, t, _)| f == "pkg.a" && t == "pkg.b");
        let kind = edge.unwrap().2;
        assert!(kind.runtime && kind.local);
    }

    #[test]
    fn concentration_pure_gini_and_share() {
        // Three packages of equal size: no inequality, share 1/3.
        let equal = concentration(&[
            "a".into(),
            "b".into(),
            "c".into(),
        ]);
        assert_eq!(equal.total_modules, 3);
        assert_eq!(equal.packages, 3);
        assert!((equal.max_package_share - 1.0 / 3.0).abs() < 1e-9);
        assert!(equal.module_count_gini.abs() < 1e-9);
        // Ties: every package has 1 module; the smallest name wins for determinism.
        assert_eq!(equal.largest_package, Some(("a".into(), 1)));

        // One dominant package (3 of 4 modules): high share, positive Gini.
        let skewed = concentration(&[
            "big".into(),
            "big".into(),
            "big".into(),
            "small".into(),
        ]);
        assert!((skewed.max_package_share - 0.75).abs() < 1e-9);
        // Population Gini of [1, 3] is 0.25.
        assert!((skewed.module_count_gini - 0.25).abs() < 1e-9);
        assert_eq!(skewed.largest_package, Some(("big".into(), 3)));
    }

    #[test]
    fn concentration_edge_cases() {
        // Empty: everything zero, no package named.
        let empty = concentration(&[]);
        assert_eq!(empty.total_modules, 0);
        assert_eq!(empty.packages, 0);
        assert_eq!(empty.max_package_share, 0.0);
        assert_eq!(empty.module_count_gini, 0.0);
        assert_eq!(empty.largest_package, None);

        // A single package holds everything: share 1.0, but no inequality (one bucket).
        let single = concentration(&["only".into(), "only".into()]);
        assert_eq!(single.packages, 1);
        assert_eq!(single.max_package_share, 1.0);
        assert_eq!(single.module_count_gini, 0.0);
        assert_eq!(single.largest_package, Some(("only".into(), 2)));
    }

    #[test]
    fn concentration_over_graph_god_package() {
        // A flat "god package" holding most modules vs. a couple of small ones.
        let g = graph_of(&[
            ("dump/__init__.py", ""),
            ("dump/a.py", ""),
            ("dump/b.py", ""),
            ("dump/c.py", ""),
            ("small/__init__.py", ""),
            ("top.py", ""),
        ]);
        let c = g.concentration();
        // 6 modules over 3 packages: dump(4: __init__,a,b,c), small(1), root `.`(top) (1).
        assert_eq!(c.total_modules, 6);
        assert_eq!(c.packages, 3);
        assert_eq!(c.largest_package, Some(("dump".into(), 4)));
        assert!((c.max_package_share - 4.0 / 6.0).abs() < 1e-9);
        assert!(c.module_count_gini > 0.0);
    }

    #[test]
    fn package_rows_aggregate_modules_and_coupling() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg.sub import helper\n"),
            ("pkg/sub/__init__.py", ""),
            ("pkg/sub/helper.py", ""),
            ("top.py", ""),
        ]);
        let rows = g.package_rows();
        let row = |name: &str| rows.iter().find(|r| r.package == name).unwrap();

        // pkg has __init__ + a = 2 modules and imports the pkg.sub package. Ce=1, Ca=0 so it is
        // purely unstable (I=1): it depends on something but nothing depends on it.
        let pkg = row("pkg");
        assert_eq!(pkg.modules, 2);
        assert_eq!(pkg.imports, vec!["pkg.sub".to_string()]);
        assert!(pkg.imported_by.is_empty());
        assert_eq!(pkg.instability, 1.0);

        // pkg.sub has __init__ + helper = 2 modules, imported by pkg. Ce=0, Ca=1 → purely stable.
        let sub = row("pkg.sub");
        assert_eq!(sub.modules, 2);
        assert_eq!(sub.imported_by, vec!["pkg".to_string()]);
        assert_eq!(sub.instability, 0.0);

        // a top-level module lands in the root package `.`, with no first-party coupling. With
        // Ce+Ca=0 instability is defined as 0.0 (not NaN from a 0/0 division).
        let root = row(".");
        assert_eq!(root.modules, 1);
        assert!(root.imports.is_empty());
        assert_eq!(root.instability, 0.0);
    }

    #[test]
    fn package_rows_sum_module_loc_per_package() {
        // loc per package is the sum of its modules' physical line counts.
        let g = graph_of(&[
            ("pkg/__init__.py", "x = 1\n"),          // 1 line
            ("pkg/a.py", "import os\n\n\nx = 2\n"),  // 4 lines
            ("pkg/sub/__init__.py", ""),             // 0 lines
            ("pkg/sub/helper.py", "y = 3\ny = 4\n"), // 2 lines
        ]);
        let rows = g.package_rows();
        let row = |name: &str| rows.iter().find(|r| r.package == name).unwrap();
        assert_eq!(row("pkg").loc, 5); // 1 + 4
        assert_eq!(row("pkg.sub").loc, 2); // 0 + 2
    }

    #[test]
    fn instability_mid_range_for_a_cycle() {
        // A 2-package cycle: each imports the other, so Ce=Ca=1 and I=0.5 on both — the "neither
        // stable nor cleanly unstable" middle the issue calls out as a tangle signal.
        let g = graph_of(&[
            ("a/__init__.py", ""),
            ("a/m.py", "from b import n\n"),
            ("b/__init__.py", ""),
            ("b/n.py", "from a import m\n"),
        ]);
        let rows = g.package_rows();
        let row = |name: &str| rows.iter().find(|r| r.package == name).unwrap();
        assert_eq!(row("a").instability, 0.5);
        assert_eq!(row("b").instability, 0.5);
    }

    #[test]
    fn instability_is_asymmetric_through_real_aggregation() {
        // `a` imports both `b` and `c`, and `b` imports `a` back: a has Ce=2 (b, c) and Ca=1 (b),
        // so I = 2/3 — an asymmetric value computed from the actual graph, not a hand-built row.
        let g = graph_of(&[
            ("a/__init__.py", ""),
            ("a/m.py", "from b import n\nfrom c import p\n"),
            ("b/__init__.py", ""),
            ("b/n.py", "from a import m\n"),
            ("c/__init__.py", ""),
            ("c/p.py", ""),
        ]);
        let rows = g.package_rows();
        let a = rows.iter().find(|r| r.package == "a").unwrap();
        assert_eq!(a.imports, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(a.imported_by, vec!["b".to_string()]);
        assert_eq!(a.instability, 2.0 / 3.0);
    }

    #[test]
    fn instability_formula_edge_cases() {
        assert_eq!(instability(0, 0), 0.0); // uncoupled → stable, no divide-by-zero
        assert_eq!(instability(3, 0), 1.0); // depends on others, depended on by none
        assert_eq!(instability(0, 3), 0.0); // depended on by others, depends on none
        assert_eq!(instability(1, 3), 0.25);
    }

    #[test]
    fn abstractness_and_distance_formula_edge_cases() {
        assert_eq!(abstractness(0, 0), 0.0); // no classes → 0, no divide-by-zero
        assert_eq!(abstractness(0, 4), 0.0); // all concrete
        assert_eq!(abstractness(4, 4), 1.0); // all abstract
        assert_eq!(abstractness(1, 4), 0.25);

        // D = |A + I − 1|: on the main sequence (A+I=1) distance is 0; at the corners it is 1.
        assert_eq!(distance(0.0, 1.0), 0.0); // pure concrete + unstable: ideal
        assert_eq!(distance(1.0, 0.0), 0.0); // pure abstract + stable: ideal
        assert_eq!(distance(0.0, 0.0), 1.0); // zone of pain: concrete + stable
        assert_eq!(distance(1.0, 1.0), 1.0); // zone of uselessness: abstract + unstable
        assert!((distance(0.25, 0.25) - 0.5).abs() < 1e-12);
    }

    /// Abstractness and distance aggregate class counts across a package's modules, then derive
    /// `A = abstract/total` and `D = |A + I − 1|`. Inputs are built directly so the test exercises
    /// the aggregation, not the (separately tested) class-detection heuristic.
    #[test]
    fn package_rows_aggregate_abstractness_and_distance() {
        fn input(path: &str, classes: usize, abstract_classes: usize) -> ModuleInput {
            ModuleInput {
                name: module_from_path(path).unwrap(),
                imports: Vec::new(),
                loc: 0,
                classes,
                abstract_classes,
            }
        }
        // `iface` is a leaf package of 3 abstract + 1 concrete class across two modules: A = 3/4,
        // and with no coupling I = 0, so D = |0.75 + 0 − 1| = 0.25.
        let g = ImportGraph::build(vec![
            input("iface/__init__.py", 0, 0),
            input("iface/a.py", 2, 2),
            input("iface/b.py", 2, 1),
        ]);
        let rows = g.package_rows();
        let iface = rows.iter().find(|r| r.package == "iface").unwrap();
        assert_eq!(iface.classes, 4);
        assert_eq!(iface.abstract_classes, 3);
        assert_eq!(iface.abstractness, 0.75);
        assert_eq!(iface.instability, 0.0);
        assert_eq!(iface.distance, 0.25);
    }

    #[test]
    fn package_with_no_classes_has_zero_abstractness() {
        // The import-graph fixtures define no classes, so abstractness is 0 and distance reduces
        // to |I − 1| — here a leaf with no coupling (I=0) sits in the zone of pain (D=1).
        let g = graph_of(&[("pkg/__init__.py", ""), ("pkg/a.py", "")]);
        let rows = g.package_rows();
        let pkg = rows.iter().find(|r| r.package == "pkg").unwrap();
        assert_eq!(pkg.classes, 0);
        assert_eq!(pkg.abstractness, 0.0);
        assert_eq!(pkg.distance, 1.0);
    }

    #[test]
    fn summary_counts_modules_packages_and_edges() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg.sub import helper\nimport pkg.b\n"),
            ("pkg/b.py", ""),
            ("pkg/sub/__init__.py", ""),
            ("pkg/sub/helper.py", ""),
        ]);
        let summary = g.summary();
        assert_eq!(summary.modules, 5);
        // packages: pkg, pkg.sub.
        assert_eq!(summary.packages, 2);
        // module edges: pkg.a -> pkg.sub.helper, pkg.a -> pkg.b.
        assert_eq!(summary.module_edges, 2);
        // package edges: pkg -> pkg.sub (the intra-pkg a->b edge is not cross-package).
        assert_eq!(summary.package_edges, 1);
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
    }

    #[test]
    fn package_row_in_cycle_flags_cycle_members() {
        let g = graph_of(&[
            ("pkg/__init__.py", ""),
            ("pkg/a.py", "from pkg import b\n"),
            ("pkg/b.py", "from pkg import a\n"),
            ("solo.py", ""),
        ]);
        let rows = g.package_rows();
        let pkg = rows.iter().find(|r| r.package == "pkg").unwrap();
        let root = rows.iter().find(|r| r.package == ".").unwrap();
        assert!(pkg.in_cycle, "pkg.a <-> pkg.b is a cycle");
        assert!(!root.in_cycle, "the standalone top-level module is not");
    }

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
}
