//! Undeclared-import detection (SLP180), a whole-tree rule. Split by concern: import `scan`ning,
//! PyPI `dist`ribution-name normalization, dependency-`manifest` parsing, and `findings` assembly.
//! This module holds the shared types, the rule metadata, and the re-exports.

use std::collections::HashSet;
use std::path::PathBuf;

use sloplint_macros::ViolationMetadata;
use sloplint_python::TextRange;

use crate::registry::WholeProjectRule;

mod dist;
mod findings;
mod manifest;
mod scan;

pub use dist::normalize_dist;
pub use findings::findings;
pub use manifest::{parse_pyproject_deps, requirement_dist_name, resolve_declared};
pub use scan::{first_party_names, scan_imports};

/// ## What it does
/// Flags a third-party import whose distribution is not declared in the project's dependency
/// manifest (`pyproject.toml`, falling back to `requirements*.txt`). Stdlib, first-party/local,
/// and relative imports are never flagged.
///
/// ## Why is this bad?
/// An import with no declared dependency breaks on a clean install — the code only runs because
/// the package happens to be present in the author's environment. It's a common tell of generated
/// code that reaches for a library without updating the manifest.
///
/// ## Example
/// ```python
/// import requests   # used here, but absent from pyproject.toml / requirements.txt
/// ```
#[derive(ViolationMetadata)]
pub struct UndeclaredImports;

impl WholeProjectRule for UndeclaredImports {
    fn code(&self) -> &'static str {
        "SLP180"
    }
}

/// A single third-party-looking import found in a file: its top-level module name and the
/// source range to point a diagnostic at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef {
    /// Top-level module name (`a` for `import a.b.c` or `from a.b import c`).
    pub top: String,
    /// Range to attribute a finding to (the alias for `import`, the module for `from`).
    pub range: TextRange,
}

/// Declared dependencies resolved from the project's manifest, plus where they came from
/// (for the diagnostic message). `dists` are PEP 503-normalized distribution names.
#[derive(Debug, Clone)]
pub struct Declared {
    pub dists: HashSet<String>,
    pub source: String,
    /// The directory the manifest was found in — the project root. First-party module names
    /// are derived from this tree so partial/single-file runs resolve local packages too.
    pub root: PathBuf,
}

