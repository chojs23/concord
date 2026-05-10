pub(super) fn fuzzy_text_score(value: &str, query: &str) -> Option<usize> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Some(0);
    }

    let haystack = value.to_lowercase();
    if haystack == needle {
        return Some(0);
    }
    if haystack.starts_with(&needle) {
        return Some(
            10 + haystack
                .chars()
                .count()
                .saturating_sub(needle.chars().count()),
        );
    }
    if let Some(byte_index) = haystack.find(&needle) {
        return Some(100 + byte_index);
    }

    let needle_chars: Vec<char> = needle.chars().collect();
    let haystack_chars: Vec<char> = haystack.chars().collect();

    if let Some(score) = subsequence_score(&haystack_chars, &needle_chars, 1000) {
        return Some(score);
    }

    typo_tolerant_subsequence_score(&haystack_chars, &needle_chars)
}

fn subsequence_score(
    haystack_chars: &[char],
    needle_chars: &[char],
    score_base: usize,
) -> Option<usize> {
    let mut positions = Vec::with_capacity(needle_chars.len());
    let mut needle_index = 0usize;
    for (haystack_index, haystack_char) in haystack_chars.iter().enumerate() {
        if needle_chars.get(needle_index) == Some(haystack_char) {
            positions.push(haystack_index);
            needle_index += 1;
            if needle_index == needle_chars.len() {
                break;
            }
        }
    }
    if positions.len() != needle_chars.len() {
        return None;
    }

    let start = positions.first().copied().unwrap_or(0);
    let end = positions.last().copied().unwrap_or(start);
    let span = end.saturating_sub(start).saturating_add(1);
    let gaps = span.saturating_sub(needle_chars.len());
    Some(score_base + span * 10 + gaps + start)
}

fn typo_tolerant_subsequence_score(
    haystack_chars: &[char],
    needle_chars: &[char],
) -> Option<usize> {
    if needle_chars.len() < 3 {
        return None;
    }

    let mut best = None;
    for removed_index in 0..needle_chars.len() {
        let candidate = needle_chars
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (index != removed_index).then_some(*value))
            .collect::<Vec<_>>();
        best = better_score(best, subsequence_score(haystack_chars, &candidate, 2100));
    }

    for swapped_index in 0..needle_chars.len().saturating_sub(1) {
        let mut candidate = needle_chars.to_vec();
        candidate.swap(swapped_index, swapped_index + 1);
        best = better_score(best, subsequence_score(haystack_chars, &candidate, 2200));
    }

    best
}

fn better_score(current: Option<usize>, candidate: Option<usize>) -> Option<usize> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::fuzzy_text_score;

    #[test]
    fn fuzzy_text_score_matches_subsequences() {
        assert!(fuzzy_text_score("general", "gnrl").is_some());
        assert_eq!(fuzzy_text_score("general", "xyz"), None);
    }

    #[test]
    fn fuzzy_text_score_prefers_exact_prefix_and_contiguous_matches() {
        let exact = fuzzy_text_score("general", "general").expect("exact match");
        let prefix = fuzzy_text_score("general", "gen").expect("prefix match");
        let contiguous = fuzzy_text_score("neo-general", "gen").expect("contiguous match");
        let spread = fuzzy_text_score("g-e-n", "gen").expect("spread match");
        let typo = fuzzy_text_score("party", "praty").expect("typo match");

        assert!(exact < prefix);
        assert!(prefix < contiguous);
        assert!(contiguous < spread);
        assert!(spread < typo);
    }

    #[test]
    fn fuzzy_text_score_tolerates_one_query_typo() {
        assert!(fuzzy_text_score("general", "gereral").is_some());
        assert!(fuzzy_text_score("general", "gexneral").is_some());
        assert!(fuzzy_text_score("party", "praty").is_some());
    }

    #[test]
    fn fuzzy_text_score_rejects_multiple_unmatched_typos() {
        assert_eq!(fuzzy_text_score("general", "zzgeneral"), None);
    }
}
