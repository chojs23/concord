use super::*;

#[test]
fn applies_guild_channels_and_messages() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
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
        owner_id: None,
    });
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id,
        author_id,
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(state.guilds().len(), 1);
    assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
    assert_eq!(state.messages_for_channel(channel_id).len(), 1);
}
#[test]
fn stores_channel_parent_and_position() {
    let guild_id = Id::new(1);
    let category_id = Id::new(2);
    let channel_id = Id::new(3);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild_id),
        channel_id,
        parent_id: Some(category_id),
        position: Some(7),
        last_message_id: Some(Id::new(9)),
        name: "general".to_owned(),
        kind: "text".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).unwrap();
    assert_eq!(channel.parent_id, Some(category_id));
    assert_eq!(channel.position, Some(7));
    assert_eq!(channel.last_message_id, Some(Id::new(9)));
}

#[test]
fn channel_upsert_stores_and_preserves_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: Some("https://cdn.discordapp.com/avatar.png".to_owned()),
            status: Some(PresenceStatus::Online),
        }]),
        permission_overwrites: Vec::new(),
    }));

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(30)),
        name: "renamed project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.name, "renamed project chat");
    assert_eq!(channel.recipients.len(), 1);
    assert_eq!(channel.recipients[0].user_id, Id::new(20));
    assert_eq!(channel.recipients[0].display_name, "alice");
    assert_eq!(
        channel.recipients[0].avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatar.png")
    );
    assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
}

#[test]
fn dm_channel_upsert_prefers_friend_nickname() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            20,
            FriendStatus::Friend,
            Some("Bestie"),
            Some("Alice Global"),
            Some("alice"),
        )],
    });
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "Alice Global".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "Alice Global".to_owned(),
            username: Some("alice".to_owned()),
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.name, "Bestie");
    assert_eq!(channel.recipients[0].display_name, "Bestie");
}

#[test]
fn relationships_without_user_fields_preserve_existing_dm_names() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id: Id<UserMarker> = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "Alice Global".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id,
            display_name: "Alice Global".to_owned(),
            username: Some("alice".to_owned()),
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id: user_id,
        author: "Alice Global".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            user_id.get(),
            FriendStatus::Friend,
            None,
            None,
            None,
        )],
    });

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.name, "Alice Global");
    assert_eq!(channel.recipients[0].display_name, "Alice Global");
    assert_eq!(
        state.messages_for_channel(channel_id)[0].author,
        "Alice Global"
    );
}

#[test]
fn relationship_nickname_refresh_preserves_explicit_group_dm_name() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "Alice Global".to_owned(),
            username: Some("alice".to_owned()),
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            20,
            FriendStatus::Friend,
            Some("Bestie"),
            Some("Alice Global"),
            Some("alice"),
        )],
    });

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.name, "project chat");
    assert_eq!(channel.recipients[0].display_name, "Bestie");
}

#[test]
fn channel_upsert_preserves_recipient_status_when_omitted() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: Some(PresenceStatus::Online),
        }]),
        permission_overwrites: Vec::new(),
    }));

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(30)),
        name: "renamed project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice renamed".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].display_name, "alice renamed");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
}

#[test]
fn channel_upsert_defaults_missing_recipient_status_to_unknown() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "alice".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Unknown);
}

#[test]
fn channel_upsert_uses_cached_user_presence_when_status_is_omitted() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id: Id<UserMarker> = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::UserPresenceUpdate {
        user_id,
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "test-user".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id,
            display_name: "test-user".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
    assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
}

#[test]
fn user_presence_update_updates_channel_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "project chat".to_owned(),
        kind: "group-dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    state.apply_event(&AppEvent::UserPresenceUpdate {
        user_id: Id::new(20),
        status: PresenceStatus::DoNotDisturb,
        activities: Vec::new(),
    });

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].status, PresenceStatus::DoNotDisturb);
}

#[test]
fn presence_update_caches_user_activities() {
    let mut state = DiscordState::default();
    let user_id: Id<UserMarker> = Id::new(20);
    let activity = ActivityInfo {
        kind: ActivityKind::Playing,
        name: "Concord".to_owned(),
        details: None,
        state: None,
        url: None,
        application_id: None,
        emoji: None,
    };

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Id::new(1),
        user_id,
        status: PresenceStatus::Online,
        activities: vec![activity.clone()],
    });

    assert_eq!(
        state.user_activities(user_id),
        std::slice::from_ref(&activity)
    );

    // Empty activities array clears the cached entry.
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Id::new(1),
        user_id,
        status: PresenceStatus::Online,
        activities: Vec::new(),
    });
    assert!(state.user_activities(user_id).is_empty());
}

