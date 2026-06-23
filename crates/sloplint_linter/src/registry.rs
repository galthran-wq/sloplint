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
    /// Register a rule under `group`. The code is taken from the rule itself (`code()`), so it
    /// has a single source of truth — there is no separate code argument to drift from it.
    pub fn new(group: RuleGroup, make: fn() -> Box<dyn Rule>) -> Self {
        let code = make().code();
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
        Self::new(crate::codes::shipped_rules())
    }

    /// The codes of every registered rule (regardless of selection).
    pub fn codes(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rules.iter().map(|rule| rule.code)
    }

    /// The registered rules, for the `rule` explainer (code, group, and a constructor that yields
    /// the rule's `ViolationMetadata`).
    pub fn rules(&self) -> &[RegisteredRule] {
        &self.rules
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::lint::{check_file, FileContext};
    use sloplint_diagnostics::{Diagnostic, Severity};
    use sloplint_macros::ViolationMetadata;
    use sloplint_python::{parse, TextRange, TextSize};

    /// Test rules that always emit one finding — exercise register → select → emit. Two distinct
    /// codes so the stable/preview fixtures are independent (the registry derives each code from
    /// the rule, so the two fixtures must be different types to register under different codes).
    #[derive(ViolationMetadata)]
    struct AlwaysFlag;
    #[derive(ViolationMetadata)]
    struct AlwaysFlagPreview;

    impl Rule for AlwaysFlag {
        fn code(&self) -> &'static str {
            "SLP999"
        }
        fn check_source(&self, _ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
            diagnostics.push(Diagnostic::new(
                self.code(),
                "always flags",
                TextRange::empty(TextSize::from(0)),
                Severity::Warning,
            ));
        }
    }

    impl Rule for AlwaysFlagPreview {
        fn code(&self) -> &'static str {
            "SLP998"
        }
        fn check_source(&self, _ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
            diagnostics.push(Diagnostic::new(
                self.code(),
                "always flags",
                TextRange::empty(TextSize::from(0)),
                Severity::Warning,
            ));
        }
    }

    fn registry() -> Registry {
        Registry::new(vec![
            RegisteredRule::new(RuleGroup::Stable, || Box::new(AlwaysFlag)),
            RegisteredRule::new(RuleGroup::Preview, || Box::new(AlwaysFlagPreview)),
        ])
    }

    /// Every shipped code is a well-formed `SLP` identifier (`SLP` + three digits). Codes are
    /// stable public identifiers (config, suppressions), so a typo'd or off-format code would be
    /// a silent contract break.
    #[test]
    fn shipped_rule_codes_are_well_formed() {
        for code in Registry::shipped().codes() {
            let digits = code.strip_prefix("SLP");
            assert!(
                digits.is_some_and(|d| d.len() == 3 && d.bytes().all(|b| b.is_ascii_digit())),
                "malformed rule code: {code}"
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

    /// Every shipped rule carries a `## What it does` doc-comment — now machine-readable via the
    /// `ViolationMetadata` derive — and a non-empty rule name, so the rule explainer can describe
    /// each one. Guards against a new rule landing without ruff-style docs.
    #[test]
    fn every_shipped_rule_is_documented() {
        for rule in &Registry::shipped().rules {
            let built = rule.build();
            assert!(
                !built.rule_name().is_empty(),
                "{} has an empty rule_name",
                rule.code
            );
            assert!(
                built.explanation().is_some(),
                "{} ({}) is missing its `## What it does` doc-comment",
                rule.code,
                built.rule_name()
            );
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
