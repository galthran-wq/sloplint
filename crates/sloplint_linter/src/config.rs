//! Configuration model and loading (`sloplint.toml`).
//!
//! Mirrors Ruff's select/ignore model, layered over named **profiles** (#96). A profile is a
//! path-matched slice of the tree — `tests`, `production`, generated code, … — carrying its own
//! rule deltas (ignores, comment allowance, threshold overrides) *and* defining a metrics panel.
//! Defaults are deliberately strict-but-safe: every stable `SLP` rule is enabled, nothing is
//! ignored, preview rules are off. A code is "enabled" for a file when a `select` prefix matches
//! it, no `ignore` prefix matches, and no profile matching that file ignores it. Per-file
//! thresholds resolve the same way: the global [`Limits`] with each matching profile's deltas
//! applied in declaration order (last writer wins).

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
    /// Named, path-matched profiles (#96). Replaces the old per-path overrides: a profile both
    /// carries rule deltas and defines a metrics panel. Omitted in TOML ⇒ the built-in `tests` /
    /// `generated` / `production` trio ([`default_profiles`]); declaring any replaces that set.
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
        }
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

/// A named, path-matched slice of the tree (#96). It carries rule deltas applied over the global
/// config for files it matches, *and* it is the unit `metrics` reports a panel for. Profiles are
/// an ordered list: a file belongs to every profile whose `match`/`exclude` globs select it
/// (overlap is allowed), and overlapping deltas resolve in declaration order. A `default` profile
/// is the complement — it claims files matched by no other profile.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Stable name, e.g. `"tests"` / `"production"`. Used by `metrics --scope <name>` and as the
    /// JSON panel key.
    pub name: String,
    /// Include globs (gitignore-style path globs, e.g. `"tests/**"`). A file is a candidate for
    /// the profile when any matches. Ignored for a `default` profile (it matches by complement).
    #[serde(default)]
    pub r#match: Vec<String>,
    /// Exclude globs carved back out of `match` — the "not" pattern. A file matching any of these
    /// is not in the profile even if an include matched.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Marks the catch-all profile: it claims exactly the files no other profile matched. At most
    /// one profile should set this.
    #[serde(default)]
    pub default: bool,
    /// Marks a content-detected profile for machine-generated code (#115). In addition to its
    /// `match`/`exclude` globs, a `generated = true` profile *also* claims any file whose header
    /// carries a generated-code marker ([`crate::detect::is_generated`]). The built-in `generated`
    /// profile uses this; a custom profile can set it to extend the same content detection.
    #[serde(default)]
    pub generated: bool,
    /// Code prefixes to additionally ignore for files in this profile (accumulate across all
    /// matching profiles, like the old overrides).
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Whether comments are allowed for files in this profile.
    #[serde(default)]
    pub allow_comments: bool,
    /// Threshold overrides for files in this profile, applied as deltas over the global
    /// [`Limits`] (only the keys set here change). Note: SLP020 (clones) and SLP090 (fanout) are
    /// cross-file/directory analyses whose unit spans profiles, so they always use the *global*
    /// thresholds — only per-file rules honor a profile's `limits`.
    #[serde(default)]
    pub limits: LimitsPatch,
}

/// The built-in profiles when none are configured: `tests` (path heuristic mirroring
/// [`crate`]'s test classification), `generated` (content-detected machine-generated code, #115),
/// and `production` (everything else). Reproduces the pre-profiles behavior with zero config, plus
/// the generated split. Declared `tests, generated, production` so that production stays the
/// catch-all complement and both special categories are carved out of it.
pub fn default_profiles() -> Vec<Profile> {
    vec![
        Profile {
            name: "tests".to_string(),
            // Mirror the test classifier: a test_*/*_test/conftest filename, or a `tests`/`test`
            // directory segment at any depth (anchored + `**/`-prefixed forms cover top-level
            // and nested alike).
            r#match: [
                "test_*.py",
                "*_test.py",
                "conftest.py",
                "**/test_*.py",
                "**/*_test.py",
                "**/conftest.py",
                "tests/**",
                "test/**",
                "**/tests/**",
                "**/test/**",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            exclude: Vec::new(),
            default: false,
            generated: false,
            ignore: Vec::new(),
            allow_comments: false,
            limits: LimitsPatch::default(),
        },
        Profile {
            name: "generated".to_string(),
            // Machine-generated code is detected by header marker (see `generated = true` below);
            // these globs add the protobuf/gRPC filename convention so those files classify even
            // through the path-only `check` resolution.
            r#match: [
                "**/*_pb2.py",
                "*_pb2.py",
                "**/*_pb2_grpc.py",
                "*_pb2_grpc.py",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            exclude: Vec::new(),
            default: false,
            generated: true,
            ignore: Vec::new(),
            allow_comments: false,
            limits: LimitsPatch::default(),
        },
        Profile {
            name: "production".to_string(),
            r#match: Vec::new(),
            exclude: Vec::new(),
            default: true,
            generated: false,
            ignore: Vec::new(),
            allow_comments: false,
            limits: LimitsPatch::default(),
        },
    ]
}