#[test]
fn guild_presence_activities_are_scoped_by_guild() {
    let mut state = DiscordState::default();
    let user_id: Id<UserMarker> = Id::new(20);
    let guild_a: Id<GuildMarker> = Id::new(1);
    let guild_b: Id<GuildMarker> = Id::new(2);
    let activity_a = ActivityInfo {
        kind: ActivityKind::Playing,
        name: "Guild A".to_owned(),
        details: None,
        state: None,
        url: None,
        application_id: None,
        emoji: None,
    };
    let activity_b = ActivityInfo {
        kind: ActivityKind::Listening,
        name: "Guild B".to_owned(),
        details: None,
        state: None,
        url: None,
        application_id: None,
        emoji: None,
    };

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: guild_a,
        user_id,
        status: PresenceStatus::Online,
        activities: vec![activity_a.clone()],
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: guild_b,
        user_id,
        status: PresenceStatus::Idle,
        activities: vec![activity_b.clone()],
    });

    assert_eq!(
        state.user_presence_for_guild(Some(guild_a), user_id),
        Some(PresenceStatus::Online)
    );
    assert_eq!(
        state.user_presence_for_guild(Some(guild_b), user_id),
        Some(PresenceStatus::Idle)
    );
    assert_eq!(
        state.user_activities_for_guild(Some(guild_a), user_id),
        std::slice::from_ref(&activity_a)
    );
    assert_eq!(
        state.user_activities_for_guild(Some(guild_b), user_id),
        std::slice::from_ref(&activity_b)
    );
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: guild_a,
        user_id,
        status: PresenceStatus::DoNotDisturb,
        activities: Vec::new(),
    });

    assert!(
        state
            .user_activities_for_guild(Some(guild_a), user_id)
            .is_empty()
    );
    assert_eq!(
        state.user_activities_for_guild(Some(guild_b), user_id),
        std::slice::from_ref(&activity_b)
    );
}

#[test]
fn guild_presence_update_updates_matching_channel_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "alice".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(20),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }]),
        permission_overwrites: Vec::new(),
    }));

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Id::new(1),
        user_id: Id::new(20),
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
}
#[test]
fn live_messages_update_channel_last_message_id() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(20)),
        name: "neo".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(30),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("new".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(10),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("old".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(
        state
            .channel(channel_id)
            .and_then(|channel| channel.last_message_id),
        Some(Id::new(30))
    );
}

#[test]
fn live_thread_messages_increment_cached_counts_once() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id,
        parent_id: Some(Id::new(2)),
        position: None,
        last_message_id: None,
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(false),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    for _ in 0..2 {
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("new".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(30)),
        messages: vec![message_info(channel_id, 20, "old")],
    });

    let channel = state
        .channel(channel_id)
        .expect("thread should stay cached");
    assert_eq!(channel.message_count, Some(13));
    assert_eq!(channel.total_message_sent, Some(15));
}

#[test]
fn history_updates_channel_last_message_id() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(20)),
        name: "neo".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 40, "new"),
        ],
    });

    assert_eq!(
        state
            .channel(channel_id)
            .and_then(|channel| channel.last_message_id),
        Some(Id::new(40))
    );
}

#[test]
fn channel_upsert_does_not_regress_last_message_id() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(30)),
        name: "neo".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "neo renamed".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(20)),
        name: "neo renamed again".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let channel = state.channel(channel_id).unwrap();
    assert_eq!(channel.name, "neo renamed again");
    assert_eq!(channel.last_message_id, Some(Id::new(30)));
}

#[test]
fn channel_delete_removes_cached_thread() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild_id),
        channel_id,
        parent_id: Some(Id::new(2)),
        position: None,
        last_message_id: None,
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(false),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.apply_event(&AppEvent::ChannelDelete {
        guild_id: Some(guild_id),
        channel_id,
    });

    assert_eq!(state.channel(channel_id), None);
}
