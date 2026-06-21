use super::*;

#[test]
fn quit_key_requires_confirmation() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('q'));

    assert!(!state.should_quit());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::QuitConfirmation));

    handle_key(&mut state, char_key('n'));
    assert!(!state.should_quit());
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::QuitConfirmation)
    );

    handle_key(&mut state, char_key('q'));
    handle_key(&mut state, char_key('y'));

    assert!(state.should_quit());
}

#[test]
fn question_mark_opens_current_keymap_popup_and_scrolls_within_bounds() {
    let mut state = DashboardState::new();
    handle_key(&mut state, char_key('?'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::KeymapHelp));

    state.set_keymap_popup_view_height(4);
    state.set_keymap_popup_total_lines(10);

    for _ in 0..10 {
        handle_key(&mut state, ctrl_key('d'));
    }
    assert_eq!(state.keymap_popup_scroll(), 6);

    handle_key(&mut state, ctrl_key('u'));

    assert_eq!(state.keymap_popup_scroll(), 4);

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::KeymapHelp));
}

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
            parent_id: Some(Id::new(20)),
            position: Some(2),
            name: "hidden by remainder rows".to_owned(),
            message_count: Some(1),
            total_message_sent: Some(1),
            thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(29), "GuildPublicThread")
        }],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);
    let (column, row) = message_row_point(13);

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
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::DebugLog));
    assert_eq!(state.composer_input(), "`");
}

#[test]
fn a_key_no_longer_opens_actions_directly() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key('a'));

    assert!(!state.is_message_action_context_active());
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn esc_closes_modal_before_returning_from_opened_thread() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('t'));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, char_key('`'));
    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::DebugLog));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
}

#[test]
fn ctrl_v_requests_clipboard_paste_on_profile_avatar_field() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    handle_key(&mut state, ctrl_key('v'));

    assert!(state.take_paste_clipboard_request());
    assert!(state.accepts_clipboard_paste());
    assert!(state.begin_clipboard_paste());
    assert_eq!(
        state.user_profile_settings_status(),
        Some("Reading clipboard image...")
    );
}

#[test]
fn profile_status_picker_routes_selection_keys_and_enter() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_user_profile_status_picker_open());

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        Some(AppCommand::UpdateCurrentUserStatus {
            status: PresenceStatus::Idle,
        })
    );
    assert!(!state.is_user_profile_status_picker_open());
}

#[test]
fn o_key_signs_out_from_current_user_profile_popup() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    assert_eq!(
        handle_key(&mut state, char_key('o')),
        Some(AppCommand::SignOut)
    );
    assert_eq!(state.user_profile_settings_status(), Some("Signing out..."));
}

#[test]
fn profile_activity_edit_enter_dispatches_presence_update() {
    let mut state = DashboardState::new();
    let user_id = Id::new(10);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
    });
    state.open_current_user_profile_popup();
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, key(KeyCode::Enter));

    for value in "Concord".chars() {
        handle_key(&mut state, char_key(value));
    }

    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        Some(AppCommand::UpdateCurrentUserActivity {
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::playing("Concord")],
        })
    );
}

#[test]
fn pasted_file_path_sets_profile_avatar_field() {
    let avatar = temp_upload_file("avatar.png", &[1, 2, 3]);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    assert!(handle_paste(&mut state, &avatar.to_string_lossy()));
    assert!(state.user_profile_popup_has_avatar_preview());

    assert_eq!(
        state.user_profile_settings_field_value(
            crate::tui::state::UserProfileSettingsField::GlobalAvatarPath,
        ),
        "avatar.png"
    );
    assert!(matches!(
        state.save_user_profile_settings_command(),
        Some(AppCommand::UpdateUserProfile { update })
            if update
                .global
                .avatar
                .as_ref()
                .and_then(crate::discord::ProfileAvatarUpload::path)
                == Some(avatar.as_path())
    ));

    remove_temp_upload_file(&avatar);
}

#[test]
fn ctrl_v_pastes_text_into_profile_edit_field_at_cursor() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('c'));
    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, ctrl_key('v'));

    assert!(state.take_paste_clipboard_request());
    assert!(state.accepts_clipboard_paste());
    assert!(state.begin_clipboard_paste());
    assert!(handle_paste(&mut state, "b"));
    state.finish_clipboard_paste();

    assert_eq!(
        state.user_profile_settings_field_value(
            crate::tui::state::UserProfileSettingsField::GlobalDisplayName,
        ),
        "abc"
    );
}

#[test]
fn profile_text_editing_uses_configured_composer_keys() {
    let mut state = state_with_keymap(KeymapOptions {
        composer: [
            ("Submit".to_owned(), KeymapBinding::one("<C-s>")),
            (
                "DeletePreviousWord".to_owned(),
                KeymapBinding::one("<A-backspace>"),
            ),
            ("MoveCursorLeft".to_owned(), KeymapBinding::one("<A-left>")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    handle_key(&mut state, key(KeyCode::Enter));
    for value in "hello world".chars() {
        handle_key(&mut state, char_key(value));
    }
    handle_key(&mut state, alt_key(KeyCode::Backspace));
    assert_eq!(
        state.user_profile_settings_field_value(
            crate::tui::state::UserProfileSettingsField::GlobalDisplayName,
        ),
        "hello "
    );

    handle_key(&mut state, char_key('X'));
    handle_key(&mut state, alt_key(KeyCode::Left));
    handle_key(&mut state, char_key('Y'));
    handle_key(&mut state, ctrl_key('s'));
    assert!(!state.is_user_profile_popup_editing());
    assert_eq!(
        state.user_profile_settings_field_value(
            crate::tui::state::UserProfileSettingsField::GlobalDisplayName,
        ),
        "hello YX"
    );
}

#[test]
fn profile_text_editing_moves_cursor_with_arrow_keys() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('b'));
    handle_key(&mut state, char_key('c'));
    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, char_key('X'));
    handle_key(&mut state, key(KeyCode::Right));
    handle_key(&mut state, key(KeyCode::Backspace));
    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        state.user_profile_settings_field_value(
            crate::tui::state::UserProfileSettingsField::GlobalDisplayName,
        ),
        "aXc"
    );
}
