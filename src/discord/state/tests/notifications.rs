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
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&user_guild_settings_init(vec![settings]));

    assert!(state.channel_notification_muted(thread_id));
    assert!(state.channel_visible_in_notification_settings(thread_id));
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
        (ChannelUnreadState::Unread, 0)
    );
}

#[test]
fn private_all_messages_settings_show_numeric_badge() {
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
        ChannelUnreadState::Notified(1)
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
            ChannelUnreadState::Unread,
            ChannelUnreadState::Unread,
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
            ChannelUnreadState::Seen,
            ChannelUnreadState::Seen,
            0,
        ),
        (
            "muted private scope",
            true,
            None,
            ChannelUnreadState::Unread,
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

        state.apply_event(&message_create(
            None,
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(state.channel_unread_message_count(channel_id), 0, "{name}");
        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Unread,
            "{name}"
        );
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
fn notification_settings_init_replaces_private_settings() {
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
        ChannelUnreadState::Unread
    );

    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        guild_id,
        NotificationLevel::OnlyMentions,
    )]));

    assert_eq!(
        state.channel_unread(private_channel_id),
        ChannelUnreadState::Notified(1)
    );
}
