//! Whole-tree (cross-file) rule drivers: rules that need every file at once — SLP020 clones,
//! SLP090 directory fanout, SLP180 undeclared imports, SLP240 ghost scaffolding. Each runs after
//! the per-file pass and attributes its findings back onto the affected `FileResult`s.

use std::collections::HashMap;
use std::env;

use sloplint_clone::{find_clones, CloneConfig, FunctionUnit};
use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_linter::config::Selector;
use sloplint_linter::{fanout, imports, stdlib};
use sloplint_python::TextRange;

use crate::ghost;
use crate::{first_party_under, line_of, FileResult};

/// Run cross-file clone detection and push exactly one `SLP020` diagnostic onto each
/// clone-involved function, pointing at its lowest-index duplicate.
///
/// Reporting every pair would be O(n^2) on a cluster of N identical functions (N(N-1)
/// diagnostics), burying real signal. Collapsing to one finding per function keeps output
/// proportional to the number of duplicated functions while still flagging each of them.
pub(crate) fn attribute_clones(
    units: &[FunctionUnit],
    unit_result: &[usize],
    clone_config: &CloneConfig,
    results: &mut [FileResult],
) {
    // For each clone-involved function, keep its lowest-index partner (deterministic).
    let mut partner: HashMap<usize, (usize, f64)> = HashMap::new();
    let mut record = |from: usize, to: usize, similarity: f64| {
        partner
            .entry(from)
            .and_modify(|best| {
                if to < best.0 {
                    *best = (to, similarity);
                }
            })
            .or_insert((to, similarity));
    };
    for pair in find_clones(units, clone_config) {
        record(pair.a, pair.b, pair.similarity);
        record(pair.b, pair.a, pair.similarity);
    }

    let mut involved: Vec<usize> = partner.keys().copied().collect();
    involved.sort_unstable();
    for unit_index in involved {
        let (partner_index, similarity) = partner[&unit_index];
        let unit = &units[unit_index];
        let partner_unit = &units[partner_index];
        let result_index = unit_result[unit_index];
        let partner_result = unit_result[partner_index];
        let percent = (similarity * 100.0).round() as u32;

        let partner_line = line_of(
            &results[partner_result].source,
            partner_unit.range.start().into(),
        );
        let partner_path = results[partner_result].path.clone();

        results[result_index].diagnostics.push(Diagnostic::new(
            "SLP020",
            format!(
                "duplicate of {partner_path}:{partner_line} (function `{}`, {percent}% similar)",
                partner_unit.name
            ),
            unit.range,
            Severity::Warning,
        ));
    }
}

/// Flag directories holding more than `max_modules` Python files directly (flat fanout —
/// SLP090). One diagnostic per over-full directory, attributed to its first file.
pub(crate) fn attribute_fanout(
    results: &mut [FileResult],
    selector: &Selector,
    max_modules: usize,
) {
    // SLP090's logic lives in the linter (`fanout`); here we just feed it the discovered paths,
    // honor per-path selection, and attach each finding to its directory's representative file.
    let paths: Vec<String> = results.iter().map(|result| result.path.clone()).collect();
    for finding in fanout::findings(&paths, max_modules) {
        if !selector.is_enabled("SLP090", &finding.path) {
            continue;
        }
        if let Some(result) = results
            .iter_mut()
            .find(|result| result.path == finding.path)
        {
            result.diagnostics.push(Diagnostic::new(
                "SLP090",
                finding.message,
                TextRange::default(),
                Severity::Warning,
            ));
        }
    }
}

/// SLP180: flag third-party imports not declared in the project's dependency manifest.
///
/// Whole-project, so emission (not collection) is gated per-path: imports are collected for
/// every file (above) so the first-party set is complete, and a per-path `ignore` only
/// suppresses the *finding*. Resolves the manifest once from the working directory; if none
/// declares dependencies, the rule stays silent (conservative — false negatives over false
/// positives).
pub(crate) fn attribute_undeclared_imports(
    import_scans: &[(String, Vec<imports::ImportRef>)],
    all_paths: &[String],
    extra: &[String],
    selector: &Selector,
    results: &mut [FileResult],
) {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => return,
    };
    let Some(declared) = imports::resolve_declared(&cwd) else {
        return; // no manifest declaring deps -> ambiguous, don't fire.
    };
    // First-party names must reflect the whole project tree, not just the scanned paths —
    // otherwise `sloplint check one_file.py` (or a pre-commit run over changed files only)
    // would flag local packages outside the scan as undeclared. Walk the manifest's project
    // root for that, then union the scanned paths (cheap, and covers files above the root).
    let mut first_party = first_party_under(&declared.root);
    first_party.extend(imports::first_party_names(all_paths));
    let extra_set: std::collections::HashSet<String> =
        extra.iter().map(|e| imports::normalize_dist(e)).collect();

    let by_path: HashMap<String, usize> = results
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.clone(), i))
        .collect();

    let findings = imports::findings(
        import_scans,
        &first_party,
        &declared,
        &extra_set,
        stdlib::is_stdlib,
    );
    for finding in findings {
        if !selector.is_enabled("SLP180", &finding.path) {
            continue;
        }
        if let Some(&index) = by_path.get(finding.path.as_str()) {
            results[index].diagnostics.push(Diagnostic::new(
                "SLP180",
                finding.message,
                finding.range,
                Severity::Warning,
            ));
        }
    }
}

/// SLP240: flag ghost scaffolding (unreferenced top-level defs + ghost config flags) across the
/// project. Whole-project like SLP180: scans are collected for every file (so references are
/// complete), then emission is gated per-path via `is_enabled`.
pub(crate) fn attribute_ghost_scaffolding(
    scans: &[ghost::FileScan],
    selector: &Selector,
    results: &mut [FileResult],
) {
    let by_path: HashMap<String, usize> = results
        .iter()
        .enumerate()
        .map(|(i, r)| (r.path.clone(), i))
        .collect();
    for finding in ghost::findings(scans) {
        if !selector.is_enabled("SLP240", &finding.path) {
            continue;
        }
        if let Some(&index) = by_path.get(finding.path.as_str()) {
            results[index].diagnostics.push(Diagnostic::new(
                "SLP240",
                finding.message,
                finding.range,
                Severity::Warning,
            ));
        }
    }
}
