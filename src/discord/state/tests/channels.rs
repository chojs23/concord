use super::*;
use crate::discord::ArchivedThreadsPage;

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
fn active_and_joined_thread_state_are_independent() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let joined_id = Id::new(10);
    let unjoined_id = Id::new(11);
    let mut state = DiscordState::default();
    let joined_member = ThreadMemberInfo {
        thread_id: Some(joined_id),
        user_id: Some(Id::new(99)),
        join_timestamp: None,
        flags: Some(4),
        muted: Some(true),
        mute_end_time: Some("2099-01-01T00:00:00.000Z".to_owned()),
        selected_time_window: Some(3600),
        member: None,
        presence: None,
        extra_fields: BTreeMap::new(),
    };

    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "forum".to_owned(),
                ..channel_info(forum_id, "forum", Vec::new())
            },
            test_thread(guild_id, forum_id, joined_id, "joined"),
            test_thread(guild_id, forum_id, unjoined_id, "not joined"),
        ],
        current_user_thread_members: vec![joined_member],
        ..GuildCreateFixture::new(guild_id)
    }));

    assert_eq!(
        state.active_thread_ids_for_parent(forum_id),
        vec![joined_id, unjoined_id]
    );
    assert!(state.thread_is_joined(joined_id));
    assert!(state.thread_is_muted(joined_id));
    assert_eq!(
        state.thread_mute_end_time(joined_id),
        Some("2099-01-01T00:00:00.000Z")
    );
    assert_eq!(
        state.thread_mute_selected_time_window(joined_id),
        Some(3600)
    );
    assert!(!state.thread_is_joined(unjoined_id));
    assert!(state.thread_is_sidebar_active(joined_id));
    assert!(!state.thread_is_sidebar_active(unjoined_id));

    state.apply_event(&AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: Some(vec![forum_id]),
            threads: vec![test_thread(guild_id, forum_id, joined_id, "joined")],
            current_user_members: None,
            extra_fields: BTreeMap::new(),
        },
    });
    assert!(state.thread_is_joined(joined_id));
    assert!(state.thread_is_sidebar_active(joined_id));

    state.apply_event(&AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: Some(vec![forum_id]),
            threads: vec![test_thread(guild_id, forum_id, joined_id, "joined")],
            current_user_members: Some(Vec::new()),
            extra_fields: BTreeMap::new(),
        },
    });
    assert_eq!(
        state.active_thread_ids_for_parent(forum_id),
        vec![joined_id]
    );
    assert!(!state.thread_is_joined(joined_id));
    assert!(!state.thread_is_sidebar_active(joined_id));
}

#[test]
fn thread_mute_update_caches_duration_metadata() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "forum".to_owned(),
                ..channel_info(forum_id, "forum", Vec::new())
            },
            test_thread(guild_id, forum_id, thread_id, "joined"),
        ],
        current_user_thread_members: vec![ThreadMemberInfo::joined_snapshot(thread_id)],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::ThreadMuteUpdate {
        channel_id: thread_id,
        muted: true,
        mute_end_time: Some("2099-01-01T00:00:00.000Z".to_owned()),
        selected_time_window: Some(3600),
    });

    assert!(state.thread_is_muted(thread_id));
    assert_eq!(
        state.thread_mute_end_time(thread_id),
        Some("2099-01-01T00:00:00.000Z")
    );
    assert_eq!(
        state.thread_mute_selected_time_window(thread_id),
        Some(3600)
    );
}

#[test]
fn partial_guild_snapshot_preserves_active_and_joined_threads() {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![test_thread(guild_id, parent_id, thread_id, "joined")],
        current_user_thread_members: vec![ThreadMemberInfo::joined_snapshot(thread_id)],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&guild_create_event(GuildCreateFixture {
        thread_snapshot_complete: false,
        ..GuildCreateFixture::new(guild_id)
    }));

    assert_eq!(
        state.active_thread_ids_for_parent(parent_id),
        vec![thread_id]
    );
    assert!(state.thread_is_sidebar_active(thread_id));
}

