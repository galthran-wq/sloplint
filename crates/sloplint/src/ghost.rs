//! SLP240: ghost scaffolding — a definition created but never wired up.
//!
//! A multi-turn-agent artifact: the model scaffolds an abstraction (or a feature flag) in one
//! iteration and forgets to connect it, leaving a dangling class/Protocol/helper or a branch gated
//! on a config flag that's defined nowhere. It reads as intentional structure but is incomplete.
//!
//! Two whole-project (cross-file) checks, both deliberately conservative (preview-gated):
//!
//! 1. **Ghost definition** — a top-level `def`/`class` whose name is referenced *nowhere* in the
//!    project (no `Name`/attribute use, no import, no `__all__`/string mention). Decorated defs
//!    (often registered via the decorator) and base/abstract/protocol types (extension points
//!    subclassed elsewhere, possibly downstream) are allowlisted.
//! 2. **Ghost config flag** — a `settings.ENABLE_X` / `config.FEATURE_Y`-style read of an
//!    UPPER_SNAKE flag that is **assigned nowhere** in the project, *and only when the project does
//!    define some config constants in code* (otherwise config lives entirely outside the source and
//!    we can't reason about it). `os.environ`/`os.getenv` reads are intentionally never flagged —
//!    env vars are external by design.
//!
//! Resolution is name-based and over-approximates references (any occurrence counts), so the bias
//! is firmly toward false negatives — a name that *might* be used is never flagged.

use std::collections::HashSet;

use sloplint_python::ast::visitor::{self, Visitor};
use sloplint_python::ast::{Expr, ExprContext, ModModule, Stmt};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange};

/// Config-object receiver names whose UPPER_SNAKE attributes are treated as config flags.
const CONFIG_RECEIVERS: &[&str] = &[
    "settings",
    "config",
    "conf",
    "cfg",
    "options",
    "opts",
    "flags",
    "features",
    "feature_flags",
    "app_config",
    "app_settings",
];

/// Name suffixes for base/abstract/protocol types — extension points that may be subclassed
/// elsewhere (possibly downstream), so an unreferenced one is not necessarily ghost.
const BASE_SUFFIXES: &[&str] = &["Base", "ABC", "Mixin", "Interface", "Protocol", "Meta"];

/// Conventional entry-point names invoked by a framework/runtime rather than referenced in source
/// (gunicorn `app:application`, AWS Lambda `handler`, a `__main__` guard, etc.) — never flagged.
const ENTRY_POINTS: &[&str] = &[
    "main",
    "run",
    "app",
    "application",
    "cli",
    "handler",
    "lambda_handler",
    "create_app",
    "get_application",
    "setup",
    "teardown",
];

/// A top-level definition (the candidate for the ghost-definition check).
pub struct Def {
    pub name: String,
    pub range: TextRange,
    pub kind: &'static str,
    pub decorated: bool,
    pub base_type: bool,
}

/// A `settings.UPPER_SNAKE` config-flag read.
pub struct ConfigRead {
    pub key: String,
    pub range: TextRange,
}

/// Everything one file contributes to the whole-project ghost analysis.
pub struct FileScan {
    pub path: String,
    pub defs: Vec<Def>,
    /// Names referenced anywhere (Name ids, attribute attrs, import aliases).
    pub refs: HashSet<String>,
    /// Identifier-like words appearing inside string literals (covers `__all__`, `getattr`,
    /// string-keyed registries).
    pub string_words: HashSet<String>,
    /// `__all__` entries.
    pub exports: HashSet<String>,
    /// UPPER_SNAKE names assigned/annotated anywhere (config constants defined in code).
    pub defined_keys: HashSet<String>,
    /// `settings.UPPER_SNAKE` reads.
    pub config_reads: Vec<ConfigRead>,
}

/// A ghost finding.
pub struct Finding {
    pub path: String,
    pub range: TextRange,
    pub message: String,
}

/// Scan one file for everything the whole-project resolution needs.
pub fn scan(path: &str, parsed: &Parsed<ModModule>) -> FileScan {
    let module = parsed.syntax();
    let mut scan = FileScan {
        path: path.to_string(),
        defs: Vec::new(),
        refs: HashSet::new(),
        string_words: HashSet::new(),
        exports: HashSet::new(),
        defined_keys: HashSet::new(),
        config_reads: Vec::new(),
    };

    // Top-level defs + `__all__` (definitions are module-level only).
    for stmt in &module.body {
        match stmt {
            Stmt::FunctionDef(func) => scan.defs.push(Def {
                name: func.name.to_string(),
                range: func.name.range(),
                kind: "function",
                decorated: !func.decorator_list.is_empty(),
                base_type: false,
            }),
            Stmt::ClassDef(class) => {
                let base_type =
                    name_has_base_suffix(class.name.as_str()) || class_has_base_or_protocol(class);
                scan.defs.push(Def {
                    name: class.name.to_string(),
                    range: class.name.range(),
                    kind: "class",
                    decorated: !class.decorator_list.is_empty(),
                    base_type,
                });
            }
            Stmt::Assign(assign) => {
                if let Some(exports) = dunder_all(assign) {
                    scan.exports.extend(exports);
                }
            }
            _ => {}
        }
    }

    let mut collector = Collector { scan: &mut scan };
    for stmt in &module.body {
        collector.visit_stmt(stmt);
    }
    scan
}

