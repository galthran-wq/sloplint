//! Class cohesion over one method×attribute access graph, aggregated four ways: **LCOM4**
//! (Hitz & Montazeri, 1995) plus the CK alternatives **TCC**/**LCC** (Bieman & Kang, 1995) and
//! **LCOM\*** (Henderson-Sellers, 1996). All read the same underlying data — which methods touch
//! which instance attributes — so they're different views of one cheap AST walk.
//!
//! For **LCOM4**, nodes are the class's methods; two methods are linked when they touch a common
//! instance attribute (`self.x`) or one calls the other (`self.other()`). A class whose graph
//! splits into ≥2 components is really N unrelated classes glued together — a low-cohesion "god"
//! class (catch-all `Utils`/`Manager`/`Service`) that should have been split.
//!
//! **TCC** and **LCC** are ratios in `[0, 1]` over the **shared-attribute** graph only (a method
//! *call* is not attribute sharing, so unlike LCOM4 it does not create a TCC/LCC edge): TCC is the
//! fraction of method pairs that share ≥1 attribute directly; LCC also counts pairs connected
//! transitively (same component), so `lcc >= tcc`. Higher = more cohesive. Following the standard
//! (Bieman-Kang) formulation, this counts direct attribute sharing and does **not** propagate a
//! field access through invocation trees — a documented lower bound on connectivity, in the spirit
//! of the crate's other dynamic-Python approximations.
//!
//! **LCOM\*** is Henderson-Sellers' normalized *lack* of cohesion in `[0, 1]`:
//! `(m − mean_methods_per_field) / (m − 1)` — 0.0 when every method touches every field, 1.0 when
//! each field is touched by a single method.
//!
//! Constructors (`__init__`/`__new__`/`__post_init__`) are **excluded** from all four (they
//! initialize every attribute and would spuriously connect otherwise-unrelated method groups,
//! defeating the metrics). This is the standard LCOM treatment. (A known consequence is that a
//! getter-heavy data holder can score as low-cohesion; that is why callers allowlist data
//! classes.)

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::is_staticmethod;
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Parameters, Stmt, StmtClassDef, StmtFunctionDef};

/// The four cohesion views of one class, all over the same method×attribute access graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassCohesion {
    /// Methods considered (every method except constructors).
    pub methods: usize,
    /// LCOM4: connected components among those methods (attribute-sharing **and** method-call
    /// edges). 0 for no methods, 1 for a cohesive class, ≥2 for a class that splits into unrelated
    /// method groups.
    pub components: usize,
    /// TCC — Tight Class Cohesion (Bieman & Kang 1995): the fraction of method pairs that are
    /// **directly** connected (share ≥1 instance attribute), out of all `methods·(methods−1)/2`
    /// pairs. `[0, 1]`, higher = more cohesive. 0.0 when there are <2 methods or no sharing.
    pub tcc: f64,
    /// LCC — Loose Class Cohesion (Bieman & Kang 1995): like [`Self::tcc`] but counting method
    /// pairs connected **directly or transitively** (same component of the shared-attribute
    /// graph), so `lcc >= tcc`. `[0, 1]`. 0.0 when there are <2 methods.
    pub lcc: f64,
    /// LCOM\* — Henderson-Sellers (1996) normalized *lack* of cohesion: `(m − mean) / (m − 1)`,
    /// where `m` is the method count and `mean` is the average number of methods accessing each
    /// field. `[0, 1]`: 0.0 = maximally cohesive (every method touches every field), 1.0 =
    /// maximally incohesive. 0.0 in the degenerate cases (`m ≤ 1` or no fields accessed).
    pub lcom_star: f64,
}

