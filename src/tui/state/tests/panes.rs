use super::*;

#[test]
fn cycle_focus_uses_four_top_level_panes() {
    let mut state = DashboardState::new();

    assert_eq!(state.focus(), FocusPane::Guilds);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Channels);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Messages);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Members);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn loaded_messages_are_unselected_until_message_pane_is_focused() {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![text_channel_info(guild_id, channel_id, "general")],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in 1..=2u64 {
        state.push_event(message_create_event(MessageCreateFixture {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            content: Some(format!("msg {id}")),
            ..guild_message_create_fixture()
        }));
    }

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.focused_message_selection(), None);

    while state.focus() != FocusPane::Messages {
        state.cycle_focus();
    }
    assert_eq!(state.focused_message_selection(), Some(0));
}

#[test]
fn startup_events_do_not_auto_open_direct_messages() {
    let channel_id: Id<ChannelMarker> = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(30)),
        ..dm_channel_info(channel_id, "neo")
    }));
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(30),
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..guild_message_create_fixture()
    }));

    assert_eq!(state.selected_channel_id(), None);
    assert_eq!(state.selected_channel_state(), None);
    assert!(state.channel_pane_entries().is_empty());
    assert!(state.messages().is_empty());
}

#[test]
fn focused_pane_horizontal_scroll_is_scoped_by_focus() {
    let mut state = state_with_many_channels(1);

    state.scroll_focused_pane_horizontal_right();
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 0);
    assert_eq!(state.member_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Channels);
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 1);

    state.focus_pane(FocusPane::Members);
    state.scroll_focused_pane_horizontal_right();
    state.scroll_focused_pane_horizontal_left();
    state.scroll_focused_pane_horizontal_left();
    assert_eq!(state.member_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Messages);
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 1);
    assert_eq!(state.member_horizontal_scroll(), 0);
}

#[test]
fn focused_pane_horizontal_scroll_stops_before_blank_labels() {
    let mut state = DashboardState::new();

    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(
        state.guild_horizontal_scroll(),
        "Direct Messages".width() - 1
    );

    let mut state = state_with_many_channels(1);
    state.focus_pane(FocusPane::Channels);
    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(state.channel_horizontal_scroll(), "channel 1".width() - 1);

    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(state.member_horizontal_scroll(), "member 1".width() - 1);
}

#[test]
fn guild_scroll_uses_scrolloff() {
    let mut state = state_with_many_guilds(8);
    state.focus_pane(FocusPane::Guilds);
    state.set_guild_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_guild(), 8);
    assert_eq!(state.guild_scroll(), 2);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_guild(), 6);
    assert_eq!(state.guild_scroll(), 2);

    state.move_up();
    assert_eq!(state.selected_guild(), 5);
    assert_eq!(state.guild_scroll(), 2);
}

#[test]
fn channel_scroll_uses_scrolloff() {
    let mut state = state_with_many_channels(8);
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_channel(), 7);
    assert_eq!(state.channel_scroll(), 1);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_channel(), 5);
    assert_eq!(state.channel_scroll(), 1);

    state.move_up();
    assert_eq!(state.selected_channel(), 4);
    assert_eq!(state.channel_scroll(), 1);
}

#[test]
fn member_scroll_uses_scrolloff() {
    let mut state = state_with_members(8);
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_member(), 7);
    assert_eq!(state.member_scroll(), 2);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_member(), 5);
    assert_eq!(state.member_scroll(), 2);

    state.move_up();
    assert_eq!(state.selected_member(), 4);
    assert_eq!(state.member_scroll(), 2);
}

#[test]
fn half_page_scrolls_all_list_panes() {
    let mut guild_state = state_with_many_guilds(8);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(9);
    guild_state.half_page_down();
    assert_eq!(guild_state.selected_guild(), 5);

    let mut channel_state = state_with_many_channels(8);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(9);
    channel_state.half_page_down();
    assert_eq!(channel_state.selected_channel(), 4);

    let mut member_state = state_with_members(8);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(9);
    member_state.half_page_down();
    assert_eq!(member_state.selected_member(), 4);
}