/// Per-field overrides for [`Limits`]; `None` leaves the global value untouched. Same keys as
/// `Limits`, all optional.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsPatch {
    pub file_max_lines: Option<usize>,
    pub nesting_max_depth: Option<usize>,
    pub data_nesting_max_depth: Option<usize>,
    pub max_identifier_words: Option<usize>,
    pub dir_max_modules: Option<usize>,
    pub lcom4_max_components: Option<usize>,
    pub lcom4_min_methods: Option<usize>,
    pub corrupted_prose_ratio: Option<f64>,
}

impl LimitsPatch {
    /// Apply the set fields onto `base`, leaving the rest as the global default.
    fn apply(&self, mut base: Limits) -> Limits {
        if let Some(v) = self.file_max_lines {
            base.file_max_lines = v;
        }
        if let Some(v) = self.nesting_max_depth {
            base.nesting_max_depth = v;
        }
        if let Some(v) = self.data_nesting_max_depth {
            base.data_nesting_max_depth = v;
        }
        if let Some(v) = self.max_identifier_words {
            base.max_identifier_words = v;
        }
        if let Some(v) = self.dir_max_modules {
            base.dir_max_modules = v;
        }
        if let Some(v) = self.lcom4_max_components {
            base.lcom4_max_components = v;
        }
        if let Some(v) = self.lcom4_min_methods {
            base.lcom4_min_methods = v;
        }
        if let Some(v) = self.corrupted_prose_ratio {
            base.corrupted_prose_ratio = v;
        }
        base
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

    /// Compile each profile's globs once so per-file resolution is cheap. Fails on an invalid
    /// glob.
    pub fn prepare(&self) -> Result<Selector<'_>, globset::Error> {
        let mut profiles = Vec::with_capacity(self.profiles.len());
        for profile in &self.profiles {
            let include = compile_globs(&profile.r#match)?;
            let exclude = compile_globs(&profile.exclude)?;
            profiles.push(CompiledProfile {
                profile,
                include,
                exclude,
            });
        }
        Ok(Selector {
            config: self,
            profiles,
        })
    }
}

/// Compile a list of path globs into matchers.
fn compile_globs(globs: &[String]) -> Result<Vec<GlobMatcher>, globset::Error> {
    globs
        .iter()
        .map(|g| Ok(Glob::new(g)?.compile_matcher()))
        .collect()
}

/// A profile with its include/exclude globs compiled.
struct CompiledProfile<'a> {
    profile: &'a Profile,
    include: Vec<GlobMatcher>,
    exclude: Vec<GlobMatcher>,
}

impl CompiledProfile<'_> {
    /// Whether this profile's *globs* select `path` (independent of the `default` complement).
    fn glob_matches(&self, path: &str) -> bool {
        self.include.iter().any(|m| m.is_match(path))
            && !self.exclude.iter().any(|m| m.is_match(path))
    }

    /// Whether this profile claims a file at `path` with the given content classification: its
    /// globs match, or it is a `generated` profile and the file was detected as generated (#115).
    /// `is_generated` is `false` for the path-only callers (`check`), so a content-detected
    /// generated file only routes into the `generated` panel where the caller has scanned its
    /// header — the `metrics` command.
    fn matches(&self, path: &str, is_generated: bool) -> bool {
        (self.profile.generated && is_generated) || self.glob_matches(path)
    }
}

/// A config with its profile globs compiled, ready for repeated per-file queries.
pub struct Selector<'a> {
    config: &'a Config,
    profiles: Vec<CompiledProfile<'a>>,
}

