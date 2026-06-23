//! SLP060: verbose mechanical naming (preview).

use std::collections::HashSet;

use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_python::{Ranged, TokenKind};

use crate::lint::{FileContext, Rule};
use sloplint_macros::ViolationMetadata;

/// ## What it does
/// Flags identifiers with more words than the configured limit (splitting on `_` and camelCase
/// humps), reporting each distinct name once — `calculate_total_value_for_user`.
///
/// ## Why is this bad?
/// AI-generated code rarely writes `i` or `buf`; over-long mechanical names hurt readability.
/// Preview — naming taste is subjective and this is a heuristic.
#[derive(ViolationMetadata)]
pub struct VerboseNaming;

impl Rule for VerboseNaming {
    fn code(&self) -> &'static str {
        "SLP060"
    }

    fn check(&self, ctx: &FileContext, diagnostics: &mut Vec<Diagnostic>) {
        let max_words = ctx.limits.max_identifier_words;
        let mut seen: HashSet<&str> = HashSet::new();
        for token in ctx.parsed.tokens().iter() {
            if token.kind() != TokenKind::Name {
                continue;
            }
            let text = &ctx.source[token.range()];
            if seen.contains(text) {
                continue;
            }
            let words = word_count(text);
            if words > max_words {
                seen.insert(text);
                diagnostics.push(Diagnostic::new(
                    self.code(),
                    format!("identifier `{text}` is verbose ({words} words)"),
                    token.range(),
                    Severity::Warning,
                ));
            }
        }
    }
}

/// Number of words in an identifier: split on `_`, then count camelCase humps within each
/// part. `calculate_total_value` -> 3, `getUserById` -> 3, `id` -> 1.
fn word_count(identifier: &str) -> usize {
    identifier
        .split('_')
        .filter(|part| !part.is_empty())
        .map(camel_humps)
        .sum::<usize>()
        .max(1)
}

fn camel_humps(part: &str) -> usize {
    let mut humps = 1;
    let mut prev_lower = false;
    for ch in part.chars() {
        if ch.is_uppercase() && prev_lower {
            humps += 1;
        }
        prev_lower = ch.is_lowercase();
    }
    humps
}
