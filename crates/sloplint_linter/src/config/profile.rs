//! Profiles: the per-path `Profile` model, the built-in defaults, and the `Limits` override patch.

use serde::Deserialize;

use super::model::Limits;

/// A named, path-matched slice of the tree. It carries rule deltas applied over the global
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
    /// Marks a content-detected profile for machine-generated code. In addition to its
    /// `match`/`exclude` globs, a `generated = true` profile *also* claims any file whose header
    /// carries a generated-code marker ([`crate::detect::is_generated`]). The built-in `generated`
    /// profile uses this; a custom profile can set it to extend the same content detection.
    #[serde(default)]
    pub generated: bool,
    /// Code prefixes to additionally ignore for files in this profile (accumulated across all
    /// matching profiles).
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
/// [`crate`]'s test classification), `generated` (content-detected machine-generated code),
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
    pub dispatch_max_branches: Option<usize>,
    pub max_identifier_words: Option<usize>,
    pub dir_max_modules: Option<usize>,
    pub lcom4_max_components: Option<usize>,
    pub lcom4_min_methods: Option<usize>,
    pub corrupted_prose_ratio: Option<f64>,
}

impl LimitsPatch {
    /// Apply the set fields onto `base`, leaving the rest as the global default.
    pub(crate) fn apply(&self, mut base: Limits) -> Limits {
        if let Some(v) = self.file_max_lines {
            base.file_max_lines = v;
        }
        if let Some(v) = self.nesting_max_depth {
            base.nesting_max_depth = v;
        }
        if let Some(v) = self.data_nesting_max_depth {
            base.data_nesting_max_depth = v;
        }
        if let Some(v) = self.dispatch_max_branches {
            base.dispatch_max_branches = v;
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
