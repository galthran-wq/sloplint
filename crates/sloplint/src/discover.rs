//! Filesystem discovery for the CLI: walk input paths into the set of `.py` files to check,
//! and map a path to its dotted module name for the import graph.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use sloplint_metrics::graph;

/// Discover `.py` files under the given paths. Returns the files and whether any path was
/// missing or a traversal error occurred — callers fail the run on that, so a typo'd path
/// never reports "clean". Inside a git repo the `ignore` crate honors `.gitignore`;
/// explicitly-passed files are always included.
pub(crate) fn discover_python_files(paths: &[String]) -> (Vec<PathBuf>, bool) {
    let default = [".".to_string()];
    let inputs: &[String] = if paths.is_empty() { &default } else { paths };

    let mut files = Vec::new();
    let mut had_error = false;
    for input in inputs {
        let path = Path::new(input);
        if path.is_file() {
            if is_python(path) {
                files.push(normalize(path));
            }
            continue;
        }
        if !path.is_dir() {
            eprintln!("error: path not found: {input}");
            had_error = true;
            continue;
        }
        for result in WalkBuilder::new(path).build() {
            match result {
                Ok(entry) => {
                    let entry_path = entry.path();
                    if entry_path.is_file() && is_python(entry_path) {
                        files.push(normalize(entry_path));
                    }
                }
                Err(err) => {
                    eprintln!("error: walking {input}: {err}");
                    had_error = true;
                }
            }
        }
    }
    files.sort();
    files.dedup();
    (files, had_error)
}

/// Strip a leading `./` so paths from `WalkBuilder::new(".")` (`./a/b.py`) match globs
/// written the documented way (`a/**`) and display cleanly. Other paths pass through.
fn normalize(path: &Path) -> PathBuf {
    path.strip_prefix(".").unwrap_or(path).to_path_buf()
}

pub(crate) fn is_python(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "py")
}

/// The first-party dotted module name for a discovered `.py` file, for the import graph.
///
/// The dotted name must match what `import` statements actually reference, regardless of where
/// the project sits relative to the working directory. So we find the file's **source root** —
/// the nearest ancestor directory that is *not* itself a Python package — by walking up while a
/// directory contains `__init__.py`, then name the module relative to that root. This resolves
/// `tests/fixtures/proj/a.py` to `proj.a` (not `tests.fixtures.proj.a`) and handles the `src/`
/// layout for free (the walk stops at `src`, which has no `__init__.py`).
///
/// Known limitation (documented): a PEP 420 namespace package (a directory with no
/// `__init__.py`) is treated as a source-root boundary, so its prefix is dropped from the names
/// of modules in nested regular sub-packages. Full multi-root namespace handling is out of scope
/// for this foundation.
pub(crate) fn module_name(path: &Path) -> Option<graph::ModuleName> {
    let mut root = path.parent()?;
    while root.join("__init__.py").is_file() {
        match root.parent() {
            Some(parent) => root = parent,
            None => break,
        }
    }
    let rel = path.strip_prefix(root).ok()?;
    graph::module_from_path(&rel.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_linter::config::Config;

    #[test]
    fn normalize_strips_leading_dot_slash() {
        assert_eq!(
            normalize(Path::new("./tests/t.py")),
            Path::new("tests/t.py")
        );
        assert_eq!(normalize(Path::new("tests/t.py")), Path::new("tests/t.py"));
        assert_eq!(normalize(Path::new("/abs/t.py")), Path::new("/abs/t.py"));
    }

    #[test]
    fn normalized_walk_paths_match_documented_globs() {
        // Regression: WalkBuilder::new(".") yields "./tests/t.py"; a `tests/**` profile glob
        // must still apply after normalization.
        let config = Config::from_toml_str(
            "[[profiles]]\nname = \"tests\"\nmatch = [\"tests/**\"]\nignore = [\"SLP010\"]\n\
             [[profiles]]\nname = \"production\"\ndefault = true\n",
        )
        .unwrap();
        let selector = config.prepare().unwrap();
        let walked = normalize(Path::new("./tests/t.py"));
        assert!(!selector.is_enabled("SLP010", &walked.to_string_lossy()));
    }
}
