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

    let haystack_chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();
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
    Some(1000 + span * 10 + gaps + start)
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

        assert!(exact < prefix);
        assert!(prefix < contiguous);
        assert!(contiguous < spread);
    }
}
