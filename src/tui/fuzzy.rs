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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FuzzyMatchQuality {
    Exact,
    Prefix,
    Fuzzy,
    Context,
}

pub(super) fn fuzzy_name_match_score(
    value: &str,
    query: &str,
) -> Option<(FuzzyMatchQuality, FuzzyScore)> {
    let score = fuzzy_text_score(value, query)?;
    let value = value.to_lowercase();
    let query = query.trim().to_lowercase();
    let quality = if value == query {
        FuzzyMatchQuality::Exact
    } else if value.starts_with(&query) {
        FuzzyMatchQuality::Prefix
    } else {
        FuzzyMatchQuality::Fuzzy
    };
    Some((quality, score))
}

pub(super) fn best_fuzzy_name_match_score(
    values: &[&str],
    query: &str,
) -> Option<(FuzzyMatchQuality, FuzzyScore)> {
    values
        .iter()
        .filter_map(|value| fuzzy_name_match_score(value, query))
        .min_by_key(|(quality, score)| (*quality, *score))
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

    if value_lower.starts_with(&query_lower) {
        return Some(FuzzyScore(FuzzyScore::PREFIX - value_chars.len() as i32));
    }

    // Dynamic programming:
    //
    // dp[q][v] = best score matching query[..=q]
    //            ending exactly at value[v]
    //
    let mut dp = vec![vec![i32::MIN; value_chars.len()]; query_chars.len()];

    for v in 0..value_chars.len() {
        if value_lower[v] != query_lower[0] {
            continue;
        }

        dp[0][v] = character_score(&value_chars, &query_chars, v, 0, true);
    }

    for q in 1..query_chars.len() {
        for v in q..value_chars.len() {
            if value_lower[v] != query_lower[q] {
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

                    score += character_score(&value_chars, &query_chars, v, q, false);

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
    query_index: usize,
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
    if value_chars[index] == query_chars.get(query_index).copied().unwrap_or('\0') {
        score += 5;
    }

    score
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
    use super::{FuzzyMatchQuality, FuzzyScore, fuzzy_name_match_score, fuzzy_text_score};

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
    fn ranking_prefers_tighter_earlier_and_boundary_aligned_matches() {
        for (name, better, worse, query) in [
            ("contiguous over fragmented", "foobar", "foo_bar_baz", "oba"),
            ("tight run over spaced", "abc", "a_b_c", "abc"),
            ("spaced run over wide", "a_b_c", "a___b___c", "abc"),
            (
                "word boundary over interior",
                "foo_bar_test",
                "foobartest",
                "bt",
            ),
            (
                "camel case boundary over flat",
                "FooBarTest",
                "foobartest",
                "bt",
            ),
            (
                "earlier match over later",
                "testingDocument",
                "veryLongTestingDocument",
                "doc",
            ),
            (
                "shorter candidate when similarity ties",
                "foo_bar",
                "foo_bar_baz_qux",
                "fb",
            ),
        ] {
            assert!(
                score(better, query) > score(worse, query),
                "{name}: {better:?} should outrank {worse:?} for {query:?}"
            );
        }
    }

    #[test]
    fn degenerate_queries_return_a_defined_score() {
        for (candidate, query, expected) in [
            ("anything", "", Some(FuzzyScore(0))),
            ("abc", "xyz", None),
            ("short", "muchlonger", None),
        ] {
            assert_eq!(
                fuzzy_text_score(candidate, query),
                expected,
                "{candidate:?} {query:?}"
            );
        }
    }

    /// The ranking tests above only compare candidates against each other, so a
    /// changed weight or operator survives as long as the order holds. These
    /// pin the arithmetic itself: each row exercises one scoring term.
    #[test]
    fn scoring_weights_produce_exact_scores() {
        let cases = [
            ("abc", "abc", 10_000),
            ("foobar", "foo", 8_994),
            ("FooBar", "b", 35),
            ("FooBar", "B", 40),
            ("xab", "ab", 86),
            ("axxxb", "ab", 54),
        ];

        for (value, query, expected) in cases {
            assert_eq!(
                fuzzy_text_score(value, query),
                Some(FuzzyScore(expected)),
                "{value} / {query}"
            );
        }
    }

    #[test]
    fn name_match_quality_separates_exact_prefix_and_fuzzy() {
        let cases = [
            ("abc", "abc", FuzzyMatchQuality::Exact),
            ("abcdef", "abc", FuzzyMatchQuality::Prefix),
            ("axbxc", "abc", FuzzyMatchQuality::Fuzzy),
        ];

        for (value, query, expected) in cases {
            let (quality, _) =
                fuzzy_name_match_score(value, query).expect("value should match query");
            assert_eq!(quality, expected, "{value} / {query}");
        }
    }
}
