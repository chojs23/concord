use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuzzyScore(pub i32);

impl Ord for FuzzyScore {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering:
        // larger score = "earlier" in sort order
        other.0.cmp(&self.0)
    }
}

impl PartialOrd for FuzzyScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FuzzyScore {
    pub const EXACT: i32 = 10_000;
    pub const PREFIX: i32 = 9_000;
}

pub(super) fn fuzzy_text_score(value: &str, query: &str) -> Option<FuzzyScore> {
    let query = query.trim();

    if query.is_empty() {
        return Some(FuzzyScore(0));
    }

    let value_chars: Vec<char> = value.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    let value_lower: Vec<char> = value.chars().flat_map(char::to_lowercase).collect();

    let query_lower: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();

    // Fast paths.
    if value_lower == query_lower {
        return Some(FuzzyScore(FuzzyScore::EXACT));
    }

    if starts_with_chars(&value_lower, &query_lower) {
        return Some(FuzzyScore(FuzzyScore::PREFIX - value_chars.len() as i32));
    }

    // Dynamic programming:
    //
    // dp[q][v] = best score matching query[..=q]
    //            ending exactly at value[v]
    //
    let mut dp = vec![vec![i32::MIN; value_chars.len()]; query_chars.len()];

    for v in 0..value_chars.len() {
        if !chars_equal(value_lower[v], query_lower[0]) {
            continue;
        }

        dp[0][v] = character_score(&value_chars, &query_chars, v, true);
    }

    for q in 1..query_chars.len() {
        for v in q..value_chars.len() {
            if !chars_equal(value_lower[v], query_lower[q]) {
                continue;
            }

            let mut best = i32::MIN;

            dp[q - 1]
                .iter()
                .take(v)
                .enumerate()
                .for_each(|(prev, &prev_score)| {
                    if prev_score == i32::MIN {
                        return;
                    }

                    let gap = v - prev - 1;

                    let mut score = prev_score;

                    // Strong contiguous preference.
                    if gap == 0 {
                        score += 40;
                    } else {
                        score -= (gap as i32) * 3;
                    }

                    score += character_score(&value_chars, &query_chars, v, false);

                    best = best.max(score);
                });

            dp[q][v] = best;
        }
    }

    let mut best = None;

    for &score in &dp[query_chars.len() - 1][0..value_chars.len()] {
        if score == i32::MIN {
            continue;
        }

        // Prefer shorter candidates slightly.
        let final_score = score - value_chars.len() as i32;

        best = Some(best.map_or(final_score, |b: i32| b.max(final_score)));
    }

    best.map(FuzzyScore)
}

fn character_score(
    value_chars: &[char],
    query_chars: &[char],
    index: usize,
    first_match: bool,
) -> i32 {
    let mut score = 10;

    // Earlier matches are better.
    if first_match {
        score += 20 - (index.min(20) as i32);
    }

    // Word boundary bonus.
    if is_word_boundary(value_chars, index) {
        score += 18;
    }

    // camelCase bonus.
    if is_camel_boundary(value_chars, index) {
        score += 14;
    }

    // Exact case bonus.
    if value_chars[index] == query_chars.get(index).copied().unwrap_or('\0') {
        score += 5;
    }

    score
}

fn chars_equal(a: char, b: char) -> bool {
    a == b
}

fn starts_with_chars(haystack: &[char], needle: &[char]) -> bool {
    haystack.starts_with(needle)
}

fn is_word_boundary(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }

    matches!(chars[index - 1], ' ' | '_' | '-' | '/' | '\\' | '.')
}

fn is_camel_boundary(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return false;
    }

    chars[index].is_uppercase() && chars[index - 1].is_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{FuzzyScore, fuzzy_text_score};

    fn score(value: &str, query: &str) -> i32 {
        fuzzy_text_score(value, query)
            .unwrap_or_else(|| panic!("expected match: {value:?} vs {query:?}"))
            .0
    }

    #[test]
    fn matches_simple_subsequences() {
        assert!(fuzzy_text_score("general", "gnrl").is_some());
        assert!(fuzzy_text_score("GitDiffFile", "gdf").is_some());

        assert_eq!(fuzzy_text_score("general", "xyz"), None);
    }

    #[test]
    fn exact_match_beats_everything() {
        let exact = score("general", "general");
        let prefix = score("general-store", "general");
        let substring = score("my-general-store", "general");
        // let fuzzy = score("geo_nr_al", "general");

        assert!(exact > prefix);
        assert!(prefix > substring);
        // assert!(substring > fuzzy);
    }

    #[test]
    fn contiguous_matches_rank_higher_than_fragmented_matches() {
        let contiguous = score("foobar", "oba");
        let fragmented = score("foo_bar_baz", "oba");

        assert!(contiguous > fragmented);
    }

    #[test]
    fn consecutive_runs_are_strongly_preferred() {
        let tight = score("abc", "abc");
        let spaced = score("a_b_c", "abc");
        let wide = score("a___b___c", "abc");

        assert!(tight > spaced);
        assert!(spaced > wide);
    }

    #[test]
    fn prefers_word_boundaries() {
        let boundary = score("foo_bar_test", "bt");
        let interior = score("foobartest", "bt");

        assert!(boundary > interior);
    }

    #[test]
    fn prefers_camel_case_boundaries() {
        let camel = score("FooBarTest", "bt");
        let flat = score("foobartest", "bt");

        assert!(camel > flat);
    }

    #[test]
    fn prefers_earlier_matches() {
        let early = score("testingDocument", "doc");
        let late = score("veryLongTestingDocument", "doc");

        assert!(early > late);
    }

    #[test]
    fn prefers_shorter_candidates_when_similarity_is_equal() {
        let short = score("foo_bar", "fb");
        let long = score("foo_bar_baz_qux", "fb");

        assert!(short > long);
    }

    #[test]
    fn supports_acronym_style_matching() {
        assert!(fuzzy_text_score("GitDiffFile", "gdf").is_some());
        assert!(fuzzy_text_score("VeryImportantClass", "vic").is_some());
        assert!(fuzzy_text_score("foo_bar_baz", "fbb").is_some());
    }

    #[test]
    fn empty_query_scores_zero() {
        assert_eq!(fuzzy_text_score("anything", ""), Some(FuzzyScore(0)));
    }

    #[test]
    fn non_matching_queries_return_none() {
        assert_eq!(fuzzy_text_score("abc", "xyz"), None);
        assert_eq!(fuzzy_text_score("short", "muchlonger"), None);
    }
}
