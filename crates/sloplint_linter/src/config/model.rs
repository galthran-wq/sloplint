//! Config data model: the `Config` struct, its per-category settings, `Limits`, and `ConfigError`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::profile::{default_profiles, Profile};

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
    /// Named, path-matched profiles: a profile both carries rule deltas and defines a metrics
    /// panel. Omitted in TOML ⇒ the built-in `tests` / `generated` / `production` trio
    /// ([`default_profiles`]); declaring any replaces that set.
    #[serde(default = "default_profiles")]
    pub profiles: Vec<Profile>,
    /// Near-duplicate (clone) detection settings (global defaults; a profile may override).
    pub clone: CloneSettings,
    /// Size/shape limits for the structural rules (global defaults; a profile may override).
    pub limits: Limits,
    /// Which metric badges `metrics --badges` emits, and an optional combined summary.
    pub badges: BadgeSettings,
    /// Settings for the import rules (SLP180 undeclared third-party import).
    pub imports: ImportSettings,
    /// Settings for the security rules (SLP210 phantom security guard).
    pub security: SecuritySettings,
    /// Settings for the placeholder rules (SLP230 mock/placeholder data).
    pub placeholders: PlaceholderSettings,
    /// Settings for the comment rules (SLP004 hedging/narration comment tells).
    pub comments: CommentSettings,
    /// Settings for the cross-language rule (SLP250 cross-language pollution).
    pub crosslang: CrossLangSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            select: vec!["SLP".to_string()],
            ignore: Vec::new(),
            preview: false,
            profiles: default_profiles(),
            clone: CloneSettings::default(),
            limits: Limits::default(),
            badges: BadgeSettings::default(),
            imports: ImportSettings::default(),
            security: SecuritySettings::default(),
            placeholders: PlaceholderSettings::default(),
            comments: CommentSettings::default(),
            crosslang: CrossLangSettings::default(),
        }
    }
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
}

/// Settings for SLP180 (undeclared third-party import). The check resolves declared
/// dependencies from the project manifest automatically; `extra` lets a project declare
/// additional distribution names that aren't in the manifest (e.g. dynamically installed or
/// namespace packages) to suppress false positives. Names are matched PEP 503-normalized.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImportSettings {
    /// Extra distribution names to treat as declared, beyond the manifest.
    pub extra: Vec<String>,
}

/// Settings for SLP210 (phantom security guard). The check uses a built-in catalog of
/// security-guard names; `extra` adds project-specific guard names (e.g. an in-house
/// `require_tenant`) so a call to one that isn't defined/imported is also flagged.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecuritySettings {
    /// Extra security-guard names to treat as guards, beyond the built-in catalog.
    pub extra: Vec<String>,
}

/// Settings for SLP230 (mock/placeholder data). The check uses built-in placeholder sets;
/// `extra` adds project-specific placeholder literal values (matched against credential values and
/// dummy return strings) so an in-house sentinel like `"REPLACE_ME"` is also flagged.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlaceholderSettings {
    /// Extra placeholder literal values to flag, beyond the built-in sets.
    pub extra: Vec<String>,
}

/// Settings for SLP004 (comment tells). The check uses a built-in hedging/deferral lexicon;
/// `extra` adds project-specific hedging phrases to flag in comments.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommentSettings {
    /// Extra hedging/deferral comment phrases to flag, beyond the built-in lexicon.
    pub extra: Vec<String>,
}

/// Settings for SLP250 (cross-language pollution). The check uses a narrow built-in blocklist of
/// foreign idioms; `allow` adds project-specific names to treat as legitimate Python (suppressing
/// false positives), extending the built-in FP-prone allow-list.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CrossLangSettings {
    /// Extra names to treat as legitimate Python (never flagged), beyond the built-in allow-list.
    pub allow: Vec<String>,
}

/// Controls `metrics --badges` output. Defaults to today's behavior: every per-metric badge,
/// no combined summary.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BadgeSettings {
    /// Per-metric badge slugs to emit. `None` (key omitted) = all of them; `Some([])` = none.
    /// Unknown slugs are ignored.
    pub include: Option<Vec<String>>,
    /// Metric slugs to fold into a single combined `sloplint` badge (colored by the worst
    /// tier among them). Empty = no summary badge.
    pub summary: Vec<String>,
}

/// Tunable thresholds for the structural rules. `Copy` so it can ride along in a
/// `FileContext` without lifetimes. Defaults are opinionated but not punishing.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// SLP080: file length (lines) above which a file is flagged.
    pub file_max_lines: usize,
    /// SLP082: maximum nesting depth allowed inside one function.
    pub nesting_max_depth: usize,
    /// SLP084: maximum nesting depth of a single data-structure literal/comprehension.
    pub data_nesting_max_depth: usize,
    /// SLP130: maximum same-subject `if`/`elif` dispatch branches (`== <literal>` or
    /// `isinstance(...)`) before the chain is flagged as a hand-unrolled dispatch ladder; a
    /// longer chain should be a lookup table (dict), `match`, or polymorphism.
    pub dispatch_max_branches: usize,
    /// SLP060: maximum words in an identifier before it's "verbose" (preview).
    pub max_identifier_words: usize,
    /// SLP090: maximum `.py` modules directly in one directory before it's "flat fanout".
    pub dir_max_modules: usize,
    /// SLP120: maximum LCOM4 cohesion components a class may split into before it's a
    /// low-cohesion "god class" (fire when components exceed this).
    pub lcom4_max_components: usize,
    /// SLP120: minimum methods a class must have before LCOM4 is applied — small classes are
    /// too noisy to judge for cohesion.
    pub lcom4_min_methods: usize,
    /// SLP220: fraction of a file's token-bearing lines that look like natural-language prose above
    /// which the file is flagged as pasted LLM explanation (preview). 0.0–1.0.
    pub corrupted_prose_ratio: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_max_lines: 400,
            nesting_max_depth: 4,
            data_nesting_max_depth: 3,
            dispatch_max_branches: 3,
            max_identifier_words: 4,
            dir_max_modules: 15,
            lcom4_max_components: 1,
            lcom4_min_methods: 3,
            corrupted_prose_ratio: 0.5,
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
