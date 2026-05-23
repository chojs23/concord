use super::*;

#[test]
fn image_viewer_preview_area_centers_rendered_image() {
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
    assert!(lines[0].image_slots.is_empty());
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
        assert_eq!(lines[0].image_slots[0].display_width, 2);
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
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 3,
        me: true,
    }];
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::super::message_viewport_layout(200, 80, 80, 16, 3),
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
        super::super::message_viewport_layout(20, 80, 80, 16, 3),
        &[],
    );
    let sent_time = format_message_sent_time(Id::new(1));

    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.len(), 3);
    assert!(texts[0].starts_with("╭─oooo  neo "));
    assert!(texts[0].contains(&sent_time));
    assert!(texts[0].ends_with("╮"));
    assert!(texts[1].starts_with("│ oooo  abcdefghijkl"));
    assert!(texts[1].ends_with(" │"));
    assert!(texts[2].starts_with("╰"));
    assert!(texts[2].ends_with("╯"));
    assert_eq!(lines[0].spans[0].style.fg, Some(SELECTED_MESSAGE_BORDER));
    assert_eq!(lines[1].spans[0].style.fg, Some(SELECTED_MESSAGE_BORDER));
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

#[test]
fn inline_image_preview_slot_follows_image_message_content() {
    let area = Rect::new(10, 5, 80, 12);

    assert_eq!(
        inline_image_preview_area(area, 2, 0, 77, 4, None),
        Some(Rect::new(18, 8, 72, 4))
    );
}

#[test]
fn embed_image_preview_area_leaves_room_for_gutter() {
    let area = Rect::new(10, 5, 80, 12);

    assert_eq!(
        inline_image_preview_area(area, 2, 0, 77, 4, Some(0xff0000)),
        Some(Rect::new(22, 8, 68, 4))
    );
}

#[test]
fn selected_inline_image_preview_area_keeps_fixed_content_column() {
    let area = Rect::new(10, 5, 80, 12);
    let selected_offset = selected_message_content_x_offset(true);

    assert_eq!(
        inline_image_preview_area(area, 2, selected_offset, 77, 4, None),
        Some(Rect::new(18, 8, 72, 4))
    );
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
        inline_image_preview_area(area, row, 0, 77, 4, None),
        Some(Rect::new(18, 20, 72, 3))
    );
}

#[test]
fn inline_image_preview_row_ignores_reaction_footer_for_current_message() {
    let mut message = message_with_attachment(Some("one".to_owned()), image_attachment());
    message.reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 3,
        me: true,
    }];
    let messages = [&message];
    let state = DashboardState::new();

    assert_eq!(inline_image_preview_row(&messages, &state, 0, 200, 0, 0), 2);
}

#[test]
fn inline_image_preview_area_hides_preview_at_list_bottom() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(
        inline_image_preview_area(area, 3, 0, 77, 4, None),
        Some(Rect::new(18, 9, 72, 2))
    );
}

#[test]
fn inline_image_preview_area_clips_preview_at_list_top() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(
        inline_image_preview_area(area, -2, 0, 77, 4, None),
        Some(Rect::new(18, 5, 72, 3))
    );
}

#[test]
fn inline_image_preview_area_returns_none_when_preview_starts_below_list() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(inline_image_preview_area(area, 5, 0, 77, 4, None), None);
}

#[test]
fn inline_image_preview_area_returns_none_when_preview_ends_above_list() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(inline_image_preview_area(area, -5, 0, 77, 4, None), None);
}
