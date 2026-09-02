use super::*;

#[test]
fn attachment_viewer_preview_area_centers_rendered_image() {
    let area = Rect::new(21, 10, 78, 29);

    let preview = centered_viewer_preview_area(area, 52, 13);

    assert_eq!(preview, Rect::new(34, 18, 52, 13));
}

#[test]
fn custom_emoji_markup_uses_id_fallback_when_disabled() {
    let message = message_with_content(Some("hello <:wave:42>".to_owned()));
    let state = DashboardState::new_with_display_options(DisplayOptions {
        show_custom_emoji: false,
        ..DisplayOptions::default()
    });

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(lines[0].text, "hello 42");
}

#[test]
fn loaded_custom_emoji_message_uses_image_width() {
    let message = message_with_content(Some("<:long_custom:42>text".to_owned()));
    let loaded_urls = vec!["https://cdn.discordapp.com/emojis/42.png".to_owned()];

    for width in [200, 6] {
        let lines = format_message_content_lines_with_loaded_custom_emoji_urls(
            &message,
            &DashboardState::new(),
            width,
            &loaded_urls,
        );

        assert_eq!(line_texts(&lines), vec!["  text"]);
        assert_eq!(lines[0].image_slots[0].col, 0);
        assert_eq!(lines[0].image_slots[0].image_size, EmojiImageSize::Compact);
    }
}

#[test]
fn image_preview_rows_are_part_of_the_message_item() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("look".to_owned())],
        14,
        3,
        None,
        0,
    );

    assert_eq!(lines.len(), 6);
}

#[test]
fn message_viewport_lines_put_reactions_below_image_preview_rows() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.reactions = vec![ReactionInfo {
        count: 3,
        me: true,
        ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
    }];
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::default_message_viewport_layout(),
        &[],
    );

    assert_eq!(lines.len(), 8);
    assert_eq!(line_texts_from_ratatui(&lines)[6], "        [👍 3]");
}

#[test]
fn embed_image_preview_rows_continue_embed_gutter() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("look".to_owned())],
        14,
        2,
        Some(0xff0000),
        0,
    );

    assert_eq!(line_texts_from_ratatui(&lines)[2], "          ▎ ");
    assert_eq!(lines[2].spans[1].style.fg, Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn selected_author_group_keeps_avatar_body_inside_border() {
    let message = message_with_content(Some("abcdefghijkl".to_owned()));
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        Some(0),
        &DashboardState::new(),
        super::narrow_message_viewport_layout(20),
        &[],
    );
    let sent_time = format_message_sent_time(Id::new(1), true);

    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.len(), 3);
    assert!(texts[0].starts_with("╭─oooo  neo "));
    assert!(texts[0].contains(&sent_time));
    assert!(texts[0].ends_with("╮"));
    assert!(texts[1].starts_with("│ oooo  abcdefghijkl"));
    assert!(texts[1].ends_with(" │"));
    assert!(texts[2].starts_with("╰"));
    assert!(texts[2].ends_with("╯"));
    assert_eq!(
        lines[0].spans[0].style.fg,
        theme::current()
            .style(theme::HighlightGroup::MessageSelectedBorder)
            .fg
    );
    assert_eq!(
        lines[1].spans[0].style.fg,
        theme::current()
            .style(theme::HighlightGroup::MessageSelectedBorder)
            .fg
    );
    assert!(
        lines[1].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .all(|span| span.style.bg.is_none())
    );
}

#[test]
fn selected_message_avatar_stays_in_fixed_gutter() {
    assert_eq!(selected_avatar_x_offset(Some(0), 0), 2);
    assert_eq!(selected_avatar_x_offset(Some(1), 0), 2);
}

