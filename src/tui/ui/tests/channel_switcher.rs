use super::*;

fn switcher_view<'a>(
    items: &'a [ChannelSwitcherItem],
    mode: ChannelSwitcherMode,
    query: &'a str,
) -> ChannelSwitcherView<'a> {
    ChannelSwitcherView {
        query,
        query_cursor: query.len(),
        mode,
        items,
        selected: 0,
        scroll: 0,
    }
}

#[test]
fn channel_switcher_lines_show_search_and_grouped_selection() {
    let items = vec![
        ChannelSwitcherItem::test(
            Id::new(1),
            ChannelSwitcherDisplay {
                group_label: "Direct Messages".to_owned(),
                label: "@alice".to_owned(),
                search_text: "alice".to_owned(),
                ..ChannelSwitcherDisplay::test()
            },
        ),
        ChannelSwitcherItem::test(
            Id::new(2),
            ChannelSwitcherDisplay {
                group_label: "guild".to_owned(),
                parent_label: Some("Text".to_owned()),
                label: "#general".to_owned(),
                search_text: "general".to_owned(),
                depth: 1,
                group_order: 1,
                original_index: 1,
                ..ChannelSwitcherDisplay::test()
            },
        ),
    ];

    let lines = channel_switcher_lines(
        ChannelSwitcherView {
            selected: 1,
            ..switcher_view(&items, ChannelSwitcherMode::Channels, "gen")
        },
        10,
        40,
    );

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
    let items = vec![ChannelSwitcherItem::test(
        Id::new(1),
        ChannelSwitcherDisplay {
            group_label: "Direct Messages".to_owned(),
            label: "@new".to_owned(),
            unread: ChannelUnreadState::Unread,
            badge_state: ChannelUnreadState::Notified(5),
            search_text: "new".to_owned(),
            ..ChannelSwitcherDisplay::test()
        },
    )];

    let lines = channel_switcher_lines(
        switcher_view(&items, ChannelSwitcherMode::Channels, ""),
        10,
        40,
    );

    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("(5) @new"))
    );
}

#[test]
fn selected_channel_switcher_unread_row_uses_selection_color() {
    let items = vec![ChannelSwitcherItem::test(
        Id::new(1),
        ChannelSwitcherDisplay {
            group_label: "guild".to_owned(),
            label: "#alerts".to_owned(),
            unread: ChannelUnreadState::Mentioned(2),
            badge_state: ChannelUnreadState::Mentioned(2),
            search_text: "alerts".to_owned(),
            ..ChannelSwitcherDisplay::test()
        },
    )];

    let lines = channel_switcher_lines(
        switcher_view(&items, ChannelSwitcherMode::Channels, ""),
        10,
        40,
    );
    let item_line = lines
        .iter()
        .find(|line| line.to_string().contains("#alerts"))
        .expect("selected channel row");
    let label = item_line.spans.last().expect("channel label span");

    assert_eq!(label.content, "#alerts");
    assert_eq!(
        label.style.bg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .bg
    );
    assert_eq!(
        label.style.fg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .fg
    );
}

#[test]
fn channel_switcher_lines_name_servers_when_guild_search_has_no_results() {
    let lines = channel_switcher_lines(
        switcher_view(&[], ChannelSwitcherMode::Guilds, "nope"),
        10,
        40,
    );

    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("No servers found"))
    );
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

    let lines = channel_switcher_lines(
        switcher_view(&[], ChannelSwitcherMode::Channels, query),
        10,
        12,
    );
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
