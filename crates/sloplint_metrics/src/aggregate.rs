//! Project-wide rollup: fold per-file [`FileMetrics`] into a [`RepoMetrics`] with the
//! distribution percentiles and risk histograms.

use crate::model::FileMetrics;
use crate::RepoMetrics;

pub fn aggregate(files: &[FileMetrics]) -> RepoMetrics {
    let mut repo = RepoMetrics {
        files: files.len(),
        ..RepoMetrics::default()
    };
    let mut function_loc_sum = 0usize;
    let mut cyclomatic_sum = 0usize;
    let mut cyclomatic_values: Vec<usize> = Vec::new();
    let mut arity_sum = 0usize;
    let mut arity_values: Vec<usize> = Vec::new();
    let mut cognitive_sum = 0usize;
    let mut cognitive_values: Vec<usize> = Vec::new();
    let mut typed_params_sum = 0usize;
    let mut annotatable_params_sum = 0usize;
    let mut fully_annotated = 0usize;
    let mut wmc_sum = 0usize;
    let mut wmc_values: Vec<usize> = Vec::new();
    let mut dit_sum = 0usize;
    let mut noc_sum = 0usize;
    let mut noc_values: Vec<usize> = Vec::new();
    let mut cbo_sum = 0usize;
    let mut cbo_values: Vec<usize> = Vec::new();
    let mut rfc_sum = 0usize;
    let mut rfc_values: Vec<usize> = Vec::new();
    let mut module_nloc_sum = 0usize;
    let mut module_nloc_values: Vec<usize> = Vec::new();
    // Top-level-code ratio: averaged only over modules that contain executable logic.
    let mut top_level_ratio_sum = 0f64;
    let mut modules_with_logic = 0usize;
    // Docstring coverage: every public def/class (functions *and* classes) is in the
    // denominator, those carrying a docstring in the numerator. The docstring/code ratio is
    // kept strictly function-scoped — function docstring lines over function NCSS — so its two
    // sides share one unit (NCSS exists only for functions). Class docstrings drive coverage,
    // not the ratio.
    let mut public_units = 0usize;
    let mut public_documented = 0usize;
    let mut fn_docstring_lines_sum = 0usize;
    let mut ncss_sum = 0usize;
    for file in files {
        repo.total_loc += file.loc;
        repo.exception.handlers += file.exception.handlers;
        repo.exception.bare += file.exception.bare;
        repo.exception.broad += file.exception.broad;
        repo.exception.swallow += file.exception.swallow;
        module_nloc_sum += file.nloc;
        module_nloc_values.push(file.nloc);
        repo.max_module_nloc = repo.max_module_nloc.max(file.nloc);
        repo.module_size_risk.record_module_size(file.nloc);
        // Top-level-code ratio: only meaningful for modules that contain executable logic.
        let module_logic = file.top_level_code + file.function_code;
        if module_logic > 0 {
            let ratio = file.top_level_code as f64 / module_logic as f64;
            top_level_ratio_sum += ratio;
            modules_with_logic += 1;
            if ratio > repo.max_top_level_ratio {
                repo.max_top_level_ratio = ratio;
            }
            if file.top_level_code >= TOP_LEVEL_MIN_LOGIC && ratio >= TOP_LEVEL_RATIO_THRESHOLD {
                repo.undecomposed_modules += 1;
            }
        }
        for function in &file.functions {
            repo.functions += 1;
            function_loc_sum += function.loc;
            cyclomatic_sum += function.cyclomatic;
            cyclomatic_values.push(function.cyclomatic);
            repo.cyclomatic_risk.record(function.cyclomatic);
            arity_sum += function.arity;
            arity_values.push(function.arity);
            repo.param_count_risk.record_arity(function.arity);
            repo.max_params = repo.max_params.max(function.arity);
            repo.max_function_loc = repo.max_function_loc.max(function.loc);
            // Longest *logic* function: ignore data/config-init blobs (very low cognitive)
            // so the god-function signal isn't crowned by a 2,733-line assignment run.
            if function.cognitive >= LOGIC_FUNCTION_MIN_COGNITIVE {
                repo.max_logic_function_loc = repo.max_logic_function_loc.max(function.loc);
            }
            repo.max_cyclomatic = repo.max_cyclomatic.max(function.cyclomatic);
            repo.max_cognitive = repo.max_cognitive.max(function.cognitive);
            cognitive_sum += function.cognitive;
            cognitive_values.push(function.cognitive);
            repo.cognitive_risk.record_cognitive(function.cognitive);
            repo.max_nesting = repo.max_nesting.max(function.max_nesting);
            typed_params_sum += function.typed_params;
            annotatable_params_sum += function.annotatable_params;
            // Fully annotated = every annotatable param typed *and* a return type. A function with
            // no annotatable params still needs its return annotated to count.
            if function.has_return_annotation
                && function.typed_params == function.annotatable_params
            {
                fully_annotated += 1;
            }
            ncss_sum += function.ncss;
            fn_docstring_lines_sum += function.docstring_lines;
            if is_public(&function.name) {
                public_units += 1;
                public_documented += usize::from(function.has_docstring);
            }
        }
        for class in &file.classes {
            repo.classes += 1;
            wmc_sum += class.wmc;
            wmc_values.push(class.wmc);
            repo.wmc_risk.record_wmc(class.wmc);
            dit_sum += class.dit;
            noc_sum += class.noc;
            noc_values.push(class.noc);
            repo.noc_risk.record_noc(class.noc);
            cbo_sum += class.cbo;
            cbo_values.push(class.cbo);
            repo.cbo_risk.record_cbo(class.cbo);
            rfc_sum += class.rfc;
            rfc_values.push(class.rfc);
            repo.rfc_risk.record_rfc(class.rfc);
            repo.max_wmc = repo.max_wmc.max(class.wmc);
            repo.max_dit = repo.max_dit.max(class.dit);
            repo.max_noc = repo.max_noc.max(class.noc);
            repo.max_cbo = repo.max_cbo.max(class.cbo);
            repo.max_rfc = repo.max_rfc.max(class.rfc);
            if is_public(&class.name) {
                public_units += 1;
                public_documented += usize::from(class.has_docstring);
            }
        }
    }
    repo.avg_function_loc = if repo.functions == 0 {
        0.0
    } else {
        function_loc_sum as f64 / repo.functions as f64
    };
    repo.avg_cyclomatic = if repo.functions == 0 {
        0.0
    } else {
        cyclomatic_sum as f64 / repo.functions as f64
    };
    repo.p95_cyclomatic = percentile(&mut cyclomatic_values, 0.95);
    repo.avg_params = if repo.functions == 0 {
        0.0
    } else {
        arity_sum as f64 / repo.functions as f64
    };
    repo.p95_params = percentile(&mut arity_values, 0.95);
    repo.avg_cognitive = if repo.functions == 0 {
        0.0
    } else {
        cognitive_sum as f64 / repo.functions as f64
    };
    repo.p95_cognitive = percentile(&mut cognitive_values, 0.95);
    let comment_lines: usize = files.iter().map(|f| f.comment_lines).sum();
    repo.comment_density = if repo.total_loc == 0 {
        0.0
    } else {
        comment_lines as f64 / repo.total_loc as f64
    };
    repo.param_annotation_coverage = if annotatable_params_sum == 0 {
        0.0
    } else {
        typed_params_sum as f64 / annotatable_params_sum as f64
    };
    repo.fully_annotated_function_rate = if repo.functions == 0 {
        0.0
    } else {
        fully_annotated as f64 / repo.functions as f64
    };
    repo.avg_wmc = if repo.classes == 0 {
        0.0
    } else {
        wmc_sum as f64 / repo.classes as f64
    };
    repo.p95_wmc = percentile(&mut wmc_values, 0.95);
    repo.avg_module_nloc = if repo.files == 0 {
        0.0
    } else {
        module_nloc_sum as f64 / repo.files as f64
    };
    repo.p95_module_nloc = percentile(&mut module_nloc_values, 0.95);
    repo.avg_top_level_ratio = if modules_with_logic == 0 {
        0.0
    } else {
        top_level_ratio_sum / modules_with_logic as f64
    };
    repo.avg_dit = if repo.classes == 0 {
        0.0
    } else {
        dit_sum as f64 / repo.classes as f64
    };
    repo.avg_noc = if repo.classes == 0 {
        0.0
    } else {
        noc_sum as f64 / repo.classes as f64
    };
    repo.p95_noc = percentile(&mut noc_values, 0.95);
    repo.avg_cbo = if repo.classes == 0 {
        0.0
    } else {
        cbo_sum as f64 / repo.classes as f64
    };
    repo.p95_cbo = percentile(&mut cbo_values, 0.95);
    repo.avg_rfc = if repo.classes == 0 {
        0.0
    } else {
        rfc_sum as f64 / repo.classes as f64
    };
    repo.p95_rfc = percentile(&mut rfc_values, 0.95);
    repo.docstring_coverage = if public_units == 0 {
        0.0
    } else {
        public_documented as f64 / public_units as f64
    };
    repo.docstring_code_ratio = if ncss_sum == 0 {
        0.0
    } else {
        fn_docstring_lines_sum as f64 / ncss_sum as f64
    };
    let handlers = repo.exception.handlers;
    repo.broad_except_rate = if handlers == 0 {
        0.0
    } else {
        repo.exception.broad as f64 / handlers as f64
    };
    repo.swallow_except_rate = if handlers == 0 {
        0.0
    } else {
        repo.exception.swallow as f64 / handlers as f64
    };
    repo
}

