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

const CUSTOM_EMOJI_CDN_BASE: &str = "https://cdn.discordapp.com/emojis";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedText {
    pub text: String,
    pub highlights: Vec<TextHighlight>,
    pub emoji_slots: Vec<InlineEmojiSlot>,
}

/// `byte_start..byte_start+byte_len` holds the `:name:` textual fallback;
/// the renderer overwrites it with spaces and blits the image only once the
/// cache has a protocol for `url`. `display_width` equals `byte_len` because
/// Discord emoji names are ASCII-only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineEmojiSlot {
    pub byte_start: usize,
    pub byte_len: usize,
    pub display_width: u16,
    pub url: String,
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
            emoji_slots: Vec::new(),
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
        emoji_slots: Vec::new(),
    }
}

/// String-only fallback used by thread/channel previews where no image
/// overlay is possible; replaces `<:name:id>` and `<a:name:id>` with
/// `:name:`. The body pipeline uses
/// [`replace_custom_emoji_markup_in_rendered`].
pub fn replace_custom_emoji_markup(value: &str) -> String {
    if !value.contains('<') {
        return value.to_owned();
    }

    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find('<') {
        let start = cursor.saturating_add(relative_start);
        output.push_str(&value[cursor..start]);

        match parse_custom_emoji(value, start) {
            Some((end, name)) => {
                output.push(':');
                output.push_str(name);
                output.push(':');
                cursor = end;
            }
            None => {
                output.push('<');
                cursor = start.saturating_add(1);
            }
        }
    }
    output.push_str(&value[cursor..]);
    output
}

/// Image-overlay variant of [`replace_custom_emoji_markup`]: rewrites each
/// match to its `:name:` fallback and records a slot the renderer can blit
/// the image over. Mention highlights are remapped through the byte-shift.
pub fn replace_custom_emoji_markup_in_rendered(rendered: RenderedText) -> RenderedText {
    let matches = scan_custom_emoji_matches(&rendered.text);
    if matches.is_empty() {
        return rendered;
    }

    let RenderedText {
        text,
        highlights,
        mut emoji_slots,
    } = rendered;

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for emoji in &matches {
        output.push_str(&text[cursor..emoji.input_start]);
        let slot_byte_start = output.len();
        output.push(':');
        output.push_str(&emoji.name);
        output.push(':');
        let slot_byte_len = output.len() - slot_byte_start;
        let extension = if emoji.animated { "gif" } else { "png" };
        emoji_slots.push(InlineEmojiSlot {
            byte_start: slot_byte_start,
            byte_len: slot_byte_len,
            display_width: u16::try_from(slot_byte_len).unwrap_or(u16::MAX),
            url: format!("{CUSTOM_EMOJI_CDN_BASE}/{}.{extension}", emoji.id),
        });
        cursor = emoji.input_end;
    }
    output.push_str(&text[cursor..]);

    let new_highlights = highlights
        .into_iter()
        .map(|highlight| TextHighlight {
            start: remap_offset(&matches, highlight.start),
            end: remap_offset(&matches, highlight.end),
            kind: highlight.kind,
        })
        .collect();

    RenderedText {
        text: output,
        highlights: new_highlights,
        emoji_slots,
    }
}

struct CustomEmojiMatch {
    input_start: usize,
    input_end: usize,
    name: String,
    id: String,
    animated: bool,
}

impl CustomEmojiMatch {
    fn input_len(&self) -> usize {
        self.input_end - self.input_start
    }

    /// Bytes the textual fallback (`:name:`) consumes in the rewritten string.
    fn output_len(&self) -> usize {
        self.name.len() + 2
    }
}

fn scan_custom_emoji_matches(text: &str) -> Vec<CustomEmojiMatch> {
    if !text.contains('<') {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find('<') {
        let start = cursor.saturating_add(rel);
        match parse_custom_emoji_full(text, start) {
            Some((end, name, id, animated)) => {
                matches.push(CustomEmojiMatch {
                    input_start: start,
                    input_end: end,
                    name: name.to_owned(),
                    id: id.to_owned(),
                    animated,
                });
                cursor = end;
            }
            None => cursor = start.saturating_add(1),
        }
    }
    matches
}

fn remap_offset(matches: &[CustomEmojiMatch], pos: usize) -> usize {
    let mut delta: isize = 0;
    for emoji in matches {
        if emoji.input_end <= pos {
            delta += emoji.output_len() as isize - emoji.input_len() as isize;
        } else {
            break;
        }
    }
    let new = pos as isize + delta;
    new.max(0) as usize
}

fn parse_custom_emoji_full(value: &str, start: usize) -> Option<(usize, &str, &str, bool)> {
    let bytes = value.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }

    let mut index = start.saturating_add(1);
    let animated = bytes.get(index) == Some(&b'a');
    if animated {
        index = index.saturating_add(1);
    }
    if bytes.get(index) != Some(&b':') {
        return None;
    }
    index = index.saturating_add(1);

    let name_start = index;
    while let Some(byte) = bytes.get(index) {
        if *byte == b':' {
            break;
        }
        if !(byte.is_ascii_alphanumeric() || *byte == b'_') {
            return None;
        }
        index = index.saturating_add(1);
    }
    if index == name_start || bytes.get(index) != Some(&b':') {
        return None;
    }
    let name_end = index;
    index = index.saturating_add(1);

    let id_start = index;
    while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
        index = index.saturating_add(1);
    }
    if index == id_start || bytes.get(index) != Some(&b'>') {
        return None;
    }

    Some((
        index.saturating_add(1),
        &value[name_start..name_end],
        &value[id_start..index],
        animated,
    ))
}

