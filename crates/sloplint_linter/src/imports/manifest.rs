//! Dependency-manifest parsing: resolve declared dependencies from `pyproject.toml`
//! (falling back to `requirements*.txt`), walking up to an ancestor manifest.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::dist::normalize_dist;
use super::Declared;

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
