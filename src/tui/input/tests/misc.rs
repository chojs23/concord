use super::*;

#[test]
fn forum_blank_bottom_rows_do_not_select_hidden_posts() {
    let mut state = state_with_forum_channel_posts();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: Id::new(20),
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 2,
        next_offset: 3,
        threads: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(29),
            parent_id: Some(Id::new(20)),
            owner_id: None,
            position: Some(2),
            last_message_id: None,
            name: "hidden by remainder rows".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: Some(1),
            member_count: None,
            total_message_sent: Some(1),
            thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
            flags: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);
    let (column, row) = message_row_point(11);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.selected_forum_post(), 0);
}

#[test]
fn backtick_types_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('`'));

    assert!(state.is_composing());
    assert!(!state.is_debug_log_popup_open());
    assert_eq!(state.composer_input(), "`");
}

#[test]
fn a_key_no_longer_opens_actions_directly() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key('a'));

    assert!(!state.is_message_action_menu_open());
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn esc_closes_modal_before_returning_from_opened_thread() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, char_key('`'));
    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_debug_log_popup_open());
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
}
