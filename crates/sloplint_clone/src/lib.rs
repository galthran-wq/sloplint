//! Clone-detection engine (the flagship feature).
//!
//! Deterministic, no-LLM near-duplicate function detection. Each function is reduced to a
//! set of normalized k-gram shingles (see [`normalize`]); similarity between two functions
//! is the Jaccard overlap of their shingle sets. To avoid an O(n^2) all-pairs comparison we
//! use MinHash + LSH banding to surface *candidate* pairs cheaply, then confirm each
//! candidate with an exact Jaccard computation. Conservative defaults (high similarity, a
//! minimum function size) keep this near-identical-only, protecting precision.

pub mod normalize;

use std::collections::{HashMap, HashSet};

pub use normalize::{extract_functions, FunctionUnit};

/// Tunable knobs for clone detection. Defaults are conservative (near-identical only).
#[derive(Debug, Clone, Copy)]
pub struct CloneConfig {
    /// Minimum statements (incl. nested) for a function to be considered. Excludes trivial
    /// getters/one-liners that are legitimately similar.
    pub min_statements: usize,
    /// Jaccard similarity at/above which two functions are reported as clones.
    pub similarity: f64,
    /// Shingle size (k-gram length over the normalized symbol stream).
    pub shingle_k: usize,
    /// Number of MinHash hash functions (signature length). Must be divisible by `bands`.
    pub num_hashes: usize,
    /// Number of LSH bands. More bands = more candidates = higher recall, more work.
    pub bands: usize,
    /// Opt-in (preview): also detect Type-3 "gapped" clones — functions with the same
    /// statements reordered, or with a few inserted/removed — via the unordered statement bag.
    /// Off by default so Type-1/2 precision is unaffected. The unordered bag trades precision
    /// for recall: distinct functions built from the same statement *shapes* can read as
    /// similar, so this is a lower-confidence band.
    pub detect_gapped: bool,
    /// Statement-bag Jaccard at/above which two functions are reported as a gapped clone. A
    /// separate, typically lower threshold from `similarity`; only used when `detect_gapped`.
    pub gapped_similarity: f64,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            min_statements: 3,
            similarity: 0.85,
            shingle_k: 4,
            num_hashes: 64,
            bands: 16,
            detect_gapped: false,
            gapped_similarity: 0.8,
        }
    }
}

/// A confirmed clone pair: indices into the `units` slice passed to [`find_clones`], plus
/// their exact Jaccard similarity. `a < b` always.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClonePair {
    pub a: usize,
    pub b: usize,
    pub similarity: f64,
}

/// Find near-duplicate functions among `units`. Returns confirmed pairs, sorted for
/// determinism. Functions below `min_statements`, or with an empty signature, are ignored.
///
/// Always runs the high-precision *ordered* shingle pass (Type-1/2 + shallow Type-3). When
/// `config.detect_gapped` is set, it also runs an *unordered* statement-bag pass that catches
/// reordered / gap-edited (Type-3) clones the ordered pass misses, and unions the results —
/// so with the default config the result is byte-identical to the ordered pass alone.
pub fn find_clones(units: &[FunctionUnit], config: &CloneConfig) -> Vec<ClonePair> {
    let mut pairs = find_pairs(units, config, config.similarity, |unit| &unit.shingles);

    if config.detect_gapped {
        let mut seen: HashSet<(usize, usize)> = pairs.iter().map(|p| (p.a, p.b)).collect();
        // Add only pairs the ordered pass didn't already report; keep the ordered (higher-
        // confidence) similarity when a pair is found by both.
        for pair in find_pairs(units, config, config.gapped_similarity, |unit| {
            &unit.statement_set
        }) {
            if seen.insert((pair.a, pair.b)) {
                pairs.push(pair);
            }
        }
    }

    // Deterministic order: by position, then similarity.
    pairs.sort_by(|p, q| {
        p.a.cmp(&q.a)
            .then(p.b.cmp(&q.b))
            .then(p.similarity.total_cmp(&q.similarity))
    });
    pairs
}

/// MinHash + LSH + exact-Jaccard confirm over the signature selected by `signature`, reporting
/// pairs at/above `threshold`. Eligibility uses `min_statements` and a non-empty signature.
fn find_pairs(
    units: &[FunctionUnit],
    config: &CloneConfig,
    threshold: f64,
    signature: impl Fn(&FunctionUnit) -> &HashSet<u64>,
) -> Vec<ClonePair> {
    let eligible: Vec<usize> = (0..units.len())
        .filter(|&i| {
            units[i].statements >= config.min_statements && !signature(&units[i]).is_empty()
        })
        .collect();

    let signatures: HashMap<usize, Vec<u64>> = eligible
        .iter()
        .map(|&i| (i, min_hash(signature(&units[i]), config.num_hashes)))
        .collect();

    let candidates = lsh_candidates(&eligible, &signatures, config);

    candidates
        .into_iter()
        .filter_map(|(a, b)| {
            let similarity = jaccard(signature(&units[a]), signature(&units[b]));
            (similarity >= threshold).then_some(ClonePair { a, b, similarity })
        })
        .collect()
}

/// MinHash signature: for each hash function, the minimum hashed shingle. The fraction of
/// equal signature positions between two sets estimates their Jaccard similarity.
fn min_hash(shingles: &HashSet<u64>, num_hashes: usize) -> Vec<u64> {
    (0..num_hashes as u64)
        .map(|i| {
            let seed = splitmix64(i);
            shingles
                .iter()
                .map(|&shingle| splitmix64(shingle ^ seed))
                .min()
                .unwrap_or(u64::MAX)
        })
        .collect()
}

