//! Project-wide inheritance & coupling resolution: the CK metrics DIT (depth), NOC (breadth),
//! and CBO (coupling), plus the per-class coupling candidates and the abstractness heuristic.
//!
//! These resolve **first-party** relationships across the whole file set, so they run as a
//! second pass ([`resolve_inheritance`]) after per-file metrics are collected — dit/noc/cbo are
//! 0 on a `ClassMetrics` until it runs.

use crate::expr_trailing_name;
use crate::FileMetrics;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Stmt, StmtClassDef};

pub(crate) fn coupling_candidates(class: &StmtClassDef) -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Base classes are coupling.
    for base in class.bases() {
        if let Some(name) = expr_trailing_name(base) {
            names.insert(name.to_string());
        }
    }
    let mut collector = CouplingCollector { names: &mut names };
    for stmt in &class.body {
        collector.visit_stmt(stmt);
    }
    names.into_iter().collect()
}

/// Every name in a type-annotation expression is a type reference, so collect them all — recursing
/// through subscripts (`list[T]`), unions (`A | B`), and tuples (`tuple[A, B]` / `isinstance` arg
/// tuples). String forward-refs (`"Foo"`) are skipped — part of the documented lower bound.
fn collect_type_names(expr: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Name(n) => {
            out.insert(n.id.to_string());
        }
        Expr::Attribute(a) => {
            out.insert(a.attr.to_string());
        }
        Expr::Subscript(s) => {
            collect_type_names(&s.value, out);
            collect_type_names(&s.slice, out);
        }
        Expr::Tuple(t) => {
            for elt in &t.elts {
                collect_type_names(elt, out);
            }
        }
        Expr::List(l) => {
            for elt in &l.elts {
                collect_type_names(elt, out);
            }
        }
        Expr::BinOp(b) => {
            collect_type_names(&b.left, out);
            collect_type_names(&b.right, out);
        }
        _ => {}
    }
}

/// Walks a class body collecting CBO coupling candidates: annotation type names (handled at the
/// statement level for defs/`AnnAssign`) and instantiation/`isinstance`/`issubclass` callees
/// (handled at the expression level). Descends into methods and their closures, but **not into a
/// nested class** — a nested class is its own unit with its own CBO row, so its couplings belong to
/// it, not the enclosing class (matching `class_wmc`/`ncss`/`exit_count`). The enclosing class still
/// couples to the nested class if it *uses* it (e.g. instantiates it in a method).
struct CouplingCollector<'a> {
    names: &'a mut std::collections::BTreeSet<String>,
}

impl Visitor<'_> for CouplingCollector<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        if matches!(stmt, Stmt::ClassDef(_)) {
            return; // nested class — its own unit; don't attribute its coupling to the outer class.
        }
        match stmt {
            Stmt::FunctionDef(func) => {
                let params = &func.parameters;
                for param in params
                    .posonlyargs
                    .iter()
                    .chain(&params.args)
                    .chain(&params.kwonlyargs)
                {
                    if let Some(annotation) = &param.parameter.annotation {
                        collect_type_names(annotation, self.names);
                    }
                }
                for variadic in [&params.vararg, &params.kwarg].into_iter().flatten() {
                    if let Some(annotation) = &variadic.annotation {
                        collect_type_names(annotation, self.names);
                    }
                }
                if let Some(returns) = &func.returns {
                    collect_type_names(returns, self.names);
                }
            }
            Stmt::AnnAssign(ann) => collect_type_names(&ann.annotation, self.names),
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Call(call) = expr {
            if let Some(name) = expr_trailing_name(&call.func) {
                // The callee itself (a class instantiation `Foo(...)`, or the isinstance/issubclass
                // builtin — the latter trails to a non-class name and is filtered out at resolution).
                self.names.insert(name.to_string());
                // For type checks, the class argument(s) are the real coupling.
                if matches!(name, "isinstance" | "issubclass") {
                    if let Some(class_arg) = call.arguments.args.get(1) {
                        collect_type_names(class_arg, self.names);
                    }
                }
            }
        }
        visitor::walk_expr(self, expr);
    }
}

