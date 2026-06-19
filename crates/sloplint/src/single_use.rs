//! SLP111: single-use single-method classes (preview, whole-project).
//!
//! A class with exactly one non-dunder method that is instantiated in exactly one place across
//! the project is needless ceremony — a `Strategy`/`Manager` scaffolded for a single concrete
//! case that could have been a plain function. Like clone detection, this needs a cross-file
//! view (the instantiation count), so it runs as a whole-tree pass in the CLI rather than a
//! per-file rule.
//!
//! High-precision-by-construction guards: a class is a candidate only if it has *exactly one*
//! non-dunder method and isn't a data/interface/exception class (allowlisted by base or
//! decorator); and it's flagged only when its name is instantiated exactly once *and* never
//! used as a base class anywhere. Name-based and conservative: a name reused across modules
//! inflates the instantiation count and silently suppresses the finding rather than risk a
//! false positive.

use std::collections::{HashMap, HashSet};

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ModModule, Stmt, StmtClassDef};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange};

/// A class that could collapse to a function, pending the project-wide instantiation count.
pub struct Candidate {
    pub name: String,
    /// Range of the class name (where the diagnostic points).
    pub name_range: TextRange,
}

/// What one file contributes to the whole-project single-use analysis.
#[derive(Default)]
pub struct FileScan {
    /// Single-method, non-allowlisted classes defined in this file.
    pub candidates: Vec<Candidate>,
    /// Names called as `Name(...)` from OUTSIDE the class of that name — the external
    /// instantiation-count signal (a class instantiating itself doesn't count).
    pub instantiations: Vec<String>,
    /// Names used as a base class — an extension point shouldn't be flagged.
    pub bases: Vec<String>,
    /// Classes that instantiate themselves (recursive / builder / fluent types) — never
    /// "single-use ceremony", so they're suppressed.
    pub self_referential: Vec<String>,
}

/// A confirmed single-use-class finding: which file's scan it came from, and where.
pub struct Finding {
    pub scan_index: usize,
    pub name: String,
    pub range: TextRange,
}

/// Scan one parsed file for single-use-class signals.
pub fn scan_file(parsed: &Parsed<ModModule>) -> FileScan {
    let mut scanner = Scanner {
        scan: FileScan::default(),
        class_stack: Vec::new(),
    };
    for stmt in &parsed.syntax().body {
        scanner.visit_stmt(stmt);
    }
    scanner.scan
}

/// Decide which candidates to flag: instantiated exactly once across `scans`, and never used
/// as a base class. Pure over the per-file scans, so it's directly testable.
pub fn findings(scans: &[FileScan]) -> Vec<Finding> {
    let mut instantiations: HashMap<&str, usize> = HashMap::new();
    let mut suppressed: HashSet<&str> = HashSet::new();
    for scan in scans {
        for name in &scan.instantiations {
            *instantiations.entry(name.as_str()).or_default() += 1;
        }
        // A name used as a base class, or that instantiates itself, is a genuine type.
        for name in scan.bases.iter().chain(&scan.self_referential) {
            suppressed.insert(name.as_str());
        }
    }

    let mut out = Vec::new();
    for (scan_index, scan) in scans.iter().enumerate() {
        for candidate in &scan.candidates {
            let name = candidate.name.as_str();
            if instantiations.get(name) == Some(&1) && !suppressed.contains(name) {
                out.push(Finding {
                    scan_index,
                    name: candidate.name.clone(),
                    range: candidate.name_range,
                });
            }
        }
    }
    out
}

struct Scanner {
    scan: FileScan,
    /// Names of the classes enclosing the node currently being visited, so a `Name(...)` call
    /// to an enclosing class is recognized as self-instantiation, not an external use.
    class_stack: Vec<String>,
}

impl Scanner {
    fn analyze_class(&mut self, class: &StmtClassDef) {
        // Record base names (for both the allowlist and subclass-detection).
        let mut base_names = Vec::new();
        if let Some(arguments) = &class.arguments {
            for base in arguments.args.iter() {
                if let Some(name) = trailing_name(base) {
                    base_names.push(name.to_string());
                }
            }
        }
        let allowlisted =
            !class.decorator_list.is_empty() || base_names.iter().any(|name| is_exempt_base(name));
        self.scan.bases.extend(base_names);

        if allowlisted {
            return;
        }
        let methods = class
            .body
            .iter()
            .filter(|stmt| matches!(stmt, Stmt::FunctionDef(f) if !is_dunder(f.name.as_str())))
            .count();
        if methods == 1 {
            self.scan.candidates.push(Candidate {
                name: class.name.to_string(),
                name_range: class.name.range(),
            });
        }
    }
}

impl<'a> Visitor<'a> for Scanner {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::ClassDef(class) = stmt {
            self.analyze_class(class);
            // Track this class while walking its body so self-instantiations are recognized.
            self.class_stack.push(class.name.to_string());
            visitor::walk_stmt(self, stmt);
            self.class_stack.pop();
        } else {
            visitor::walk_stmt(self, stmt);
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if let Expr::Name(name) = call.func.as_ref() {
                let id = name.id.as_str();
                if self.class_stack.iter().any(|enclosing| enclosing == id) {
                    self.scan.self_referential.push(id.to_string());
                } else {
                    self.scan.instantiations.push(id.to_string());
                }
            }
        }
        visitor::walk_expr(self, expr);
    }
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

