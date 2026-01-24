use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn truncate_text(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let text: String = chars.by_ref().take(limit).collect();

    if chars.next().is_some() {
        format!("{text}...")
    } else {
        text
    }
}

pub fn truncate_display_width(value: &str, limit: usize) -> String {
    if value.width() <= limit {
        return value.to_owned();
    }

    const ELLIPSIS: &str = "...";
    let ellipsis_width = ELLIPSIS.width();
    if limit <= ellipsis_width {
        return ELLIPSIS.chars().take(limit).collect::<String>();
    }

    let text_width = limit.saturating_sub(ellipsis_width);
    let mut width = 0usize;
    let mut text = String::new();
    for grapheme in value.graphemes(true) {
        let grapheme_width = grapheme.width();
        if width.saturating_add(grapheme_width) > text_width {
            break;
        }
        text.push_str(grapheme);
        width = width.saturating_add(grapheme_width);
    }
    text.push_str(ELLIPSIS);
    text
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{truncate_display_width, truncate_text};

    #[test]
    fn truncates_long_text() {
        assert_eq!(truncate_text("abcdef", 3), "abc...");
    }

    #[test]
    fn keeps_short_text() {
        assert_eq!(truncate_text("abc", 10), "abc");
    }

    #[test]
    fn truncates_by_display_width() {
        let text = truncate_display_width("가나다라마바사아자", 8);

        assert_eq!(text, "가나...");
        assert!(text.width() <= 8);
    }
}