/// Fill in [`ClassMetrics::dit`] and [`ClassMetrics::noc`] for every class across the project —
/// the CK inheritance pair: DIT (depth) and NOC (breadth). Both resolve bases by **trailing class
/// name** against the set of first-party classes in `files`; a base that doesn't resolve —
/// `object`, the stdlib, a third party, or any name no first-party class claims — is invisible.
///
/// - **DIT** (depth): the longest path from a class up to a root via its bases. An external base
///   terminates the chain, so this is a conservative under-count; a class with no first-party base
///   has DIT 0.
/// - **NOC** (breadth): the number of **direct** subclass *definitions* that name this class as a
///   base — the in-degree of the same graph. A class no first-party class extends has NOC 0.
///
/// When a class name is defined more than once, depth uses the first definition's bases and both
/// figures are assigned by name (every class of that name gets the same DIT/NOC); names are sorted
/// so the result is deterministic. Real Python inheritance is acyclic, but name collisions could
/// synthesize a cycle; a name already on the current resolution path terminates it, so the pass
/// always halts.
pub fn resolve_inheritance(files: &mut [&mut FileMetrics]) {
    use std::collections::HashMap;

    let mut bases_of: HashMap<&str, &[String]> = HashMap::new();
    for file in files.iter() {
        for class in &file.classes {
            bases_of
                .entry(class.name.as_str())
                .or_insert(class.bases.as_slice());
        }
    }

    let mut cache: HashMap<&str, usize> = HashMap::new();
    let mut names: Vec<&str> = bases_of.keys().copied().collect();
    names.sort_unstable();
    for name in names {
        dit_of(name, &bases_of, &mut cache, &mut Vec::new());
    }
    // NOC (breadth): the in-degree of the inheritance graph. Count, per first-party class name,
    // every class *definition* that lists it as a direct base (so two distinct subclasses of the
    // same base count twice, even if the base is defined once). A single child's bases are deduped
    // by name first, so a class is counted once per base even if it names that base twice (e.g.
    // `class X(a.Base, b.Base)` where both trail to `Base`) — it's still one child of `Base`.
    let mut children: HashMap<&str, usize> = HashMap::new();
    for file in files.iter() {
        for class in &file.classes {
            let mut counted: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for base in &class.bases {
                if bases_of.contains_key(base.as_str()) && counted.insert(base.as_str()) {
                    *children.entry(base.as_str()).or_insert(0) += 1;
                }
            }
        }
    }

    // Detach the maps + the first-party class-name set from `bases_of`'s borrow of `files` so we
    // can write them back. The name set is what CBO resolves coupling candidates against.
    let depths: HashMap<String, usize> = cache.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let noc: HashMap<String, usize> = children.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let class_names: std::collections::HashSet<String> =
        bases_of.keys().map(|k| k.to_string()).collect();

    for file in files.iter_mut() {
        for class in &mut file.classes {
            class.dit = depths.get(&class.name).copied().unwrap_or(0);
            class.noc = noc.get(&class.name).copied().unwrap_or(0);
            // CBO: distinct first-party classes this one couples to, excluding itself. The
            // candidates are pre-deduped, so a plain count of those in the class-name set is the CBO.
            let cbo = class
                .coupled
                .iter()
                .filter(|name| name.as_str() != class.name && class_names.contains(name.as_str()))
                .count();
            class.cbo = cbo;
        }
    }
}

/// Longest first-party base chain above `name`, memoized in `cache`. `path` holds the names on
/// the current DFS branch; revisiting one means a (collision-induced) cycle, severed by
/// returning 0 there without caching. Depths *on or just above* such a cycle are then
/// ill-defined — they reflect where the back-edge happened to be cut — but the cut point is
/// fixed (names are resolved in sorted order), so the result is at least deterministic, and the
/// only contract for the cyclic case is that the pass halts. Acyclic inheritance (i.e. all real
/// Python) memoizes exactly.
fn dit_of<'a>(
    name: &'a str,
    bases_of: &std::collections::HashMap<&'a str, &'a [String]>,
    cache: &mut std::collections::HashMap<&'a str, usize>,
    path: &mut Vec<&'a str>,
) -> usize {
    if let Some(depth) = cache.get(name) {
        return *depth;
    }
    if path.contains(&name) {
        return 0;
    }
    let Some(bases) = bases_of.get(name) else {
        return 0;
    };
    path.push(name);
    let mut best = 0;
    for base in bases.iter() {
        // Resolve by name against the first-party class set; only a base another first-party
        // class claims extends the chain.
        if let Some((first_party_name, _)) = bases_of.get_key_value(base.as_str()) {
            best = best.max(1 + dit_of(first_party_name, bases_of, cache, path));
        }
    }
    path.pop();
    cache.insert(name, best);
    best
}

