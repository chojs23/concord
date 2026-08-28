use super::*;

#[test]
fn all_message_notification_settings_show_numeric_badge() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::SelectedMessageChannelChanged { channel_id: None });
    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        guild_id,
        NotificationLevel::AllMessages,
    )]));

    state.apply_event(&message_create(
        Some(guild_id),
        channel_id,
        Id::new(30),
        author_id,
        "hello",
        Vec::new(),
    ));

    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Notified(1)
    );
    assert_eq!(
        state.guild_unread(guild_id),
        ChannelUnreadState::Notified(1)
    );
    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, None);
}

#[test]
fn notification_flags_drive_low_priority_mentions_and_sidebar_visibility() {
    let guild_id = Id::new(1);
    let opted_in_channel_id = Id::new(2);
    let hidden_channel_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut settings = notification_settings(guild_id, NotificationLevel::NoMessages);
    settings.flags = 1 << 14;
    settings.hide_muted_channels = true;
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            flags: (1 << 10) | (1 << 12),
            ..ChannelNotificationOverrideInfo::test(opted_in_channel_id)
        });
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            muted: true,
            ..ChannelNotificationOverrideInfo::test(hidden_channel_id)
        });

    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "opted-in".to_owned(),
                ..channel_info(opted_in_channel_id, "GuildText", Vec::new())
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "hidden".to_owned(),
                ..channel_info(hidden_channel_id, "GuildText", Vec::new())
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&user_guild_settings_init(vec![settings]));
    state.apply_event(&AppEvent::UserNotificationSettingsUpdate { flags: 1 << 5 });

    let message = message_create(
        Some(guild_id),
        opted_in_channel_id,
        Id::new(30),
        author_id,
        "hello",
        Vec::new(),
    );
    assert!(
        !state.message_event_triggers_notification(&message),
        "low-priority all-message mentions should not play a notification"
    );
    state.apply_event(&message);

    assert_eq!(
        state.channel_sidebar_unread(opted_in_channel_id),
        ChannelUnreadState::Mentioned(1)
    );
    assert_eq!(
        state
            .sidebar_channels_for_guild(Some(guild_id))
            .into_iter()
            .map(|channel| channel.id)
            .collect::<Vec<_>>(),
        vec![opted_in_channel_id]
    );
    assert_eq!(
        state
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .map(|channel| channel.id)
            .collect::<Vec<_>>(),
        vec![opted_in_channel_id, hidden_channel_id],
        "notification hiding must not change permission-based visibility"
    );
}

#[test]
fn loaded_guild_messages_use_notification_numeric_badge() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(channel_id, Some(Id::new(29)), 0)],
    });
    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        guild_id,
        NotificationLevel::AllMessages,
    )]));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: Some("loaded".to_owned()),
            ..MessageInfo::default()
        }],
    ));

    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Notified(1)
    );
    assert_eq!(
        state.guild_unread(guild_id),
        ChannelUnreadState::Notified(1)
    );
}

#[test]
fn muted_channel_does_not_add_numeric_notification_badge() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();
    let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            ..ChannelNotificationOverrideInfo::test(channel_id)
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&user_guild_settings_init(vec![settings]));

    state.apply_event(&message_create(
        Some(guild_id),
        channel_id,
        Id::new(30),
        author_id,
        "hello",
        Vec::new(),
    ));

    assert_eq!(state.channel_unread_message_count(channel_id), 0);
    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
    assert_eq!(
        state.channel_sidebar_unread(channel_id),
        ChannelUnreadState::Seen
    );
    assert_eq!(
        state.guild_sidebar_unread(guild_id),
        ChannelUnreadState::Seen
    );
}

