//! SLP180 — undeclared third-party import.
//!
//! A whole-project rule (like SLP020/090): it needs every file's imports, the project's
//! own first-party module names, and the dependency manifest before it can decide whether
//! an import is declared. So it runs in the CLI cross-file pass, not the per-file registry.
//!
//! For each `import x` / `from x import y` whose top-level module is third-party, we flag it
//! when its distribution is not declared in the project's dependency manifest
//! (`pyproject.toml`, falling back to `requirements*.txt`). We never flag stdlib modules,
//! first-party/local modules, or relative imports.
//!
//! Conservative bias (slop = broken code, not provenance): when dependency resolution is
//! ambiguous — no manifest at all — we don't fire. False negatives over false positives.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sloplint_macros::ViolationMetadata;
use sloplint_python::ast::{ModModule, Stmt};
use sloplint_python::parser::Parsed;
use sloplint_python::{Ranged, TextRange};

use crate::registry::WholeProjectRule;

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

/// PEP 503 normalization of a distribution name: lowercase, with runs of `-`, `_`, and `.`
/// collapsed to a single `-`. So `Foo.Bar_baz` and `foo-bar-baz` compare equal.
pub fn normalize_dist(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' || ch == '.' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.extend(ch.to_lowercase());
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

/// Distribution names that ship under an import name different from the distribution name.
/// Returns the normalized distribution name(s) an import maps to, for the common mismatches
/// where `import foo` does *not* come from a distribution called `foo`. The caller always
/// also tests the normalized import name itself, so this only lists the exceptions.
fn distribution_aliases(import_top: &str) -> &'static [&'static str] {
    match import_top {
        "cv2" => &["opencv-python"],
        "PIL" => &["pillow"],
        "yaml" => &["pyyaml"],
        "bs4" => &["beautifulsoup4"],
        "sklearn" => &["scikit-learn"],
        "skimage" => &["scikit-image"],
        "dateutil" => &["python-dateutil"],
        "dotenv" => &["python-dotenv"],
        "jose" => &["python-jose"],
        "slugify" => &["python-slugify"],
        "magic" => &["python-magic"],
        "docx" => &["python-docx"],
        "pptx" => &["python-pptx"],
        "attr" => &["attrs"],
        "jwt" => &["pyjwt"],
        "nacl" => &["pynacl"],
        "zmq" => &["pyzmq"],
        "serial" => &["pyserial"],
        "usb" => &["pyusb"],
        "OpenSSL" => &["pyopenssl"],
        "Crypto" => &["pycryptodome", "pycrypto"],
        "Cryptodome" => &["pycryptodomex"],
        "fitz" => &["pymupdf"],
        "bson" | "gridfs" => &["pymongo"],
        "psycopg2" => &["psycopg2-binary"],
        "grpc" => &["grpcio"],
        "mpl_toolkits" => &["matplotlib"],
        _ => &[],
    }
}

/// Top-level modules that ship with essentially every pip/virtualenv environment but are not
/// part of `sys.stdlib_module_names` and are routinely imported without being declared
/// (version lookups, `conftest.py`, build/entry-point helpers). Treating them as always
/// available keeps the conservative bias — flagging them would be a false positive on code
/// that works fine on a clean install.
fn is_always_available(module: &str) -> bool {
    matches!(
        module,
        "setuptools" | "pkg_resources" | "pip" | "wheel" | "_distutils_hack"
    )
}

/// Pure core: produce SLP180 findings for the given files' imports against the resolved
/// declared dependencies and first-party names. `is_stdlib` is injected so this is testable
/// without the bundled stdlib table. `extra` is an additional set of (already normalized)
/// declared distribution names from config.
pub fn findings(
    files: &[(String, Vec<ImportRef>)],
    first_party: &HashSet<String>,
    declared: &Declared,
    extra: &HashSet<String>,
    is_stdlib: impl Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (path, imports) in files {
        for import in imports {
            if is_stdlib(&import.top)
                || is_always_available(&import.top)
                || first_party.contains(&import.top)
            {
                continue;
            }
            let aliases = distribution_aliases(&import.top);
            let mut candidates: Vec<String> = vec![normalize_dist(&import.top)];
            candidates.extend(aliases.iter().map(|a| normalize_dist(a)));
            let declared_here = candidates
                .iter()
                .any(|c| declared.dists.contains(c) || extra.contains(c));
            if declared_here {
                continue;
            }
            let named = match aliases.first() {
                Some(dist) => format!("`{}` (distribution `{dist}`)", import.top),
                None => format!("`{}`", import.top),
            };
            let message = format!(
                "{named} is imported but not declared in the project dependencies ({})",
                declared.source
            );
            out.push(Finding {
                path: path.clone(),
                range: import.range,
                message,
            });
        }
    }
    out
}

/// Resolve the project's declared dependencies, walking up from `start`.
///
/// Resolution order, at the first ancestor directory that yields anything:
/// 1. `pyproject.toml` with a recognized dependency table (PEP 621 `[project]`, PEP 735
///    `[dependency-groups]`, or poetry).
/// 2. otherwise `requirements*.txt` in that directory.
///
/// Returns `None` when no manifest declaring dependencies is found anywhere — the
/// conservative signal to skip SLP180 entirely rather than flag every import.
pub fn resolve_declared(start: &Path) -> Option<Declared> {
    for dir in start.ancestors() {
        let pyproject = dir.join("pyproject.toml");
        if pyproject.is_file() {
            if let Ok(text) = std::fs::read_to_string(&pyproject) {
                if let Some(dists) = parse_pyproject_deps(&text) {
                    return Some(Declared {
                        dists,
                        source: display_path(&pyproject, start),
                        root: dir.to_path_buf(),
                    });
                }
            }
            // A pyproject without any deps table: fall back to requirements beside it.
            if let Some(declared) = requirements_in(dir, start) {
                return Some(declared);
            }
            // pyproject present but declares nothing resolvable -> ambiguous, don't fire.
            return None;
        }
        if let Some(declared) = requirements_in(dir, start) {
            return Some(declared);
        }
    }
    None
}

