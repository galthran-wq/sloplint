//! SLP090: flat-directory fanout — too many `.py` modules dumped in one directory.
//!
//! A whole-tree rule: it needs every discovered file's path, so (like SLP180 import resolution) it
//! runs after the per-file pass. The pure logic lives here in the linter; the binary drives it over
//! the discovered files and attributes each finding, honoring per-path selection.

use std::collections::BTreeMap;
use std::path::Path;

use sloplint_macros::ViolationMetadata;

use crate::registry::WholeProjectRule;

/// ## What it does
/// Flags directories that hold more than `dir_max_modules` Python modules directly (a flat
/// "dumping ground" of files in one folder, rather than a deliberate package structure).
///
/// ## Why is this bad?
/// A directory with dozens of unrelated modules has no architectural shape: it signals missing
/// sub-packages and makes it hard to find anything or reason about cohesion. Such flat fanout is
/// a frequent tell of code generated file-by-file without an organizing module layout.
///
/// ## Example
/// ```text
/// myapp/            # 40 *.py files dumped here, no sub-packages
///   user_create.py
///   user_update.py
///   user_delete.py
///   ...
/// ```
#[derive(ViolationMetadata)]
pub struct Fanout;

impl WholeProjectRule for Fanout {
    fn code(&self) -> &'static str {
        "SLP090"
    }
}

/// One over-full directory, reported on its first module (in input order).
pub struct Finding {
    /// The file the `SLP090` diagnostic attaches to — the directory's first module.
    pub path: String,
    /// The rendered finding message.
    pub message: String,
}

/// Flag each directory holding more than `max_modules` Python modules. `paths` are the project's
/// discovered files; returns one finding per over-full directory (in directory-sorted order),
/// attached to that directory's first file in input order.
pub fn findings(paths: &[String], max_modules: usize) -> Vec<Finding> {
    let mut by_dir: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for path in paths {
        let dir = Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_default();
        by_dir.entry(dir).or_default().push(path);
    }

    let mut out = Vec::new();
    for (dir, files) in by_dir {
        if files.len() <= max_modules {
            continue;
        }
        let shown_dir = if dir.is_empty() { "." } else { &dir };
        out.push(Finding {
            path: files[0].clone(),
            message: format!(
                "directory `{shown_dir}` holds {} Python modules (max {max_modules}); \
                 split it into sub-packages",
                files.len()
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_a_directory_over_the_limit_on_its_first_file() {
        let files = paths(&["pkg/a.py", "pkg/b.py", "pkg/c.py"]);
        let found = findings(&files, 2);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "pkg/a.py"); // representative = first file in the dir
        assert!(
            found[0].message.contains("`pkg`") && found[0].message.contains("3 Python modules"),
            "{}",
            found[0].message
        );
    }

    #[test]
    fn at_or_under_the_limit_is_silent() {
        let files = paths(&["pkg/a.py", "pkg/b.py"]);
        assert!(findings(&files, 2).is_empty());
    }

    #[test]
    fn only_over_full_directories_are_reported_in_sorted_order() {
        let files = paths(&[
            "z/a.py",
            "z/b.py",
            "z/c.py", // over (3 > 2)
            "a/x.py",
            "a/y.py",
            "a/w.py", // over (3 > 2)
            "ok/one.py",
            "ok/two.py", // at limit
        ]);
        let found = findings(&files, 2);
        let dirs: Vec<&str> = found.iter().map(|f| f.path.as_str()).collect();
        // Directory-sorted: `a` before `z`; `ok` not reported.
        assert_eq!(dirs, vec!["a/x.py", "z/a.py"]);
    }

    #[test]
    fn root_level_modules_show_as_dot() {
        let files = paths(&["a.py", "b.py", "c.py"]);
        let found = findings(&files, 2);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("`.`"), "{}", found[0].message);
    }
}