#[test]
fn thread_notification_settings_walk_the_full_channel_ancestry() {
    let guild_id = Id::new(1);
    let category_id = Id::new(2);
    let channel_id = Id::new(3);
    let thread_id = Id::new(4);
    let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
    settings.flags = 1 << 14;
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            muted: true,
            flags: 1 << 12,
            ..ChannelNotificationOverrideInfo::test(category_id)
        });
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![
            guild_category_channel(guild_id, category_id, "category", 0),
            guild_child_text_channel(guild_id, channel_id, category_id, "general", 0),
            guild_thread_channel(guild_id, thread_id, channel_id, "thread"),
        ],
        current_user_thread_members: vec![ThreadMemberInfo {
            thread_id: Some(thread_id),
            user_id: Some(Id::new(10)),
            join_timestamp: None,
            flags: None,
            muted: Some(false),
            mute_end_time: None,
            selected_time_window: None,
            member: None,
            presence: None,
            extra_fields: BTreeMap::new(),
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&user_guild_settings_init(vec![settings]));

    assert!(state.channel_notification_muted(thread_id));
    assert!(state.channel_visible_in_notification_settings(thread_id));
}

#[test]
fn thread_notification_policy_uses_membership_activity_permissions_mute_and_level() {
    const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;

    struct Case {
        name: &'static str,
        joined: bool,
        archived: bool,
        can_view: bool,
        muted: bool,
        flags: Option<u64>,
        mentions_current_user: bool,
        audible: bool,
        unread_count: usize,
        inbox: ChannelUnreadState,
    }

    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);

    for case in [
        Case {
            name: "joined active thread inherits all messages",
            joined: true,
            archived: false,
            can_view: true,
            muted: false,
            flags: None,
            mentions_current_user: false,
            audible: true,
            unread_count: 1,
            inbox: ChannelUnreadState::Notified(1),
        },
        Case {
            name: "unjoined thread",
            joined: false,
            archived: false,
            can_view: true,
            muted: false,
            flags: Some(2),
            mentions_current_user: true,
            audible: false,
            unread_count: 1,
            inbox: ChannelUnreadState::Seen,
        },
        Case {
            name: "archived thread",
            joined: true,
            archived: true,
            can_view: true,
            muted: false,
            flags: Some(2),
            mentions_current_user: true,
            audible: false,
            unread_count: 1,
            inbox: ChannelUnreadState::Seen,
        },
        Case {
            name: "hidden thread",
            joined: true,
            archived: false,
            can_view: false,
            muted: false,
            flags: Some(2),
            mentions_current_user: true,
            audible: false,
            unread_count: 1,
            inbox: ChannelUnreadState::Seen,
        },
        Case {
            name: "muted thread member",
            joined: true,
            archived: false,
            can_view: true,
            muted: true,
            flags: Some(2),
            mentions_current_user: true,
            audible: false,
            unread_count: 1,
            inbox: ChannelUnreadState::Seen,
        },
        Case {
            name: "all messages thread flag",
            joined: true,
            archived: false,
            can_view: true,
            muted: false,
            flags: Some(2),
            mentions_current_user: false,
            audible: true,
            unread_count: 1,
            inbox: ChannelUnreadState::Notified(1),
        },
        Case {
            name: "mentions-only thread flag without mention",
            joined: true,
            archived: false,
            can_view: true,
            muted: false,
            flags: Some(4),
            mentions_current_user: false,
            audible: false,
            unread_count: 0,
            inbox: ChannelUnreadState::Unread,
        },
        Case {
            name: "mentions-only thread flag with mention",
            joined: true,
            archived: false,
            can_view: true,
            muted: false,
            flags: Some(4),
            mentions_current_user: true,
            audible: true,
            unread_count: 1,
            inbox: ChannelUnreadState::Mentioned(1),
        },
        Case {
            name: "no-messages thread flag",
            joined: true,
            archived: false,
            can_view: true,
            muted: false,
            flags: Some(8),
            mentions_current_user: true,
            audible: false,
            unread_count: 1,
            inbox: ChannelUnreadState::Mentioned(1),
        },
    ] {
        let parent_permissions = if case.can_view { VIEW_CHANNEL } else { 0 };
        let mut thread = guild_thread_channel(guild_id, thread_id, parent_id, "post");
        thread.kind = "GuildPublicThread".to_owned();
        thread.thread_metadata = Some(crate::discord::ThreadMetadataInfo::test(
            case.archived,
            false,
        ));
        let current_user_thread_members = case
            .joined
            .then(|| ThreadMemberInfo {
                thread_id: Some(thread_id),
                user_id: Some(current_user_id),
                join_timestamp: None,
                flags: case.flags,
                muted: Some(case.muted),
                mute_end_time: None,
                selected_time_window: None,
                member: None,
                presence: None,
                extra_fields: BTreeMap::new(),
            })
            .into_iter()
            .collect();
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&guild_create_event(GuildCreateFixture {
            guild_id,
            owner_id: Some(Id::new(99)),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    name: "forum".to_owned(),
                    kind: "forum".to_owned(),
                    ..channel_info(parent_id, "forum", Vec::new())
                },
                thread,
            ],
            current_user_thread_members,
            members: vec![member_with_roles(current_user_id, "me", Vec::new())],
            roles: vec![role_info(
                Id::new(guild_id.get()),
                "@everyone",
                parent_permissions,
            )],
            ..GuildCreateFixture::new(guild_id)
        }));
        state.apply_event(&user_guild_settings_init(vec![notification_settings(
            guild_id,
            NotificationLevel::AllMessages,
        )]));

        let mentions = if case.mentions_current_user {
            vec![mention_info(current_user_id.get(), "me")]
        } else {
            Vec::new()
        };
        let event = message_create(
            Some(guild_id),
            thread_id,
            Id::new(30),
            author_id,
            "hello",
            mentions,
        );

        assert_eq!(
            state.message_event_triggers_notification(&event),
            case.audible,
            "{}",
            case.name
        );
        state.apply_event(&event);
        assert_eq!(
            state.channel_unread_message_count(thread_id),
            case.unread_count,
            "{}",
            case.name
        );
        assert_eq!(
            state.channel_inbox_unread(thread_id),
            case.inbox,
            "{}",
            case.name
        );
        assert_eq!(
            state.channel_sidebar_unread(thread_id),
            case.inbox,
            "{}",
            case.name
        );
    }
}