/// The trailing identifier of a base expression — `Protocol` from `typing.Protocol`, `Base`
/// from `Base[T]`.
fn trailing_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attribute) => Some(attribute.attr.as_str()),
        Expr::Subscript(subscript) => trailing_name(&subscript.value),
        Expr::Call(call) => trailing_name(&call.func),
        _ => None,
    }
}

/// Bases that mark a data/interface/exception class — never "needless ceremony".
fn is_exempt_base(name: &str) -> bool {
    matches!(
        name,
        "Protocol"
            | "ABC"
            | "ABCMeta"
            | "Enum"
            | "IntEnum"
            | "IntFlag"
            | "Flag"
            | "StrEnum"
            | "NamedTuple"
            | "TypedDict"
            | "Exception"
            | "BaseException"
    ) || name.ends_with("Error")
        || name.ends_with("Exception")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn scan(source: &str) -> FileScan {
        scan_file(&parse(source).expect("valid python"))
    }

    #[test]
    fn single_method_class_is_a_candidate() {
        let s = scan("class Doubler:\n    def apply(self, x):\n        return x * 2\n");
        assert_eq!(s.candidates.len(), 1);
        assert_eq!(s.candidates[0].name, "Doubler");
    }

    #[test]
    fn init_plus_one_method_is_a_candidate() {
        // __init__ is setup, not a behavior method.
        let s = scan(
            "class Greeter:\n    def __init__(self, name):\n        self.name = name\n\n    def greet(self):\n        return self.name\n",
        );
        assert_eq!(s.candidates.len(), 1);
    }

    #[test]
    fn multi_method_and_zero_method_classes_are_not_candidates() {
        assert_eq!(
            scan("class C:\n    def a(self):\n        return 1\n\n    def b(self):\n        return 2\n")
                .candidates
                .len(),
            0
        );
        assert_eq!(
            scan("class D:\n    def __init__(self):\n        self.x = 1\n")
                .candidates
                .len(),
            0
        );
    }

    #[test]
    fn data_interface_and_exception_classes_are_allowlisted() {
        assert!(scan("import dataclasses\n\n@dataclasses.dataclass\nclass C:\n    def m(self):\n        return self.x\n")
            .candidates
            .is_empty());
        assert!(scan(
            "from typing import Protocol\n\nclass C(Protocol):\n    def m(self):\n        ...\n"
        )
        .candidates
        .is_empty());
        assert!(
            scan("class MyError(ValueError):\n    def detail(self):\n        return 1\n")
                .candidates
                .is_empty()
        );
    }

    #[test]
    fn collects_instantiations_and_bases() {
        let s = scan(
            "class Base:\n    pass\n\nclass Sub(Base):\n    def m(self):\n        return 1\n\nx = Sub()\ny = helper(Sub())\n",
        );
        assert!(s.bases.contains(&"Base".to_string()));
        let count = s.instantiations.iter().filter(|n| *n == "Sub").count();
        assert_eq!(count, 2, "two Sub(...) calls");
    }

    #[test]
    fn flags_class_instantiated_exactly_once() {
        // `Once` is a single-method class used once; `Twice` is reused; `Never` is unused.
        let s = scan(
            "\
class Once:
    def run(self):
        return 1

class Twice:
    def run(self):
        return 2

class Never:
    def run(self):
        return 3

a = Once()
b = Twice()
c = Twice()
",
        );
        let result = findings(&[s]);
        let found: Vec<&str> = result.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(found, vec!["Once"]);
    }

    #[test]
    fn subclassed_candidate_is_not_flagged() {
        // `Strategy` has one method and is instantiated once, but it's also a base class.
        let s = scan(
            "\
class Strategy:
    def run(self):
        return 1

class Special(Strategy):
    pass

s = Strategy()
",
        );
        assert!(findings(&[s]).is_empty());
    }

    #[test]
    fn self_instantiating_class_is_not_flagged() {
        // Regression: a recursive/builder class instantiates itself; that isn't an external
        // "single use" and the class is a genuine type.
        let recursive = scan("class Node:\n    def add(self):\n        return Node()\n");
        assert!(findings(&[recursive]).is_empty());

        // Even with one external instantiation, a self-referential type stays exempt.
        let builder = scan(
            "class Builder:\n    def step(self):\n        return Builder()\n\nb = Builder()\n",
        );
        assert!(findings(&[builder]).is_empty());
    }

    #[test]
    fn a_same_named_call_elsewhere_suppresses_the_finding() {
        // `Helper` is instantiated once, but a second call to the same name (a function, or a
        // collision) bumps the project-wide count to 2 — conservatively suppressed.
        let s = scan(
            "class Helper:\n    def go(self):\n        return 1\n\nx = Helper()\n\ndef Helper2():\n    return Helper()\n",
        );
        assert!(findings(&[s]).is_empty());
    }

    #[test]
    fn instantiation_count_is_project_wide() {
        // Same class name instantiated once in each of two files -> count 2 -> not flagged.
        let a = scan("class Helper:\n    def go(self):\n        return 1\n\nx = Helper()\n");
        let b = scan("y = Helper()\n");
        assert!(findings(&[a, b]).is_empty());
    }
}
