use super::*;

#[test]
fn enter_toggles_selected_folder_and_focuses_channels_after_server_selection() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_folder_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    assert_selected_folder_collapsed(&state, true);

    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.focus(), FocusPane::Channels);
}

#[test]
fn channel_filter_opens_child_inside_collapsed_category() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, char_key('/'));
    for value in "random".chars() {
        handle_key(&mut state, char_key(value));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert_eq!(state.selected_channel_id(), None);
    assert_eq!(state.selected_channel(), 0);
    assert_eq!(state.channel_pane_filter_query(), Some("random"));
    assert_selected_channel_category_collapsed(&state, true);

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(state.selected_channel(), 0);
    assert_eq!(state.channel_pane_filter_query(), Some("random"));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.channel_pane_filter_query(), None);
}

#[test]
fn guild_filter_opens_child_inside_collapsed_folder() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_folder_collapsed(&state, true);

    handle_key(&mut state, char_key('/'));
    for value in "second".chars() {
        handle_key(&mut state, char_key(value));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_guild_id(), None);
    assert_eq!(state.selected_guild(), 0);
    assert_eq!(state.guild_pane_filter_query(), Some("second"));
    assert_selected_folder_collapsed(&state, true);

    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_guild_id(), Some(Id::new(2)));
    assert_eq!(state.selected_guild(), 0);
    assert_eq!(state.guild_pane_filter_query(), Some("second"));
    assert_eq!(state.focus(), FocusPane::Channels);
    assert_selected_folder_collapsed(&state, true);

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.guild_pane_filter_query(), None);
}

#[test]
fn movement_waits_for_enter_to_activate_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    assert_eq!(state.selected_channel_id(), None);

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_channel_id(), None);

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(11)));
    assert_eq!(state.focus(), FocusPane::Messages);

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn number_keys_focus_top_level_panes() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('2'));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, char_key('3'));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, char_key('4'));
    assert_eq!(state.focus(), FocusPane::Members);

    handle_key(&mut state, char_key('1'));
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn number_keys_show_hidden_panes_before_focusing() {
    let mut state = DashboardState::new();
    state.toggle_pane_visibility(FocusPane::Guilds);
    state.toggle_pane_visibility(FocusPane::Channels);
    state.toggle_pane_visibility(FocusPane::Members);
    state.take_ui_state_save_request();

    handle_key(&mut state, char_key('1'));
    assert!(state.is_pane_visible(FocusPane::Guilds));
    assert_eq!(state.focus(), FocusPane::Guilds);
    assert!(state.take_ui_state_save_request().is_some());

    handle_key(&mut state, char_key('2'));
    assert!(state.is_pane_visible(FocusPane::Channels));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, char_key('4'));
    assert!(state.is_pane_visible(FocusPane::Members));
    assert_eq!(state.focus(), FocusPane::Members);
}

#[test]
fn bare_m_no_longer_mutes_focused_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    let command = handle_key(&mut state, char_key('m'));

    assert_eq!(command, None);
}

#[test]
fn alt_arrows_adjust_focused_side_pane_width() {
    let mut state = DashboardState::new();

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, alt_key(KeyCode::Right));
    assert_eq!(state.pane_width(FocusPane::Channels), 25);

    handle_key(&mut state, alt_key(KeyCode::Left));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
    assert_eq!(state.take_options_save_request(), None);
    let ui_state = state
        .take_ui_state_save_request()
        .expect("pane resize should request a UI state save");
    assert_eq!(ui_state.channel_list_width, 24);

    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, alt_key(KeyCode::Right));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
    assert_eq!(state.take_options_save_request(), None);
    assert_eq!(state.take_ui_state_save_request(), None);
}

#[test]
fn alt_h_l_adjust_focused_side_pane_width() {
    let mut state = DashboardState::new();

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, alt_key(KeyCode::Char('l')));
    assert_eq!(state.pane_width(FocusPane::Channels), 25);

    handle_key(&mut state, alt_key(KeyCode::Char('h')));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
}

