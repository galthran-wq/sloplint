//! Lightweight lexical helpers shared by the heuristic comment/docstring rules.
//!
//! These power the "does this prose just restate the code?" check. The approach is
//! deliberately shallow — bag-of-words overlap, no semantics — which keeps it fast and
//! predictable. It catches verbatim restatements and misses synonyms (`increment` vs
//! `+= 1`); that trade favors precision, which is what protects user trust.

use std::collections::HashSet;

/// Common English function words that carry no signal for redundancy. Intentionally does
/// NOT include programming terms (`return`, `value`, ...): those overlapping with code is
/// exactly the redundancy we want to detect.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "to", "of", "in", "is", "it", "this", "that", "for", "on",
    "with", "as", "by", "at", "from", "into", "be", "are", "was", "were", "will", "can", "we",
    "you", "its", "their", "here", "there", "if", "else", "then", "so", "but", "not", "no", "yes",
    "do", "does", "when", "where", "which", "all", "any",
];

fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(&word)
}

/// Extract the set of meaningful content words from `text`: lowercased alphanumeric runs
/// of length >= 2 that aren't stopwords. Splitting on non-alphanumerics also breaks
/// `snake_case` identifiers into their parts (`compute_total` -> `compute`, `total`).
pub fn content_words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 2)
        .map(str::to_ascii_lowercase)
        .filter(|word| !is_stopword(word))
        .collect()
}

/// Fraction of `subset` words that also appear in `universe` (0.0 when `subset` is empty).
/// Used as "what share of the comment's words are already in the code?".
pub fn overlap_ratio(subset: &HashSet<String>, universe: &HashSet<String>) -> f64 {
    if subset.is_empty() {
        return 0.0;
    }
    let hits = subset
        .iter()
        .filter(|word| universe.contains(*word))
        .count();
    hits as f64 / subset.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_snake_case_and_drops_stopwords_and_single_chars() {
        let words = content_words("compute_total of a and b");
        assert!(words.contains("compute"));
        assert!(words.contains("total"));
        assert!(!words.contains("of")); // stopword
        assert!(!words.contains("a")); // single char
    }

    #[test]
    fn overlap_is_share_of_subset_present() {
        let comment = content_words("return the result");
        let code = content_words("return result");
        assert_eq!(overlap_ratio(&comment, &code), 1.0);
        let unrelated = content_words("cache dashboard");
        assert_eq!(overlap_ratio(&unrelated, &code), 0.0);
    }
}