#[test]
fn only_mentions_settings_use_resolved_mentions() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let role_id = Id::new(40);
    let suppress_notifications = 1 << 12;
    let unread_for = |content: &str,
                      mentions: Vec<MentionInfo>,
                      mention_everyone: bool,
                      mention_roles: Vec<Id<RoleMarker>>,
                      flags: u64| {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&guild_create_event(GuildCreateFixture {
            guild_id,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                ..channel_info(channel_id, "GuildText", Vec::new())
            }],
            members: vec![member_with_roles(current_user_id, "me", vec![role_id])],
            roles: vec![role_info(role_id, "notify", 0)],
            ..GuildCreateFixture::new(guild_id)
        }));
        state.apply_event(&user_guild_settings_init(vec![notification_settings(
            guild_id,
            NotificationLevel::OnlyMentions,
        )]));
        state.apply_event(&message_create_event(MessageCreateFixture {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(30),
            author_id,
            content: Some(content.to_owned()),
            mentions,
            mention_everyone,
            mention_roles,
            flags,
            ..MessageCreateFixture::test_fixture_default()
        }));
        (
            state.channel_unread(channel_id),
            state.channel_unread_message_count(channel_id),
        )
    };

    assert_eq!(
        unread_for(
            "hello @me",
            vec![mention_info(current_user_id.get(), "me")],
            false,
            Vec::new(),
            0,
        ),
        (ChannelUnreadState::Mentioned(1), 1)
    );
    assert_eq!(
        unread_for("@everyone", Vec::new(), false, Vec::new(), 0),
        (ChannelUnreadState::Unread, 0)
    );
    assert_eq!(
        unread_for("@everyone", Vec::new(), true, Vec::new(), 0),
        (ChannelUnreadState::Mentioned(1), 1)
    );
    assert_eq!(
        unread_for("<@&40>", Vec::new(), false, Vec::new(), 0),
        (ChannelUnreadState::Unread, 0)
    );
    assert_eq!(
        unread_for("<@&40>", Vec::new(), false, vec![role_id], 0),
        (ChannelUnreadState::Mentioned(1), 1)
    );
    assert_eq!(
        unread_for(
            "@everyone",
            Vec::new(),
            true,
            Vec::new(),
            suppress_notifications,
        ),
        (ChannelUnreadState::Mentioned(1), 1)
    );
}