// The three rects are only readable next to each other. An embed accent bar
// pushes x from 18 to 22, and a negative row offset clips height instead of
// moving the preview above the list.
#[test]
fn inline_image_preview_area_places_the_preview_in_the_content_column() {
    for (name, area, row_offset, x_offset, accent, expected) in [
        (
            "plain preview under a message",
            Rect::new(10, 5, 80, 12),
            2,
            0,
            None,
            Rect::new(18, 8, 72, 4),
        ),
        (
            "embed accent bar leaves room for the gutter",
            Rect::new(10, 5, 80, 12),
            2,
            0,
            Some(0xff0000),
            Rect::new(22, 8, 68, 4),
        ),
        (
            "selected row keeps the same content column",
            Rect::new(10, 5, 80, 12),
            2,
            selected_message_content_x_offset(true),
            None,
            Rect::new(18, 8, 72, 4),
        ),
        (
            "negative offset clips at the list top",
            Rect::new(10, 5, 80, 6),
            -2,
            0,
            None,
            Rect::new(18, 5, 72, 3),
        ),
    ] {
        assert_eq!(
            inline_image_preview_area(
                area,
                row_offset,
                x_offset,
                77,
                4,
                accent,
                MESSAGE_AVATAR_OFFSET
            ),
            Some(expected),
            "{name}"
        );
    }
}

#[test]
fn later_image_preview_slot_accounts_for_prior_preview_rows() {
    let area = Rect::new(10, 5, 80, 18);
    let messages = [
        message_with_attachment(Some("one".to_owned()), image_attachment()),
        message_with_attachment(Some("two".to_owned()), image_attachment()),
        message_with_attachment(Some("three".to_owned()), image_attachment()),
    ];
    let messages = messages.iter().collect::<Vec<_>>();
    let state = DashboardState::new();
    let row = inline_image_preview_row(&messages, &state, 2, 200, 0, 4);

    assert_eq!(row, 14);
    assert_eq!(
        inline_image_preview_area(area, row, 0, 77, 4, None, MESSAGE_AVATAR_OFFSET),
        Some(Rect::new(18, 20, 72, 3))
    );
}

#[test]
fn inline_image_preview_row_ignores_reaction_footer_for_current_message() {
    let mut message = message_with_attachment(Some("one".to_owned()), image_attachment());
    message.reactions = vec![ReactionInfo {
        count: 3,
        me: true,
        ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
    }];
    let messages = [&message];
    let state = DashboardState::new();

    assert_eq!(inline_image_preview_row(&messages, &state, 0, 200, 0, 0), 2);
}

#[test]
fn overlay_registry_occludes_modal_and_non_modal_popups() {
    let frame_area = Rect::new(0, 0, 120, 50);
    let mut options_state = DashboardState::new();
    options_state.open_options_popup();
    let mut keymap_state = DashboardState::new();
    keymap_state.open_keymap_help_popup();
    let mut search_state = state_with_message();
    search_state.open_search_popup_for_focus(FocusPane::Messages);
    // Folder settings is a non-modal overlay and still has to occlude media.
    let folder_settings_state = state_with_folder_settings();

    for state in [
        &options_state,
        &keymap_state,
        &search_state,
        &folder_settings_state,
    ] {
        let areas = background_media_occlusion_areas(frame_area, state);

        assert_eq!(areas.len(), 1, "{areas:?}");
        assert!(!areas[0].is_empty(), "{areas:?}");
    }
}

#[test]
fn inline_image_preview_renders_when_not_occluded() {
    let mut state = state_with_message();
    let preview = loading_image_preview_at_message_offset(1);

    let rendered =
        render_dashboard_dump_with_previews(120, 30, &mut state, vec![preview]).join("\n");

    assert!(rendered.contains("loading cat.png"), "{rendered}");
}

fn loading_image_preview_at_message_offset(preview_y_offset_rows: usize) -> ImagePreview<'static> {
    ImagePreview {
        viewer: false,
        thread_card: false,
        message_index: 0,
        body_line_index: None,
        preview_x_offset_columns: 0,
        preview_y_offset_rows,
        preview_width: 72,
        preview_height: 4,
        visible_preview_height: 4,
        accent_color: None,
        state: ImagePreviewState::Loading {
            filename: "cat.png".to_owned(),
        },
    }
}

#[test]
fn inline_image_preview_area_follows_content_and_clips_at_the_list_bottom() {
    let cases = [
        (Rect::new(10, 5, 80, 12), 2, Rect::new(18, 8, 72, 4)),
        (Rect::new(10, 5, 80, 6), 3, Rect::new(18, 9, 72, 2)),
    ];

    for (area, row, expected) in cases {
        assert_eq!(
            inline_image_preview_area(area, row, 0, 77, 4, None, MESSAGE_AVATAR_OFFSET),
            Some(expected),
            "{area:?} row {row}"
        );
    }
}
