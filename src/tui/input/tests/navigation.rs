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

    handle_key(&mut state, char_key('1'));
    assert!(state.is_pane_visible(FocusPane::Guilds));
    assert_eq!(state.focus(), FocusPane::Guilds);

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
    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
            ui_state: Default::default(),
        })
    );

    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, alt_key(KeyCode::Right));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
    assert_eq!(state.take_options_save_request(), None);
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

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus(), FocusPane::Guilds);

    handle_key(&mut state, key(KeyCode::BackTab));
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