/// Whole-project resolution: a def is ghost when its name appears in none of the project-wide
/// reference sets; a config flag is ghost when it's read but defined nowhere.
pub fn findings(scans: &[FileScan]) -> Vec<Finding> {
    let union = |pick: fn(&FileScan) -> &HashSet<String>| -> HashSet<String> {
        scans.iter().flat_map(|s| pick(s).iter().cloned()).collect()
    };
    let refs = union(|s| &s.refs);
    let strings = union(|s| &s.string_words);
    let exports = union(|s| &s.exports);
    let defined_keys = union(|s| &s.defined_keys);

    let mut out = Vec::new();
    for scan in scans {
        // Defs in test files are framework-invoked (fixtures, test_* cases) — never "ghost".
        if is_test_file(&scan.path) {
            continue;
        }
        for def in &scan.defs {
            if def.decorated
                || def.base_type
                || is_dunder(&def.name)
                || is_entry_point(&def.name)
                || is_test_name(&def.name)
                || refs.contains(&def.name)
                || strings.contains(&def.name)
                || exports.contains(&def.name)
            {
                continue;
            }
            out.push(Finding {
                path: scan.path.clone(),
                range: def.range,
                message: format!(
                    "ghost scaffolding: {} `{}` is defined but never referenced anywhere in the \
                     project (dangling — wire it up or remove it)",
                    def.kind, def.name
                ),
            });
        }
    }

    // Ghost config only when the project defines *some* config constants in code (else config is
    // fully external and unanalyzable).
    if !defined_keys.is_empty() {
        for scan in scans {
            for read in &scan.config_reads {
                if defined_keys.contains(&read.key) || strings.contains(&read.key) {
                    continue;
                }
                out.push(Finding {
                    path: scan.path.clone(),
                    range: read.range,
                    message: format!(
                        "ghost config flag: `{}` is read but defined/assigned nowhere in the \
                         project (the branch is dead or relies on an undefined default)",
                        read.key
                    ),
                });
            }
        }
    }
    out
}

/// Collects references, string words, defined keys, and config reads across a file's whole tree.
struct Collector<'a> {
    scan: &'a mut FileScan,
}

