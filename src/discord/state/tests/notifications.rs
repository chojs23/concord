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
fn muted_parent_category_does_not_add_server_sidebar_unread() {
    let guild_id = Id::new(1);
    let category_id = Id::new(2);
    let channel_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();
    let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            ..ChannelNotificationOverrideInfo::test(category_id)
        });

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

    assert!(state.channel_notification_muted(channel_id));
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
fn explicit_channel_unmute_override_beats_muted_parent_category() {
    let guild_id = Id::new(1);
    let category_id = Id::new(2);
    let channel_id = Id::new(3);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();
    let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            ..ChannelNotificationOverrideInfo::test(category_id)
        });
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..ChannelNotificationOverrideInfo::test(channel_id)
        });

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

    assert!(!state.channel_notification_muted(channel_id));
    assert_eq!(state.channel_unread_message_count(channel_id), 1);
    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Notified(1)
    );
    assert_eq!(
        state.channel_sidebar_unread(channel_id),
        ChannelUnreadState::Notified(1)
    );
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
fn private_channel_override_no_messages_suppresses_numeric_badge() {
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();
    let mut settings = private_notification_settings(NotificationLevel::AllMessages);
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            message_notifications: Some(NotificationLevel::NoMessages),
            ..ChannelNotificationOverrideInfo::test(channel_id)
        });

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

    assert_eq!(state.channel_unread_message_count(channel_id), 0);
    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
}

#[test]
fn muted_private_channel_override_suppresses_numeric_badge() {
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();
    let mut settings = private_notification_settings(NotificationLevel::AllMessages);
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

    assert_eq!(state.channel_unread_message_count(channel_id), 0);
    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
    assert_eq!(
        state.channel_sidebar_unread(channel_id),
        ChannelUnreadState::Seen
    );
    assert_eq!(state.direct_message_unread_count(), 0);
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