/// One SLP180 finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub range: TextRange,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn imports_of(src: &str) -> Vec<String> {
        let parsed = parse(src).unwrap();
        scan_imports(&parsed).into_iter().map(|i| i.top).collect()
    }

    #[test]
    fn scans_top_level_imports_only() {
        let src = "\
import os
import requests
from a.b import c
from .local import d
from . import e
import pkg.sub as p

def f():
    import nested_only
";
        let tops = imports_of(src);
        assert_eq!(tops, vec!["os", "requests", "a", "pkg"]);
        // relative imports and function-local imports are excluded.
        assert!(!tops.contains(&"nested_only".to_string()));
        assert!(!tops.contains(&"local".to_string()));
    }

    #[test]
    fn skips_try_guarded_imports() {
        // Optional-dependency guards live under `try:`, not at module top level.
        let src = "\
try:
    import ujson as json
except ImportError:
    import json
";
        assert!(imports_of(src).is_empty());
    }

    #[test]
    fn normalize_follows_pep503() {
        assert_eq!(normalize_dist("Foo.Bar_baz"), "foo-bar-baz");
        assert_eq!(normalize_dist("PyYAML"), "pyyaml");
        assert_eq!(normalize_dist("scikit__learn"), "scikit-learn");
        assert_eq!(normalize_dist("__weird__"), "weird");
    }

    #[test]
    fn requirement_name_extraction() {
        assert_eq!(
            requirement_dist_name("requests").as_deref(),
            Some("requests")
        );
        assert_eq!(
            requirement_dist_name("Flask==2.0").as_deref(),
            Some("flask")
        );
        assert_eq!(
            requirement_dist_name("uvicorn[standard]>=0.18").as_deref(),
            Some("uvicorn")
        );
        assert_eq!(
            requirement_dist_name("ruff ; python_version >= '3.8'").as_deref(),
            Some("ruff")
        );
        assert_eq!(requirement_dist_name("  # a comment"), None);
        assert_eq!(requirement_dist_name("-r base.txt"), None);
        assert_eq!(requirement_dist_name("--hash=sha256:abc"), None);
        assert_eq!(requirement_dist_name("https://example.com/pkg.whl"), None);
        // VCS installs with an explicit egg fragment are declarations.
        assert_eq!(
            requirement_dist_name("git+https://github.com/foo/Bar.git#egg=Bar_baz").as_deref(),
            Some("bar-baz")
        );
        assert_eq!(
            requirement_dist_name("-e git+https://h/r.git#egg=mypkg").as_deref(),
            Some("mypkg")
        );
    }

    #[test]
    fn first_party_from_tree() {
        let paths = vec![
            "mypkg/__init__.py".to_string(),
            "mypkg/util.py".to_string(),
            "src/other/mod.py".to_string(),
            "toplevel.py".to_string(),
            "tests/test_x.py".to_string(),
        ];
        let names = first_party_names(&paths);
        assert!(names.contains("mypkg"));
        assert!(names.contains("other")); // src-layout stripped
        assert!(names.contains("toplevel"));
        assert!(names.contains("tests"));
    }

    fn declared(names: &[&str]) -> Declared {
        Declared {
            dists: names.iter().map(|n| normalize_dist(n)).collect(),
            source: "pyproject.toml".to_string(),
            root: PathBuf::from("."),
        }
    }

    fn refs(tops: &[&str]) -> Vec<ImportRef> {
        tops.iter()
            .map(|t| ImportRef {
                top: t.to_string(),
                range: TextRange::default(),
            })
            .collect()
    }

    #[test]
    fn flags_only_undeclared_third_party() {
        let files = vec![(
            "app.py".to_string(),
            refs(&["os", "requests", "numpy", "mypkg"]),
        )];
        let first_party: HashSet<String> = ["mypkg".to_string()].into_iter().collect();
        let dec = declared(&["requests"]);
        let extra = HashSet::new();
        let found = findings(&files, &first_party, &dec, &extra, |m| m == "os");
        // os = stdlib, requests = declared, mypkg = first-party -> only numpy fires.
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("`numpy`"));
    }

    #[test]
    fn always_available_packaging_modules_never_fire() {
        // setuptools / pkg_resources / pip ship with every pip env and are commonly imported
        // undeclared — flagging them would be a false positive on working code.
        let files = vec![(
            "conftest.py".to_string(),
            refs(&["setuptools", "pkg_resources", "pip"]),
        )];
        let first_party = HashSet::new();
        let extra = HashSet::new();
        let dec = declared(&[]); // explicitly zero declared deps
        let found = findings(&files, &first_party, &dec, &extra, |_| false);
        assert!(found.is_empty(), "packaging modules are always available");
    }

    #[test]
    fn import_to_distribution_mapping() {
        let files = vec![("app.py".to_string(), refs(&["cv2", "yaml", "PIL"]))];
        let first_party = HashSet::new();
        let extra = HashSet::new();
        // All three are declared under their real distribution names -> no findings.
        let dec = declared(&["opencv-python", "PyYAML", "pillow"]);
        let found = findings(&files, &first_party, &dec, &extra, |_| false);
        assert!(found.is_empty(), "mapped distributions count as declared");

        // Drop the mappings: now cv2 fires and names its distribution.
        let dec = declared(&["pillow"]);
        let found = findings(&files, &first_party, &dec, &extra, |_| false);
        let tops: Vec<&str> = found.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(found.len(), 2); // cv2 + yaml
        assert!(tops.iter().any(|m| m.contains("opencv-python")));
    }

    #[test]
    fn config_extra_suppresses() {
        let files = vec![("app.py".to_string(), refs(&["internal_mirror"]))];
        let first_party = HashSet::new();
        let dec = declared(&[]);
        let extra: HashSet<String> = [normalize_dist("internal-mirror")].into_iter().collect();
        let found = findings(&files, &first_party, &dec, &extra, |_| false);
        assert!(found.is_empty());
    }

    #[test]
    fn empty_declared_table_still_fires() {
        // An explicit `dependencies = []` is a real declaration of zero deps.
        let dists = parse_pyproject_deps("[project]\nname = \"x\"\ndependencies = []\n").unwrap();
        assert!(dists.is_empty());
    }

    #[test]
    fn pyproject_without_deps_table_is_none() {
        // No recognized table -> caller falls back / skips.
        assert!(parse_pyproject_deps("[build-system]\nrequires = [\"setuptools\"]\n").is_none());
    }

    #[test]
    fn parses_pep621_and_optional_and_groups() {
        let text = "\
[project]
name = \"demo\"
dependencies = [\"requests>=2\", \"rich\"]
[project.optional-dependencies]
dev = [\"pytest\", \"ruff\"]
[dependency-groups]
test = [\"coverage\"]
";
        let dists = parse_pyproject_deps(text).unwrap();
        for d in ["requests", "rich", "pytest", "ruff", "coverage"] {
            assert!(dists.contains(d), "{d} should be declared");
        }
    }

    #[test]
    fn parses_poetry_dependencies() {
        let text = "\
[tool.poetry]
name = \"demo\"
[tool.poetry.dependencies]
python = \"^3.10\"
requests = \"^2.0\"
SQLAlchemy = \"^2.0\"
[tool.poetry.group.dev.dependencies]
pytest = \"^7.0\"
";
        let dists = parse_pyproject_deps(text).unwrap();
        assert!(dists.contains("requests"));
        assert!(dists.contains("sqlalchemy")); // normalized
        assert!(dists.contains("pytest"));
        assert!(!dists.contains("python")); // the interpreter constraint is skipped
    }

    /// A fresh, empty temp directory unique to this process + `tag` (so concurrent tests
    /// don't collide).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sloplint-resolve-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_reads_pyproject_with_deps() {
        let dir = temp_dir("pyproj");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"x\"\ndependencies = [\"requests\"]\n",
        )
        .unwrap();
        let declared = resolve_declared(&dir).unwrap();
        assert!(declared.dists.contains("requests"));
        assert_eq!(declared.root, dir, "root is the manifest directory");
        assert_eq!(declared.source, "pyproject.toml", "source is cwd-relative");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_falls_back_to_requirements_when_pyproject_has_no_deps() {
        let dir = temp_dir("fallback");
        // A pyproject with no recognized deps table -> look at requirements beside it.
        std::fs::write(
            dir.join("pyproject.toml"),
            "[build-system]\nrequires = [\"setuptools\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("requirements.txt"), "flask==2.0\n").unwrap();
        let declared = resolve_declared(&dir).unwrap();
        assert!(declared.dists.contains("flask"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_pyproject_without_deps_and_no_requirements_is_none() {
        let dir = temp_dir("nodeps");
        // pyproject present but declares nothing resolvable and no requirements beside it ->
        // ambiguous, so we stop (don't walk further up) and report nothing.
        std::fs::write(
            dir.join("pyproject.toml"),
            "[build-system]\nrequires = [\"setuptools\"]\n",
        )
        .unwrap();
        assert!(resolve_declared(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_reads_requirements_when_no_pyproject() {
        let dir = temp_dir("reqonly");
        // All `requirements*.txt` are globbed; comments and option lines are ignored.
        std::fs::write(
            dir.join("requirements.txt"),
            "Django>=4\n# comment\n-r other.txt\n",
        )
        .unwrap();
        std::fs::write(dir.join("requirements-dev.txt"), "pytest\n").unwrap();
        let declared = resolve_declared(&dir).unwrap();
        assert!(declared.dists.contains("django")); // normalized
        assert!(declared.dists.contains("pytest")); // requirements-*.txt also read
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_walks_up_to_an_ancestor_manifest() {
        let root = temp_dir("walkup");
        let child = root.join("a").join("b");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.join("pyproject.toml"),
            "[project]\ndependencies = [\"rich\"]\n",
        )
        .unwrap();
        // Starting deep in the tree, resolution climbs to the ancestor that has the manifest.
        let declared = resolve_declared(&child).unwrap();
        assert!(declared.dists.contains("rich"));
        assert_eq!(
            declared.root, root,
            "root is the ancestor that owns the manifest"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