#[test]
fn private_messages_increment_discord_mention_count() {
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(channel_id, "dm")));
    state.apply_event(&user_guild_settings_init(vec![
        private_notification_settings(NotificationLevel::AllMessages),
    ]));

    state.apply_event(&message_create(
        None,
        channel_id,
        Id::new(30),
        author_id,
        "hello",
        Vec::new(),
    ));

    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Mentioned(1)
    );
    assert_eq!(state.channel_unread_message_count(channel_id), 1);
}

#[test]
fn private_notification_settings_control_unread_surfaces() {
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);

    for (
        name,
        scope_muted,
        channel_override,
        mentions_current_user,
        unread_count,
        unread,
        sidebar_unread,
        inbox_unread,
        direct_message_unread,
    ) in [
        (
            "no-messages level",
            false,
            Some(ChannelNotificationOverrideInfo {
                message_notifications: Some(NotificationLevel::NoMessages),
                ..ChannelNotificationOverrideInfo::test(channel_id)
            }),
            false,
            1,
            ChannelUnreadState::Mentioned(1),
            ChannelUnreadState::Mentioned(1),
            ChannelUnreadState::Mentioned(1),
            1,
        ),
        (
            "muted channel",
            false,
            Some(ChannelNotificationOverrideInfo {
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                ..ChannelNotificationOverrideInfo::test(channel_id)
            }),
            false,
            0,
            ChannelUnreadState::Unread,
            ChannelUnreadState::Seen,
            ChannelUnreadState::Seen,
            0,
        ),
        (
            "muted channel with a direct mention",
            false,
            Some(ChannelNotificationOverrideInfo {
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                ..ChannelNotificationOverrideInfo::test(channel_id)
            }),
            true,
            1,
            ChannelUnreadState::Mentioned(1),
            ChannelUnreadState::Seen,
            ChannelUnreadState::Seen,
            0,
        ),
        (
            "muted private scope",
            true,
            None,
            false,
            0,
            ChannelUnreadState::Unread,
            ChannelUnreadState::Unread,
            ChannelUnreadState::Seen,
            0,
        ),
        (
            "muted private scope with a direct mention",
            true,
            None,
            true,
            1,
            ChannelUnreadState::Mentioned(1),
            ChannelUnreadState::Mentioned(1),
            ChannelUnreadState::Seen,
            0,
        ),
    ] {
        let mut state = DiscordState::default();
        let mut settings = private_notification_settings(NotificationLevel::AllMessages);
        settings.muted = scope_muted;
        settings.channel_overrides.extend(channel_override);

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(dm_channel(channel_id, "dm")));
        state.apply_event(&user_guild_settings_init(vec![settings]));

        let mentions = mentions_current_user
            .then(|| mention_info(current_user_id.get(), "me"))
            .into_iter()
            .collect();
        let event = message_create(None, channel_id, Id::new(30), author_id, "hello", mentions);
        assert!(!state.message_event_triggers_notification(&event), "{name}");
        state.apply_event(&event);

        assert_eq!(
            state.channel_unread_message_count(channel_id),
            unread_count,
            "{name}"
        );
        assert_eq!(state.channel_unread(channel_id), unread, "{name}");
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            sidebar_unread,
            "{name}"
        );
        assert_eq!(
            state.channel_inbox_unread(channel_id),
            inbox_unread,
            "{name}"
        );
        assert_eq!(
            state.direct_message_unread_count(),
            direct_message_unread,
            "{name}"
        );
    }
}

#[test]
fn relationship_events_update_the_notification_center_badge() {
    let current_user_id = Id::new(10);
    let requester_id = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });

    state.apply_event(&AppEvent::RelationshipUpsert {
        relationship: relationship_info(
            requester_id.get(),
            FriendStatus::IncomingRequest,
            None,
            None,
            None,
        ),
    });
    assert_eq!(notification_center_badge(&state, current_user_id), 1);

    state.apply_event(&AppEvent::RelationshipUpsert {
        relationship: relationship_info(requester_id.get(), FriendStatus::Friend, None, None, None),
    });
    assert_eq!(notification_center_badge(&state, current_user_id), 0);

    state.apply_event(&AppEvent::RelationshipUpsert {
        relationship: relationship_info(
            requester_id.get(),
            FriendStatus::IncomingRequest,
            None,
            None,
            None,
        ),
    });
    state.apply_event(&AppEvent::RelationshipRemove {
        user_id: requester_id,
        status: Some(FriendStatus::IncomingRequest),
    });
    state.apply_event(&AppEvent::RelationshipRemove {
        user_id: requester_id,
        status: Some(FriendStatus::IncomingRequest),
    });
    assert_eq!(notification_center_badge(&state, current_user_id), 0);
}