#[test]
fn channel_tree_groups_category_children() {
    let state = state_with_channel_tree();
    let entries = state.channel_pane_entries();

    assert!(matches!(
        &entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: false,
            ..
        }
    ));
    assert!(matches!(
        &entries[1],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Middle,
            ..
        }
    ));
    assert!(matches!(
        &entries[2],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Last,
            ..
        }
    ));
}

#[test]
fn channel_tree_shows_joined_threads_under_parent_channel() {
    let guild_id = Id::new(1);
    let parent_id = Id::new(11);
    let joined_thread_id = Id::new(30);
    let hidden_thread_id = Id::new(31);
    let mut state = state_with_channel_tree();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, parent_id, joined_thread_id, "joined thread")
    }));
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(false),
        ..thread_channel_info(guild_id, parent_id, hidden_thread_id, "hidden thread")
    }));

    let entries = state.channel_pane_entries();

    assert!(matches!(
        &entries[2],
        ChannelPaneEntry::Thread {
            state,
            parent_branch: ChannelBranch::Middle,
            branch: ChannelBranch::Last,
        } if state.id == joined_thread_id
    ));
    assert_eq!(
        channel_entry_names(&state),
        vec!["general", "joined thread", "random"]
    );
}

#[test]
fn channel_tree_removes_thread_after_current_user_leaves() {
    let guild_id = Id::new(1);
    let parent_id = Id::new(11);
    let thread_id = Id::new(30);
    let current_user_id = Id::new(99);
    let mut state = state_with_channel_tree();

    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, parent_id, thread_id, "joined thread")
    }));
    assert_eq!(
        channel_entry_names(&state),
        vec!["general", "joined thread", "random"]
    );

    state.push_event(AppEvent::ThreadMembersUpdate {
        channel_id: thread_id,
        added_user_ids: Vec::new(),
        removed_user_ids: vec![current_user_id],
    });

    assert_eq!(channel_entry_names(&state), vec!["general", "random"]);
}

#[test]
fn selected_channel_category_toggles_open_and_closed() {
    let mut state = state_with_channel_tree();

    assert_eq!(state.channel_pane_entries().len(), 3);
    assert_eq!(state.selected_channel_id(), None);

    state.toggle_selected_channel_category();
    let closed_entries = state.channel_pane_entries();
    assert_eq!(closed_entries.len(), 1);
    assert!(matches!(
        &closed_entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: true,
            ..
        }
    ));

    state.toggle_selected_channel_category();
    assert_eq!(state.channel_pane_entries().len(), 3);
}

#[test]
fn selected_channel_child_can_close_parent_category() {
    let mut state = state_with_channel_tree();
    state.navigation.selected_channel = 1;

    state.toggle_selected_channel_category();
    let entries = state.channel_pane_entries();
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: true,
            ..
        }
    ));
}

#[test]
fn collapsed_category_keeps_unread_child_visible_until_another_channel_is_selected() {
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(Id::new(11), Some(Id::new(99)), 0)],
    });
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(11),
        message_id: Id::new(100),
        author_id: Id::new(20),
        author: "alice".to_owned(),
        content: Some("unread".to_owned()),
        ..guild_message_create_fixture()
    }));

    state.toggle_selected_channel_category();
    assert_eq!(channel_entry_names(&state), vec!["general"]);

    state.activate_channel(Id::new(11));
    let commands = state.drain_pending_commands();
    apply_optimistic_ack_commands(&mut state, &commands);
    assert_eq!(channel_entry_names(&state), vec!["general"]);

    state.activate_channel(Id::new(12));
    assert_eq!(channel_entry_names(&state), vec!["random"]);
}

#[test]
fn collapsed_category_state_is_saved_and_restored() {
    let mut state = state_with_channel_tree();
    state.toggle_selected_channel_category();

    let ui_state = state
        .take_ui_state_save_request()
        .expect("collapse should request a UI state save");
    let restored = DashboardState::new_with_options(
        DisplayOptions::default(),
        Default::default(),
        Default::default(),
        NotificationOptions::default(),
        VoiceOptions::default(),
        Default::default(),
        ui_state,
    );

    assert!(
        restored
            .navigation
            .collapsed_channel_categories
            .contains(&Id::new(10))
    );
}

