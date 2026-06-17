use super::*;

#[test]
fn applies_guild_channels_and_messages() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id,
        message_id,
        author_id,
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

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
        parent_id: Some(category_id),
        owner_id: None,
        position: Some(7),
        last_message_id: Some(Id::new(9)),
        kind: "text".to_owned(),
        guild_id: Some(guild_id),
        name: "general".to_owned(),
        ..channel_info(channel_id, "text", Vec::new())
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

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo {
            avatar_url: Some("https://cdn.discordapp.com/avatar.png".to_owned()),
            status: Some(PresenceStatus::Online),
            ..ChannelRecipientInfo::test(Id::new(20), "alice")
        }],
    )));

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(30)),
        kind: "group-dm".to_owned(),
        ..dm_channel(channel_id, "renamed project chat")
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
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "Alice Global",
        "dm",
        vec![ChannelRecipientInfo {
            username: Some("alice".to_owned()),
            ..ChannelRecipientInfo::test(Id::new(20), "Alice Global")
        }],
    )));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.name, "Bestie");
    assert_eq!(channel.recipients[0].display_name, "Bestie");
}

#[test]
fn relationships_without_user_fields_preserve_existing_dm_names() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id: Id<UserMarker> = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "Alice Global",
        "dm",
        vec![ChannelRecipientInfo {
            username: Some("alice".to_owned()),
            ..ChannelRecipientInfo::test(user_id, "Alice Global")
        }],
    )));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id: user_id,
        author: "Alice Global".to_owned(),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
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

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo {
            username: Some("alice".to_owned()),
            ..ChannelRecipientInfo::test(Id::new(20), "Alice Global")
        }],
    )));
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

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::Online),
            ..ChannelRecipientInfo::test(Id::new(20), "alice")
        }],
    )));

    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(30)),
        ..dm_channel_with_recipients(
            channel_id,
            "renamed project chat",
            "group-dm",
            vec![ChannelRecipientInfo::test(Id::new(20), "alice renamed")],
        )
    }));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].display_name, "alice renamed");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
}

#[test]
fn channel_upsert_defaults_missing_recipient_status_to_unknown() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "alice",
        "dm",
        vec![ChannelRecipientInfo::test(Id::new(20), "alice")],
    )));

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
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "test-user",
        "dm",
        vec![ChannelRecipientInfo::test(user_id, "test-user")],
    )));

    let channel = state.channel(channel_id).expect("channel should be stored");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
    assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
}

#[test]
fn user_presence_update_updates_channel_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo::test(Id::new(20), "alice")],
    )));

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
    let activity = ActivityInfo::test(ActivityKind::Playing, "Concord");

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
    let activity_a = ActivityInfo::test(ActivityKind::Playing, "Guild A");
    let activity_b = ActivityInfo::test(ActivityKind::Listening, "Guild B");

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
fn current_user_activity_updates_profile_and_guild_views() {
    let mut state = DiscordState::default();
    let user_id: Id<UserMarker> = Id::new(20);
    let stale_guild_id: Id<GuildMarker> = Id::new(1);
    let empty_guild_id: Id<GuildMarker> = Id::new(2);
    let old_activity = ActivityInfo::test(ActivityKind::Playing, "Old Game");
    let activity = ActivityInfo::test(ActivityKind::Playing, "Concord");

    state.apply_event(&AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: stale_guild_id,
        user_id,
        status: PresenceStatus::Online,
        activities: vec![old_activity],
    });
    state.apply_event(&AppEvent::UserPresenceUpdate {
        user_id,
        status: PresenceStatus::Online,
        activities: vec![activity.clone()],
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: empty_guild_id,
        user_id,
        status: PresenceStatus::Online,
        activities: Vec::new(),
    });

    assert_eq!(
        state.user_activities(user_id),
        std::slice::from_ref(&activity)
    );
    assert_eq!(
        state.user_activities_for_guild(Some(stale_guild_id), user_id),
        std::slice::from_ref(&activity)
    );
    assert_eq!(
        state.user_activities_for_guild(Some(empty_guild_id), user_id),
        std::slice::from_ref(&activity)
    );
}

#[test]
fn non_current_user_presence_update_preserves_guild_activity() {
    let mut state = DiscordState::default();
    let user_id: Id<UserMarker> = Id::new(20);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let guild_activity = ActivityInfo::test(ActivityKind::Playing, "Guild Game");
    let global_activity = ActivityInfo::test(ActivityKind::Playing, "Global Game");

    state.apply_event(&AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id,
        status: PresenceStatus::Online,
        activities: vec![guild_activity.clone()],
    });
    state.apply_event(&AppEvent::UserPresenceUpdate {
        user_id,
        status: PresenceStatus::Online,
        activities: vec![global_activity.clone()],
    });

    assert_eq!(
        state.user_activities(user_id),
        std::slice::from_ref(&global_activity)
    );
    assert_eq!(
        state.user_activities_for_guild(Some(guild_id), user_id),
        std::slice::from_ref(&guild_activity)
    );
}

#[test]
fn guild_presence_update_updates_matching_channel_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "alice",
        "dm",
        vec![ChannelRecipientInfo::test(Id::new(20), "alice")],
    )));

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
        last_message_id: Some(Id::new(20)),
        ..dm_channel(channel_id, "neo")
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(30),
        author_id: Id::new(99),
        content: Some("new".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(10),
        author_id: Id::new(99),
        content: Some("old".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

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
        message_count: Some(12),
        member_count: None,
        total_message_sent: Some(14),
        ..guild_thread_channel(Id::new(1), channel_id, Id::new(2), "release notes")
    }));
    for _ in 0..2 {
        state.apply_event(&message_create_event(MessageCreateFixture {
            guild_id: Some(Id::new(1)),
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            content: Some("new".to_owned()),
            ..MessageCreateFixture::test_fixture_default()
        }));
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
        last_message_id: Some(Id::new(20)),
        ..dm_channel(channel_id, "neo")
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 40, "new"),
        ],
    ));

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
        last_message_id: Some(Id::new(30)),
        ..dm_channel(channel_id, "neo")
    }));
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(
        channel_id,
        "neo renamed",
    )));
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(20)),
        ..dm_channel(channel_id, "neo renamed again")
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
        message_count: Some(12),
        member_count: None,
        total_message_sent: Some(14),
        ..guild_thread_channel(guild_id, channel_id, Id::new(2), "release notes")
    }));
    state.apply_event(&AppEvent::ChannelDelete {
        guild_id: Some(guild_id),
        channel_id,
    });

    assert_eq!(state.channel(channel_id), None);
}

#[test]
fn thread_created_by_current_user_is_marked_joined() {
    let guild_id = Id::new(1);
    let current_user_id = Id::new(10);
    let own_thread = Id::new(20);
    let other_thread = Id::new(21);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        owner_id: Some(current_user_id),
        ..guild_thread_channel(guild_id, own_thread, Id::new(2), "my thread")
    }));
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        owner_id: Some(Id::new(99)),
        ..guild_thread_channel(guild_id, other_thread, Id::new(2), "someone elses thread")
    }));

    assert!(
        state
            .channel(own_thread)
            .unwrap()
            .current_user_joined_thread
    );
    assert!(
        !state
            .channel(other_thread)
            .unwrap()
            .current_user_joined_thread
    );
}