#[test]
fn current_user_thread_create_marks_the_forum_parent_read() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(2);
    let thread_id = Id::new(30);
    let current_user_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            last_message_id: Some(Id::new(thread_id.get())),
            name: "forum".to_owned(),
            ..channel_info(forum_id, "forum", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            last_acked_message_id: Some(Id::new(20)),
            mention_count: 2,
            ..ReadStateInfo::test(forum_id)
        }],
    });

    state.apply_event(&AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                owner_id: Some(current_user_id),
                name: "post".to_owned(),
                ..channel_info(thread_id, "GuildPublicThread", Vec::new())
            },
            current_user_member: None,
        },
        created: true,
    });

    assert_eq!(
        state.channel_last_acked_message_id(forum_id),
        Some(Id::new(thread_id.get()))
    );
    assert_eq!(state.channel_unread(forum_id), ChannelUnreadState::Seen);
}

fn notification_center_badge(state: &DiscordState, current_user_id: Id<UserMarker>) -> u32 {
    state
        .notifications
        .non_channel_read_states
        .get(&(2, current_user_id.get()))
        .map_or(0, |read_state| read_state.badge_count)
}

#[test]
fn notification_settings_init_clears_private_settings() {
    let guild_id = Id::new(1);
    let guild_channel_id = Id::new(2);
    let private_channel_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(guild_channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(
        private_channel_id,
        "dm",
    )));
    state.apply_event(&user_guild_settings_init(vec![
        private_notification_settings(NotificationLevel::NoMessages),
    ]));

    state.apply_event(&message_create(
        None,
        private_channel_id,
        Id::new(30),
        author_id,
        "hello",
        Vec::new(),
    ));
    assert_eq!(
        state.channel_unread(private_channel_id),
        ChannelUnreadState::Mentioned(1)
    );

    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        guild_id,
        NotificationLevel::OnlyMentions,
    )]));

    assert_eq!(
        state
            .guild_notification_settings_info(None)
            .message_notifications,
        None
    );
    assert_eq!(
        state.channel_unread(private_channel_id),
        ChannelUnreadState::Mentioned(1)
    );
}

#[test]
fn versioned_notification_settings_merge_partial_and_clear_full_empty_snapshots() {
    let first = Id::new(1);
    let second = Id::new(2);
    let mut state = DiscordState::default();
    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        first,
        NotificationLevel::AllMessages,
    )]));

    state.apply_event(&AppEvent::UserGuildSettingsSync {
        settings: vec![UserGuildSettingsInfo {
            notification_settings: notification_settings(second, NotificationLevel::OnlyMentions),
            extra_fields: BTreeMap::new(),
        }],
        partial: true,
        version: Some(7),
    });
    assert!(
        state
            .notifications
            .notification_settings
            .contains_key(&first)
    );
    assert!(
        state
            .notifications
            .notification_settings
            .contains_key(&second)
    );
    assert_eq!(state.notifications.user_guild_settings_version, Some(7));

    state.apply_event(&AppEvent::UserGuildSettingsSync {
        settings: Vec::new(),
        partial: false,
        version: Some(8),
    });
    assert!(state.notifications.notification_settings.is_empty());
    assert!(state.notifications.private_notification_settings.is_none());
    assert_eq!(state.notifications.user_guild_settings_version, Some(8));
}