#[test]
fn participant_snapshot_populates_members_without_joining_the_thread() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let thread_id = Id::new(10);
    let participant_id = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                ..channel_info(forum_id, "forum", Vec::new())
            },
            test_thread(guild_id, forum_id, thread_id, "post"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::ThreadMemberListUpdate {
        update: ThreadMemberListUpdateInfo {
            guild_id,
            channel_id: thread_id,
            members: vec![ThreadMemberInfo {
                thread_id: Some(thread_id),
                user_id: Some(participant_id),
                join_timestamp: None,
                flags: None,
                muted: None,
                mute_end_time: None,
                selected_time_window: None,
                member: Some(MemberInfo::test(participant_id, "alice")),
                presence: None,
                extra_fields: BTreeMap::new(),
            }],
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        state
            .thread_members_for_channel(guild_id, thread_id)
            .into_iter()
            .map(|member| member.user_id)
            .collect::<Vec<_>>(),
        vec![participant_id]
    );
    assert!(!state.thread_is_joined(thread_id));
    assert!(!state.thread_is_sidebar_active(thread_id));
}

#[test]
fn archived_or_removed_thread_updates_leave_the_active_set() {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: test_thread(guild_id, parent_id, thread_id, "post"),
            current_user_member: None,
        },
        created: false,
    });
    assert_eq!(
        state.active_thread_ids_for_parent(parent_id),
        vec![thread_id]
    );

    let mut removed = test_thread(guild_id, parent_id, thread_id, "post");
    removed.flags = Some(1 << 2);
    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: removed,
            current_user_member: None,
        },
        created: false,
    });
    assert!(state.active_thread_ids_for_parent(parent_id).is_empty());

    let partial = ChannelInfo {
        guild_id: Some(guild_id),
        parent_id: Some(parent_id),
        name: "renamed post".to_owned(),
        ..channel_info(thread_id, "GuildPublicThread", Vec::new())
    };
    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: partial,
            current_user_member: None,
        },
        created: false,
    });
    assert!(state.active_thread_ids_for_parent(parent_id).is_empty());

    let mut archived = test_thread(guild_id, parent_id, thread_id, "post");
    archived.flags = Some(0);
    archived.thread_metadata = Some(ThreadMetadataInfo::test(true, false));
    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: archived,
            current_user_member: None,
        },
        created: false,
    });
    assert!(state.active_thread_ids_for_parent(parent_id).is_empty());
}

fn test_thread(
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
    thread_id: Id<ChannelMarker>,
    name: &str,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        parent_id: Some(parent_id),
        name: name.to_owned(),
        thread_metadata: Some(ThreadMetadataInfo::test(false, false)),
        ..channel_info(thread_id, "GuildPublicThread", Vec::new())
    }
}

fn archived_test_thread(
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
    thread_id: Id<ChannelMarker>,
    name: &str,
    archive_timestamp: &str,
) -> ChannelInfo {
    let mut thread = test_thread(guild_id, parent_id, thread_id, name);
    let metadata = thread
        .thread_metadata
        .as_mut()
        .expect("test thread should have metadata");
    metadata.archived = true;
    metadata.archive_timestamp = Some(archive_timestamp.to_owned());
    thread
}

#[test]
fn archived_response_keeps_membership_without_promoting_sidebar_state() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let active_id = Id::new(10);
    let archived_ids = [Id::new(31), Id::new(30)];
    let current_user_id = Id::new(9);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                ..channel_info(forum_id, "forum", Vec::new())
            },
            test_thread(guild_id, forum_id, active_id, "active"),
        ],
        current_user_thread_members: vec![ThreadMemberInfo::joined_snapshot(active_id)],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: None,
        page: ArchivedThreadsPage {
            threads: vec![
                archived_test_thread(
                    guild_id,
                    forum_id,
                    archived_ids[0],
                    "newer archive",
                    "2026-08-14T02:00:00.000000+00:00",
                ),
                archived_test_thread(
                    guild_id,
                    forum_id,
                    archived_ids[1],
                    "older archive",
                    "2026-08-14T01:00:00.000000+00:00",
                ),
            ],
            members: vec![ThreadMemberInfo {
                thread_id: Some(archived_ids[0]),
                user_id: Some(current_user_id),
                join_timestamp: Some("2026-08-14T00:00:00.000000+00:00".to_owned()),
                flags: Some(4),
                muted: Some(true),
                mute_end_time: None,
                selected_time_window: None,
                member: Some(MemberInfo::test(current_user_id, "current user")),
                presence: None,
                extra_fields: BTreeMap::from([(
                    "future_member_field".to_owned(),
                    serde_json::json!(true),
                )]),
            }],
            has_more: false,
            next_before: Some("2026-08-14T01:00:00.000000+00:00".to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        state.active_thread_ids_for_parent(forum_id),
        vec![active_id]
    );
    assert_eq!(state.archived_thread_ids_for_parent(forum_id), archived_ids);
    assert!(state.thread_is_joined(archived_ids[0]));
    assert_eq!(
        state.thread_notification_level_flags(archived_ids[0]),
        Some(4)
    );
    assert!(state.thread_is_muted(archived_ids[0]));
    assert!(!state.thread_is_sidebar_active(archived_ids[0]));

    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: test_thread(guild_id, forum_id, archived_ids[0], "reopened"),
            current_user_member: None,
        },
        created: false,
    });

    assert!(state.thread_is_sidebar_active(archived_ids[0]));
    assert_eq!(
        state.archived_thread_ids_for_parent(forum_id),
        vec![archived_ids[1]]
    );
}

