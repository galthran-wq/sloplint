//! Output formatters for diagnostics: JSON, SARIF (for GitHub code scanning), and a
//! GitHub-flavored markdown summary. Plain-text rendering lives in
//! [`sloplint_diagnostics::render`]; everything here shares its line/column mapping so all
//! formats agree on positions.

use serde_json::{json, Value};
use sloplint_diagnostics::render::line_col;
use sloplint_diagnostics::{Diagnostic, Severity};

/// One file's diagnostics together with the source needed to resolve line/column.
pub struct ReportEntry<'a> {
    pub path: &'a str,
    pub source: &'a str,
    pub diagnostics: &'a [Diagnostic],
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// Position of a diagnostic as (line, column), both 1-based.
fn position(entry: &ReportEntry, diagnostic: &Diagnostic) -> (usize, usize) {
    line_col(entry.source, u32::from(diagnostic.range.start()) as usize)
}

/// Machine-readable JSON: a flat array of findings.
pub fn to_json(entries: &[ReportEntry]) -> String {
    let findings: Vec<Value> = entries
        .iter()
        .flat_map(|entry| {
            entry.diagnostics.iter().map(move |d| {
                let (line, column) = position(entry, d);
                json!({
                    "path": entry.path,
                    "code": d.code,
                    "message": d.message,
                    "severity": severity_str(d.severity),
                    "line": line,
                    "column": column,
                })
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "findings": findings })).unwrap()
}

/// SARIF 2.1.0 — uploadable to GitHub code scanning for inline PR annotations.
pub fn to_sarif(entries: &[ReportEntry]) -> String {
    let results: Vec<Value> = entries
        .iter()
        .flat_map(|entry| {
            entry.diagnostics.iter().map(move |d| {
                let (line, column) = position(entry, d);
                json!({
                    "ruleId": d.code,
                    "level": severity_str(d.severity),
                    "message": { "text": d.message },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": { "uri": entry.path },
                            "region": { "startLine": line, "startColumn": column }
                        }
                    }]
                })
            })
        })
        .collect();

    let sarif = json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": { "driver": { "name": "sloplint", "informationUri": "https://github.com/galthran-wq/sloplint", "rules": [] } },
            "results": results
        }]
    });
    serde_json::to_string_pretty(&sarif).unwrap()
}

/// A GitHub-flavored markdown summary: a count-by-code table plus the total. Suitable for a
/// PR comment.
pub fn to_github_markdown(entries: &[ReportEntry]) -> String {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in entries {
        for diagnostic in entry.diagnostics {
            *counts.entry(diagnostic.code.as_str()).or_default() += 1;
        }
    }
    let total: usize = counts.values().sum();

    let mut out = String::from("### sloplint\n\n");
    if total == 0 {
        out.push_str("No slop found. \u{2705}\n");
        return out;
    }
    out.push_str(&format!(
        "Found **{total}** issue(s):\n\n| Rule | Count |\n| --- | ---: |\n"
    ));
    for (code, count) in &counts {
        out.push_str(&format!("| `{code}` | {count} |\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::{TextRange, TextSize};

    fn diag(code: &str, start: u32) -> Diagnostic {
        Diagnostic::new(
            code,
            "msg",
            TextRange::new(TextSize::from(start), TextSize::from(start + 1)),
            Severity::Warning,
        )
    }

    #[test]
    fn json_has_position_and_code() {
        let source = "a = 1\nb = 2\n";
        let diags = [diag("SLP010", 6)];
        let entries = [ReportEntry {
            path: "a.py",
            source,
            diagnostics: &diags,
        }];
        let value: Value = serde_json::from_str(&to_json(&entries)).unwrap();
        let finding = &value["findings"][0];
        assert_eq!(finding["code"], "SLP010");
        assert_eq!(finding["line"], 2);
        assert_eq!(finding["column"], 1);
    }

    #[test]
    fn sarif_is_valid_json_with_results() {
        let source = "x = 1\n";
        let diags = [diag("SLP050", 0)];
        let entries = [ReportEntry {
            path: "x.py",
            source,
            diagnostics: &diags,
        }];
        let value: Value = serde_json::from_str(&to_sarif(&entries)).unwrap();
        assert_eq!(value["version"], "2.1.0");
        assert_eq!(value["runs"][0]["results"][0]["ruleId"], "SLP050");
    }

    #[test]
    fn markdown_summarizes_counts() {
        let source = "x = 1\n";
        let diags = [diag("SLP010", 0), diag("SLP010", 2), diag("SLP050", 4)];
        let entries = [ReportEntry {
            path: "x.py",
            source,
            diagnostics: &diags,
        }];
        let md = to_github_markdown(&entries);
        assert!(md.contains("Found **3** issue(s)"));
        assert!(md.contains("| `SLP010` | 2 |"));
    }
}