/// Nearest-rank percentile of an unsorted slice (sorts it in place). `p` is a fraction in
/// `0.0..=1.0`. Rank = ceil(p * n), clamped to `1..=n`; returns the value at that 1-based
/// rank. Empty input yields 0. Documented explicitly because percentile conventions differ
/// between tools and the reported number must be reproducible.
pub(crate) fn percentile(values: &mut [usize], p: f64) -> usize {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let n = values.len();
    let rank = (p * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    values[rank - 1]
}

/// Whether a def/class name is "public" for docstring coverage — i.e. not `_`-prefixed.
/// Dunder methods (`__init__`, `__repr__`) start with `_`, so they are treated as non-public and
/// excluded from the coverage denominator, matching the convention that documentation effort
/// targets the public API. The test is purely a name-prefix check applied to *every* collected
/// def/class regardless of nesting depth — a function-local helper or a setter is still a unit,
/// matching how the rest of the crate collects functions.
fn is_public(name: &str) -> bool {
    !name.starts_with('_')
}

/// Aggregate per-file metrics into repo-level figures.
/// Minimum cognitive complexity for a function to count toward [`RepoMetrics::max_logic_function_loc`].
/// Excludes straight-line data/config-init blobs (cognitive ≈ 0–1) from the "longest logic
/// function" signal, so a 2,733-line `__init__` of assignments doesn't outrank a real god-function.
pub const LOGIC_FUNCTION_MIN_COGNITIVE: usize = 5;

/// Minimum module-scope logic statements for a module to be considered for the undecomposed flag
/// — small scripts / entry points legitimately have a little top-level code.
pub const TOP_LEVEL_MIN_LOGIC: usize = 15;

/// Top-level-code ratio at/above which a non-trivial module is "undecomposed" — a procedural
/// script-dump. Descriptive, calibrated; never a gate.
pub const TOP_LEVEL_RATIO_THRESHOLD: f64 = 0.6;
