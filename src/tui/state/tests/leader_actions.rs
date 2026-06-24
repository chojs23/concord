use super::*;
use crate::discord::AppCommand;

#[test]
fn leader_message_action_copy_closes_action_popup() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.open_leader_actions_for_focused_target();

    assert!(state.is_leader_action_mode());
    assert!(state.is_message_action_context_active());

    let command = state.activate_message_action_kind(MessageActionKind::CopyContent);

    assert_eq!(command, None);
    assert!(!state.is_leader_active());
    assert!(!state.is_message_action_context_active());
    assert_eq!(
        state.take_copy_text_request(),
        Some(("msg 1".to_owned(), "Message copied"))
    );
}

#[test]
fn channel_leader_action_show_threads_opens_thread_list_view() {
    use crate::tui::state::MessagePaneSource;

    let parent_id = Id::new(2);
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    assert!(state.is_channel_leader_action_active());
    let actions = state.selected_channel_action_items();
    assert_eq!(actions.len(), 6);
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert_eq!(actions[0].label, "Join voice");
    assert!(!actions[0].enabled);
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    assert_eq!(actions[1].label, "Leave voice");
    assert!(!actions[1].enabled);
    assert_eq!(actions[2].kind, ChannelActionKind::LoadPinnedMessages);
    assert_eq!(actions[2].label, "Show pinned messages");
    assert!(actions[2].enabled);
    assert_eq!(actions[3].kind, ChannelActionKind::ShowThreads);
    assert!(actions[3].enabled);
    assert_eq!(actions[4].kind, ChannelActionKind::MarkAsRead);
    assert_eq!(actions[4].label, "Mark as read");
    assert_eq!(actions[5].kind, ChannelActionKind::ToggleMute);
    assert_eq!(actions[5].label, "Mute channel");

    // "Show threads" opens the thread-list view in the message pane, not a submenu.
    let command = state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert_eq!(command, None);
    assert!(!state.is_channel_leader_action_active());
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads {
            channel_id: parent_id
        })
    );

    // The gateway-cached child thread shows immediately, before the
    // `/threads/search` fetch for the channel completes.
    let cards = state.selected_thread_card_items();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].channel_id, Id::new(10));
    assert_eq!(cards[0].label, "release notes");
}

#[test]
fn channel_thread_list_view_fetches_and_sections_active_and_archived_threads() {
    use crate::discord::ForumPostArchiveState;
    use crate::tui::state::MessagePaneSource;

    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = state_with_messages(1);

    // The action is offered even with no threads cached: opening the view is what
    // triggers the fetch that fills the list.
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let show_threads = state
        .selected_channel_action_items()
        .into_iter()
        .find(|action| action.kind == ChannelActionKind::ShowThreads)
        .expect("show threads action is present");
    assert!(show_threads.enabled);

    assert_eq!(
        state.activate_channel_action_shortcut("t".parse().expect("t parses")),
        None
    );
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads { channel_id })
    );
    // The open view is now the fetch target, so the scheduler issues the
    // `/threads/search` request for this non-forum channel.
    assert_eq!(
        state
            .selected_forum_channel_with_load_more()
            .map(|(guild, channel, _)| (guild, channel)),
        Some((guild_id, channel_id))
    );

    for (archive_state, thread_id, name, archived) in [
        (ForumPostArchiveState::Active, 30, "active thread", false),
        (ForumPostArchiveState::Archived, 31, "archived thread", true),
    ] {
        state.push_event(AppEvent::ForumPostsLoaded {
            channel_id,
            archive_state,
            offset: 0,
            next_offset: 1,
            threads: vec![forum_thread_info(
                guild_id, channel_id, thread_id, name, None, archived,
            )],
            first_messages: Vec::new(),
            has_more: false,
        });
    }

    let cards = state.selected_thread_card_items();
    assert_eq!(
        cards
            .iter()
            .map(|card| (card.label.as_str(), card.section_label.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("active thread", Some("Active threads")),
            ("archived thread", Some("Archived threads")),
        ]
    );
}

