use super::*;

#[test]
fn channel_leader_action_lists_threads_for_selected_channel() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    assert!(state.is_channel_leader_action_active());
    let actions = state.selected_channel_action_items();
    assert_eq!(actions.len(), 4);
    assert_eq!(actions[0].kind, ChannelActionKind::LoadPinnedMessages);
    assert_eq!(actions[0].label, "Show pinned messages");
    assert!(actions[0].enabled);
    assert_eq!(actions[1].kind, ChannelActionKind::ShowThreads);
    assert!(actions[1].enabled);
    assert_eq!(actions[2].kind, ChannelActionKind::MarkAsRead);
    assert_eq!(actions[2].label, "Mark as read");
    assert_eq!(actions[3].kind, ChannelActionKind::ToggleMute);
    assert_eq!(actions[3].label, "Mute channel");

    let command = state.activate_channel_action_shortcut('t');
    assert_eq!(command, None);
    assert!(state.is_channel_action_threads_phase());

    let threads = state.channel_action_thread_items();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].channel_id, Id::new(10));
    assert_eq!(threads[0].label, "release notes");
}

#[test]
fn mark_as_read_action_enablement_is_scoped_to_action_channel() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let unread_channel: Id<ChannelMarker> = Id::new(2);
    let read_channel: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: unread_channel,
                parent_id: None,
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                name: "unread".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: read_channel,
                parent_id: None,
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                name: "read".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: unread_channel,
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: read_channel,
                last_acked_message_id: Some(Id::new(30)),
                mention_count: 0,
            },
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
fn channel_leader_action_open_thread_activates_and_subscribes() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.activate_channel_action_shortcut('t');
    let command = state.activate_selected_channel_action();

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
    assert!(!state.is_channel_leader_action_active());
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
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
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert_eq!(actions[0].label, "Mark server as read");
    assert!(!actions[0].enabled);
    assert_eq!(actions[1].kind, GuildActionKind::ToggleMute);
    assert_eq!(actions[1].label, "Mute server");
    assert_eq!(state.activate_selected_guild_action(), None);
}

#[test]
fn channel_leader_action_toggle_mute_opens_duration_then_dispatches_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();
    state.select_channel_action_row(3);

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
fn category_leader_action_only_lists_mute_and_dispatches_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_up();
    state.open_selected_channel_actions();

    assert!(state.is_channel_leader_action_active());
    let actions = state.selected_channel_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, ChannelActionKind::ToggleMute);
    assert_eq!(actions[0].label, "Mute category");

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
fn guild_leader_action_marks_unread_server_channels_as_read() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(2),
                parent_id: None,
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                name: "unread-a".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(3),
                parent_id: None,
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                name: "read".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(4),
                parent_id: None,
                position: Some(2),
                last_message_id: Some(Id::new(40)),
                name: "unread-b".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(2),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(3),
                last_acked_message_id: Some(Id::new(30)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(4),
                last_acked_message_id: Some(Id::new(35)),
                mention_count: 0,
            },
        ],
    });
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert!(actions[0].enabled);

    let command = state.activate_selected_guild_action();

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
            ReadStateInfo {
                channel_id: Id::new(2),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(3),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
        ],
    });
    state.push_event(notification_message_event(Id::new(2), "hidden"));
    state.push_event(notification_message_event(Id::new(3), "visible"));
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    let command = state.activate_selected_guild_action();

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
