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

pub fn render_user_mentions<F>(value: &str, mut resolve_name: F) -> String
where
    F: FnMut(u64) -> Option<String>,
{
    if !value.contains("<@") {
        return value.to_owned();
    }

    let mut rendered = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find("<@") {
        let start = cursor.saturating_add(relative_start);
        rendered.push_str(&value[cursor..start]);

        let Some((end, user_id)) = parse_user_mention(value, start) else {
            rendered.push_str("<@");
            cursor = start.saturating_add(2);
            continue;
        };

        match resolve_name(user_id) {
            Some(name) => {
                rendered.push('@');
                rendered.push_str(&name);
            }
            None => rendered.push_str(&value[start..end]),
        }
        cursor = end;
    }
    rendered.push_str(&value[cursor..]);
    rendered
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedText {
    pub text: String,
    pub highlights: Vec<TextHighlight>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextHighlight {
    pub start: usize,
    pub end: usize,
    pub kind: TextHighlightKind,
}

/// Style class for a mention highlight. The renderer maps each kind to a
/// distinct background colour so the user can tell at a glance whether they
/// were the target or just a witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextHighlightKind {
    /// The current user is being notified (`<@me>`, `@everyone`, `@here`).
    SelfMention,
    /// Some other user is being mentioned. Subdued background; informational.
    OtherMention,
}

pub fn render_user_mentions_with_highlights<F, H>(
    value: &str,
    mut resolve_name: F,
    mut highlight_kind: H,
) -> RenderedText
where
    F: FnMut(u64) -> Option<String>,
    H: FnMut(u64) -> Option<TextHighlightKind>,
{
    if !value.contains("<@") {
        return RenderedText {
            text: value.to_owned(),
            highlights: Vec::new(),
        };
    }

    let mut rendered = String::with_capacity(value.len());
    let mut highlights = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find("<@") {
        let start = cursor.saturating_add(relative_start);
        rendered.push_str(&value[cursor..start]);

        let Some((end, user_id)) = parse_user_mention(value, start) else {
            rendered.push_str("<@");
            cursor = start.saturating_add(2);
            continue;
        };

        match resolve_name(user_id) {
            Some(name) => {
                let highlight_start = rendered.len();
                rendered.push('@');
                rendered.push_str(&name);
                let highlight_end = rendered.len();
                if let Some(kind) = highlight_kind(user_id) {
                    highlights.push(TextHighlight {
                        start: highlight_start,
                        end: highlight_end,
                        kind,
                    });
                }
            }
            None => rendered.push_str(&value[start..end]),
        }
        cursor = end;
    }
    rendered.push_str(&value[cursor..]);

    RenderedText {
        text: rendered,
        highlights,
    }
}

fn parse_user_mention(value: &str, start: usize) -> Option<(usize, u64)> {
    let bytes = value.as_bytes();
    if bytes.get(start..start.saturating_add(2)) != Some(b"<@") {
        return None;
    }

    let mut index = start.saturating_add(2);
    if bytes.get(index) == Some(&b'!') {
        index = index.saturating_add(1);
    }

    let digits_start = index;
    while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
        index = index.saturating_add(1);
    }
    if index == digits_start || bytes.get(index) != Some(&b'>') {
        return None;
    }

    let user_id = value[digits_start..index].parse().ok()?;
    if user_id == 0 {
        return None;
    }
    Some((index.saturating_add(1), user_id))
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{render_user_mentions, truncate_display_width, truncate_text};

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
        let text = truncate_display_width("漢字仮名交じり", 8);

        assert_eq!(text, "漢字...");
        assert!(text.width() <= 8);
    }

    #[test]
    fn renders_known_user_mentions() {
        let text = render_user_mentions("hello <@10>", |user_id| {
            (user_id == 10).then(|| "alice".to_owned())
        });

        assert_eq!(text, "hello @alice");
    }

    #[test]
    fn renders_deprecated_nickname_mentions_like_user_mentions() {
        let text = render_user_mentions("hello <@!10>", |user_id| {
            (user_id == 10).then(|| "alice".to_owned())
        });

        assert_eq!(text, "hello @alice");
    }

    #[test]
    fn keeps_unknown_user_mentions_raw() {
        let text = render_user_mentions("hello <@10>", |_| None);

        assert_eq!(text, "hello <@10>");
    }

    #[test]
    fn keeps_zero_user_mentions_raw() {
        let text = render_user_mentions("hello <@0>", |user_id| {
            (user_id == 0).then(|| "nobody".to_owned())
        });

        assert_eq!(text, "hello <@0>");
    }

    #[test]
    fn keeps_role_mentions_raw() {
        let text = render_user_mentions("hello <@&10>", |user_id| {
            (user_id == 10).then(|| "role".to_owned())
        });

        assert_eq!(text, "hello <@&10>");
    }

    #[test]
    fn keeps_overflowing_user_mentions_raw() {
        let text = render_user_mentions("hello <@18446744073709551616>", |_| {
            Some("overflow".to_owned())
        });

        assert_eq!(text, "hello <@18446744073709551616>");
    }

    #[test]
    fn renders_user_mentions_next_to_unicode() {
        let text = render_user_mentions("café<@10>!", |user_id| {
            (user_id == 10).then(|| "alice".to_owned())
        });

        assert_eq!(text, "café@alice!");
    }

    #[test]
    fn keeps_malformed_user_mentions_raw() {
        let text = render_user_mentions("hello <@abc> <@10", |user_id| {
            (user_id == 10).then(|| "alice".to_owned())
        });

        assert_eq!(text, "hello <@abc> <@10");
    }
}
