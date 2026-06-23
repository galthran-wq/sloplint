//! The `sloplint rule` explainer — mirrors ruff's `ruff rule`.
//!
//! With no argument it lists the per-file rules (code, kebab-case name, stability); with a code
//! it prints that rule's `## What it does` / `## Why is this bad?` documentation. Both are sourced
//! from the rule's `ViolationMetadata` (derived from its doc-comment), so there is a single
//! source of truth for a rule's prose.

use std::fmt::Write;

use anyhow::anyhow;
use sloplint_linter::registry::{Registry, RuleDoc, RuleGroup};

/// Print one rule's docs (`code` given) or the full rule list (`code` is `None`), as human text
/// or JSON (`as_json`). The JSON form mirrors `ruff rule --output-format json` — machine-readable
/// rule metadata for tooling / docs generation.
pub fn run_rule(code: Option<&str>, as_json: bool) -> anyhow::Result<()> {
    let registry = Registry::shipped();
    match (code, as_json) {
        (None, false) => print!("{}", rule_list(&registry)),
        (Some(code), false) => println!("{}", rule_detail(&registry, code)?),
        (None, true) => println!("{}", rule_list_json(&registry)),
        (Some(code), true) => println!("{}", rule_detail_json(&registry, code)?),
    }
    Ok(())
}

/// One rule's machine-readable metadata: code, kebab name, preview flag, and the rendered
/// `## What it does` explanation (or null).
fn rule_json(entry: &RuleDoc) -> serde_json::Value {
    serde_json::json!({
        "code": entry.code,
        "name": to_kebab(entry.name),
        "preview": entry.group == RuleGroup::Preview,
        "explanation": entry.explanation,
    })
}

/// Every rule as a JSON array, sorted by code.
fn rule_list_json(registry: &Registry) -> String {
    let mut catalog = registry.catalog();
    catalog.sort_by_key(|entry| entry.code);
    let array: Vec<serde_json::Value> = catalog.iter().map(rule_json).collect();
    serde_json::to_string_pretty(&array).expect("rule metadata serializes")
}

/// One rule as a JSON object; errors on an unknown code.
fn rule_detail_json(registry: &Registry, code: &str) -> anyhow::Result<String> {
    let want = code.to_ascii_uppercase();
    let catalog = registry.catalog();
    let entry = catalog
        .iter()
        .find(|entry| entry.code == want)
        .ok_or_else(|| anyhow!("unknown rule `{code}` (run `sloplint rule` to list all rules)"))?;
    Ok(serde_json::to_string_pretty(&rule_json(entry)).expect("rule metadata serializes"))
}

/// One line per registered rule — `CODE  kebab-name  (stability)` — sorted by code.
fn rule_list(registry: &Registry) -> String {
    let mut rows: Vec<(&'static str, String, &'static str)> = registry
        .catalog()
        .iter()
        .map(|entry| (entry.code, to_kebab(entry.name), group_label(entry.group)))
        .collect();
    rows.sort_by_key(|(code, _, _)| *code);

    let mut out = String::new();
    for (code, name, group) in rows {
        writeln!(out, "{code}  {name}  ({group})").unwrap();
    }
    // The remaining whole-tree rules (SLP180 imports, SLP220 corrupted, SLP240 ghost) run during
    // `check` but aren't yet in the catalog, so they don't appear above.
    out.push_str("\nWhole-tree rules SLP180, SLP220 and SLP240 run during `check` are not listed here yet.\n");
    out
}

/// A rule's code/name/stability header followed by its `## What it does` documentation.
fn rule_detail(registry: &Registry, code: &str) -> anyhow::Result<String> {
    let want = code.to_ascii_uppercase();
    let catalog = registry.catalog();
    let entry = catalog
        .iter()
        .find(|entry| entry.code == want)
        .ok_or_else(|| anyhow!("unknown rule `{code}` (run `sloplint rule` to list all rules)"))?;

    let mut out = format!(
        "{} ({}) [{}]\n",
        entry.code,
        to_kebab(entry.name),
        group_label(entry.group)
    );
    if let Some(explanation) = entry.explanation {
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
    fn json_list_is_valid_and_includes_a_known_rule() {
        let out = rule_list_json(&Registry::shipped());
        let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = value.as_array().expect("array");
        assert!(arr.iter().any(|r| r["code"] == "SLP030"
            && r["name"] == "defensive-except"
            && r["preview"] == false
            && r["explanation"]
                .as_str()
                .is_some_and(|e| e.contains("## What it does"))));
    }

    #[test]
    fn json_detail_is_a_single_object() {
        let out = rule_detail_json(&Registry::shipped(), "slp030").unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(value["code"], "SLP030");
        assert_eq!(value["preview"], false);
        assert!(value["explanation"]
            .as_str()
            .unwrap()
            .contains("## Why is this bad?"));
    }

    #[test]
    fn json_detail_errors_on_unknown_code() {
        assert!(rule_detail_json(&Registry::shipped(), "SLP999").is_err());
    }

    #[test]
    fn detail_errors_on_unknown_code() {
        let err = rule_detail(&Registry::shipped(), "SLP999").unwrap_err();
        assert!(err.to_string().contains("unknown rule"), "{err}");
    }

    #[test]
    fn every_shipped_rule_has_a_renderable_detail() {
        let registry = Registry::shipped();
        for entry in registry.catalog() {
            let out = rule_detail(&registry, entry.code).unwrap();
            assert!(
                out.contains("## What it does"),
                "{} lacks docs:\n{out}",
                entry.code
            );
        }
    }
}
