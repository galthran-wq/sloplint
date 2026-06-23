//! Import scanning: a file's top-level imports, and the project's own first-party module names.

use std::collections::HashSet;

use sloplint_python::ast::{ModModule, Stmt};
use sloplint_python::parser::Parsed;
use sloplint_python::Ranged;

use super::ImportRef;

/// Collect the top-level, module-level imports of a parsed file.
///
/// Only direct children of the module body are scanned — imports nested in functions,
/// `try`/`except ImportError` fallbacks, or `if TYPE_CHECKING:` blocks are deliberately
/// skipped. Those are the idioms most likely to legitimately reference an optional or
/// guarded dependency, so skipping them keeps the conservative (false-negative) bias.
/// Relative imports (`from . import x`) are never third-party, so they're skipped too.
pub fn scan_imports(parsed: &Parsed<ModModule>) -> Vec<ImportRef> {
    let mut imports = Vec::new();
    for stmt in &parsed.syntax().body {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    if let Some(top) = top_level(alias.name.as_str()) {
                        imports.push(ImportRef {
                            top,
                            range: alias.range,
                        });
                    }
                }
            }
            Stmt::ImportFrom(from) => {
                // `level > 0` is a relative import (`from . import x` / `from ..pkg import y`).
                if from.level > 0 {
                    continue;
                }
                if let Some(module) = &from.module {
                    if let Some(top) = top_level(module.as_str()) {
                        imports.push(ImportRef {
                            top,
                            range: module.range(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    imports
}

/// The top-level component of a dotted module path (`a.b.c` -> `a`), or `None` if empty.
fn top_level(dotted: &str) -> Option<String> {
    let head = dotted.split('.').next().unwrap_or("");
    if head.is_empty() {
        None
    } else {
        Some(head.to_string())
    }
}

/// The set of first-party (project-local) top-level module names, derived from the
/// discovered source tree. Over-collecting here is safe: a name treated as first-party is
/// never flagged, which keeps the false-negative bias.
///
/// For a path `pkg/sub/mod.py` the top-level importable name is `pkg`; for a top-level file
/// `mod.py` it's `mod`. A leading `src/` (the src-layout convention) is stripped first.
pub fn first_party_names(paths: &[String]) -> HashSet<String> {
    let mut names = HashSet::new();
    for path in paths {
        let mut segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segs.first() == Some(&"src") && segs.len() > 1 {
            segs.remove(0);
        }
        match segs.as_slice() {
            [] => {}
            [file] => {
                if let Some(stem) = file.strip_suffix(".py") {
                    if !stem.is_empty() {
                        names.insert(stem.to_string());
                    }
                }
            }
            [dir, ..] => {
                names.insert((*dir).to_string());
            }
        }
    }
    names
}