/// Heuristic for whether a class is "abstract" for Martin's package abstractness ratio.
/// Python has no interface keyword, so this approximates — a class counts as abstract if it:
///
/// - subclasses `ABC` / `abc.ABC` or `Protocol` / `typing.Protocol` (incl. subscripted
///   `Protocol[T]`),
/// - declares `metaclass=ABCMeta`, or
/// - has any method decorated with `@abstractmethod` (or the `abstractproperty` /
///   `abstractclassmethod` / `abstractstaticmethod` family).
///
/// This is deliberately an approximation — abstractness is fuzzy in Python, and the derived
/// metric ships clearly labeled as heuristic — but it only fires on the genuine abstract-base /
/// protocol idioms. We pointedly do *not* treat a stub body (`class Foo(Bar): ...`) as a signal:
/// such a class has no `def`, so a whole-class stub is always a concrete leaf/marker — an empty
/// exception subclass (`class ReadError(NetworkError): ...`) or a sentinel (`class UnsetType: ...`),
/// not an interface. Counting those inflated Abstractness ~5× on exception-heavy modules and skewed
/// Distance `D`.
pub(crate) fn class_is_abstract(class: &StmtClassDef) -> bool {
    let abstract_base = class
        .bases()
        .iter()
        .filter_map(expr_trailing_name)
        .any(|name| name == "ABC" || name == "Protocol");

    let abc_metaclass = class.keywords().iter().any(|keyword| {
        keyword
            .arg
            .as_ref()
            .is_some_and(|arg| arg.as_str() == "metaclass")
            && expr_trailing_name(&keyword.value) == Some("ABCMeta")
    });

    let has_abstractmethod = class.body.iter().any(|stmt| match stmt {
        Stmt::FunctionDef(func) => func
            .decorator_list
            .iter()
            .filter_map(|decorator| expr_trailing_name(&decorator.expression))
            .any(|name| name.starts_with("abstract")),
        _ => false,
    });

    abstract_base || abc_metaclass || has_abstractmethod
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_metrics;
    use sloplint_python::parse;

    /// Build `FileMetrics` for one source file (the cross-file resolution tests then run
    /// `resolve_inheritance` over several of these).
    fn metrics(source: &str) -> FileMetrics {
        file_metrics(source, &parse(source).unwrap())
    }

    /// DIT resolves over the whole project by class name: a chain `Grandchild -> Child -> Root`
    /// split across two files gives depths 2/1/0, and bases that don't resolve to a first-party
    /// class (`object`, a third-party import) terminate at 0.
    #[test]
    fn dit_resolves_first_party_chain_across_files() {
        let mut base = metrics("class Root(object):\n    pass\n");
        let mut derived = metrics(
            "\
from base import Root
from third_party import Plugin


class Child(Root):
    pass

class Grandchild(Child):
    pass

class External(Plugin):
    pass
",
        );
        resolve_inheritance(&mut [&mut base, &mut derived]);

        let dit = |file: &FileMetrics, name: &str| {
            file.classes.iter().find(|c| c.name == name).unwrap().dit
        };
        assert_eq!(dit(&base, "Root"), 0, "object is external → root");
        assert_eq!(dit(&derived, "Child"), 1, "Root is first-party");
        assert_eq!(dit(&derived, "Grandchild"), 2, "Child -> Root");
        assert_eq!(
            dit(&derived, "External"),
            0,
            "Plugin is third-party → invisible"
        );
    }
    #[test]
    fn dit_takes_longest_path_and_survives_name_cycles() {
        // Multiple inheritance: D(B, C), B(A), C(A), A. Longest path D->B->A (or D->C->A) = 2.
        let mut multi = metrics(
            "\
class A:
    pass

class B(A):
    pass

class C(A):
    pass

class D(B, C):
    pass
",
        );
        resolve_inheritance(&mut [&mut multi]);
        let dit = |name: &str| multi.classes.iter().find(|c| c.name == name).unwrap().dit;
        assert_eq!(dit("D"), 2, "longest path to a root is two hops");

        // A name collision can synthesize a cycle (X(Y), Y(X)); resolution must still halt
        // rather than recurse forever.
        let mut cyclic = metrics("class X(Y):\n    pass\n\nclass Y(X):\n    pass\n");
        resolve_inheritance(&mut [&mut cyclic]);
        // No assertion on the (ill-defined) depth — the contract is that the pass terminates.
    }
    #[test]
    fn noc_counts_direct_first_party_children_across_files() {
        let mut base = metrics(
            "\
from third_party import Plugin


class Base:
    pass

class A(Base):
    pass

class B(Base):
    pass

class Ext(Plugin):
    pass
",
        );
        // A third child of Base, defined in another file — NOC must see across the project.
        let mut more = metrics("from base import Base\n\nclass C(Base):\n    pass\n");
        resolve_inheritance(&mut [&mut base, &mut more]);

        let noc = |file: &FileMetrics, name: &str| {
            file.classes.iter().find(|c| c.name == name).unwrap().noc
        };
        assert_eq!(noc(&base, "Base"), 3, "A, B (same file) + C (cross-file)");
        assert_eq!(noc(&base, "A"), 0, "a leaf has no children");
        assert_eq!(
            noc(&base, "Ext"),
            0,
            "Ext has no children; its third-party base doesn't make it one"
        );
        // A grandchild does not count toward the grandparent's NOC — breadth is one level only.
        let mut chain = metrics(
            "class Root:\n    pass\n\nclass Mid(Root):\n    pass\n\nclass Leaf(Mid):\n    pass\n",
        );
        resolve_inheritance(&mut [&mut chain]);
        let n = |name: &str| chain.classes.iter().find(|c| c.name == name).unwrap().noc;
        assert_eq!(n("Root"), 1, "only Mid is a direct child, not Leaf");
        assert_eq!(n("Mid"), 1);
        assert_eq!(n("Leaf"), 0);
    }
    #[test]
    fn cbo_counts_distinct_first_party_classes_via_all_sources() {
        // `Hub` couples to first-party classes via: base (Base), annotation (Widget on a param +
        // Result return), instantiation (Engine()), and isinstance (Plugin). `int`/`list` are not
        // first-party → dropped. Self-references and third-party names don't count.
        let mut file = metrics(
            "\
class Base:
    pass

class Widget:
    pass

class Engine:
    pass

class Result:
    pass

class Plugin:
    pass

class Hub(Base):
    def run(self, w: Widget, n: int) -> Result:
        items: list = []
        e = Engine()
        if isinstance(w, Plugin):
            return Result()
        return Hub()
",
        );
        resolve_inheritance(&mut [&mut file]);
        let hub = file.classes.iter().find(|c| c.name == "Hub").unwrap();
        // Base, Widget, Engine, Result, Plugin = 5 distinct first-party classes. `int`/`list`
        // dropped (not first-party); `Hub` (self) and `Result()` counted once via the annotation.
        assert_eq!(hub.cbo, 5, "coupled: {:?}", hub.coupled);
    }
    #[test]
    fn cbo_resolves_across_files_and_excludes_self_and_external() {
        let mut a = metrics(
            "from third_party import External\n\nclass Service:\n    def make(self) -> 'Helper':\n        return Helper()\n",
        );
        // Helper lives in another file — cross-file resolution must see it.
        let mut b = metrics("class Helper:\n    pass\n");
        resolve_inheritance(&mut [&mut a, &mut b]);
        let service = a.classes.iter().find(|c| c.name == "Service").unwrap();
        // Helper() instantiation resolves first-party; External is third-party (dropped); the
        // 'Helper' string forward-ref in the return annotation is NOT counted (documented lower
        // bound) but the Helper() call is, so cbo = 1.
        assert_eq!(service.cbo, 1, "coupled: {:?}", service.coupled);
    }
    #[test]
    fn cbo_does_not_descend_into_nested_classes() {
        // A nested class is its own unit; its coupling (to Target) belongs to Inner, not Outer.
        // Outer couples only to what it uses directly (Other, via instantiation in its own method).
        let mut file = metrics(
            "\
class Target:
    pass

class Other:
    pass

class Outer:
    class Inner:
        def use(self, x: Target) -> Target:
            return Target()

    def f(self):
        return Other()
",
        );
        resolve_inheritance(&mut [&mut file]);
        let cbo = |name: &str| file.classes.iter().find(|c| c.name == name).unwrap().cbo;
        assert_eq!(cbo("Outer"), 1, "Other only — Target belongs to Inner");
        assert_eq!(cbo("Inner"), 1, "Target");
    }
}
