//! Import resolution: turn file paths and `import`/`from` statements into dotted module names,
//! resolve them against the first-party module set (handling relative imports and `src/`
//! layout), and skip `if TYPE_CHECKING:` blocks. The deterministic front-end that feeds the
//! [`super::ImportGraph`].

use std::collections::HashSet;

use sloplint_python::ast::{Expr, ModModule, Stmt};
use sloplint_python::parser::Parsed;

use super::{ModuleName, RawImport};

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
pub(crate) fn candidates(importer: &ModuleName, import: &RawImport) -> Vec<String> {
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
}