#[test]
fn show_threads_opens_a_highlighted_but_unopened_channel() {
    use crate::tui::state::MessagePaneSource;

    let guild_id: Id<GuildMarker> = Id::new(1);
    let opened: Id<ChannelMarker> = Id::new(2);
    let highlighted: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                ..text_channel_info(guild_id, opened, "opened")
            },
            ChannelInfo {
                position: Some(1),
                ..text_channel_info(guild_id, highlighted, "highlighted")
            },
        ],
    ));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.activate_channel(opened);

    // Highlight the second channel in the pane without opening it.
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();

    assert_eq!(
        state.activate_channel_action_shortcut("t".parse().expect("t parses")),
        None
    );

    // Show threads makes the highlighted channel active and switches the message
    // pane to its thread list, rather than silently staying on `opened`.
    assert_eq!(state.selected_channel_id(), Some(highlighted));
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads {
            channel_id: highlighted
        })
    );
}

#[test]
fn mark_as_read_action_enablement_is_scoped_to_action_channel() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let unread_channel: Id<ChannelMarker> = Id::new(2);
    let read_channel: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                ..text_channel_info(guild_id, unread_channel, "unread")
            },
            ChannelInfo {
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                ..text_channel_info(guild_id, read_channel, "read")
            },
        ],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(unread_channel, Some(Id::new(10)), 0),
            read_state_info(read_channel, Some(Id::new(30)), 0),
        ],
    });
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.activate_channel(unread_channel);
    assert_eq!(state.unread_divider_last_acked_id(), Some(Id::new(10)));

    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    let mark_as_read = actions
        .iter()
        .find(|action| action.kind == ChannelActionKind::MarkAsRead)
        .expect("channel actions include Mark as read");
    assert!(!mark_as_read.enabled);
}

#[test]
fn channel_thread_list_card_opens_thread_and_subscribes() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert!(state.is_channel_thread_list_view());

    let command = state.activate_selected_message_pane_item();

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
    assert!(!state.is_channel_thread_list_view());
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
        })
    );
}

#[test]
fn channel_thread_list_view_esc_restores_previous_channel_view() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert!(state.is_channel_thread_list_view());

    assert!(state.return_from_channel_thread_list_view());
    assert!(!state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(crate::tui::state::MessagePaneSource::ChannelMessages {
            channel_id: Id::new(2)
        })
    );
}

#[test]
fn guild_leader_action_lists_disabled_mark_server_read_when_guild_is_read() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    assert!(state.is_guild_leader_action_active());
    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert_eq!(actions[0].label, "Mark server as read");
    assert!(!actions[0].enabled);
    assert_eq!(actions[1].kind, GuildActionKind::ToggleMute);
    assert_eq!(actions[1].label, "Mute server");
    assert_eq!(actions[2].kind, GuildActionKind::LeaveServer);
    assert_eq!(actions[2].label, "Leave server");
    assert_eq!(state.activate_selected_guild_action(), None);
}

#[test]
fn folder_leader_action_opens_settings() {
    let mut state = state_with_folder(Some(42));
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, GuildActionKind::FolderSettings);
    assert_eq!(actions[0].label, "Folder settings");
    assert!(actions[0].enabled);

    assert_eq!(state.activate_selected_guild_action(), None);
    assert!(state.is_folder_settings_open());
    assert_eq!(state.folder_settings_name_value(), Some("folder"));
    assert_eq!(state.folder_settings_color_value(), Some(""));
}

#[test]
fn channel_leader_action_toggle_mute_opens_duration_then_dispatches_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();
    state.select_channel_action_row(5);

    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_action_mute_duration_phase());

    let command = state.activate_selected_channel_action();

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
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn category_leader_action_lists_disabled_rows_and_dispatches_mute_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_up();
    state.open_selected_channel_actions();

    assert!(state.is_channel_leader_action_active());
    let actions = state.selected_channel_action_items();
    assert_eq!(actions.len(), 6);
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].enabled);
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    assert!(!actions[1].enabled);
    assert_eq!(actions[2].kind, ChannelActionKind::LoadPinnedMessages);
    assert!(!actions[2].enabled);
    assert_eq!(actions[3].kind, ChannelActionKind::ShowThreads);
    assert!(!actions[3].enabled);
    assert_eq!(actions[4].kind, ChannelActionKind::MarkAsRead);
    assert!(!actions[4].enabled);
    assert_eq!(actions[5].kind, ChannelActionKind::ToggleMute);
    assert_eq!(actions[5].label, "Mute category");
    assert!(actions[5].enabled);

    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_leader_action_active());
    state.select_channel_action_row(5);
    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_action_mute_duration_phase());

    let command = state.activate_selected_channel_action();

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
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn guild_leader_action_toggle_mute_opens_duration_then_dispatches_command() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();
    state.select_guild_action_row(1);

    assert_eq!(state.activate_selected_guild_action(), None);
    assert!(state.is_guild_action_mute_duration_phase());

    let command = state.activate_selected_guild_action();

    assert_eq!(
        command,
        Some(AppCommand::SetGuildMuted {
            guild_id: Id::new(1),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "guild 1".to_owned(),
        })
    );
    assert!(!state.is_guild_leader_action_active());
}

