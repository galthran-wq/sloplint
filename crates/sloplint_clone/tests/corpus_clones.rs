//! Corpus-level clone detection: the engine must find the known near-duplicate pair under
//! `corpus/duplicates/` and must NOT invent clones among the idiomatic `corpus/clean/`
//! functions.

use std::fs;
use std::path::{Path, PathBuf};

use sloplint_clone::{extract_functions, find_clones, CloneConfig, FunctionUnit};
use sloplint_python::parse;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn collect_py(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_py(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "py") {
            files.push(path);
        }
    }
}

fn units_under(dir: &Path, config: &CloneConfig) -> Vec<FunctionUnit> {
    let mut files = Vec::new();
    collect_py(dir, &mut files);
    files.sort();
    let mut units = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        let parsed = parse(&source).unwrap();
        let label = path.to_string_lossy().to_string();
        units.extend(extract_functions(
            &label,
            &source,
            &parsed,
            config.shingle_k,
            config.canonicalize_commutative,
        ));
    }
    units
}

#[test]
fn finds_the_known_duplicate_pair() {
    let config = CloneConfig::default();
    let units = units_under(&corpus().join("duplicates/price_sum"), &config);
    assert_eq!(
        units.len(),
        2,
        "expected two functions in the duplicate pair"
    );
    let pairs = find_clones(&units, &config);
    assert_eq!(pairs.len(), 1, "the renamed duplicate should be detected");
    assert!(pairs[0].similarity >= config.similarity);
}

#[test]
fn does_not_invent_clones_in_clean_code() {
    let config = CloneConfig::default();
    let units = units_under(&corpus().join("clean"), &config);
    assert!(
        find_clones(&units, &config).is_empty(),
        "idiomatic clean code must not be flagged as containing clones"
    );
}
