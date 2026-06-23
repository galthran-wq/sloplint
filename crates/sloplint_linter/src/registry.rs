//! The rule registry: the catalog of known rules and selection by config.
//!
//! Like Ruff, rules carry a stability group — new rules land in [`RuleGroup::Preview`] and
//! are enabled only with `preview = true`, then graduate to [`RuleGroup::Stable`] once
//! proven. A rule is instantiated for a file only when it is both in scope (stable, or
//! preview-enabled) and selected by the per-path config.

use crate::config::Selector;
use crate::lint::Rule;

/// Stability of a rule. Stable rules run by default; preview rules require opting in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleGroup {
    Stable,
    Preview,
}

/// A rule known to the registry: its code, stability, and a constructor.
pub struct RegisteredRule {
    pub code: &'static str,
    pub group: RuleGroup,
    make: fn() -> Box<dyn Rule>,
}

impl RegisteredRule {
    pub const fn new(code: &'static str, group: RuleGroup, make: fn() -> Box<dyn Rule>) -> Self {
        Self { code, group, make }
    }

    /// Instantiate a fresh instance of this rule.
    pub fn build(&self) -> Box<dyn Rule> {
        (self.make)()
    }
}

/// A catalog of rules. Construct via [`Registry::shipped`] for the real set, or
/// [`Registry::new`] in tests.
pub struct Registry {
    rules: Vec<RegisteredRule>,
}

impl Registry {
    pub fn new(rules: Vec<RegisteredRule>) -> Self {
        Self { rules }
    }

    /// All rules that ship with sloplint, aggregated from every category.
    pub fn shipped() -> Self {
        Self::new(shipped_rules())
    }

    /// The codes of every registered rule (regardless of selection).
    pub fn codes(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rules.iter().map(|rule| rule.code)
    }

    /// Instantiate the rules enabled for `path` under `selector`: in scope (stable, or
    /// preview when enabled) and not deselected by config / path overrides.
    pub fn enabled_for(&self, selector: &Selector, path: &str) -> Vec<Box<dyn Rule>> {
        self.rules
            .iter()
            .filter(|rule| rule.group == RuleGroup::Stable || selector.preview())
            .filter(|rule| selector.is_enabled(rule.code, path))
            .map(|rule| rule.build())
            .collect()
    }
}

/// The shipped rule catalog. Each rule slice contributes its category's rules here.
fn shipped_rules() -> Vec<RegisteredRule> {
    let mut rules = crate::rules::comments::rules();
    rules.extend(crate::rules::structure::rules());
    rules.extend(crate::rules::cohesion::rules());
    rules.extend(crate::rules::security::rules());
    rules.extend(crate::rules::placeholders::rules());
    rules.extend(crate::rules::crosslang::rules());
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::lint::{check_file, FileContext};
    use sloplint_diagnostics::{Diagnostic, Severity};
    use sloplint_python::{parse, TextRange, TextSize};

    /// Test rule that always emits one finding — exercises register → select → emit.
    struct AlwaysFlag;

    impl Rule for AlwaysFlag {
        fn code(&self) -> &'static str {
            "SLP999"
        }
        fn check(&self, _ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
            diagnostics.push(Diagnostic::new(
                "SLP999",
                "always flags",
                TextRange::empty(TextSize::from(0)),
                Severity::Warning,
            ));
        }
    }

    fn registry() -> Registry {
        Registry::new(vec![
            RegisteredRule::new("SLP999", RuleGroup::Stable, || Box::new(AlwaysFlag)),
            RegisteredRule::new("SLP998", RuleGroup::Preview, || Box::new(AlwaysFlag)),
        ])
    }

    /// The code a rule is registered under must equal the code it reports. Config selection and
    /// inline suppression key off the *registered* code, so a mismatch would target a code the
    /// rule never emits — a silent drift. Each rule now derives its diagnostic code from
    /// `code()`, so this single check guards the whole registered -> reported chain.
    #[test]
    fn shipped_rule_registration_matches_reported_code() {
        for rule in &Registry::shipped().rules {
            assert_eq!(
                rule.code,
                rule.build().code(),
                "rule registered as {} reports a different code",
                rule.code,
            );
        }
    }

    /// Codes are stable public identifiers (config, suppressions); two rules sharing one would be
    /// ambiguous.
    #[test]
    fn shipped_rule_codes_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for code in Registry::shipped().codes() {
            assert!(seen.insert(code), "duplicate rule code: {code}");
        }
    }

    #[test]
    fn stable_rule_registers_selects_and_emits() {
        let config = Config::default();
        let selector = config.prepare().unwrap();
        let rules = registry().enabled_for(&selector, "src/app.py");
        // Only the stable rule by default (preview off).
        assert_eq!(rules.len(), 1);

        let source = "x = 1\n";
        let parsed = parse(source).unwrap();
        let ctx = FileContext {
            path: "src/app.py",
            source,
            parsed: &parsed,
            limits: Default::default(),
            security_extra: &[],
            placeholders_extra: &[],
            comment_phrases_extra: &[],
            crosslang_allow: &[],
        };
        let refs: Vec<&dyn Rule> = rules.iter().map(|b| b.as_ref()).collect();
        let diagnostics = check_file(&ctx, &refs);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "SLP999");
    }

    #[test]
    fn preview_rule_requires_preview_flag() {
        let config = Config::from_toml_str("preview = true").unwrap();
        let selector = config.prepare().unwrap();
        assert_eq!(registry().enabled_for(&selector, "a.py").len(), 2);
    }

    #[test]
    fn config_ignore_disables_a_rule() {
        let config = Config::from_toml_str("ignore = [\"SLP999\"]").unwrap();
        let selector = config.prepare().unwrap();
        assert_eq!(registry().enabled_for(&selector, "a.py").len(), 0);
    }

    #[test]
    fn profile_disables_for_matching_files_only() {
        let config = Config::from_toml_str(
            "[[profiles]]\nname = \"tests\"\nmatch = [\"tests/**\"]\nignore = [\"SLP999\"]\n\
             [[profiles]]\nname = \"production\"\ndefault = true\n",
        )
        .unwrap();
        let selector = config.prepare().unwrap();
        assert_eq!(
            registry().enabled_for(&selector, "tests/test_app.py").len(),
            0
        );
        assert_eq!(registry().enabled_for(&selector, "src/app.py").len(), 1);
    }
}
