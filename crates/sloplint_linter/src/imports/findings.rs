//! Turn scanned imports + declared deps into undeclared-import findings.

use std::collections::HashSet;

use super::dist::{distribution_aliases, is_always_available, normalize_dist};
use super::{Declared, Finding, ImportRef};

/// Pure core: produce SLP180 findings for the given files' imports against the resolved
/// declared dependencies and first-party names. `is_stdlib` is injected so this is testable
/// without the bundled stdlib table. `extra` is an additional set of (already normalized)
/// declared distribution names from config.
pub fn findings(
    files: &[(String, Vec<ImportRef>)],
    first_party: &HashSet<String>,
    declared: &Declared,
    extra: &HashSet<String>,
    is_stdlib: impl Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (path, imports) in files {
        for import in imports {
            if is_stdlib(&import.top)
                || is_always_available(&import.top)
                || first_party.contains(&import.top)
            {
                continue;
            }
            let aliases = distribution_aliases(&import.top);
            let mut candidates: Vec<String> = vec![normalize_dist(&import.top)];
            candidates.extend(aliases.iter().map(|a| normalize_dist(a)));
            let declared_here = candidates
                .iter()
                .any(|c| declared.dists.contains(c) || extra.contains(c));
            if declared_here {
                continue;
            }
            let named = match aliases.first() {
                Some(dist) => format!("`{}` (distribution `{dist}`)", import.top),
                None => format!("`{}`", import.top),
            };
            let message = format!(
                "{named} is imported but not declared in the project dependencies ({})",
                declared.source
            );
            out.push(Finding {
                path: path.clone(),
                range: import.range,
                message,
            });
        }
    }
    out
}