impl<'a> Selector<'a> {
    /// Indices (into the profile list, in declaration order) of every profile `path` belongs to,
    /// given whether its content was detected as machine-generated (#115). A file matches each
    /// profile whose globs select it (plus the `generated` profile when `is_generated`); if none
    /// do, it falls to the `default` profile (the complement). Empty only when there is neither a
    /// match nor a default.
    fn matching_indices_with(&self, path: &str, is_generated: bool) -> Vec<usize> {
        let matched: Vec<usize> = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.profile.default && c.matches(path, is_generated))
            .map(|(i, _)| i)
            .collect();
        if !matched.is_empty() {
            return matched;
        }
        self.profiles
            .iter()
            .position(|c| c.profile.default)
            .into_iter()
            .collect()
    }

    /// Path-only classification (no content scan) — the `generated` profile only matches via its
    /// globs here. Used by `check`'s rule resolution.
    fn matching_indices(&self, path: &str) -> Vec<usize> {
        self.matching_indices_with(path, false)
    }

    /// The names of the profiles `path` belongs to, in declaration order (path-only). The
    /// classification used where file content has not been scanned.
    pub fn profiles_for(&self, path: &str) -> Vec<&'a str> {
        self.matching_indices(path)
            .into_iter()
            .map(|i| self.profiles[i].profile.name.as_str())
            .collect()
    }

    /// The names of the profiles a file belongs to, accounting for machine-generated detection
    /// (#115): pass `is_generated` from [`crate::detect::is_generated`] so a generated file routes
    /// into the `generated` profile (and thus out of the `production` complement). Used by
    /// `metrics`, which has the file's content in hand.
    pub fn profiles_for_file(&self, path: &str, is_generated: bool) -> Vec<&'a str> {
        self.matching_indices_with(path, is_generated)
            .into_iter()
            .map(|i| self.profiles[i].profile.name.as_str())
            .collect()
    }

    /// Every configured profile name, in declaration order.
    pub fn profile_names(&self) -> Vec<&'a str> {
        self.profiles
            .iter()
            .map(|c| c.profile.name.as_str())
            .collect()
    }

    /// The name of the `default` (catch-all) profile, if one is configured.
    pub fn default_profile(&self) -> Option<&'a str> {
        self.profiles
            .iter()
            .find(|c| c.profile.default)
            .map(|c| c.profile.name.as_str())
    }

    /// Whether `code` is enabled for the file at `path`: globally selected, not globally ignored,
    /// and not ignored by any profile the file belongs to.
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
        for i in self.matching_indices(path) {
            if self.profiles[i]
                .profile
                .ignore
                .iter()
                .any(|p| code.starts_with(p.as_str()))
            {
                return false;
            }
        }
        true
    }

    /// Whether comments are allowed for the file at `path` (any profile it belongs to opts in).
    pub fn comments_allowed(&self, path: &str) -> bool {
        self.matching_indices(path)
            .into_iter()
            .any(|i| self.profiles[i].profile.allow_comments)
    }

    /// The effective thresholds for the file at `path`: the global [`Limits`] with each matching
    /// profile's deltas applied in declaration order (last writer wins).
    pub fn limits(&self, path: &str) -> Limits {
        let mut limits = self.config.limits;
        for i in self.matching_indices(path) {
            limits = self.profiles[i].profile.limits.apply(limits);
        }
        limits
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
    fn badges_default_to_all_individual_no_summary() {
        let config = Config::default();
        assert!(config.badges.include.is_none()); // None => emit all
        assert!(config.badges.summary.is_empty());
    }

    #[test]
    fn badges_section_parses_include_and_summary() {
        let config = Config::from_toml_str(
            "[badges]\ninclude = []\nsummary = [\"max-cyclomatic\", \"max-cognitive\"]\n",
        )
        .unwrap();
        assert_eq!(config.badges.include, Some(vec![])); // explicit [] => none
        assert_eq!(
            config.badges.summary,
            vec!["max-cyclomatic".to_string(), "max-cognitive".to_string()]
        );
    }

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
    fn profile_ignores_and_allows_comments_by_path() {
        let toml = r#"
[[profiles]]
name = "migrations"
match = ["alembic/**"]
ignore = ["SLP010"]
allow_comments = true

[[profiles]]
name = "production"
default = true
"#;
        let config = Config::from_toml_str(toml).unwrap();
        let selector = config.prepare().unwrap();
        // Disabled under the profile path, enabled elsewhere.
        assert!(!selector.is_enabled("SLP010", "alembic/versions/001_init.py"));
        assert!(selector.is_enabled("SLP010", "src/app.py"));
        // Comment allowance follows the same glob.
        assert!(selector.comments_allowed("alembic/versions/001_init.py"));
        assert!(!selector.comments_allowed("src/app.py"));
        // Classification: the migrations file is in `migrations`; everything else is `production`.
        assert_eq!(
            selector.profiles_for("alembic/versions/001_init.py"),
            ["migrations"]
        );
        assert_eq!(selector.profiles_for("src/app.py"), ["production"]);
    }

    #[test]
    fn default_profiles_classify_tests_vs_production() {
        let selector_cfg = Config::default();
        let selector = selector_cfg.prepare().unwrap();
        for test_path in [
            "test_foo.py",
            "pkg/test_foo.py",
            "pkg/foo_test.py",
            "conftest.py",
            "pkg/tests/thing.py",
            "a/b/test/thing.py",
        ] {
            assert_eq!(
                selector.profiles_for(test_path),
                ["tests"],
                "{test_path} should be a test"
            );
        }
        for prod_path in [
            "foo.py",
            "src/app.py",
            "src/latest/thing.py",
            "src/testing.py",
        ] {
            assert_eq!(
                selector.profiles_for(prod_path),
                ["production"],
                "{prod_path} should be production"
            );
        }
    }

    #[test]
    fn generated_profile_claims_marked_files_out_of_production() {
        let selector_cfg = Config::default();
        let selector = selector_cfg.prepare().unwrap();
        // A generated-detected file routes into `generated` and OUT of the `production` complement.
        assert_eq!(
            selector.profiles_for_file("src/api/core_v1_api.py", true),
            ["generated"]
        );
        // The same path, NOT detected as generated, stays production.
        assert_eq!(
            selector.profiles_for_file("src/api/core_v1_api.py", false),
            ["production"]
        );
        // The protobuf path convention classifies as generated even by the path-only resolution
        // (`is_generated = false`), via the built-in globs.
        assert_eq!(selector.profiles_for("proto/thing_pb2.py"), ["generated"]);
        // A generated file under a tests path is claimed by BOTH (overlap is allowed).
        assert_eq!(
            selector.profiles_for_file("tests/test_thing.py", true),
            ["tests", "generated"]
        );
    }

    #[test]
    fn profile_limits_override_globally_with_last_writer_wins() {
        let toml = r#"
limits = { file_max_lines = 400 }

[[profiles]]
name = "tests"
match = ["tests/**"]
limits = { file_max_lines = 1000, nesting_max_depth = 8 }

[[profiles]]
name = "production"
default = true
"#;
        let config = Config::from_toml_str(toml).unwrap();
        let selector = config.prepare().unwrap();
        // Production keeps the global threshold.
        assert_eq!(selector.limits("src/app.py").file_max_lines, 400);
        // Tests get the profile delta; unset keys (e.g. data_nesting) stay global.
        let test_limits = selector.limits("tests/test_app.py");
        assert_eq!(test_limits.file_max_lines, 1000);
        assert_eq!(test_limits.nesting_max_depth, 8);
        assert_eq!(
            test_limits.data_nesting_max_depth,
            Limits::default().data_nesting_max_depth
        );
    }

    #[test]
    fn exclude_carves_files_back_out_of_a_profile() {
        let toml = r#"
[[profiles]]
name = "src"
match = ["src/**"]
exclude = ["src/legacy/**"]

[[profiles]]
name = "rest"
default = true
"#;
        let config = Config::from_toml_str(toml).unwrap();
        let selector = config.prepare().unwrap();
        assert_eq!(selector.profiles_for("src/app.py"), ["src"]);
        // Excluded path falls through to the default profile.
        assert_eq!(selector.profiles_for("src/legacy/old.py"), ["rest"]);
    }

    #[test]
    fn overlapping_profiles_both_claim_a_file() {
        let toml = r#"
[[profiles]]
name = "api"
match = ["src/api/**"]

[[profiles]]
name = "py"
match = ["**/*.py"]

[[profiles]]
name = "production"
default = true
"#;
        let config = Config::from_toml_str(toml).unwrap();
        let selector = config.prepare().unwrap();
        // A file under src/api matches both `api` and `py` — overlap is allowed, both claim it.
        assert_eq!(selector.profiles_for("src/api/users.py"), ["api", "py"]);
        // The default never applies when a glob profile matched.
        assert_eq!(selector.profiles_for("src/main.py"), ["py"]);
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(Config::from_toml_str("slect = [\"SLP\"]").is_err());
    }
}