/// Collect declared distributions from `requirements*.txt` files directly in `dir`.
fn requirements_in(dir: &Path, start: &Path) -> Option<Declared> {
    let mut files: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("requirements") && name.ends_with(".txt") {
            files.push(entry.path());
        }
    }
    if files.is_empty() {
        return None;
    }
    files.sort();
    let mut dists = HashSet::new();
    for file in &files {
        if let Ok(text) = std::fs::read_to_string(file) {
            for line in text.lines() {
                if let Some(name) = requirement_dist_name(line) {
                    dists.insert(name);
                }
            }
        }
    }
    let source = files
        .iter()
        .map(|f| display_path(f, start))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Declared {
        dists,
        source,
        root: dir.to_path_buf(),
    })
}

/// Parse declared distribution names from `pyproject.toml` text.
///
/// `None` means no recognized dependency table is present (so the caller should look
/// elsewhere); `Some(set)` means at least one table was found — even an explicitly empty
/// `dependencies = []`, which legitimately declares zero dependencies.
pub fn parse_pyproject_deps(text: &str) -> Option<HashSet<String>> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let mut dists = HashSet::new();
    let mut found = false;

    // PEP 621: [project].dependencies + [project.optional-dependencies].
    if let Some(project) = value.get("project").and_then(|v| v.as_table()) {
        if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
            found = true;
            collect_pep508(deps, &mut dists);
        }
        if let Some(opt) = project
            .get("optional-dependencies")
            .and_then(|v| v.as_table())
        {
            found = true;
            for group in opt.values() {
                if let Some(arr) = group.as_array() {
                    collect_pep508(arr, &mut dists);
                }
            }
        }
    }

    // PEP 735: [dependency-groups] — arrays of requirement strings (or include-group dicts,
    // which we skip).
    if let Some(groups) = value.get("dependency-groups").and_then(|v| v.as_table()) {
        found = true;
        for group in groups.values() {
            if let Some(arr) = group.as_array() {
                collect_pep508(arr, &mut dists);
            }
        }
    }

    // Poetry: [tool.poetry.dependencies] (+ groups), where the *keys* are distribution names.
    if let Some(poetry) = value
        .get("tool")
        .and_then(|v| v.get("poetry"))
        .and_then(|v| v.as_table())
    {
        if let Some(deps) = poetry.get("dependencies").and_then(|v| v.as_table()) {
            found = true;
            collect_poetry_keys(deps, &mut dists);
        }
        if let Some(group) = poetry.get("group").and_then(|v| v.as_table()) {
            for g in group.values() {
                if let Some(deps) = g.get("dependencies").and_then(|v| v.as_table()) {
                    found = true;
                    collect_poetry_keys(deps, &mut dists);
                }
            }
        }
    }

    found.then_some(dists)
}

/// Add the normalized distribution names from an array of PEP 508 requirement strings.
fn collect_pep508(arr: &[toml::Value], dists: &mut HashSet<String>) {
    for item in arr {
        if let Some(s) = item.as_str() {
            if let Some(name) = requirement_dist_name(s) {
                dists.insert(name);
            }
        }
    }
}

/// Add the normalized distribution names from a poetry dependency table's keys (skipping the
/// implicit `python` constraint).
fn collect_poetry_keys(table: &toml::value::Table, dists: &mut HashSet<String>) {
    for key in table.keys() {
        if key == "python" {
            continue;
        }
        dists.insert(normalize_dist(key));
    }
}

/// Extract and normalize the distribution name from one requirement specifier line.
///
/// Handles PEP 508 specifiers (`pkg`, `pkg==1.2`, `pkg[extra]>=1`, `pkg ; marker`,
/// `pkg @ url`) and VCS/URL installs with an explicit `#egg=name` fragment (incl. `-e`
/// editable installs). Returns `None` for blanks, comments, other pip options (`-r`,
/// `--hash`), and bare URLs/VCS specs where there is no reliable name to extract.
pub fn requirement_dist_name(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // VCS/URL install with an explicit egg fragment, e.g. `git+https://h/r.git#egg=name` or
    // `-e git+...#egg=name`. Extracted before comment-stripping (the fragment uses `#`).
    if let Some(idx) = line.find("#egg=") {
        let rest = &line[idx + "#egg=".len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
            .unwrap_or(rest.len());
        let name = &rest[..end];
        return (!name.is_empty()).then(|| normalize_dist(name));
    }
    // Strip an inline comment, then reject blanks and pip option lines.
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() || line.starts_with('-') {
        return None;
    }
    // A leading URL/VCS reference (`git+https://...`, `https://...`) has no parseable name.
    if line.contains("://") && !line.contains(" @ ") {
        return None;
    }
    // The name runs up to the first specifier/extras/marker/url delimiter.
    let end = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(line.len());
    let name = &line[..end];
    if name.is_empty() {
        return None;
    }
    Some(normalize_dist(name))
}

/// Render a manifest path for the diagnostic message, relative to the working directory when
/// possible (so a discovered `/abs/proj/pyproject.toml` reads as `pyproject.toml`).
fn display_path(path: &Path, start: &Path) -> String {
    let stripped = path
        .strip_prefix(start)
        .or_else(|_| path.strip_prefix("./"))
        .unwrap_or(path);
    stripped.to_string_lossy().to_string()
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
