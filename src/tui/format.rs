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

pub fn truncate_display_width_from(value: &str, offset: usize, limit: usize) -> String {
    if offset == 0 {
        return truncate_display_width(value, limit);
    }
    if limit == 0 {
        return String::new();
    }

    let mut skipped_width = 0usize;
    let mut start = value.len();
    for (index, grapheme) in value.grapheme_indices(true) {
        let next_width = skipped_width.saturating_add(grapheme.width());
        if next_width > offset {
            start = index;
            break;
        }
        skipped_width = next_width;
    }

    truncate_display_width(&value[start..], limit)
}

pub fn sanitize_for_display_width(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for grapheme in value.graphemes(true) {
        if grapheme.width() == 1 && grapheme_is_likely_wide_emoji(grapheme) {
            out.push('?');
        } else {
            out.push_str(grapheme);
        }
    }
    out
}

fn grapheme_is_likely_wide_emoji(grapheme: &str) -> bool {
    grapheme.chars().any(|c| {
        let cp = c as u32;
        matches!(
            cp,
            0x2300..=0x27FF       // Misc Tech / Misc Symbols / Dingbats
            | 0x2900..=0x2BFF     // Supp Arrows-A/B, Misc Symbols & Arrows
            | 0x1F000..=0x1FFFF   // Most modern emoji blocks
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MentionTarget {
    User(u64),
    Role(u64),
    Channel(u64),
}

pub fn render_user_mentions<U, R, C>(
    value: &str,
    mut resolve_user_name: U,
    mut resolve_role_name: R,
    mut resolve_channel_name: C,
) -> String
where
    U: FnMut(u64) -> Option<String>,
    R: FnMut(u64) -> Option<String>,
    C: FnMut(u64) -> Option<String>,
{
    if !contains_any_mention_prefix(value) {
        return value.to_owned();
    }

    let mut rendered = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while let Some(start) = next_mention_start(value, cursor) {
        rendered.push_str(&value[cursor..start]);

        let Some((end, target)) = parse_mention(value, start) else {
            rendered.push('<');
            cursor = start.saturating_add(1);
            continue;
        };

        let resolved = match target {
            MentionTarget::User(user_id) => resolve_user_name(user_id),
            MentionTarget::Role(role_id) => resolve_role_name(role_id),
            MentionTarget::Channel(channel_id) => resolve_channel_name(channel_id),
        };
        match resolved {
            Some(name) => {
                rendered.push(mention_prefix(target));
                rendered.push_str(&name);
            }
            None => rendered.push_str(&value[start..end]),
        }
        cursor = end;
    }
    rendered.push_str(&value[cursor..]);
    rendered
}

fn mention_prefix(target: MentionTarget) -> char {
    match target {
        MentionTarget::Channel(_) => '#',
        MentionTarget::User(_) | MentionTarget::Role(_) => '@',
    }
}

fn contains_any_mention_prefix(value: &str) -> bool {
    value.contains("<@") || value.contains("<#")
}

fn next_mention_start(value: &str, cursor: usize) -> Option<usize> {
    let rest = &value[cursor..];
    let user = rest.find("<@");
    let channel = rest.find("<#");
    let relative = match (user, channel) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    Some(cursor.saturating_add(relative))
}

const CUSTOM_EMOJI_CDN_BASE: &str = "https://cdn.discordapp.com/emojis";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderedText {
    pub text: String,
    pub highlights: Vec<TextHighlight>,
    pub emoji_slots: Vec<InlineEmojiSlot>,
}

/// `byte_start..byte_start+byte_len` holds the `:name:` textual fallback.
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
    /// Some other user is being mentioned. Subdued background for information.
    OtherMention,
}

pub fn render_user_mentions_with_highlights<U, R, C, H>(
    value: &str,
    mut resolve_user_name: U,
    mut resolve_role_name: R,
    mut resolve_channel_name: C,
    mut highlight_kind: H,
) -> RenderedText
where
    U: FnMut(u64) -> Option<String>,
    R: FnMut(u64) -> Option<String>,
    C: FnMut(u64) -> Option<String>,
    H: FnMut(MentionTarget) -> Option<TextHighlightKind>,
{
    if !contains_any_mention_prefix(value) {
        return RenderedText {
            text: value.to_owned(),
            highlights: Vec::new(),
            emoji_slots: Vec::new(),
        };
    }

    let mut rendered = String::with_capacity(value.len());
    let mut highlights = Vec::new();
    let mut cursor = 0usize;
    while let Some(start) = next_mention_start(value, cursor) {
        rendered.push_str(&value[cursor..start]);

        let Some((end, target)) = parse_mention(value, start) else {
            rendered.push('<');
            cursor = start.saturating_add(1);
            continue;
        };

        let resolved = match target {
            MentionTarget::User(user_id) => resolve_user_name(user_id),
            MentionTarget::Role(role_id) => resolve_role_name(role_id),
            MentionTarget::Channel(channel_id) => resolve_channel_name(channel_id),
        };
        match resolved {
            Some(name) => {
                let highlight_start = rendered.len();
                rendered.push(mention_prefix(target));
                rendered.push_str(&name);
                let highlight_end = rendered.len();
                if let Some(kind) = highlight_kind(target) {
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
/// overlay is possible. Replaces `<:name:id>` and `<a:name:id>` with
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

/// Text fallback used when custom emoji images are disabled. The id is the
/// most stable value Discord gives us and matches the user's requested
/// fallback better than the display name, which can be missing or renamed.
pub fn replace_custom_emoji_markup_with_ids(value: &str) -> String {
    if !value.contains('<') {
        return value.to_owned();
    }

    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find('<') {
        let start = cursor.saturating_add(relative_start);
        output.push_str(&value[cursor..start]);

        match parse_custom_emoji_full(value, start) {
            Some((end, _name, id, _animated)) => {
                output.push_str(id);
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
#[cfg(test)]
pub fn replace_custom_emoji_markup_in_rendered(rendered: RenderedText) -> RenderedText {
    replace_custom_emoji_markup_in_rendered_with_images(rendered, true)
}

pub fn replace_custom_emoji_markup_in_rendered_with_images(
    rendered: RenderedText,
    images_enabled: bool,
) -> RenderedText {
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
        if images_enabled {
            output.push(':');
            output.push_str(&emoji.name);
            output.push(':');
        } else {
            output.push_str(&emoji.id);
        }
        let slot_byte_len = output.len() - slot_byte_start;
        if images_enabled {
            let extension = if emoji.animated { "gif" } else { "png" };
            emoji_slots.push(InlineEmojiSlot {
                byte_start: slot_byte_start,
                byte_len: slot_byte_len,
                display_width: u16::try_from(slot_byte_len).unwrap_or(u16::MAX),
                url: format!("{CUSTOM_EMOJI_CDN_BASE}/{}.{extension}", emoji.id),
            });
        }
        cursor = emoji.input_end;
    }
    output.push_str(&text[cursor..]);

    let new_highlights = highlights
        .into_iter()
        .map(|highlight| TextHighlight {
            start: remap_offset(&matches, highlight.start, images_enabled),
            end: remap_offset(&matches, highlight.end, images_enabled),
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
    fn output_len(&self, images_enabled: bool) -> usize {
        if images_enabled {
            self.name.len() + 2
        } else {
            self.id.len()
        }
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

fn remap_offset(matches: &[CustomEmojiMatch], pos: usize, images_enabled: bool) -> usize {
    let mut delta: isize = 0;
    for emoji in matches {
        if emoji.input_end <= pos {
            delta += emoji.output_len(images_enabled) as isize - emoji.input_len() as isize;
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

fn parse_mention(value: &str, start: usize) -> Option<(usize, MentionTarget)> {
    let bytes = value.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }

    enum Prefix {
        User,
        Role,
        Channel,
    }

    let mut index = start.saturating_add(1);
    let prefix = match bytes.get(index) {
        Some(&b'@') => {
            index = index.saturating_add(1);
            match bytes.get(index) {
                Some(&b'&') => {
                    index = index.saturating_add(1);
                    Prefix::Role
                }
                Some(&b'!') => {
                    // Legacy nickname-mention prefix. Same target as a plain user mention.
                    index = index.saturating_add(1);
                    Prefix::User
                }
                _ => Prefix::User,
            }
        }
        Some(&b'#') => {
            index = index.saturating_add(1);
            Prefix::Channel
        }
        _ => return None,
    };

    let digits_start = index;
    while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
        index = index.saturating_add(1);
    }
    if index == digits_start || bytes.get(index) != Some(&b'>') {
        return None;
    }

    let id: u64 = value[digits_start..index].parse().ok()?;
    if id == 0 {
        return None;
    }
    let target = match prefix {
        Prefix::User => MentionTarget::User(id),
        Prefix::Role => MentionTarget::Role(id),
        Prefix::Channel => MentionTarget::Channel(id),
    };
    Some((index.saturating_add(1), target))
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{
        InlineEmojiSlot, RenderedText, TextHighlight, TextHighlightKind, render_user_mentions,
        replace_custom_emoji_markup, replace_custom_emoji_markup_in_rendered,
        replace_custom_emoji_markup_in_rendered_with_images, sanitize_for_display_width,
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
    fn rendered_replacer_uses_id_text_when_images_are_disabled() {
        let rendered = RenderedText {
            text: "hi <:wave:42>".to_owned(),
            ..Default::default()
        };

        let out = replace_custom_emoji_markup_in_rendered_with_images(rendered, false);

        assert_eq!(out.text, "hi 42");
        assert!(out.emoji_slots.is_empty());
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
    fn replaces_animated_custom_emoji_markup() {
        let text = replace_custom_emoji_markup("<a:partying_face:42> woo");
        assert_eq!(text, ":partying_face: woo");
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
    fn truncates_by_display_width() {
        let text = truncate_display_width("漢字仮名交じり", 8);

        assert_eq!(text, "漢字...");
        assert!(text.width() <= 8);
    }

    #[test]
    fn sanitize_replaces_misc_symbol_emoji_with_placeholder() {
        let sanitized = sanitize_for_display_width("⚜ ok");
        assert_eq!(sanitized, "? ok");
    }

    #[test]
    fn sanitize_keeps_ascii_and_cjk_unchanged() {
        assert_eq!(sanitize_for_display_width("hello world"), "hello world");
        assert_eq!(sanitize_for_display_width("漢字テスト"), "漢字テスト");
    }

    #[test]
    fn sanitize_keeps_modern_emoji_blocks_unchanged() {
        // 🦀 (U+1F980) is correctly reported as width 2 by `unicode-width`
        // and rendered as 2 columns by terminals.
        assert_eq!(sanitize_for_display_width("🦀 ferris"), "🦀 ferris");
    }

    #[test]
    fn sanitize_replaces_lone_regional_indicator() {
        let sanitized = sanitize_for_display_width("hi \u{1F1F6}!");
        assert_eq!(sanitized, "hi ?!");
    }

    #[test]
    fn renders_deprecated_nickname_mentions_like_user_mentions() {
        let text = render_user_mentions(
            "hello <@!10>",
            |user_id| (user_id == 10).then(|| "alice".to_owned()),
            |_| None,
            |_| None,
        );

        assert_eq!(text, "hello @alice");
    }

    #[test]
    fn keeps_zero_user_mentions_raw() {
        let text = render_user_mentions(
            "hello <@0>",
            |user_id| (user_id == 0).then(|| "nobody".to_owned()),
            |_| None,
            |_| None,
        );

        assert_eq!(text, "hello <@0>");
    }

    #[test]
    fn renders_or_keeps_role_and_channel_mentions() {
        let cases = [
            ("hello <@&10>", "hello @Mods"),
            ("hello <@&11>", "hello <@&11>"),
            ("see <#42> for details", "see #general for details"),
            ("see <#43>", "see <#43>"),
            ("see <#0>", "see <#0>"),
        ];

        for (input, expected) in cases {
            let text = render_user_mentions(
                input,
                |_| None,
                |role_id| (role_id == 10).then(|| "Mods".to_owned()),
                |channel_id| (channel_id == 42).then(|| "general".to_owned()),
            );
            assert_eq!(text, expected);
        }
    }

    #[test]
    fn renders_mixed_mentions_in_one_string() {
        let text = render_user_mentions(
            "hi <@10> in <#20> and <@&30>",
            |user_id| (user_id == 10).then(|| "alice".to_owned()),
            |role_id| (role_id == 30).then(|| "Mods".to_owned()),
            |channel_id| (channel_id == 20).then(|| "general".to_owned()),
        );

        assert_eq!(text, "hi @alice in #general and @Mods");
    }

    #[test]
    fn keeps_overflowing_user_mentions_raw() {
        let text = render_user_mentions(
            "hello <@18446744073709551616>",
            |_| Some("overflow".to_owned()),
            |_| None,
            |_| None,
        );

        assert_eq!(text, "hello <@18446744073709551616>");
    }

    #[test]
    fn renders_user_mentions_next_to_unicode() {
        let text = render_user_mentions(
            "café<@10>!",
            |user_id| (user_id == 10).then(|| "alice".to_owned()),
            |_| None,
            |_| None,
        );

        assert_eq!(text, "café@alice!");
    }

    #[test]
    fn keeps_malformed_user_mentions_raw() {
        let text = render_user_mentions(
            "hello <@abc> <@10",
            |user_id| (user_id == 10).then(|| "alice".to_owned()),
            |_| None,
            |_| None,
        );

        assert_eq!(text, "hello <@abc> <@10");
    }
}
