//! Configuration: the data `model`, `profile` defaults, and the per-file `selector` query engine.

mod model;
mod profile;
mod selector;

pub use model::{
    BadgeSettings, CloneSettings, CommentSettings, Config, ConfigError, CrossLangSettings,
    ImportSettings, Limits, PlaceholderSettings, SecuritySettings,
};
pub use profile::{default_profiles, LimitsPatch, Profile};
pub use selector::Selector;

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
