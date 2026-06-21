//! Corpus regression gate.
//!
//! Runs the *shipped* rule set (`lint::all_rules`) over the labeled corpus at the
//! workspace root and computes precision/recall of file-level slop detection. A `slop/`
//! file is a positive (should be flagged); a `clean/` file is a negative (must not be).
//!
//! - **Precision must stay at 1.0**: a shipped rule firing on clean, idiomatic code is a
//!   false positive, which erodes trust faster than a missed finding.
//! - **Recall** is gated softly and raised as rules land, so the mechanism is wired from
//!   day one without overclaiming coverage.

use std::fs;
use std::path::{Path, PathBuf};

use sloplint_linter::lint::{all_rules, check_file, FileContext, Rule};
use sloplint_python::parse;

fn corpus_dir() -> PathBuf {
    // tests/ run with CARGO_MANIFEST_DIR = crates/sloplint_linter; corpus is at the root.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn python_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading corpus dir {}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "py"))
        .collect();
    files.sort();
    files
}

/// Every `.py` under `dir`, recursively (so nested `duplicates/<pair>/` files are covered).
fn python_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "py") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// True if the shipped rule set produces at least one finding for `path`.
fn is_flagged(path: &Path) -> bool {
    let source = fs::read_to_string(path).unwrap();
    let parsed = parse(&source)
        .unwrap_or_else(|e| panic!("corpus file {} must be valid Python: {e}", path.display()));
    let rules = all_rules();
    let refs: Vec<&dyn Rule> = rules.iter().map(|boxed| boxed.as_ref()).collect();
    let ctx = FileContext {
        path: path.to_str().unwrap(),
        source: &source,
        parsed: &parsed,
        limits: Default::default(),
        security_extra: &[],
        placeholders_extra: &[],
    };
    !check_file(&ctx, &refs).is_empty()
}

#[test]
fn corpus_precision_recall() {
    let base = corpus_dir();
    let slop = python_files(&base.join("slop"));
    let clean = python_files(&base.join("clean"));
    assert!(!slop.is_empty(), "expected files under corpus/slop");
    assert!(!clean.is_empty(), "expected files under corpus/clean");

    let true_positives = slop.iter().filter(|p| is_flagged(p)).count();
    let false_negatives = slop.len() - true_positives;
    let false_positives = clean.iter().filter(|p| is_flagged(p)).count();
    let true_negatives = clean.len() - false_positives;

    let precision = if true_positives + false_positives == 0 {
        1.0
    } else {
        true_positives as f64 / (true_positives + false_positives) as f64
    };
    let recall = if true_positives + false_negatives == 0 {
        1.0
    } else {
        true_positives as f64 / (true_positives + false_negatives) as f64
    };

    eprintln!(
        "corpus: slop={} clean={} TP={true_positives} FP={false_positives} \
         FN={false_negatives} TN={true_negatives} precision={precision:.3} recall={recall:.3}",
        slop.len(),
        clean.len(),
    );

    // Hard precision gate: shipped rules must never fire on clean code.
    const MIN_PRECISION: f64 = 1.0;
    assert!(
        precision >= MIN_PRECISION,
        "precision {precision:.3} fell below {MIN_PRECISION}: a shipped rule flagged clean code"
    );

    // Recall gate, raised as rules land. SLP010 (comment ban) + SLP050 (ASCII) now flag
    // the comment-bearing slop files; type-hint/duplication slop awaits later slices.
    const MIN_RECALL: f64 = 0.6;
    assert!(
        recall >= MIN_RECALL,
        "recall {recall:.3} fell below {MIN_RECALL}: shipped rules regressed on slop"
    );
}

/// Guard the whole corpus — including `duplicates/`, which the metrics test doesn't load
/// yet — so a malformed example can't silently rot until the clone slice needs it.
#[test]
fn all_corpus_files_are_valid_python() {
    let files = python_files_recursive(&corpus_dir());
    assert!(
        files.len() >= 7,
        "corpus unexpectedly small: {}",
        files.len()
    );
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        parse(&source)
            .unwrap_or_else(|e| panic!("corpus file {} must be valid Python: {e}", path.display()));
    }
}
