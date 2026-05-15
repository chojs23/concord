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
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![guild_text_channel(guild_id, channel_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::SelectedMessageChannelChanged { channel_id: None });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![notification_settings(
            guild_id,
            NotificationLevel::AllMessages,
        )],
    });

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
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![guild_text_channel(guild_id, channel_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(29)),
            mention_count: 0,
        }],
    });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![notification_settings(
            guild_id,
            NotificationLevel::AllMessages,
        )],
    });
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![MessageInfo {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: Some("loaded".to_owned()),
            ..MessageInfo::default()
        }],
    });

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
            channel_id,
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            mute_end_time: None,
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![guild_text_channel(guild_id, channel_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![settings],
    });

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
            channel_id: category_id,
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            mute_end_time: None,
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: category_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "category".to_owned(),
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
                guild_id: Some(guild_id),
                channel_id,
                parent_id: Some(category_id),
                position: Some(1),
                last_message_id: Some(Id::new(30)),
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
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![settings],
    });

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
            channel_id: category_id,
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            mute_end_time: None,
        });
    settings
        .channel_overrides
        .push(ChannelNotificationOverrideInfo {
            channel_id,
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: false,
            mute_end_time: None,
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: category_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "category".to_owned(),
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
                guild_id: Some(guild_id),
                channel_id,
                parent_id: Some(category_id),
                position: Some(1),
                last_message_id: Some(Id::new(30)),
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
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![settings],
    });

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
fn only_mentions_settings_count_direct_mentions() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let author_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![guild_text_channel(guild_id, channel_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![notification_settings(
            guild_id,
            NotificationLevel::OnlyMentions,
        )],
    });

    state.apply_event(&message_create(
        Some(guild_id),
        channel_id,
        Id::new(30),
        author_id,
        "hello @me",
        vec![mention_info(current_user_id.get(), "me")],
    ));

    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Mentioned(1)
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
    state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![private_notification_settings(
            NotificationLevel::AllMessages,
        )],
    });

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
            channel_id,
            message_notifications: Some(NotificationLevel::NoMessages),
            muted: false,
            mute_end_time: None,
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![settings],
    });

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
            channel_id,
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: true,
            mute_end_time: None,
        });

    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![settings],
    });

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
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![guild_text_channel(guild_id, guild_channel_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.apply_event(&AppEvent::ChannelUpsert(private_channel(
        private_channel_id,
    )));
    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![private_notification_settings(NotificationLevel::NoMessages)],
    });

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

    state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![notification_settings(
            guild_id,
            NotificationLevel::OnlyMentions,
        )],
    });

    assert_eq!(
        state.channel_unread(private_channel_id),
        ChannelUnreadState::Notified(1)
    );
}
