use super::*;

#[test]
fn channel_pane_excludes_threads() {
    let state = state_with_thread_created_message();
    let entries = state.channel_pane_entries();
    let channel_ids: Vec<Id<ChannelMarker>> = entries
        .iter()
        .filter_map(|entry| match entry {
            ChannelPaneEntry::Channel { state, .. } => Some(state.id),
            ChannelPaneEntry::CategoryHeader { .. } | ChannelPaneEntry::VoiceParticipant { .. } => {
                None
            }
        })
        .collect();
    assert!(channel_ids.contains(&Id::new(2)));
    assert!(!channel_ids.contains(&Id::new(10)));
}

#[test]
fn channel_switcher_groups_channels_and_filters_by_fuzzy_name() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(40),
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(100)),
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
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(10),
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Text".to_owned(),
                kind: "category".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(11),
                parent_id: Some(Id::new(10)),
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(12),
                parent_id: Some(Id::new(10)),
                position: Some(1),
                last_message_id: None,
                name: "random".to_owned(),
                kind: "text".to_owned(),
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
    });

    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: Id::new(40),
            last_acked_message_id: Some(Id::new(100)),
            mention_count: 0,
        }],
    });

    state.open_channel_switcher();
    let all_items = state.channel_switcher_items();
    assert_eq!(all_items[0].group_label, "Direct Messages");
    assert_eq!(all_items[1].group_label, "guild");
    assert_eq!(all_items[1].parent_label.as_deref(), Some("Text"));

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(13),
        parent_id: Some(Id::new(10)),
        position: Some(2),
        last_message_id: None,
        name: "general-new".to_owned(),
        kind: "text".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    for ch in "gnrl".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered = state.channel_switcher_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].channel_id, Id::new(11));

    state.close_channel_switcher();
    state.open_channel_switcher();
    for ch in "gnrl".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered: Vec<Id<ChannelMarker>> = state
        .channel_switcher_items()
        .into_iter()
        .map(|item| item.channel_id)
        .collect();
    assert!(filtered.contains(&Id::new(11)));
    assert!(filtered.contains(&Id::new(13)));
}

#[test]
fn channel_switcher_items_carry_unread_metadata() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(40),
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(100)),
        name: "new".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: Id::new(40),
            last_acked_message_id: Some(Id::new(90)),
            mention_count: 0,
        }],
    });
    state.open_channel_switcher();

    let items = state.channel_switcher_items();

    assert_eq!(items[0].channel_id, Id::new(40));
    assert_eq!(items[0].unread, ChannelUnreadState::Unread);
}

#[test]
fn channel_switcher_query_prefers_channel_name_before_context() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "acme".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "general".to_owned(),
            kind: "text".to_owned(),
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
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(2),
        name: "other".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(2)),
            channel_id: Id::new(21),
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "acme-chat".to_owned(),
            kind: "text".to_owned(),
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

    state.open_channel_switcher();
    for ch in "acme".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered: Vec<Id<ChannelMarker>> = state
        .channel_switcher_items()
        .into_iter()
        .map(|item| item.channel_id)
        .collect();

    assert_eq!(filtered, vec![Id::new(21), Id::new(11)]);
}

#[test]
fn channel_switcher_lists_recent_channels_first() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(11),
                parent_id: None,
                position: Some(0),
                last_message_id: Some(Id::new(101)),
                name: "alerts".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(12),
                parent_id: None,
                position: Some(1),
                last_message_id: None,
                name: "quiet".to_owned(),
                kind: "text".to_owned(),
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
    });

    state.activate_channel(Id::new(11));
    state.activate_channel(Id::new(12));
    state.activate_channel(Id::new(11));
    state.open_channel_switcher();
    let items = state.channel_switcher_items();

    assert_eq!(items[0].group_label, "Recent Channels");
    assert_eq!(items[0].channel_id, Id::new(12));
    assert_eq!(items[0].parent_label.as_deref(), Some("guild"));
    assert_eq!(
        items
            .iter()
            .filter(|item| {
                item.group_label == "Recent Channels" && item.channel_id == Id::new(11)
            })
            .count(),
        0
    );
    assert_eq!(
        items
            .iter()
            .filter(|item| {
                item.group_label == "Recent Channels" && item.channel_id == Id::new(12)
            })
            .count(),
        1
    );
    assert!(!items.iter().any(|item| item.group_label == "Notifications"));
    assert!(
        items
            .iter()
            .skip(1)
            .any(|item| { item.group_label == "guild" && item.channel_id == Id::new(11) })
    );
    assert!(
        items
            .iter()
            .any(|item| { item.group_label == "guild" && item.channel_id == Id::new(12) })
    );
}

#[test]
fn channel_switcher_query_matches_display_prefixes() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(40),
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "new-dm".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "new-text".to_owned(),
            kind: "text".to_owned(),
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

    state.open_channel_switcher();
    for ch in "#new".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered: Vec<Id<ChannelMarker>> = state
        .channel_switcher_items()
        .into_iter()
        .map(|item| item.channel_id)
        .collect();
    assert_eq!(filtered, vec![Id::new(11)]);

    state.close_channel_switcher();
    state.open_channel_switcher();
    for ch in "@new".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered: Vec<Id<ChannelMarker>> = state
        .channel_switcher_items()
        .into_iter()
        .map(|item| item.channel_id)
        .collect();
    assert_eq!(filtered, vec![Id::new(40)]);
}

#[test]
fn channel_switcher_query_edits_at_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "raXndom".chars() {
        state.push_channel_switcher_char(ch);
    }

    for _ in 0..5 {
        state.move_channel_switcher_query_cursor_left();
    }
    state.move_channel_switcher_query_cursor_right();
    state.pop_channel_switcher_char();

    assert_eq!(state.channel_switcher_query(), Some("random"));
    assert_eq!(
        state.channel_switcher_query_cursor_byte_index(),
        Some("ra".len())
    );
}

#[test]
fn channel_switcher_query_deletes_grapheme_before_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "e\u{301}x".chars() {
        state.push_channel_switcher_char(ch);
    }

    state.move_channel_switcher_query_cursor_left();
    state.pop_channel_switcher_char();

    assert_eq!(state.channel_switcher_query(), Some("x"));
    assert_eq!(state.channel_switcher_query_cursor_byte_index(), Some(0));
}