#[test]
fn moving_guild_cursor_does_not_activate_guild() {
    let mut state = state_with_two_guilds();
    state.focus_pane(FocusPane::Guilds);

    state.confirm_selected_guild();
    let active_guild = state.selected_guild_id();
    assert!(active_guild.is_some());

    state.move_down();
    assert_eq!(state.navigation.selected_guild, 2);
    assert_eq!(state.selected_guild_id(), active_guild);

    state.confirm_selected_guild();
    assert_ne!(state.selected_guild_id(), active_guild);
}

#[test]
fn active_guild_entry_tracks_confirmed_guild() {
    let mut state = state_with_two_guilds();
    state.focus_pane(FocusPane::Guilds);

    {
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[0]));
        assert!(!state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.confirm_selected_guild();
    {
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[0]));
        assert!(state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.move_down();
    {
        let entries = state.guild_pane_entries();
        assert!(state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.confirm_selected_guild();
    let entries = state.guild_pane_entries();
    assert!(!state.is_active_guild_entry(&entries[1]));
    assert!(state.is_active_guild_entry(&entries[2]));
}

#[test]
fn moving_channel_cursor_does_not_activate_channel() {
    let mut state = state_with_channel_tree();
    let random_id = Id::new(12);
    state.focus_pane(FocusPane::Channels);

    assert_eq!(state.selected_channel_id(), None);

    state.move_down();
    state.move_down();
    assert_eq!(state.navigation.selected_channel, 2);
    assert_eq!(state.selected_channel_id(), None);

    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(random_id));
}

#[test]
fn active_channel_entry_tracks_confirmed_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    {
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[0]));
        assert!(!state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.move_down();
    state.confirm_selected_channel();
    {
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[0]));
        assert!(state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.move_down();
    {
        let entries = state.channel_pane_entries();
        assert!(state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.confirm_selected_channel();
    let entries = state.channel_pane_entries();
    assert!(!state.is_active_channel_entry(&entries[1]));
    assert!(state.is_active_channel_entry(&entries[2]));
}

#[test]
fn selected_folder_toggles_open_and_closed() {
    let mut state = state_with_folder(Some(42));

    assert_eq!(state.guild_pane_entries().len(), 4);
    state.toggle_selected_folder();
    let closed_entries = state.guild_pane_entries();
    assert_eq!(closed_entries.len(), 2);
    assert!(matches!(
        closed_entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));

    state.toggle_selected_folder();
    let open_entries = state.guild_pane_entries();
    assert_eq!(open_entries.len(), 4);
    assert!(matches!(
        open_entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: false,
            ..
        }
    ));
}

#[test]
fn folder_children_use_middle_and_last_branches() {
    let state = state_with_folder(Some(42));

    let entries = state.guild_pane_entries();
    assert!(matches!(
        entries[2],
        GuildPaneEntry::Guild {
            branch: GuildBranch::Middle,
            ..
        }
    ));
    assert!(matches!(
        entries[3],
        GuildPaneEntry::Guild {
            branch: GuildBranch::Last,
            ..
        }
    ));
}

#[test]
fn folder_without_id_can_be_toggled_closed() {
    let mut state = state_with_folder(None);

    state.toggle_selected_folder();
    let entries = state.guild_pane_entries();
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));
}

#[test]
fn selected_folder_child_can_close_parent() {
    let mut state = state_with_folder(Some(42));
    state.navigation.selected_guild = 2;

    state.toggle_selected_folder();
    let entries = state.guild_pane_entries();
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));
}

#[test]
fn collapsed_server_folder_state_is_saved() {
    let mut state = state_with_folder(Some(42));

    state.toggle_selected_folder();

    let ui_state = state
        .take_ui_state_save_request()
        .expect("folder collapse should request a UI state save");
    assert_eq!(ui_state.collapsed_server_folder_ids, vec![42]);
}
