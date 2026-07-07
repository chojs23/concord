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
