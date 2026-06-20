//! SLP140: tutorial / "example usage" scaffolding left in library modules (preview).
//!
//! A snippet written standalone with its own runnable `if __name__ == "__main__":` demo — it
//! instantiates the module's own classes and `print`s the results — is dead scaffolding once
//! pasted into a library module: it ships, never runs on the library's real path, and clutters
//! the file. A `__main__` guard is normally *good* practice, so this fires only on the demo
//! shape (the block both exercises the module's own API **and** prints), and never on entry
//! points: `__main__.py` / `main.py` / `cli.py` / … or files under `scripts/`, `examples/`,
//! `tests/`, etc. are skipped.
//!
//! Pure-AST and per-file: it can't prove the module is imported elsewhere (the issue's
//! cross-file import-graph confirmation is a future, CI-side refinement), so it leans on the
//! demo shape + path context and ships Preview.

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{CmpOp, Expr, Stmt};
use sloplint_python::Ranged;

use crate::lint::{FileContext, Rule};

pub struct ExampleScaffolding;

impl Rule for ExampleScaffolding {
    fn code(&self) -> &'static str {
        "SLP140"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        if is_entry_point_path(ctx.path) {
            return;
        }
        let body = &ctx.parsed.syntax().body;
        let module_api = module_api(body);

        for stmt in body {
            let Stmt::If(guard) = stmt else {
                continue;
            };
            if !is_main_guard(&guard.test) {
                continue;
            }
            let mut scan = GuardScan {
                api: &module_api,
                api_refs: HashSet::new(),
                bound: HashSet::new(),
                prints: false,
            };
            for stmt in &guard.body {
                scan.visit_stmt(stmt);
            }
            // A module-API name *referenced* in the guard, but not one merely shadowed by a
            // local binding inside it (`process = lambda: ...; print(process())`).
            let exercises_api = scan.api_refs.iter().any(|name| !scan.bound.contains(name));
            // The demo shape: it runs the module's own API *and* prints results.
            if exercises_api && scan.prints {
                diagnostics.push(Diagnostic::new(
                    "SLP140",
                    "`if __name__ == \"__main__\":` block looks like example/demo scaffolding \
                     (it exercises this module's own API and prints) — move it to `examples/` \
                     or delete it",
                    guard.test.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Top-level class / function names — the module's "own API" a demo would exercise.
fn module_api(body: &[Stmt]) -> HashSet<&str> {
    body.iter()
        .filter_map(|stmt| match stmt {
            Stmt::ClassDef(class) => Some(class.name.as_str()),
            Stmt::FunctionDef(function) => Some(function.name.as_str()),
            _ => None,
        })
        .collect()
}

/// Whether `test` is `__name__ == "__main__"` (in either operand order).
fn is_main_guard(test: &Expr) -> bool {
    let Expr::Compare(compare) = test else {
        return false;
    };
    if compare.ops.len() != 1 || compare.ops[0] != CmpOp::Eq || compare.comparators.len() != 1 {
        return false;
    }
    let left = compare.left.as_ref();
    let right = &compare.comparators[0];
    is_dunder_name_main(left, right) || is_dunder_name_main(right, left)
}

/// `a` is the `__name__` name and `b` is the `"__main__"` string literal.
fn is_dunder_name_main(a: &Expr, b: &Expr) -> bool {
    matches!(a, Expr::Name(name) if name.id.as_str() == "__name__")
        && matches!(b, Expr::StringLiteral(string) if string.value.to_str() == "__main__")
}

/// Files that are legitimately runnable — never library scaffolding.
fn is_entry_point_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if matches!(
        file,
        "__main__.py"
            | "main.py"
            | "cli.py"
            | "app.py"
            | "manage.py"
            | "setup.py"
            | "conftest.py"
            | "run.py"
            | "__init__.py"
    ) {
        return true;
    }
    // Plural dir names only (`tests`, not a lone `test` segment) so a `resources/test/...`
    // path doesn't blanket-allowlist real source.
    lower.split('/').any(|segment| {
        matches!(
            segment,
            "scripts" | "examples" | "samples" | "bin" | "tests" | "demos" | "docs" | "benchmarks"
        )
    })
}

/// Walks a `__main__` guard body collecting: a `print(...)` call, references to
/// module-level-defined names, and names *bound* inside the guard (so a local that shadows a
/// module-API name isn't mistaken for exercising the API).
struct GuardScan<'a> {
    api: &'a HashSet<&'a str>,
    api_refs: HashSet<&'a str>,
    bound: HashSet<&'a str>,
    prints: bool,
}

impl<'a> Visitor<'a> for GuardScan<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    collect_bound(target, &mut self.bound);
                }
            }
            Stmt::AnnAssign(node) => collect_bound(&node.target, &mut self.bound),
            Stmt::AugAssign(node) => collect_bound(&node.target, &mut self.bound),
            Stmt::For(node) => collect_bound(&node.target, &mut self.bound),
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(vars) = &item.optional_vars {
                        collect_bound(vars, &mut self.bound);
                    }
                }
            }
            Stmt::FunctionDef(node) => {
                self.bound.insert(node.name.as_str());
            }
            Stmt::ClassDef(node) => {
                self.bound.insert(node.name.as_str());
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Call(call) if matches!(call.func.as_ref(), Expr::Name(n) if n.id.as_str() == "print") =>
            {
                self.prints = true;
            }
            // A walrus binds its target.
            Expr::Named(named) => {
                if let Expr::Name(name) = named.target.as_ref() {
                    self.bound.insert(name.id.as_str());
                }
            }
            Expr::Name(name) if self.api.contains(name.id.as_str()) => {
                self.api_refs.insert(name.id.as_str());
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

/// Collect the simple names bound by an assignment/`for`/`with` target (descending into
/// tuple/list unpacking); attribute and subscript targets bind no name.
fn collect_bound<'a>(target: &'a Expr, out: &mut HashSet<&'a str>) {
    match target {
        Expr::Name(name) => {
            out.insert(name.id.as_str());
        }
        Expr::Tuple(tuple) => tuple.elts.iter().for_each(|e| collect_bound(e, out)),
        Expr::List(list) => list.elts.iter().for_each(|e| collect_bound(e, out)),
        Expr::Starred(starred) => collect_bound(&starred.value, out),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn findings(path: &str, source: &str) -> usize {
        let parsed = parse(source).expect("valid python");
        let ctx = FileContext {
            path,
            source,
            parsed: &parsed,
            limits: Default::default(),
        };
        let mut diagnostics = Vec::new();
        ExampleScaffolding.check(&ctx, &mut diagnostics);
        diagnostics.len()
    }

    const DEMO: &str = "\
class Greeter:
    def __init__(self, name):
        self.name = name

    def greet(self):
        return f\"hi {self.name}\"


if __name__ == \"__main__\":
    g = Greeter(\"world\")
    print(g.greet())
";

    #[test]
    fn flags_library_self_demo_block() {
        assert_eq!(findings("pkg/greeter.py", DEMO), 1);
    }

    #[test]
    fn reversed_operand_order_is_detected() {
        let source = "\
def run():
    return 1


if \"__main__\" == __name__:
    print(run())
";
        assert_eq!(findings("pkg/mod.py", source), 1);
    }

    #[test]
    fn entry_point_delegating_to_main_is_not_flagged() {
        // A real CLI entry: calls its own `main()` but doesn't print a demo.
        let source = "\
import sys


def main():
    print(\"running\")
    return 0


if __name__ == \"__main__\":
    sys.exit(main())
";
        assert_eq!(findings("pkg/service.py", source), 0);
    }

    #[test]
    fn guard_without_own_api_is_not_flagged() {
        // Prints, but exercises no module-defined name — not this module's dead scaffolding.
        let source = "if __name__ == \"__main__\":\n    print(\"hello\")\n";
        assert_eq!(findings("pkg/mod.py", source), 0);
    }

    #[test]
    fn guard_without_print_is_not_flagged() {
        let source = "\
class Server:
    def serve(self):
        return 1


if __name__ == \"__main__\":
    Server().serve()
";
        assert_eq!(findings("pkg/server.py", source), 0);
    }

    #[test]
    fn guard_local_shadowing_a_module_name_is_not_flagged() {
        // Regression: `process` is rebound locally in the guard, so it doesn't exercise the
        // module's `process` — must not fire.
        let source = "\
def process():
    return 1


if __name__ == \"__main__\":
    process = lambda: 2
    print(process())
";
        assert_eq!(findings("pkg/mod.py", source), 0);
    }

    #[test]
    fn non_main_comparisons_are_not_guards() {
        let api = "def run():\n    return 1\n\n\n";
        // `!=`, `is`, a wrong literal, and a chained compare are all rejected.
        assert_eq!(
            findings(
                "pkg/m.py",
                &format!("{api}if __name__ != \"__main__\":\n    print(run())\n")
            ),
            0
        );
        assert_eq!(
            findings(
                "pkg/m.py",
                &format!("{api}if __name__ is \"__main__\":\n    print(run())\n")
            ),
            0
        );
        assert_eq!(
            findings(
                "pkg/m.py",
                &format!("{api}if __name__ == \"__mn__\":\n    print(run())\n")
            ),
            0
        );
        assert_eq!(
            findings(
                "pkg/m.py",
                &format!("{api}x = 1\nif x == __name__ == \"__main__\":\n    print(run())\n")
            ),
            0
        );
    }

    #[test]
    fn guard_nested_in_a_function_is_not_flagged() {
        // Only module-level `__main__` guards are scaffolding; a nested one isn't scanned.
        let source = "\
def demo():
    if __name__ == \"__main__\":
        print(demo())
";
        assert_eq!(findings("pkg/mod.py", source), 0);
    }

    #[test]
    fn entry_point_paths_are_skipped() {
        // The same demo shape under entry-point paths/filenames is allowlisted.
        assert_eq!(findings("examples/quickstart.py", DEMO), 0);
        assert_eq!(findings("pkg/__main__.py", DEMO), 0);
        assert_eq!(findings("scripts/seed.py", DEMO), 0);
        assert_eq!(findings("cli.py", DEMO), 0);
    }
}
