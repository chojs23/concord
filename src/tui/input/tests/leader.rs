use super::*;

#[test]
fn enter_toggles_selected_channel_category_and_space_opens_leader() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    assert_selected_channel_category_collapsed(&state, true);
}

#[test]
fn leader_channel_actions_offer_mute_duration_and_submit_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    let command = handle_key(&mut state, char_key('1'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "#general".to_owned(),
        })
    );
}

#[test]
fn leader_channel_actions_unmute_when_already_muted() {
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                channel_id: Id::new(11),
                message_notifications: None,
                muted: true,
                mute_end_time: None,
            }],
        }],
    });
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    let command = handle_key(&mut state, char_key('u'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            muted: false,
            duration: None,
            label: "#general".to_owned(),
        })
    );
}

#[test]
fn leader_category_actions_offer_mute_duration_and_submit_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Up));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    let command = handle_key(&mut state, char_key('1'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "Text Channels".to_owned(),
        })
    );
}

#[test]
fn leader_server_actions_unmute_when_already_muted() {
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: true,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }],
    });
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    let command = handle_key(&mut state, char_key('u'));

    assert_eq!(
        command,
        Some(AppCommand::SetGuildMuted {
            guild_id: Id::new(1),
            muted: false,
            duration: None,
            label: "guild".to_owned(),
        })
    );
}

#[test]
fn leader_o_opens_options_category_picker() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));

    assert!(!state.is_leader_active());
    assert!(state.is_options_category_picker_open());
    assert_eq!(state.options_popup_title(), "Options");
    assert_eq!(state.display_option_items()[0].label, "Display");
    assert_eq!(state.display_option_items()[1].label, "Notifications");
    assert_eq!(state.display_option_items()[2].label, "Voice");
}

#[test]
fn leader_v_opens_voice_actions() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('v'));

    assert!(state.is_leader_active());
    assert!(state.is_leader_action_mode());
    assert!(state.is_voice_leader_action_active());
    let actions = state.selected_voice_action_items();
    assert_eq!(actions[0].label, "Deafen voice");
    assert_eq!(actions[1].label, "Mute voice");
    assert_eq!(actions[2].label, "Leave voice");
    assert!(!actions[2].enabled);
}

#[test]
fn leader_o_category_shortcuts_open_scoped_options() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('d'));

    assert_eq!(state.options_popup_title(), "Display Options");
    assert_eq!(
        state.display_option_items()[0].label,
        "Disable all image previews"
    );
    assert!(
        !state
            .display_option_items()
            .iter()
            .any(|item| item.label == "Voice muted")
    );
    assert!(
        !state
            .display_option_items()
            .iter()
            .any(|item| item.label == "Desktop notifications")
    );

    state.close_options_popup();
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('n'));

    assert_eq!(state.options_popup_title(), "Notification Options");
    assert_eq!(
        state.display_option_items()[0].label,
        "Desktop notifications"
    );
    assert_eq!(state.display_option_items().len(), 1);

    state.close_options_popup();
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));

    assert_eq!(state.options_popup_title(), "Voice Options");
    assert_eq!(state.display_option_items()[0].label, "Voice muted");
    assert!(
        !state
            .display_option_items()
            .iter()
            .any(|item| item.label == "Show avatars")
    );
}

#[test]
fn leader_number_keys_toggle_side_panes() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());

    handle_key(&mut state, char_key('1'));
    assert!(!state.is_leader_active());
    assert!(!state.is_pane_visible(FocusPane::Guilds));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('2'));
    assert!(!state.is_pane_visible(FocusPane::Channels));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('4'));
    assert!(!state.is_pane_visible(FocusPane::Members));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('1'));
    assert!(state.is_pane_visible(FocusPane::Guilds));
}

#[test]
fn leader_esc_and_unknown_key_cancel_without_toggling_panes() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_leader_active());
    assert!(state.is_pane_visible(FocusPane::Guilds));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('x'));
    assert!(!state.is_leader_active());
    assert!(state.is_pane_visible(FocusPane::Channels));
}

#[test]
fn leader_leader_switcher_filters_and_opens_selected_channel() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    assert!(!state.is_leader_active());
    assert!(state.is_channel_switcher_open());

    for ch in "rand".chars() {
        handle_key(&mut state, char_key(ch));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert!(!state.is_channel_switcher_open());
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn leader_leader_switcher_expands_collapsed_parent_category() {
    let mut state = state_with_channel_tree();
    state.toggle_selected_channel_category();
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    for ch in "rand".chars() {
        handle_key(&mut state, char_key(ch));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_selected_channel_category_collapsed(&state, false);
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert!(matches!(
        state.channel_pane_entries().get(state.selected_channel()),
        Some(ChannelPaneEntry::Channel { state, .. }) if state.id == Id::new(12)
    ));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn leader_leader_switcher_opens_direct_message() {
    let mut state = state_with_direct_message("dm");

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeDirectMessage {
            channel_id: Id::new(20),
        })
    );
}

#[test]
fn leader_leader_switcher_j_and_k_type_into_search() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('k'));

    assert_eq!(state.channel_switcher_query(), Some("jk"));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn leader_leader_switcher_selection_aliases_move_selection() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn leader_leader_switcher_left_right_move_search_cursor() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    for ch in "raXndom".chars() {
        handle_key(&mut state, char_key(ch));
    }
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Left));
    }
    handle_key(&mut state, key(KeyCode::Right));
    handle_key(&mut state, key(KeyCode::Backspace));

    assert_eq!(state.channel_switcher_query(), Some("random"));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn mouse_input_closes_leader_hint() {
    let mut state = DashboardState::new();
    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());

    handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 1),
        dashboard_area(),
    );

    assert!(!state.is_leader_active());
}

#[test]
fn enter_opens_message_action_menu_and_space_opens_leader() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.is_message_action_menu_open());
    state.close_message_action_menu();

    handle_key(&mut state, char_key(' '));

    assert!(state.is_leader_active());
    assert!(!state.is_message_action_menu_open());
}

#[test]
fn leader_a_p_enters_pinned_message_view_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(command, None);
    assert!(state.is_pinned_message_view());
    assert!(!state.is_leader_active());
}

#[test]
fn leader_a_opens_selected_channel_actions_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_channel_leader_action_active());
}

#[test]
fn leader_channel_subphase_esc_returns_to_channel_actions() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('t'));
    assert!(state.is_channel_action_threads_phase());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_leader_action_mode());
    assert!(state.is_channel_leader_action_active());
    assert!(!state.is_channel_action_threads_phase());
}

#[test]
fn leader_guild_subphase_esc_returns_to_server_actions() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    assert!(state.is_guild_action_mute_duration_phase());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_leader_action_mode());
    assert!(state.is_guild_leader_action_active());
    assert!(!state.is_guild_action_mute_duration_phase());
}

#[test]
fn leader_a_opens_message_actions_from_message_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_message_action_menu_open());
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn leader_a_opens_server_actions_from_guild_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_guild_leader_action_active());
}

#[test]
fn leader_a_opens_member_actions_from_member_pane() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_member_leader_action_active());
    let actions = state.selected_member_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].label, "Show profile");
    assert!(actions[0].enabled);
}

#[test]
fn leader_a_p_opens_member_profile() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(
        command,
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(1),
            guild_id: Some(Id::new(1)),
        })
    );
    assert!(state.is_user_profile_popup_open());
    assert!(!state.is_leader_active());
}
