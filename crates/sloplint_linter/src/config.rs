//! Configuration model and loading (`sloplint.toml`).
//!
//! Mirrors Ruff's select/ignore model with per-path overrides. Defaults are deliberately
//! strict-but-safe: every stable `SLP` rule is enabled, nothing is ignored, preview rules
//! are off. A code is "enabled" for a file when a `select` prefix matches it, no `ignore`
//! prefix matches, and no matching path override ignores it.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use serde::Deserialize;

/// Top-level configuration, as parsed from `sloplint.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Code prefixes to enable; a rule is selected when any prefix is a prefix of its code.
    pub select: Vec<String>,
    /// Code prefixes to disable; takes precedence over `select`.
    pub ignore: Vec<String>,
    /// Enable preview-group (unstable) rules. Off by default, like Ruff's `--preview`.
    pub preview: bool,
    /// Per-path overrides; all matching overrides apply (their ignores accumulate).
    pub overrides: Vec<PathOverride>,
    /// Near-duplicate (clone) detection settings.
    pub clone: CloneSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            select: vec!["SLP".to_string()],
            ignore: Vec::new(),
            preview: false,
            overrides: Vec::new(),
            clone: CloneSettings::default(),
        }
    }
}

/// Tunable thresholds for SLP020 near-duplicate detection. Conservative by default.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloneSettings {
    /// Minimum statements (incl. nested) for a function to be considered for cloning.
    pub min_statements: usize,
    /// Jaccard similarity at/above which two functions are reported as clones.
    pub similarity: f64,
}

impl Default for CloneSettings {
    fn default() -> Self {
        Self {
            min_statements: 3,
            similarity: 0.85,
        }
    }
}

/// A per-path override: extra rule ignores (and, later, comment allowances) for files whose
/// path matches `path` (a gitignore-style glob).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathOverride {
    /// Glob matched against the file path, e.g. `"alembic/**"` or `"tests/**"`.
    pub path: String,
    /// Code prefixes to additionally ignore for matching files.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Whether comments are allowed for matching files (consumed by the comment rules).
    #[serde(default)]
    pub allow_comments: bool,
}

impl Config {
    /// Parse a config from TOML text.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Load `sloplint.toml` from `dir`, walking up to ancestors. Returns the default config
    /// if none is found.
    pub fn discover(start: &Path) -> Result<Self, ConfigError> {
        for dir in start.ancestors() {
            let candidate = dir.join("sloplint.toml");
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)
                    .map_err(|e| ConfigError::Read(candidate.clone(), e))?;
                return Self::from_toml_str(&text)
                    .map_err(|e| ConfigError::Parse(candidate, Box::new(e)));
            }
        }
        Ok(Self::default())
    }

    /// Compile globs once so per-file selection is cheap. Fails on an invalid glob.
    pub fn prepare(&self) -> Result<Selector<'_>, globset::Error> {
        let mut overrides = Vec::with_capacity(self.overrides.len());
        for ov in &self.overrides {
            overrides.push((Glob::new(&ov.path)?.compile_matcher(), ov));
        }
        Ok(Selector {
            config: self,
            overrides,
        })
    }
}

/// A config with its path globs compiled, ready for repeated per-file queries.
pub struct Selector<'a> {
    config: &'a Config,
    overrides: Vec<(GlobMatcher, &'a PathOverride)>,
}

impl Selector<'_> {
    /// Whether `code` is enabled for the file at `path`.
    pub fn is_enabled(&self, code: &str, path: &str) -> bool {
        let selected = self
            .config
            .select
            .iter()
            .any(|p| code.starts_with(p.as_str()));
        if !selected {
            return false;
        }
        if self
            .config
            .ignore
            .iter()
            .any(|p| code.starts_with(p.as_str()))
        {
            return false;
        }
        for (matcher, ov) in &self.overrides {
            if matcher.is_match(path) && ov.ignore.iter().any(|p| code.starts_with(p.as_str())) {
                return false;
            }
        }
        true
    }

    /// Whether comments are allowed for the file at `path` (any matching override opts in).
    pub fn comments_allowed(&self, path: &str) -> bool {
        self.overrides
            .iter()
            .any(|(matcher, ov)| ov.allow_comments && matcher.is_match(path))
    }

    /// Whether preview-group rules are enabled.
    pub fn preview(&self) -> bool {
        self.config.preview
    }
}

/// Errors from discovering/parsing a config file.
#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, Box<toml::de::Error>),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read(path, err) => write!(f, "reading {}: {err}", path.display()),
            ConfigError::Parse(path, err) => write!(f, "parsing {}: {err}", path.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_enables_all_slp_rules() {
        let config = Config::default();
        let selector = config.prepare().unwrap();
        assert!(selector.is_enabled("SLP001", "src/app.py"));
        assert!(selector.is_enabled("SLP090", "src/app.py"));
        assert!(!selector.preview());
    }

    #[test]
    fn ignore_prefix_disables() {
        let config = Config::from_toml_str("ignore = [\"SLP01\"]").unwrap();
        let selector = config.prepare().unwrap();
        assert!(!selector.is_enabled("SLP010", "a.py"));
        assert!(selector.is_enabled("SLP020", "a.py"));
    }

    #[test]
    fn per_path_override_ignores_and_allows_comments() {
        let toml = r#"
[[overrides]]
path = "alembic/**"
ignore = ["SLP010"]
allow_comments = true
"#;
        let config = Config::from_toml_str(toml).unwrap();
        let selector = config.prepare().unwrap();
        // Disabled under the override path, enabled elsewhere.
        assert!(!selector.is_enabled("SLP010", "alembic/versions/001_init.py"));
        assert!(selector.is_enabled("SLP010", "src/app.py"));
        // Comment allowance follows the same glob.
        assert!(selector.comments_allowed("alembic/versions/001_init.py"));
        assert!(!selector.comments_allowed("src/app.py"));
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(Config::from_toml_str("slect = [\"SLP\"]").is_err());
    }
}