#[test]
fn mute_duration_metadata_round_trips_through_notification_state() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut settings = notification_settings(guild_id, NotificationLevel::OnlyMentions);
    settings.muted = true;
    settings.mute_end_time = Some("2099-01-01T00:00:00.000Z".to_owned());
    settings.selected_time_window = Some(3600);
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            muted: true,
            mute_end_time: Some("2099-01-02T00:00:00.000Z".to_owned()),
            selected_time_window: Some(900),
            ..ChannelNotificationOverrideInfo::test(channel_id)
        });

    let mut state = DiscordState::default();
    state.apply_event(&user_guild_settings_init(vec![settings]));

    let cached = state.guild_notification_settings_info(Some(guild_id));
    assert_eq!(
        cached.mute_end_time.as_deref(),
        Some("2099-01-01T00:00:00.000Z")
    );
    assert_eq!(cached.selected_time_window, Some(3600));
    assert_eq!(cached.channel_overrides.len(), 1);
    assert_eq!(
        cached.channel_overrides[0].mute_end_time.as_deref(),
        Some("2099-01-02T00:00:00.000Z")
    );
    assert_eq!(cached.channel_overrides[0].selected_time_window, Some(900));
}

#[test]
fn user_guild_settings_updates_advance_the_cached_version() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::UserGuildSettingsSync {
        settings: Vec::new(),
        partial: false,
        version: Some(7),
    });

    let mut first = notification_settings(guild_id, NotificationLevel::AllMessages);
    first.version = 8;
    state.apply_event(&AppEvent::UserGuildSettingsUpdate {
        settings: UserGuildSettingsInfo {
            notification_settings: first,
            extra_fields: BTreeMap::new(),
        },
    });

    let mut newer = notification_settings(guild_id, NotificationLevel::OnlyMentions);
    newer.version = 9;
    state.apply_event(&AppEvent::UserGuildSettingsUpdate {
        settings: UserGuildSettingsInfo {
            notification_settings: newer,
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(state.notifications.user_guild_settings_version, Some(9));
    assert_eq!(
        state
            .guild_notification_settings_info(Some(guild_id))
            .message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
}

#[test]
fn explicit_channel_override_beats_a_muted_parent_category() {
    struct Case {
        name: &'static str,
        channel_override: bool,
        muted: bool,
        unread_count: usize,
        unread: ChannelUnreadState,
        sidebar: ChannelUnreadState,
        guild_sidebar: ChannelUnreadState,
    }

    let guild_id = Id::new(1);
    let category_id = Id::new(2);
    let channel_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);

    for case in [
        Case {
            name: "muted category only",
            channel_override: false,
            muted: true,
            unread_count: 0,
            unread: ChannelUnreadState::Unread,
            sidebar: ChannelUnreadState::Seen,
            guild_sidebar: ChannelUnreadState::Seen,
        },
        Case {
            name: "channel override under a muted category",
            channel_override: true,
            muted: false,
            unread_count: 1,
            unread: ChannelUnreadState::Notified(1),
            sidebar: ChannelUnreadState::Notified(1),
            guild_sidebar: ChannelUnreadState::Notified(1),
        },
    ] {
        let mut state = DiscordState::default();
        let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                ..ChannelNotificationOverrideInfo::test(category_id)
            });
        if case.channel_override {
            settings
                .channel_overrides
                .push(ChannelNotificationOverrideInfo {
                    message_notifications: Some(NotificationLevel::AllMessages),
                    ..ChannelNotificationOverrideInfo::test(channel_id)
                });
        }

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&guild_create_event(GuildCreateFixture {
            guild_id,
            channels: vec![
                guild_category_channel(guild_id, category_id, "category", 0),
                ChannelInfo {
                    last_message_id: Some(Id::new(30)),
                    ..guild_child_text_channel(guild_id, channel_id, category_id, "general", 1)
                },
            ],
            ..GuildCreateFixture::new(guild_id)
        }));
        state.apply_event(&user_guild_settings_init(vec![settings]));

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(
            state.channel_notification_muted(channel_id),
            case.muted,
            "{}",
            case.name
        );
        assert_eq!(
            state.channel_unread_message_count(channel_id),
            case.unread_count,
            "{}",
            case.name
        );
        assert_eq!(
            state.channel_unread(channel_id),
            case.unread,
            "{}",
            case.name
        );
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            case.sidebar,
            "{}",
            case.name
        );
        assert_eq!(
            state.guild_sidebar_unread(guild_id),
            case.guild_sidebar,
            "{}",
            case.name
        );
    }
}