fn parse_custom_emoji(value: &str, start: usize) -> Option<(usize, &str)> {
    let (end, name, _id, _animated) = parse_custom_emoji_full(value, start)?;
    Some((end, name))
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

    use super::{
        InlineEmojiSlot, RenderedText, TextHighlight, TextHighlightKind, render_user_mentions,
        replace_custom_emoji_markup, replace_custom_emoji_markup_in_rendered,
        truncate_display_width, truncate_text,
    };

    #[test]
    fn rendered_replacer_emits_text_fallback_and_records_slot() {
        let rendered = RenderedText {
            text: "hi <:emoji_48:1146289325491892225>!".to_owned(),
            highlights: Vec::new(),
            emoji_slots: Vec::new(),
        };
        let out = replace_custom_emoji_markup_in_rendered(rendered);
        assert_eq!(out.text, "hi :emoji_48:!");
        assert_eq!(out.emoji_slots.len(), 1);
        let slot = &out.emoji_slots[0];
        assert_eq!(slot.byte_start, "hi ".len());
        assert_eq!(slot.byte_len, ":emoji_48:".len());
        assert_eq!(slot.display_width, ":emoji_48:".len() as u16);
        assert_eq!(
            slot.url,
            "https://cdn.discordapp.com/emojis/1146289325491892225.png"
        );
    }

    #[test]
    fn rendered_replacer_uses_gif_for_animated() {
        let rendered = RenderedText {
            text: "<a:wave:42>".to_owned(),
            ..Default::default()
        };
        let out = replace_custom_emoji_markup_in_rendered(rendered);
        assert_eq!(out.text, ":wave:");
        assert_eq!(
            out.emoji_slots[0].url,
            "https://cdn.discordapp.com/emojis/42.gif"
        );
    }

    #[test]
    fn rendered_replacer_remaps_highlights_after_replacement() {
        let text = "<:e:1>@alice and bob".to_owned();
        let highlight_start = "<:e:1>".len();
        let highlight_end = highlight_start + "@alice".len();
        let rendered = RenderedText {
            text,
            highlights: vec![TextHighlight {
                start: highlight_start,
                end: highlight_end,
                kind: TextHighlightKind::OtherMention,
            }],
            emoji_slots: Vec::new(),
        };
        let out = replace_custom_emoji_markup_in_rendered(rendered);
        assert_eq!(out.text, ":e:@alice and bob");
        assert_eq!(out.highlights.len(), 1);
        let h = out.highlights[0];
        assert_eq!(&out.text[h.start..h.end], "@alice");
        assert_eq!(out.emoji_slots[0].byte_start, 0);
    }

    #[test]
    fn rendered_replacer_handles_multiple_emojis_in_one_string() {
        let rendered = RenderedText {
            text: "a<:x:1>b<:y:2>c".to_owned(),
            ..Default::default()
        };
        let out = replace_custom_emoji_markup_in_rendered(rendered);
        assert_eq!(out.text, "a:x:b:y:c");
        assert_eq!(out.emoji_slots.len(), 2);
        assert_eq!(out.emoji_slots[0].byte_start, "a".len());
        assert_eq!(out.emoji_slots[1].byte_start, "a:x:b".len());
    }

    #[test]
    fn rendered_replacer_is_a_noop_without_emoji_markup() {
        let original = RenderedText {
            text: "no emojis here".to_owned(),
            highlights: vec![TextHighlight {
                start: 0,
                end: 2,
                kind: TextHighlightKind::SelfMention,
            }],
            emoji_slots: vec![InlineEmojiSlot {
                byte_start: 5,
                byte_len: 4,
                display_width: 4,
                url: "preexisting".to_owned(),
            }],
        };
        let out = replace_custom_emoji_markup_in_rendered(original.clone());
        assert_eq!(out, original);
    }

    #[test]
    fn replaces_custom_emoji_markup_with_shorthand() {
        let text = replace_custom_emoji_markup("hi <:emoji_48:1146289325491892225>!");
        assert_eq!(text, "hi :emoji_48:!");
    }

    #[test]
    fn replaces_animated_custom_emoji_markup() {
        let text = replace_custom_emoji_markup("<a:partying_face:42> woo");
        assert_eq!(text, ":partying_face: woo");
    }

    #[test]
    fn keeps_text_without_emoji_markup_unchanged() {
        let text = replace_custom_emoji_markup("hello world");
        assert_eq!(text, "hello world");
    }

    #[test]
    fn ignores_malformed_custom_emoji_markup() {
        let text = replace_custom_emoji_markup("<:no_id:> <:bad-name:1> <@10> <:ok:7>");
        assert_eq!(text, "<:no_id:> <:bad-name:1> <@10> :ok:");
    }

    #[test]
    fn preserves_unicode_around_custom_emoji_markup() {
        let text = replace_custom_emoji_markup("héllo<:emoji_48:1146289325491892225>!");
        assert_eq!(text, "héllo:emoji_48:!");
    }

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
    fn renders_deprecated_nickname_mentions_like_user_mentions() {
        let text = render_user_mentions("hello <@!10>", |user_id| {
            (user_id == 10).then(|| "alice".to_owned())
        });

        assert_eq!(text, "hello @alice");
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
