//! Class cohesion via **LCOM4** (Hitz & Montazeri, 1995): the number of connected components
//! in a class's method graph.
//!
//! Nodes are the class's methods; two methods are linked when they touch a common instance
//! attribute (`self.x`) or one calls the other (`self.other()`). A class whose graph splits
//! into ≥2 components is really N unrelated classes glued together — a low-cohesion "god"
//! class (catch-all `Utils`/`Manager`/`Service`) that should have been split.
//!
//! Constructors (`__init__`/`__new__`/`__post_init__`) are **excluded** from the graph: they
//! initialize every attribute and would spuriously connect otherwise-unrelated method groups,
//! defeating the metric. This is the standard LCOM4 treatment. (A known consequence is that a
//! getter-heavy data holder can score as low-cohesion; that is why callers allowlist data
//! classes.)

use std::collections::{BTreeMap, HashMap, HashSet};

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, Parameters, Stmt, StmtClassDef, StmtFunctionDef};

/// LCOM4 result for one class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassCohesion {
    /// Methods considered (every method except constructors).
    pub methods: usize,
    /// LCOM4: connected components among those methods. 0 for no methods, 1 for a cohesive
    /// class, ≥2 for a class that splits into unrelated method groups.
    pub components: usize,
}

/// Compute [`ClassCohesion`] (LCOM4) for `class`.
pub fn class_cohesion(class: &StmtClassDef) -> ClassCohesion {
    // Every method directly in the class body, and the subset that are graph nodes.
    let all_methods: Vec<&StmtFunctionDef> = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some(function),
            _ => None,
        })
        .collect();
    let nodes: Vec<&StmtFunctionDef> = all_methods
        .iter()
        .copied()
        .filter(|method| !is_constructor(method.name.as_str()))
        .collect();

    if nodes.len() <= 1 {
        return ClassCohesion {
            methods: nodes.len(),
            components: nodes.len(),
        };
    }

    let node_index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, method)| (method.name.as_str(), i))
        .collect();

    let mut uf = UnionFind::new(nodes.len());
    // attribute name -> the nodes that access it (to union methods sharing state).
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
                // `self.other()` / `self.other` — a method-to-method link.
                uf.union(i, j);
            } else {
                // `self.attr` — record for shared-attribute unioning.
                attr_users.entry(name).or_default().push(i);
            }
        }
    }

    // Two methods touching the same attribute are linked.
    for users in attr_users.values() {
        for pair in users.windows(2) {
            uf.union(pair[0], pair[1]);
        }
    }

    ClassCohesion {
        methods: nodes.len(),
        components: uf.components(),
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

fn is_staticmethod(method: &StmtFunctionDef) -> bool {
    method
        .decorator_list
        .iter()
        .any(|decorator| decorator_name(&decorator.expression) == Some("staticmethod"))
}

/// The trailing identifier of a decorator expression (`staticmethod` from
/// `@builtins.staticmethod`), or `None`.
fn decorator_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        _ => None,
    }
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
                    "{}: methods={} components={} attributes={}",
                    class.name, cohesion.methods, cohesion.components, attributes
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