#[test]
fn archived_pages_append_without_duplicates_and_advance_timestamp_cursor() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild_id),
        ..channel_info(forum_id, "forum", Vec::new())
    }));

    let first_before = "2026-08-14T01:00:00.000000+00:00";
    state.apply_event(&AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: None,
        page: ArchivedThreadsPage {
            threads: vec![
                archived_test_thread(
                    guild_id,
                    forum_id,
                    Id::new(31),
                    "first",
                    "2026-08-14T02:00:00.000000+00:00",
                ),
                archived_test_thread(guild_id, forum_id, Id::new(30), "second", first_before),
            ],
            members: Vec::new(),
            has_more: true,
            next_before: Some(first_before.to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });
    assert_eq!(
        state.next_archived_thread_page_cursor(forum_id, true),
        Some(crate::discord::ArchivedThreadPageCursor::Before(
            first_before.to_owned()
        ))
    );

    state.apply_event(&AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: Some(first_before.to_owned()),
        page: ArchivedThreadsPage {
            threads: vec![
                archived_test_thread(guild_id, forum_id, Id::new(30), "duplicate", first_before),
                archived_test_thread(
                    guild_id,
                    forum_id,
                    Id::new(29),
                    "third",
                    "2026-08-14T00:00:00.000000+00:00",
                ),
            ],
            members: Vec::new(),
            has_more: false,
            next_before: Some("2026-08-14T00:00:00.000000+00:00".to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        state.archived_thread_ids_for_parent(forum_id),
        vec![Id::new(31), Id::new(30), Id::new(29)]
    );
    assert_eq!(state.next_archived_thread_page_cursor(forum_id, true), None);
}

#[test]
fn group_dm_recipient_deltas_update_members_and_generated_name() {
    let channel_id = Id::new(10);
    let alice = Id::new(20);
    let bob = Id::new(30);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "Alice",
        "group-dm",
        vec![ChannelRecipientInfo::test(alice, "Alice")],
    )));

    state.apply_event(&AppEvent::ChannelRecipientAdd {
        channel_id,
        recipient: ChannelRecipientInfo::test(bob, "Bob"),
    });
    let channel = state
        .channel(channel_id)
        .expect("group DM should remain cached");
    assert_eq!(
        channel
            .recipients
            .iter()
            .map(|recipient| recipient.user_id)
            .collect::<Vec<_>>(),
        vec![alice, bob]
    );
    assert_eq!(channel.name, "Alice, Bob");

    state.apply_event(&AppEvent::ChannelRecipientRemove {
        channel_id,
        user_id: alice,
    });
    let channel = state
        .channel(channel_id)
        .expect("group DM should remain cached");
    assert_eq!(channel.recipients.len(), 1);
    assert_eq!(channel.recipients[0].user_id, bob);
    assert_eq!(channel.name, "Bob");
}

