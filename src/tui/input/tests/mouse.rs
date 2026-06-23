use super::*;

#[test]
fn left_click_focuses_top_level_pane() {
    let mut state = DashboardState::new();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
}

#[test]
fn left_click_selects_visible_channel_row() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Messages);
    let (column, row) = channel_row_point(1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), 1);
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn double_click_activates_pane_rows_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseClickTracker::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(state.selected_channel_id(), Some(Id::new(11)));
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );

    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key('/'));
    for value in "random".chars() {
        handle_key(&mut state, char_key(value));
    }
    let mut clicks = MouseClickTracker::default();
    let (column, row) = channel_row_point(0);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(state.selected_channel_id(), None);
    assert_eq!(state.channel_pane_filter_query(), Some("random"));

    assert_eq!(second.command, None);

    let mut clicks = MouseClickTracker::default();
    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(state.focus(), FocusPane::Messages);

    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, char_key('/'));
    for value in "second".chars() {
        handle_key(&mut state, char_key(value));
    }
    let mut clicks = MouseClickTracker::default();
    let event = mouse(MouseEventKind::Down(MouseButton::Left), 1, 2);

    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert!(second.handled);
    assert_eq!(state.guild_pane_filter_query(), Some("second"));
    assert_eq!(second.command, None);

    let mut clicks = MouseClickTracker::default();
    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(second.command, None);
    assert_eq!(state.selected_guild_id(), Some(Id::new(2)));
    assert_eq!(state.focus(), FocusPane::Channels);
}

#[test]
fn left_click_selects_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 6),
        dashboard_area(),
    ));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn double_click_activates_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();
    let mut clicks = MouseClickTracker::default();
    let event = mouse(MouseEventKind::Down(MouseButton::Left), 50, 6);

    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn channel_switcher_absorbs_backdrop_clicks() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();
    state.focus_pane(FocusPane::Messages);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 21, 2),
        dashboard_area(),
    ));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert_eq!(state.selected_channel(), 0);
}

#[test]
fn wheel_moves_channel_switcher_selection() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 50, 7),
        dashboard_area(),
    ));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 7),
        dashboard_area(),
    ));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn terminal_click_release_sequence_still_double_clicks_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseClickTracker::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let release = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Up(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert!(release.handled);
    assert!(second.handled);
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );
}

#[test]
fn scroll_between_clicks_prevents_stale_double_click_activation() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseClickTracker::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let scroll = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::ScrollDown, column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert!(scroll.handled);
    assert!(second.handled);
    assert_eq!(second.command, None);
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn left_click_on_message_input_starts_composer() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    let (column, row) = composer_point();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert!(state.is_composing());
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn mouse_click_outside_dashboard_panes_does_not_change_focus() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Messages);

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 10, 0),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Right), 1, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn mouse_click_outside_composer_blurs_and_focuses_clicked_pane_without_clearing_draft() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('d'));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
    assert!(!state.is_composing());
    assert_eq!(state.composer_input(), "d");
}

#[test]
fn mouse_click_outside_composer_blurs_and_selects_clicked_row() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Up));
    state.start_composer();
    let (column, row) = channel_row_point(1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert!(!state.is_composing());
    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), 1);
}

#[test]
fn mouse_scroll_outside_composer_does_not_clear_draft() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('d'));

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 100, 1),
        dashboard_area(),
    ));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "d");
}

#[test]
fn mouse_wheel_scrolls_hovered_channel_viewport_without_moving_selection() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Messages);
    state.set_channel_view_height(2);
    let selected = state.selected_channel();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 21, 1),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), selected);
    assert_eq!(state.channel_scroll(), 1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 21, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_channel(), selected);
    assert_eq!(state.channel_scroll(), 0);
}

#[test]
fn mouse_wheel_scrolls_hovered_member_viewport_without_moving_selection() {
    let mut state = state_with_members(10);
    state.focus_pane(FocusPane::Messages);
    state.set_member_view_height(4);
    let selected = state.selected_member();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 100, 1),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Members);
    assert_eq!(state.selected_member(), selected);
    assert_eq!(state.member_scroll(), 1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_member(), selected);
    assert_eq!(state.member_scroll(), 0);
}

#[test]
fn mouse_wheel_scrolls_message_viewport_without_changing_selection() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(2, 16, 3);
    let selected = state.selected_message();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 50, 1),
        dashboard_area(),
    ));
    state.clamp_message_viewport_for_image_previews(2, 16, 3);

    assert_eq!(state.focus(), FocusPane::Messages);
    assert_eq!(state.selected_message(), selected);
    assert!(state.message_line_scroll() > 0);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn user_profile_popup_absorbs_left_clicks_only_inside_popup() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Messages);
    state.open_user_profile_popup(Id::new(10), None);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 60, 10),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));
}

#[test]
fn mouse_click_selects_message_action_row() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let count = state.selected_message_action_items().len() as u16;
    let (column, row) = message_action_row_point(count, 0);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.selected_message_action_index(), Some(0));
}

#[test]
fn mouse_double_click_activates_message_action_row_like_enter() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let mut clicks = MouseClickTracker::default();
    let poll_row = state
        .selected_message_action_items()
        .iter()
        .position(|action| action.kind == MessageActionKind::OpenPollVotePicker)
        .expect("poll action should exist");
    let count = state.selected_message_action_items().len() as u16;
    let (column, row) = message_action_row_point(
        count,
        u16::try_from(poll_row).expect("message action row fits in test area"),
    );

    handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Up(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert_eq!(second.command, None);
    assert!(!state.is_message_action_context_active());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));
}

#[test]
fn mouse_wheel_moves_message_action_selection() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let count = state.selected_message_action_items().len() as u16;
    let (column, row) = message_action_row_point(count, 0);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, column, row),
        dashboard_area(),
    ));
    assert_eq!(state.selected_message_action_index(), Some(1));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, column, row),
        dashboard_area(),
    ));
    assert_eq!(state.selected_message_action_index(), Some(0));
}