#[test]
fn current_guild_leave_confirmation_dispatches_leave_command() {
    let mut state = state_with_many_guilds(1);
    state.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));

    state.open_current_guild_leave_confirmation();

    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        state.guild_leave_confirmation_name(),
        Some("guild 1".to_owned())
    );
    assert_eq!(
        state.confirm_guild_leave(),
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
    assert!(
        !state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
}

#[test]
fn focused_guild_cursor_leave_confirmation_does_not_require_active_guild() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.move_down();

    state.open_current_guild_leave_confirmation();

    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        state.confirm_guild_leave(),
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
}

#[test]
fn guild_leader_action_leave_server_opens_confirmation() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    state.select_guild_action_row(2);

    assert_eq!(state.activate_selected_guild_action(), None);

    assert!(!state.is_guild_leader_action_active());
    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        state.confirm_guild_leave(),
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
}

#[test]
fn direct_messages_do_not_open_guild_leave_confirmation() {
    let mut state = state_with_many_guilds(1);
    state.activate_guild(super::ActiveGuildScope::DirectMessages);
    state.focus_pane(FocusPane::Messages);

    state.open_current_guild_leave_confirmation();

    assert!(
        !state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
}

#[test]
fn guild_leader_action_marks_unread_server_channels_as_read() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                ..text_channel_info(guild_id, Id::new(2), "unread-a")
            },
            ChannelInfo {
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                ..text_channel_info(guild_id, Id::new(3), "read")
            },
            ChannelInfo {
                position: Some(2),
                last_message_id: Some(Id::new(40)),
                ..text_channel_info(guild_id, Id::new(4), "unread-b")
            },
        ],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(2), Some(Id::new(10)), 0),
            read_state_info(Id::new(3), Some(Id::new(30)), 0),
            read_state_info(Id::new(4), Some(Id::new(35)), 0),
        ],
    });
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert!(actions[0].enabled);

    let command = state.activate_selected_guild_action();
    let ack_commands = command.clone().into_iter().collect::<Vec<_>>();
    apply_optimistic_ack_commands(&mut state, &ack_commands);

    assert_eq!(
        state.sidebar_guild_unread(guild_id),
        ChannelUnreadState::Seen
    );
    assert!(!state.is_guild_leader_action_active());
    let Some(AppCommand::AckChannels { mut targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    targets.sort_by_key(|(channel_id, _)| channel_id.get());
    assert_eq!(
        targets,
        vec![(Id::new(2), Id::new(20)), (Id::new(4), Id::new(40))]
    );
}

#[test]
fn guild_leader_action_skips_hidden_channels_when_marking_server_read() {
    let mut state = state_with_hidden_and_visible_channels();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(2), Some(Id::new(10)), 0),
            read_state_info(Id::new(3), Some(Id::new(10)), 0),
        ],
    });
    state.push_event(notification_message_event(Id::new(2), "hidden"));
    state.push_event(notification_message_event(Id::new(3), "visible"));
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    let command = state.activate_selected_guild_action();
    let ack_commands = command.clone().into_iter().collect::<Vec<_>>();
    apply_optimistic_ack_commands(&mut state, &ack_commands);

    let Some(AppCommand::AckChannels { targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    assert_eq!(targets, vec![(Id::new(3), Id::new(50))]);
    assert_ne!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
    assert_eq!(state.channel_unread(Id::new(3)), ChannelUnreadState::Seen);
}

#[test]
fn direct_messages_keep_placeholder_guild_action() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);
    state.move_up();
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, GuildActionKind::NoActionsYet);
    assert_eq!(actions[0].label, "No server actions yet");
    assert!(!actions[0].enabled);
}
