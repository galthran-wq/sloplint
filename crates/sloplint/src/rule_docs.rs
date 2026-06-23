//! The `sloplint rule` explainer — mirrors ruff's `ruff rule`.
//!
//! With no argument it lists the per-file rules (code, kebab-case name, stability); with a code
//! it prints that rule's `## What it does` / `## Why is this bad?` documentation. Both are sourced
//! from the rule's `ViolationMetadata` (derived from its doc-comment), so there is a single
//! source of truth for a rule's prose.

use std::fmt::Write;

use anyhow::anyhow;
use sloplint_linter::registry::{Registry, RuleGroup};

/// Print one rule's docs (`code` given) or the full rule list (`code` is `None`).
pub fn run_rule(code: Option<&str>) -> anyhow::Result<()> {
    let registry = Registry::shipped();
    match code {
        None => print!("{}", rule_list(&registry)),
        Some(code) => println!("{}", rule_detail(&registry, code)?),
    }
    Ok(())
}

/// One line per registered rule — `CODE  kebab-name  (stability)` — sorted by code.
fn rule_list(registry: &Registry) -> String {
    let mut rows: Vec<(&'static str, String, &'static str)> = registry
        .rules()
        .iter()
        .map(|rule| {
            let name = to_kebab(rule.build().rule_name());
            (rule.code, name, group_label(rule.group))
        })
        .collect();
    rows.sort_by_key(|(code, _, _)| *code);

    let mut out = String::new();
    for (code, name, group) in rows {
        writeln!(out, "{code}  {name}  ({group})").unwrap();
    }
    // Whole-tree rules (SLP020 clones, SLP090 fanout, SLP180 imports, SLP220 corrupted, SLP240
    // ghost) run during `check` but aren't per-file registry rules, so they don't appear above yet.
    out.push_str("\nWhole-tree rules run during `check` are not listed here yet.\n");
    out
}

/// A rule's code/name/stability header followed by its `## What it does` documentation.
fn rule_detail(registry: &Registry, code: &str) -> anyhow::Result<String> {
    let want = code.to_ascii_uppercase();
    let rule = registry
        .rules()
        .iter()
        .find(|rule| rule.code == want)
        .ok_or_else(|| anyhow!("unknown rule `{code}` (run `sloplint rule` to list all rules)"))?;

    let built = rule.build();
    let mut out = format!(
        "{} ({}) [{}]\n",
        rule.code,
        to_kebab(built.rule_name()),
        group_label(rule.group)
    );
    if let Some(explanation) = built.explanation() {
        out.push('\n');
        out.push_str(explanation);
    }
    Ok(out)
}

fn group_label(group: RuleGroup) -> &'static str {
    match group {
        RuleGroup::Stable => "stable",
        RuleGroup::Preview => "preview",
    }
}

/// `PascalCase` → `kebab-case` (`RedundantComment` → `redundant-comment`), matching how ruff
/// displays rule names.
fn to_kebab(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_cases_rule_names() {
        assert_eq!(to_kebab("RedundantComment"), "redundant-comment");
        assert_eq!(to_kebab("GodClass"), "god-class");
        assert_eq!(to_kebab("AsciiOnly"), "ascii-only");
    }

    #[test]
    fn list_includes_a_known_rule_with_stability() {
        let out = rule_list(&Registry::shipped());
        // SLP030 (defensive-except) is a stable per-file rule.
        assert!(out.contains("SLP030"), "list missing SLP030:\n{out}");
        assert!(
            out.contains("defensive-except"),
            "list missing the kebab name:\n{out}"
        );
        assert!(out.contains("(stable)"), "list missing stability:\n{out}");
    }

    #[test]
    fn detail_renders_the_what_it_does_doc() {
        let out = rule_detail(&Registry::shipped(), "SLP030").unwrap();
        assert!(
            out.starts_with("SLP030 (defensive-except) [stable]"),
            "header:\n{out}"
        );
        assert!(
            out.contains("## What it does"),
            "missing doc heading:\n{out}"
        );
        assert!(
            out.contains("## Why is this bad?"),
            "missing rationale heading:\n{out}"
        );
    }

    #[test]
    fn detail_is_case_insensitive() {
        assert!(rule_detail(&Registry::shipped(), "slp030").is_ok());
    }

    #[test]
    fn detail_errors_on_unknown_code() {
        let err = rule_detail(&Registry::shipped(), "SLP999").unwrap_err();
        assert!(err.to_string().contains("unknown rule"), "{err}");
    }

    #[test]
    fn every_shipped_rule_has_a_renderable_detail() {
        let registry = Registry::shipped();
        for rule in registry.rules() {
            let out = rule_detail(&registry, rule.code).unwrap();
            assert!(
                out.contains("## What it does"),
                "{} lacks docs:\n{out}",
                rule.code
            );
        }
    }
}