impl<'a> Visitor<'a> for Collector<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    self.scan.refs.insert(
                        alias
                            .asname
                            .as_ref()
                            .map_or_else(
                                || alias.name.as_str().split('.').next().unwrap_or(""),
                                |a| a.as_str(),
                            )
                            .to_string(),
                    );
                }
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    self.scan.refs.insert(name.to_string());
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(name) => {
                self.scan.refs.insert(name.id.to_string());
                // A store of an UPPER_SNAKE name is a config-constant definition.
                if matches!(name.ctx, ExprContext::Store) && is_upper_snake(name.id.as_str()) {
                    self.scan.defined_keys.insert(name.id.to_string());
                }
            }
            Expr::Attribute(attr) => {
                self.scan.refs.insert(attr.attr.to_string());
                let key = attr.attr.as_str();
                if is_upper_snake(key) {
                    match attr.ctx {
                        // `settings.FLAG = ...` defines the flag.
                        ExprContext::Store => {
                            self.scan.defined_keys.insert(key.to_string());
                        }
                        // `settings.FLAG` read on a config-like receiver is a flag use.
                        ExprContext::Load => {
                            if let Expr::Name(recv) = attr.value.as_ref() {
                                if CONFIG_RECEIVERS
                                    .iter()
                                    .any(|r| recv.id.as_str().eq_ignore_ascii_case(r))
                                {
                                    self.scan.config_reads.push(ConfigRead {
                                        key: key.to_string(),
                                        range: attr.range(),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Expr::StringLiteral(string) => {
                for word in split_identifier_words(string.value.to_str()) {
                    self.scan.string_words.insert(word);
                }
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

/// `__all__ = [...]`/`(...)` string entries, if `assign` is that statement.
fn dunder_all(assign: &sloplint_python::ast::StmtAssign) -> Option<Vec<String>> {
    let is_all = assign
        .targets
        .iter()
        .any(|t| matches!(t, Expr::Name(n) if n.id.as_str() == "__all__"));
    if !is_all {
        return None;
    }
    let elts = match assign.value.as_ref() {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    Some(
        elts.iter()
            .filter_map(|e| match e {
                Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
                _ => None,
            })
            .collect(),
    )
}

fn class_has_base_or_protocol(class: &sloplint_python::ast::StmtClassDef) -> bool {
    class.bases().iter().any(|base| {
        let name = match base {
            Expr::Name(n) => n.id.as_str(),
            Expr::Attribute(a) => a.attr.as_str(),
            Expr::Subscript(s) => match s.value.as_ref() {
                Expr::Name(n) => n.id.as_str(),
                Expr::Attribute(a) => a.attr.as_str(),
                _ => "",
            },
            _ => "",
        };
        matches!(name, "ABC" | "ABCMeta" | "Protocol")
    })
}

fn name_has_base_suffix(name: &str) -> bool {
    BASE_SUFFIXES
        .iter()
        .any(|s| name.ends_with(s) && name.len() > s.len())
}

fn is_dunder(name: &str) -> bool {
    name.starts_with("__") && name.ends_with("__")
}

fn is_entry_point(name: &str) -> bool {
    ENTRY_POINTS.contains(&name)
}

/// pytest/unittest names invoked by the test runner, not referenced in source.
fn is_test_name(name: &str) -> bool {
    name.starts_with("test_") || name.starts_with("Test")
}

/// A test file (plural `tests` segment, `test_*.py`/`*_test.py`/`conftest.py`) — its defs are
/// framework-invoked, so they're never ghost.
fn is_test_file(path: &str) -> bool {
    let norm = path.replace('\\', "/");
    let file = norm.rsplit('/').next().unwrap_or(&norm);
    file == "conftest.py"
        || (file.starts_with("test_") && file.ends_with(".py"))
        || file.ends_with("_test.py")
        || norm.split('/').any(|seg| seg == "tests")
}

/// All-uppercase identifier with ≥1 letter and length ≥2 (`ENABLE_X`, `DEBUG`) — a config constant.
fn is_upper_snake(name: &str) -> bool {
    name.len() >= 2
        && name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Split a string into identifier-like words (`[A-Za-z0-9_]+`).
fn split_identifier_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn scan_src(path: &str, src: &str) -> FileScan {
        scan(path, &parse(src).unwrap())
    }

    fn messages(scans: &[FileScan]) -> Vec<String> {
        findings(scans).into_iter().map(|f| f.message).collect()
    }

    #[test]
    fn unreferenced_def_is_ghost_but_used_one_is_not() {
        let s = scan_src(
            "m.py",
            "def used():\n    return 1\n\n\ndef ghost():\n    return 2\n\n\nprint(used())\n",
        );
        let msgs = messages(&[s]);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert!(msgs[0].contains("`ghost`"), "{msgs:?}");
    }

    #[test]
    fn cross_file_reference_suppresses_ghost() {
        let a = scan_src("a.py", "class Widget:\n    pass\n");
        let b = scan_src("b.py", "from a import Widget\n\n\nw = Widget()\n");
        assert!(messages(&[a, b]).is_empty());
    }

    #[test]
    fn exported_decorated_and_base_types_are_allowlisted() {
        let s = scan_src(
            "m.py",
            "__all__ = [\"Exported\"]\n\n\nclass Exported:\n    pass\n\n\nclass WidgetBase:\n    pass\n\n\n@register\ndef handler():\n    pass\n",
        );
        // Exported (in __all__), WidgetBase (suffix), handler (decorated) all suppressed.
        assert!(messages(&[s]).is_empty());
    }

    #[test]
    fn entry_points_and_test_defs_are_not_ghost() {
        // Entry-point names (framework-invoked) and anything in a test file are never ghost.
        let src = scan_src(
            "app.py",
            "def main():\n    pass\n\n\ndef handler(event):\n    pass\n",
        );
        assert!(messages(&[src]).is_empty(), "entry points allowlisted");
        let test = scan_src(
            "tests/test_x.py",
            "def test_thing():\n    pass\n\n\ndef helper_unused():\n    pass\n",
        );
        assert!(messages(&[test]).is_empty(), "test-file defs skipped");
    }

    #[test]
    fn ghost_config_flag_when_other_constants_defined() {
        let s = scan_src(
            "m.py",
            "MAX_RETRIES = 3\n\n\ndef run():\n    if settings.ENABLE_X:\n        return MAX_RETRIES\n    return 0\n",
        );
        let msgs = messages(&[s]);
        assert!(
            msgs.iter()
                .any(|m| m.contains("ghost config flag") && m.contains("ENABLE_X")),
            "{msgs:?}"
        );
    }

    #[test]
    fn defined_config_flag_is_not_ghost() {
        let s = scan_src(
            "m.py",
            "ENABLE_X = False\n\n\ndef run():\n    if settings.ENABLE_X:\n        return 1\n    return 0\n",
        );
        assert!(!messages(&[s])
            .iter()
            .any(|m| m.contains("ghost config flag")));
    }

    #[test]
    fn no_config_finding_when_project_defines_no_constants() {
        // Without any code-defined config constants, config is external — don't guess.
        let s = scan_src(
            "m.py",
            "def run():\n    if settings.ENABLE_X:\n        return 1\n    return 0\n",
        );
        assert!(!messages(&[s])
            .iter()
            .any(|m| m.contains("ghost config flag")));
    }
}