#[test]
fn channel_upsert_stores_and_preserves_recipients() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id = Id::new(20);
    let avatar = "https://cdn.discordapp.com/avatar.png";
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo {
            username: Some("alice".to_owned()),
            is_bot: true,
            avatar_url: Some(avatar.to_owned()),
            status: Some(PresenceStatus::Online),
            ..ChannelRecipientInfo::test(user_id, "alice")
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
    assert_eq!(channel.recipients[0].user_id, user_id);
    assert_eq!(channel.recipients[0].display_name, "alice");
    assert_eq!(channel.recipients[0].avatar_url.as_deref(), Some(avatar));
    assert_eq!(channel.recipients[0].status, PresenceStatus::Online);

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "unknown",
        "group-dm",
        vec![ChannelRecipientInfo::test(user_id, "unknown")],
    )));

    let recipient = &state
        .channel(channel_id)
        .expect("DM should remain cached")
        .recipients[0];
    assert_eq!(recipient.display_name, "alice");
    assert_eq!(recipient.username.as_deref(), Some("alice"));
    assert!(recipient.is_bot);
    assert_eq!(recipient.avatar_url.as_deref(), Some(avatar));
    assert_eq!(recipient.status, PresenceStatus::Online);
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
fn partial_relationship_update_changes_and_clears_friend_nickname() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            user_id.get(),
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
            ..ChannelRecipientInfo::test(user_id, "Alice Global")
        }],
    )));

    state.apply_event(&AppEvent::RelationshipUpdate {
        update: crate::discord::RelationshipUpdateInfo {
            user_id,
            status: None,
            nickname: Some(Some("Pal".to_owned())),
            display_name: None,
            username: None,
            ignored: None,
        },
    });
    assert_eq!(
        state.channel(channel_id).expect("DM should exist").name,
        "Pal"
    );

    state.apply_event(&AppEvent::RelationshipUpdate {
        update: crate::discord::RelationshipUpdateInfo {
            user_id,
            status: None,
            nickname: Some(None),
            display_name: None,
            username: None,
            ignored: None,
        },
    });
    assert_eq!(
        state.channel(channel_id).expect("DM should exist").name,
        "Alice Global"
    );
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
fn channel_upsert_resolves_omitted_recipient_status() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let user_id: Id<UserMarker> = Id::new(20);
    let upsert = |state: &mut DiscordState, name: &str| {
        state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
            channel_id,
            "project chat",
            "group-dm",
            vec![ChannelRecipientInfo::test(user_id, name)],
        )));
    };

    // Discord frequently omits `status` on a channel refresh. Falling back to
    // offline would flicker the presence dot, so a previously known status or
    // the presence cache has to fill in; only a genuinely unknown user is
    // `Unknown`.
    let mut nothing_known = DiscordState::default();
    upsert(&mut nothing_known, "alice");
    assert_eq!(
        nothing_known
            .channel(channel_id)
            .expect("channel is stored")
            .recipients[0]
            .status,
        PresenceStatus::Unknown
    );

    let mut from_cache = DiscordState::default();
    from_cache.apply_event(&AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        },
    });
    upsert(&mut from_cache, "alice");
    assert_eq!(
        from_cache
            .channel(channel_id)
            .expect("channel is stored")
            .recipients[0]
            .status,
        PresenceStatus::Idle
    );
    assert_eq!(
        from_cache.user_presence(user_id),
        Some(PresenceStatus::Idle)
    );

    let mut from_previous = DiscordState::default();
    from_previous.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        channel_id,
        "project chat",
        "group-dm",
        vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::Online),
            ..ChannelRecipientInfo::test(user_id, "alice")
        }],
    )));
    upsert(&mut from_previous, "alice renamed");
    let channel = from_previous
        .channel(channel_id)
        .expect("channel should be stored");
    assert_eq!(channel.recipients[0].display_name, "alice renamed");
    assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
}

#[test]
fn presence_update_updates_channel_recipients_from_any_scope() {
    let channel_id: Id<ChannelMarker> = Id::new(10);

    // A guild-scoped presence still describes the same person, so a DM row has
    // to pick it up as readily as a global one.
    for (guild_id, status) in [
        (None, PresenceStatus::DoNotDisturb),
        (Some(Id::new(1)), PresenceStatus::Idle),
    ] {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
            channel_id,
            "project chat",
            "group-dm",
            vec![ChannelRecipientInfo::test(Id::new(20), "alice")],
        )));

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            presence: crate::discord::PresenceEventFields {
                user_id: Id::new(20),
                status,
                activities: Vec::new(),
            },
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, status, "{guild_id:?}");
    }
}

#[test]
fn presence_update_caches_user_activities() {
    let mut state = DiscordState::default();
    let user_id: Id<UserMarker> = Id::new(20);
    let activity = ActivityInfo::test(ActivityKind::Playing, "Concord");

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(Id::new(1)),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![activity.clone()],
        },
    });

    assert_eq!(
        state.user_activities(user_id),
        std::slice::from_ref(&activity)
    );

    // Empty activities array clears the cached entry.
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(Id::new(1)),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
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
        guild_id: Some(guild_a),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![activity_a.clone()],
        },
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(guild_b),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Idle,
            activities: vec![activity_b.clone()],
        },
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
        guild_id: Some(guild_a),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::DoNotDisturb,
            activities: Vec::new(),
        },
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
        guild_id: Some(stale_guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![old_activity],
        },
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![activity.clone()],
        },
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(empty_guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
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
        guild_id: Some(guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![guild_activity.clone()],
        },
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![global_activity.clone()],
        },
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
    state.apply_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        before: Some(Id::new(30)),
        messages: vec![message_info(channel_id, 20, "old")],
    }));

    let channel = state
        .channel(channel_id)
        .expect("thread should stay cached");
    assert_eq!(channel.message_count, Some(13));
    assert_eq!(channel.total_message_sent, Some(15));
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
