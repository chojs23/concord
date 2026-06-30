use super::*;

#[test]
fn channel_switcher_lines_show_search_and_grouped_selection() {
    let items = vec![
        ChannelSwitcherItem {
            group_label: "Direct Messages".to_owned(),
            channel_label: "@alice".to_owned(),
            search_name: "alice".to_owned(),
            ..ChannelSwitcherItem::test(Id::new(1))
        },
        ChannelSwitcherItem {
            guild_id: Some(Id::new(1)),
            guild_name: Some("guild".to_owned()),
            group_label: "guild".to_owned(),
            parent_label: Some("Text".to_owned()),
            channel_label: "#general".to_owned(),
            search_name: "general".to_owned(),
            depth: 1,
            group_order: 1,
            original_index: 1,
            ..ChannelSwitcherItem::test(Id::new(2))
        },
    ];

    let lines = channel_switcher_lines(&items, 1, "gen", "gen".len(), 10, 0, 40);

    assert_eq!(lines[0].spans[0].content, "🔎 ");
    assert_eq!(lines[0].spans[1].content, "gen");
    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("Direct Messages"))
    );
    assert!(lines.iter().any(|line| line.to_string().contains("guild")));
    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("Text / #general"))
    );
}

#[test]
fn channel_switcher_lines_show_unread_badges_like_channel_pane() {
    let items = vec![ChannelSwitcherItem {
        group_label: "Direct Messages".to_owned(),
        channel_label: "@new".to_owned(),
        unread: ChannelUnreadState::Unread,
        unread_message_count: 5,
        search_name: "new".to_owned(),
        ..ChannelSwitcherItem::test(Id::new(1))
    }];

    let lines = channel_switcher_lines(&items, 0, "", 0, 10, 0, 40);

    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("(5) @new"))
    );
}

#[test]
fn selected_channel_switcher_unread_row_keeps_highlight() {
    let items = vec![ChannelSwitcherItem {
        guild_id: Some(Id::new(1)),
        guild_name: Some("guild".to_owned()),
        group_label: "guild".to_owned(),
        channel_label: "#alerts".to_owned(),
        unread: ChannelUnreadState::Mentioned(2),
        search_name: "alerts".to_owned(),
        ..ChannelSwitcherItem::test(Id::new(1))
    }];

    let lines = channel_switcher_lines(&items, 0, "", 0, 10, 0, 40);
    let item_line = lines
        .iter()
        .find(|line| line.to_string().contains("#alerts"))
        .expect("selected channel row");
    let label = item_line.spans.last().expect("channel label span");

    assert_eq!(label.content, "#alerts");
    assert!(label.style.bg.is_some());
    assert_eq!(label.style.fg, Some(MENTION_ORANGE));
}

#[test]
fn channel_switcher_cursor_position_tracks_query_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    state.push_channel_switcher_char('g');
    state.push_channel_switcher_char('e');
    let right = channel_switcher_cursor_position(Rect::new(0, 0, 100, 20), &state)
        .expect("switcher cursor");

    state.move_channel_switcher_query_cursor_left();
    let left = channel_switcher_cursor_position(Rect::new(0, 0, 100, 20), &state)
        .expect("switcher cursor");

    assert_eq!(left.y, right.y);
    assert!(left.x < right.x);
}

#[test]
fn channel_switcher_search_line_windows_long_query_around_cursor() {
    let query = "abcdefghijklmnopqrstuvwxyz";

    let lines = channel_switcher_lines(&[], 0, query, query.len(), 10, 0, 12);
    let rendered = lines[0].to_string();

    assert!(rendered.contains("uvwxyz"));
}

#[test]
fn channel_switcher_cursor_position_stays_on_search_row_for_long_query() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
        state.push_channel_switcher_char(ch);
    }

    let position =
        channel_switcher_cursor_position(Rect::new(0, 0, 40, 20), &state).expect("switcher cursor");

    assert_eq!(position.y, 2);
    assert!(position.x < 39);
}