/// Compute the four [`ClassCohesion`] views for `class` in a single AST walk.
pub fn class_cohesion(class: &StmtClassDef) -> ClassCohesion {
    let nodes: Vec<&StmtFunctionDef> = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some(function),
            _ => None,
        })
        .filter(|method| !is_constructor(method.name.as_str()))
        .collect();

    let n = nodes.len();
    if n <= 1 {
        // <2 methods: no pairs to connect and (m ≤ 1) LCOM* is undefined → all ratios 0.0.
        return ClassCohesion {
            methods: n,
            components: n,
            tcc: 0.0,
            lcc: 0.0,
            lcom_star: 0.0,
        };
    }

    let node_index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, method)| (method.name.as_str(), i))
        .collect();

    // LCOM4 graph (attribute-sharing + method-call edges) and the per-method attribute sets that
    // the attribute-only TCC/LCC/LCOM* views read.
    let mut lcom4 = UnionFind::new(n);
    let mut attrs_per_node: Vec<HashSet<&str>> = vec![HashSet::new(); n];
    // attribute name -> the nodes that access it (shared-attribute unioning + LCOM* field counts).
    let mut attr_users: BTreeMap<&str, Vec<usize>> = BTreeMap::new();

    for (i, method) in nodes.iter().enumerate() {
        let Some(receiver) = receiver_name(method) else {
            continue; // a @staticmethod has no `self` — it shares no instance state.
        };
        let mut accesses = SelfAccess {
            receiver,
            names: HashSet::new(),
        };
        for stmt in &method.body {
            accesses.visit_stmt(stmt);
        }
        for name in accesses.names {
            if is_constructor(name) {
                // A reference to an excluded constructor (`self.__init__()`) is not an edge.
                continue;
            }
            if let Some(&j) = node_index.get(name) {
                // `self.other()` / `self.other` — a method-to-method link (LCOM4 only; a call is
                // not attribute sharing, so it is not a TCC/LCC edge).
                lcom4.union(i, j);
            } else {
                // `self.attr` — a shared-attribute candidate.
                attrs_per_node[i].insert(name);
                attr_users.entry(name).or_default().push(i);
            }
        }
    }

    // LCOM4: two methods touching the same attribute are also linked.
    for users in attr_users.values() {
        for pair in users.windows(2) {
            lcom4.union(pair[0], pair[1]);
        }
    }
    let components = lcom4.components();

    // TCC/LCC: the shared-attribute graph only. A direct connection is a shared attribute; LCC
    // additionally counts pairs in the same connected component of that graph.
    let mut attr_graph = UnionFind::new(n);
    let mut direct_pairs = 0usize;
    for i in 0..n {
        for j in (i + 1)..n {
            if !attrs_per_node[i].is_disjoint(&attrs_per_node[j]) {
                direct_pairs += 1;
                attr_graph.union(i, j);
            }
        }
    }
    let total_pairs = n * (n - 1) / 2;
    let mut component_size: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        *component_size.entry(attr_graph.find(i)).or_insert(0) += 1;
    }
    let connected_pairs: usize = component_size.values().map(|&c| c * (c - 1) / 2).sum();
    let tcc = direct_pairs as f64 / total_pairs as f64;
    let lcc = connected_pairs as f64 / total_pairs as f64;

    // LCOM*: Henderson-Sellers normalized lack of cohesion over the same method/field data. The
    // fields are those the (non-constructor) methods actually access; `mean` is their average
    // accessor count.
    let fields = attr_users.len();
    let lcom_star = if fields == 0 {
        0.0
    } else {
        let sum_mf: usize = attr_users.values().map(|users| users.len()).sum();
        let mean_mf = sum_mf as f64 / fields as f64;
        (n as f64 - mean_mf) / (n as f64 - 1.0)
    };

    ClassCohesion {
        methods: n,
        components,
        tcc,
        lcc,
        lcom_star,
    }
}

/// Count of distinct instance attributes a class has: `<receiver>.<name>` references (across
/// all methods, including constructors) whose `<name>` is not itself a method. A class-size
/// signal — bloated attribute sets are a common low-quality pattern.
pub fn class_attribute_count(class: &StmtClassDef) -> usize {
    let methods: HashSet<&str> = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some(function.name.as_str()),
            _ => None,
        })
        .collect();
    let mut attributes: HashSet<&str> = HashSet::new();
    for stmt in &class.body {
        let Stmt::FunctionDef(method) = stmt else {
            continue;
        };
        let Some(receiver) = receiver_name(method) else {
            continue;
        };
        let mut access = SelfAccess {
            receiver,
            names: HashSet::new(),
        };
        for body_stmt in &method.body {
            access.visit_stmt(body_stmt);
        }
        for name in access.names {
            if !methods.contains(name) {
                attributes.insert(name);
            }
        }
    }
    attributes.len()
}

