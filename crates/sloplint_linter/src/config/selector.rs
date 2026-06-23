//! The query engine: compile a `Config`'s profile globs once, then answer per-file questions.

use globset::{Glob, GlobMatcher};

use super::model::{Config, Limits};
use super::profile::Profile;

impl Config {
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
    /// globs match, or it is a `generated` profile and the file was detected as generated.
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
    /// given whether its content was detected as machine-generated. A file matches each
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

    /// The names of the profiles a file belongs to, accounting for machine-generated detection:
    /// pass `is_generated` from [`crate::detect::is_generated`] so a generated file routes
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