/// Group eligible units into LSH buckets by signature band; any two units sharing a bucket
/// in any band are a candidate pair.
fn lsh_candidates(
    eligible: &[usize],
    signatures: &HashMap<usize, Vec<u64>>,
    config: &CloneConfig,
) -> HashSet<(usize, usize)> {
    let rows = (config.num_hashes / config.bands).max(1);
    let mut buckets: HashMap<(usize, u64), Vec<usize>> = HashMap::new();

    for &unit in eligible {
        let signature = &signatures[&unit];
        for band in 0..config.bands {
            let start = band * rows;
            let end = (start + rows).min(signature.len());
            if start >= end {
                break;
            }
            let key = (band, hash_band(&signature[start..end]));
            buckets.entry(key).or_default().push(unit);
        }
    }

    let mut candidates = HashSet::new();
    for members in buckets.values() {
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let (a, b) = (members[i].min(members[j]), members[i].max(members[j]));
                candidates.insert((a, b));
            }
        }
    }
    candidates
}

fn hash_band(rows: &[u64]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &row in rows {
        hash = splitmix64(hash ^ row);
    }
    hash
}

fn jaccard(a: &HashSet<u64>, b: &HashSet<u64>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.iter().filter(|shingle| b.contains(*shingle)).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// SplitMix64 — a fast, well-distributed mixer used both as our hash family seed generator
/// and as the per-shingle hash. Deterministic, so results never depend on run-to-run state.
fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sloplint_python::parse;

    fn units_from(file: &str, source: &str) -> Vec<FunctionUnit> {
        let parsed = parse(source).expect("valid python");
        extract_functions(file, source, &parsed, CloneConfig::default().shingle_k)
    }

    #[test]
    fn detects_renamed_clone() {
        // Same structure, every identifier renamed — a textbook Type-2 clone.
        let source = "\
def total_price(items):
    total = 0
    for item in items:
        total += item.price * item.quantity
    return total

def sum_costs(products):
    acc = 0
    for product in products:
        acc += product.price * product.quantity
    return acc
";
        let units = units_from("a.py", source);
        let pairs = find_clones(&units, &CloneConfig::default());
        assert_eq!(pairs.len(), 1, "the two functions should be one clone pair");
        assert!(pairs[0].similarity >= 0.85);
    }

    #[test]
    fn does_not_pair_unrelated_functions() {
        let source = "\
def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    return [v / total for v in values]

def parse_config(path):
    with open(path) as handle:
        data = handle.read()
    return data.strip().splitlines()
";
        let units = units_from("a.py", source);
        assert!(find_clones(&units, &CloneConfig::default()).is_empty());
    }

    #[test]
    fn ignores_trivial_functions_below_min_statements() {
        // Two identical one-line getters must NOT be flagged — legitimately similar.
        let source = "\
def get_name(self):
    return self.name

def get_age(self):
    return self.age
";
        let units = units_from("a.py", source);
        assert!(find_clones(&units, &CloneConfig::default()).is_empty());
    }

    #[test]
    fn similarity_is_symmetric_and_deterministic() {
        let source = "\
def alpha(data):
    result = []
    for row in data:
        result.append(row * 2)
    return result

def beta(rows):
    output = []
    for entry in rows:
        output.append(entry * 2)
    return output
";
        let units = units_from("a.py", source);
        let first = find_clones(&units, &CloneConfig::default());
        let second = find_clones(&units, &CloneConfig::default());
        assert_eq!(first, second, "results must be deterministic");
        assert_eq!(first.len(), 1);
    }

    fn gapped() -> CloneConfig {
        CloneConfig {
            detect_gapped: true,
            ..CloneConfig::default()
        }
    }

    #[test]
    fn reordered_statements_caught_only_with_gapped() {
        // Same statements, reordered (the `seen`/`total` lines swapped, and swapped again
        // inside the loop) — a Type-3 clone the ordered shingle pass misses.
        let source = "\
def alpha(rows):
    total = 0
    seen = set()
    for row in rows:
        total += row.value
        seen.add(row.id)
    ratio = total / len(seen)
    return ratio

def beta(rows):
    seen = set()
    total = 0
    for row in rows:
        seen.add(row.id)
        total += row.value
    ratio = total / len(seen)
    return ratio
";
        let units = units_from("a.py", source);
        assert!(
            find_clones(&units, &CloneConfig::default()).is_empty(),
            "the ordered pass misses the reordering"
        );
        assert_eq!(
            find_clones(&units, &gapped()).len(),
            1,
            "the statement-bag pass catches it"
        );
    }

    #[test]
    fn inserted_statement_caught_with_gapped() {
        // `padded` is `compute` with one extra statement spliced in — a gapped clone.
        let source = "\
def compute(xs):
    total = 0
    biggest = 0
    for x in xs:
        total += x
        biggest = max(biggest, x)
    return total, biggest

def padded(xs):
    total = 0
    biggest = 0
    for x in xs:
        total += x
        log(x)
        biggest = max(biggest, x)
    return total, biggest
";
        let units = units_from("a.py", source);
        assert_eq!(find_clones(&units, &gapped()).len(), 1);
    }

    #[test]
    fn gapped_does_not_pair_unrelated_functions() {
        // Precision: enabling gapped detection must not invent clones among unrelated code.
        let source = "\
def normalize(values):
    total = sum(values)
    if total == 0:
        return values
    return [v / total for v in values]

def parse_config(path):
    with open(path) as handle:
        data = handle.read()
    return data.strip().splitlines()
";
        let units = units_from("a.py", source);
        assert!(find_clones(&units, &gapped()).is_empty());
    }
}
