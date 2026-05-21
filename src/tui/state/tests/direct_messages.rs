use super::*;

#[test]
fn direct_messages_are_sorted_by_latest_message_id() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);
}

#[test]
fn direct_message_selection_waits_for_channel_confirmation() {
    let mut state = state_with_direct_messages();

    state.confirm_selected_guild();
    assert_eq!(state.selected_channel_id(), None);

    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
}

#[test]
fn activate_channel_effect_moves_direct_message_cursor_to_target() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    assert_eq!(state.selected_channel(), 0);

    state.push_effect(AppEvent::ActivateChannel {
        channel_id: Id::new(30),
    });

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(state.selected_channel(), 2);
}

#[test]
fn direct_message_sorting_uses_channel_id_fallback() {
    let mut state = DashboardState::new();
    for (channel_id, name) in [(Id::new(10), "older-id"), (Id::new(30), "newer-id")] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: name.to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
    }
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["newer-id", "older-id"]);
}

#[test]
fn restoring_discord_snapshot_recovers_missed_guilds_and_direct_messages() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let guild_channel_id: Id<ChannelMarker> = Id::new(2);
    let dm_channel_id: Id<ChannelMarker> = Id::new(20);
    let mut snapshot = DiscordState::default();
    snapshot.apply_event(&AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    snapshot.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: guild_channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    snapshot.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: dm_channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(200)),
        name: "alice".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let mut state = DashboardState::new();
    state.restore_discord_snapshot(snapshot);

    assert_eq!(state.current_user(), Some("neo"));
    assert_eq!(state.current_user_id(), Some(Id::new(10)));
    assert_eq!(state.guild_pane_entries().len(), 2);

    state.confirm_selected_guild();
    assert_eq!(state.selected_guild_id(), Some(guild_id));
    assert_eq!(channel_entry_names(&state), vec!["general"]);

    state.navigation.selected_guild = 0;
    state.confirm_selected_guild();
    assert_eq!(channel_entry_names(&state), vec!["alice"]);
}

#[test]
fn direct_message_cursor_stays_on_same_channel_after_recency_sort() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    assert_eq!(state.selected_channel(), 1);
    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);

    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id: Id::new(30),
        message_id: Id::new(300),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("new empty dm".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(channel_entry_names(&state), vec!["empty", "new", "old"]);
    assert_eq!(state.selected_channel(), 2);
}