/// Constructors set up all state and are excluded from the cohesion graph.
fn is_constructor(name: &str) -> bool {
    matches!(name, "__init__" | "__new__" | "__post_init__")
}

/// The receiver (`self`/`cls`) name for a method — its first positional parameter — or `None`
/// for a `@staticmethod` (which has no receiver and shares no instance state).
fn receiver_name(method: &StmtFunctionDef) -> Option<&str> {
    if is_staticmethod(method) {
        return None;
    }
    method
        .parameters
        .posonlyargs
        .first()
        .or_else(|| method.parameters.args.first())
        .map(|param| param.parameter.name.as_str())
}

/// Whether `parameters` binds `name` — used to detect a nested scope that re-binds the
/// receiver (so its `self.x` belongs to a *different* `self`, not this method's).
fn binds(parameters: &Parameters, name: &str) -> bool {
    parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .chain(&parameters.kwonlyargs)
        .any(|param| param.parameter.name.as_str() == name)
        || parameters
            .vararg
            .as_ref()
            .is_some_and(|p| p.name.as_str() == name)
        || parameters
            .kwarg
            .as_ref()
            .is_some_and(|p| p.name.as_str() == name)
}

/// Collects the `<receiver>.<name>` attribute/method names referenced in a method body. It
/// does **not** descend into a nested scope (def/lambda/class) that re-binds the receiver
/// name, since there `self`/`cls` refers to something else.
struct SelfAccess<'a> {
    receiver: &'a str,
    names: HashSet<&'a str>,
}

impl<'a> Visitor<'a> for SelfAccess<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            // A nested class's methods each have their own receiver.
            Stmt::ClassDef(_) => {}
            // A nested function that re-binds the receiver has its own `self`.
            Stmt::FunctionDef(function) if binds(&function.parameters, self.receiver) => {}
            // Otherwise recurse (a closure still refers to this method's `self`).
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Lambda(lambda) = expr {
            if lambda
                .parameters
                .as_deref()
                .is_some_and(|parameters| binds(parameters, self.receiver))
            {
                return; // the lambda re-binds the receiver; skip its body.
            }
        }
        if let Expr::Attribute(attribute) = expr {
            if let Expr::Name(name) = attribute.value.as_ref() {
                if name.id.as_str() == self.receiver {
                    self.names.insert(attribute.attr.as_str());
                }
            }
        }
        visitor::walk_expr(self, expr);
    }
}

/// Union-find over method indices.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut current = x;
        while self.parent[current] != root {
            let next = self.parent[current];
            self.parent[current] = root;
            current = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }

    fn components(&mut self) -> usize {
        let n = self.parent.len();
        (0..n).map(|i| self.find(i)).collect::<HashSet<_>>().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture_source;
    use sloplint_python::parse;
    use std::fmt::Write;

    /// LCOM4 cohesion (+ attribute count) for every class in a fixture, in source order. The
    /// fixture documents each case's expected shape; the snapshot pins the numbers.
    fn cohesion_report(source: &str) -> String {
        let parsed = parse(source).expect("fixture parses");
        let mut out = String::new();
        for stmt in &parsed.syntax().body {
            if let Stmt::ClassDef(class) = stmt {
                let cohesion = class_cohesion(class);
                let attributes = class_attribute_count(class);
                writeln!(
                    out,
                    "{}: methods={} components={} attributes={} tcc={:.3} lcc={:.3} lcom*={:.3}",
                    class.name,
                    cohesion.methods,
                    cohesion.components,
                    attributes,
                    cohesion.tcc,
                    cohesion.lcc,
                    cohesion.lcom_star,
                )
                .unwrap();
            }
        }
        out
    }

    #[test]
    fn cohesion_metrics() {
        insta::assert_snapshot!(cohesion_report(&fixture_source("cohesion/cohesion.py")));
    }
}
