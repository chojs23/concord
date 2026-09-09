use unicode_width::UnicodeWidthStr;

use super::{
    InlineEmojiSlot, RenderedText, TextHighlight, TextHighlightKind, TextReplacement,
    remap_text_offset, render_user_mentions, render_user_mentions_in_rendered_text,
    replace_custom_emoji_markup, replace_custom_emoji_markup_in_rendered,
    replace_custom_emoji_markup_in_rendered_with_images, sanitize_for_display_width,
    truncate_display_width, truncate_text,
};

#[test]
fn text_replacements_remap_metadata_across_growing_and_shrinking_utf8_ranges() {
    let replacements = [
        TextReplacement {
            input_start: 3,
            input_end: 5,
            output_start: 3,
            output_len: 4,
        },
        TextReplacement {
            input_start: 8,
            input_end: 12,
            output_start: 10,
            output_len: 1,
        },
    ];

    for (position, expected) in [
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 7),
        (8, 10),
        (10, 11),
        (12, 11),
        (15, 14),
    ] {
        assert_eq!(remap_text_offset(&replacements, position), expected);
    }

    let mut rendered = RenderedText {
        text: "한글ab🙂tail".to_owned(),
        highlights: vec![TextHighlight {
            start: 5,
            end: 12,
            kind: TextHighlightKind::Timestamp,
        }],
        emoji_slots: vec![InlineEmojiSlot {
            byte_start: 15,
            byte_len: 1,
            display_width: 1,
            url: "emoji".to_owned(),
        }],
    };
    rendered.remap_metadata(&replacements);

    assert_eq!(
        (rendered.highlights[0].start, rendered.highlights[0].end),
        (7, 11)
    );
    assert_eq!(rendered.emoji_slots[0].byte_start, 14);
}

#[test]
fn mention_rendering_preserves_existing_semantic_highlights() {
    let input = RenderedText {
        text: "<@10> 09:00".to_owned(),
        highlights: vec![TextHighlight {
            start: 6,
            end: 11,
            kind: TextHighlightKind::Timestamp,
        }],
        emoji_slots: Vec::new(),
    };

    let output = render_user_mentions_in_rendered_text(
        input,
        |user_id| (user_id == 10).then(|| "alice".to_owned()),
        |_| None,
        |_| None,
        |_| Some(TextHighlightKind::OtherMention),
    );

    assert_eq!(output.text, "@alice 09:00");
    assert_eq!(
        output.highlights,
        vec![
            TextHighlight {
                start: 0,
                end: 6,
                kind: TextHighlightKind::OtherMention,
            },
            TextHighlight {
                start: 7,
                end: 12,
                kind: TextHighlightKind::Timestamp,
            },
        ]
    );
}

#[test]
fn rendered_emoji_fallback_preserves_slot_metadata() {
    let cases = [
        (
            "static",
            "hi <:emoji_48:1146289325491892225>!",
            "hi :emoji_48:!",
            "hi ".len(),
            ":emoji_48:".len(),
            "https://cdn.discordapp.com/emojis/1146289325491892225.png",
        ),
        (
            "animated",
            "<a:wave:42>",
            ":wave:",
            0,
            ":wave:".len(),
            "https://cdn.discordapp.com/emojis/42.webp?animated=true",
        ),
    ];

    for (name, input, expected_text, byte_start, byte_len, expected_url) in cases {
        let out = replace_custom_emoji_markup_in_rendered(RenderedText::from(input));

        assert_eq!(out.text, expected_text, "{name}");
        assert_eq!(out.emoji_slots.len(), 1, "{name}");
        let slot = &out.emoji_slots[0];
        assert_eq!(slot.byte_start, byte_start, "{name}");
        assert_eq!(slot.byte_len, byte_len, "{name}");
        assert_eq!(slot.display_width, byte_len as u16, "{name}");
        assert_eq!(slot.url, expected_url, "{name}");
    }
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
fn sanitize_replaces_only_glyphs_the_terminal_cannot_width_correctly() {
    for (input, expected) in [
        ("hello world", "hello world"),
        ("漢字テスト", "漢字テスト"),
        ("🦀 ferris", "🦀 ferris"),
        ("⚜ ok", "? ok"),
        ("hi \u{1F1F6}!", "hi ?!"),
    ] {
        assert_eq!(sanitize_for_display_width(input), expected, "{input}");
    }
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
fn user_mentions_render_only_when_the_markup_is_well_formed() {
    let cases = [
        ("hello <@10>", "hello @alice"),
        ("hello <@!10>", "hello @alice"),
        ("café<@10>!", "café@alice!"),
        ("hello <@0>", "hello <@0>"),
        ("hello <@abc> <@10", "hello <@abc> <@10"),
    ];

    for (input, expected) in cases {
        let text = render_user_mentions(
            input,
            |user_id| (user_id == 10).then(|| "alice".to_owned()),
            |_| None,
            |_| None,
        );
        assert_eq!(text, expected, "{input}");
    }
}

#[test]
fn custom_emoji_markup_collapses_to_its_shortcode_when_well_formed() {
    let cases = [
        ("<a:partying_face:42> woo", ":partying_face: woo"),
        ("héllo<:emoji_48:1146289325491892225>!", "héllo:emoji_48:!"),
        (
            "<:no_id:> <:bad-name:1> <@10> <:ok:7>",
            "<:no_id:> <:bad-name:1> <@10> :ok:",
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(replace_custom_emoji_markup(input), expected, "{input}");
    }
}