#[test]
fn tab_cycles_skip_hidden_panes() {
    let mut state = DashboardState::new();
    state.toggle_pane_visibility(FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Messages);

    state.toggle_pane_visibility(FocusPane::Members);
    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn tab_and_shift_tab_cycle_focus() {
    let mut state = DashboardState::new();
    let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, shift_tab);
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, shift_tab);
    assert_eq!(state.focus(), FocusPane::Guilds);

    handle_key(&mut state, shift_tab);
    assert_eq!(state.focus(), FocusPane::Members);
}

#[test]
fn pane_filters_treat_vim_keys_as_text() {
    let mut guild_state = state_with_folder();
    guild_state.focus_pane(FocusPane::Guilds);
    handle_key(&mut guild_state, char_key('/'));

    handle_key(&mut guild_state, char_key('j'));
    handle_key(&mut guild_state, char_key('k'));

    assert_eq!(guild_state.guild_pane_filter_query(), Some("jk"));

    let mut guild_state = state_with_folder();
    guild_state.focus_pane(FocusPane::Guilds);
    handle_key(&mut guild_state, char_key('/'));
    handle_key(&mut guild_state, char_key('s'));
    handle_key(&mut guild_state, key(KeyCode::Enter));

    assert_eq!(guild_state.guild_pane_filter_query(), Some("s"));
    assert_eq!(guild_state.selected_guild(), 0);

    handle_key(&mut guild_state, char_key('j'));
    assert_eq!(guild_state.guild_pane_filter_query(), Some("s"));
    assert_eq!(guild_state.selected_guild(), 1);

    handle_key(&mut guild_state, char_key('k'));
    assert_eq!(guild_state.guild_pane_filter_query(), Some("s"));
    assert_eq!(guild_state.selected_guild(), 0);

    let mut channel_state = state_with_channel_tree();
    channel_state.focus_pane(FocusPane::Channels);
    handle_key(&mut channel_state, char_key('/'));

    handle_key(&mut channel_state, char_key('j'));
    handle_key(&mut channel_state, char_key('k'));

    assert_eq!(channel_state.channel_pane_filter_query(), Some("jk"));

    let mut channel_state = state_with_channel_tree();
    channel_state.focus_pane(FocusPane::Channels);
    handle_key(&mut channel_state, char_key('/'));
    handle_key(&mut channel_state, char_key('a'));
    handle_key(&mut channel_state, key(KeyCode::Enter));

    assert_eq!(channel_state.channel_pane_filter_query(), Some("a"));
    assert_eq!(channel_state.selected_channel(), 0);

    handle_key(&mut channel_state, char_key('j'));
    assert_eq!(channel_state.channel_pane_filter_query(), Some("a"));
    assert_eq!(channel_state.selected_channel(), 1);

    handle_key(&mut channel_state, char_key('k'));
    assert_eq!(channel_state.channel_pane_filter_query(), Some("a"));
    assert_eq!(channel_state.selected_channel(), 0);

    let mut channel_state = state_with_channel_tree();
    channel_state.focus_pane(FocusPane::Channels);
    handle_key(&mut channel_state, char_key('/'));
    handle_key(&mut channel_state, key(KeyCode::Enter));

    assert_eq!(channel_state.channel_pane_filter_query(), Some(""));
    assert_eq!(channel_state.selected_channel(), 0);

    handle_key(&mut channel_state, char_key('j'));
    assert_eq!(channel_state.channel_pane_filter_query(), Some(""));
    assert_eq!(channel_state.selected_channel(), 1);
}

#[test]
fn navigation_selection_ignores_modified_j_and_k() {
    let mut state = state_with_messages(1);
    state.open_options_popup();

    handle_key(&mut state, ctrl_key('j'));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, char_key('j'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, ctrl_key('k'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, char_key('k'));
    assert_eq!(state.selected_option_index(), Some(0));
}

#[test]
fn navigation_selection_uses_configured_row_movement_keys() {
    let mut state = state_with_keymap(KeymapOptions {
        mappings: [
            ("SelectNext".to_owned(), KeymapBinding::one("n")),
            ("SelectPrevious".to_owned(), KeymapBinding::one("p")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    state.open_options_popup();

    handle_key(&mut state, char_key('j'));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, char_key('n'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, char_key('p'));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_option_index(), Some(0));
}

#[test]
fn uppercase_h_l_scroll_focused_side_panes_horizontally() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key('L'));
    assert_eq!(state.guild_horizontal_scroll(), 1);

    handle_key(&mut state, char_key('H'));
    handle_key(&mut state, char_key('H'));
    assert_eq!(state.guild_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.channel_horizontal_scroll(), 1);

    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.member_horizontal_scroll(), 1);

    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.member_horizontal_scroll(), 1);
}

