//! Whole-tree (cross-file) rule drivers: rules that need every file at once — SLP020 clones,
//! SLP090 directory fanout, SLP180 undeclared imports, SLP240 ghost scaffolding. Each runs after
//! the per-file pass and attributes its findings back onto the affected `FileResult`s.

use std::collections::HashMap;
use std::env;

use sloplint_clone::{CloneConfig, FunctionUnit};
use sloplint_diagnostics::{Diagnostic, Severity};
use sloplint_linter::config::Selector;
use sloplint_linter::registry::WholeProjectRule;
use sloplint_linter::{clones, fanout, imports, stdlib};
use sloplint_python::TextRange;

use crate::{first_party_under, FileResult};
use sloplint_linter::ghost;

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
    // SLP020's logic lives in the linter (`clones`); here we feed it each file's source (for the
    // partner-line render) and attach each finding. Units are collected only for SLP020-enabled
    // files (in `scan_files`), so no per-path gating is needed here.
    let found = {
        let sources: Vec<&str> = results
            .iter()
            .map(|result| result.source.as_str())
            .collect();
        clones::findings(units, unit_result, &sources, clone_config)
    };
    for finding in found {
        results[finding.file].diagnostics.push(Diagnostic::new(
            clones::Clones.code(),
            finding.message,
            finding.range,
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
        if !selector.is_enabled(fanout::Fanout.code(), &finding.path) {
            continue;
        }
        if let Some(result) = results
            .iter_mut()
            .find(|result| result.path == finding.path)
        {
            result.diagnostics.push(Diagnostic::new(
                fanout::Fanout.code(),
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
