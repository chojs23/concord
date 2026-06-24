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
    let mut state = state_with_messages(2);

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
    state.push_event(message_create_event(
        MessageCreateFixture::direct_message(channel_id, Id::new(30)).with_content("hello"),
    ));

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
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::ChannelUpsert(category_channel_info(
        Id::new(1),
        Id::new(13),
        "Empty Category",
        2,
    )));

    let entries = state.channel_pane_entries();

    assert_eq!(entries.len(), 3);
    assert!(matches!(
        &entries[0],
        ChannelPaneEntry::CategoryHeader {
            state,
            collapsed: false,
            ..
        } if state.id == Id::new(10)
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
fn channel_tree_keeps_empty_categories_for_channel_managers() {
    let current_user_id = Id::new(99);
    let manager_role_id = Id::new(50);
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.push_event(AppEvent::GuildRoleUpsert {
        guild_id: Id::new(1),
        role: role_info(
            manager_role_id,
            "Manager",
            PERM_VIEW_CHANNEL | PERM_MANAGE_CHANNELS,
        ),
    });
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: member_with_roles(current_user_id, "me", vec![manager_role_id]),
    });
    state.push_event(AppEvent::ChannelUpsert(category_channel_info(
        Id::new(1),
        Id::new(13),
        "Empty Category",
        2,
    )));

    let entries = state.channel_pane_entries();

    assert_eq!(entries.len(), 4);
    assert!(matches!(
        &entries[3],
        ChannelPaneEntry::CategoryHeader {
            state,
            collapsed: false,
            ..
        } if state.id == Id::new(13)
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
fn channel_tree_hides_archived_joined_threads() {
    let guild_id = Id::new(1);
    let parent_id = Id::new(11);
    let active_thread_id = Id::new(30);
    let archived_thread_id = Id::new(31);
    let mut state = state_with_channel_tree();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, parent_id, active_thread_id, "active thread")
    }));
    // Joined but archived: the `/threads/search` fetch for the thread-list view
    // caches these, but they must stay out of the sidebar.
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(true, false)),
        ..thread_channel_info(guild_id, parent_id, archived_thread_id, "archived thread")
    }));

    assert_eq!(
        channel_entry_names(&state),
        vec!["general", "active thread", "random"]
    );
}

#[test]
fn channel_thread_list_view_lists_joined_threads_as_cards() {
    use crate::tui::state::MessagePaneSource;

    let guild_id = Id::new(1);
    let parent_id = Id::new(11);
    let joined_thread_id = Id::new(30);
    let mut state = state_with_channel_tree();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, parent_id, joined_thread_id, "joined thread")
    }));

    state.activate_channel(parent_id);
    state.enter_channel_thread_list_view(parent_id);

    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads {
            channel_id: parent_id
        })
    );
    // Before the `/threads/search` fetch lands, the gateway-cached joined thread
    // is shown straight away.
    let cards = state.selected_thread_card_items();
    assert_eq!(
        cards.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        vec!["joined thread"]
    );
    assert_eq!(state.message_pane_title(), "Threads · #general");
}

#[test]
fn is_forum_post_thread_distinguishes_posts_from_regular_threads() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let text_id = Id::new(11); // `general` text channel in the standard tree
    let post_id = Id::new(30);
    let thread_id = Id::new(31);
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::ChannelUpsert(forum_channel_info(
        guild_id, forum_id,
    )));
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..forum_thread_info(
            guild_id,
            forum_id,
            post_id.get(),
            "a post",
            Some(300),
            false,
        )
    }));
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, text_id, thread_id, "a thread")
    }));

    assert!(state.is_forum_post_thread(post_id));
    assert!(!state.is_forum_post_thread(thread_id));
    assert!(!state.is_forum_post_thread(text_id));
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

    state.push_event(thread_members_update_event(
        thread_id,
        Vec::new(),
        vec![current_user_id],
    ));

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
    state.navigation.channels.list.selected = 1;

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
            .channels
            .collapsed_channel_categories
            .contains(&Id::new(10))
    );
}

#[test]
fn pane_layout_state_is_saved_and_restored() {
    let mut state = DashboardState::new();
    state.toggle_pane_visibility(FocusPane::Guilds);
    state.toggle_pane_visibility(FocusPane::Members);
    state.focus_pane(FocusPane::Channels);
    state.adjust_focused_pane_width(6);

    let ui_state = state
        .take_ui_state_save_request()
        .expect("pane layout changes should request a UI state save");
    let restored = DashboardState::new_with_options(
        DisplayOptions::default(),
        Default::default(),
        Default::default(),
        NotificationOptions::default(),
        VoiceOptions::default(),
        Default::default(),
        ui_state,
    );

    assert!(!restored.is_pane_visible(FocusPane::Guilds));
    assert!(restored.is_pane_visible(FocusPane::Channels));
    assert!(!restored.is_pane_visible(FocusPane::Members));
    assert_eq!(restored.pane_width(FocusPane::Channels), 30);
    assert_eq!(restored.focus(), FocusPane::Messages);
}

#[test]
fn moving_guild_cursor_does_not_activate_guild() {
    let mut state = state_with_two_guilds();
    state.focus_pane(FocusPane::Guilds);

    state.confirm_selected_guild();
    let active_guild = state.selected_guild_id();
    assert!(active_guild.is_some());

    state.move_down();
    assert_eq!(state.navigation.guilds.list.selected, 2);
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
fn guild_folder_update_reorders_sidebar_entries() {
    let mut state = state_with_two_guilds();

    state.push_event(AppEvent::UserSettingsUpdate {
        settings: UserSettingsInfo {
            guild_folders: Some(vec![
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![Id::new(2)],
                },
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![Id::new(1)],
                },
            ]),
            ..UserSettingsInfo::default()
        },
    });

    let entries = state.guild_pane_entries();
    assert!(matches!(
        entries[1],
        GuildPaneEntry::Guild { state, .. } if state.id == Id::new(2)
    ));
    assert!(matches!(
        entries[2],
        GuildPaneEntry::Guild { state, .. } if state.id == Id::new(1)
    ));
}

#[test]
fn moving_channel_cursor_does_not_activate_channel() {
    let mut state = state_with_channel_tree();
    let random_id = Id::new(12);
    state.focus_pane(FocusPane::Channels);

    assert_eq!(state.selected_channel_id(), None);

    state.move_down();
    state.move_down();
    assert_eq!(state.navigation.channels.list.selected, 2);
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
    state.navigation.guilds.list.selected = 2;

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