#[test]
fn uppercase_j_k_scroll_focused_pane_viewport_without_moving_selection() {
    let mut guild_state = DashboardState::new();
    for id in 1..=8 {
        guild_state.push_event(AppEvent::GuildCreate {
            guild_id: Id::new(id),
            name: format!("guild {id}"),
            member_count: None,
            owner_id: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
    }
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(3);
    let selected_guild = guild_state.selected_guild();

    handle_key(&mut guild_state, char_key('J'));
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), 1);
    handle_key(&mut guild_state, char_key('K'));
    assert_eq!(guild_state.guild_scroll(), 0);

    let mut channel_state = DashboardState::new();
    channel_state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: (1..=8)
            .map(|id| ChannelInfo {
                guild_id: Some(Id::new(1)),
                position: Some(id as i32),
                name: format!("c{id}"),
                ..ChannelInfo::test(Id::new(10 + id), "GuildText")
            })
            .collect(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    channel_state.confirm_selected_guild();
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(3);
    let selected_channel = channel_state.selected_channel();

    handle_key(&mut channel_state, char_key('J'));
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), 1);

    let mut member_state = state_with_members(8);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(3);
    let selected_member = member_state.selected_member();

    handle_key(&mut member_state, char_key('J'));
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), 1);
}

#[test]
fn viewport_scroll_uses_configured_keys_in_side_panes() {
    let mut state = state_with_keymap(KeymapOptions {
        mappings: [
            ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
            ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: (0..8)
            .map(|index| ChannelInfo {
                guild_id: Some(Id::new(1)),
                position: Some(index as i32),
                name: format!("c{index}"),
                ..ChannelInfo::test(Id::new(10 + index), "GuildText")
            })
            .collect(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(3);

    handle_key(&mut state, char_key('J'));
    assert_eq!(state.channel_scroll(), 0);
    handle_key(&mut state, char_key('N'));
    assert_eq!(state.channel_scroll(), 1);
    handle_key(&mut state, char_key('P'));
    assert_eq!(state.channel_scroll(), 0);
}

#[test]
fn enter_opens_member_actions_from_member_pane() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(state.is_member_leader_action_active());
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));
}

#[test]
fn h_l_and_left_right_move_focus_without_toggling_tree_nodes() {
    let mut guild_state = state_with_folder();
    guild_state.focus_pane(FocusPane::Guilds);

    handle_key(&mut guild_state, char_key('h'));
    assert_eq!(guild_state.focus(), FocusPane::Members);
    assert_selected_folder_collapsed(&guild_state, false);

    handle_key(&mut guild_state, char_key('l'));
    assert_eq!(guild_state.focus(), FocusPane::Guilds);
    assert_selected_folder_collapsed(&guild_state, false);

    handle_key(&mut guild_state, key(KeyCode::Left));
    assert_eq!(guild_state.focus(), FocusPane::Members);
    assert_selected_folder_collapsed(&guild_state, false);

    handle_key(&mut guild_state, key(KeyCode::Right));
    assert_eq!(guild_state.focus(), FocusPane::Guilds);
    assert_selected_folder_collapsed(&guild_state, false);

    let mut channel_state = state_with_channel_tree();
    channel_state.focus_pane(FocusPane::Channels);

    handle_key(&mut channel_state, char_key('l'));
    assert_eq!(channel_state.focus(), FocusPane::Messages);
    assert_selected_channel_category_collapsed(&channel_state, false);

    handle_key(&mut channel_state, char_key('h'));
    assert_eq!(channel_state.focus(), FocusPane::Channels);
    assert_selected_channel_category_collapsed(&channel_state, false);

    handle_key(&mut channel_state, key(KeyCode::Left));
    assert_eq!(channel_state.focus(), FocusPane::Guilds);
    assert_selected_channel_category_collapsed(&channel_state, false);

    handle_key(&mut channel_state, key(KeyCode::Right));
    assert_eq!(channel_state.focus(), FocusPane::Channels);
    assert_selected_channel_category_collapsed(&channel_state, false);
}
