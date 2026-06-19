use super::*;

#[test]
fn tracks_current_user_from_ready() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    assert_eq!(state.current_user(), Some("neo"));
    assert_eq!(state.current_user_id(), Some(Id::new(10)));
}

#[test]
fn desktop_notification_for_event_formats_eligible_guild_message() {
    let mut state = state_with_hidden_and_visible_channels();
    let channel_id = Id::new(3);
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
        },
    ]));
    let event = notification_message_event(channel_id, "hello from concord");

    let notification = state
        .desktop_notification_for_event(&event)
        .expect("eligible message should produce notification");

    assert_eq!(notification.title, "neo in guild #general");
    assert_eq!(notification.body, "hello from concord");
}

#[test]
fn desktop_notification_for_event_suppresses_muted_channel() {
    let mut state = state_with_hidden_and_visible_channels();
    let channel_id = Id::new(3);
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                ..ChannelNotificationOverrideInfo::test(channel_id)
            }],
            ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
        },
    ]));
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}

#[test]
fn desktop_notification_for_event_suppresses_active_channel() {
    let mut state = state_with_writable_channel();
    let channel_id = Id::new(2);
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
        },
    ]));
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}

#[test]
fn desktop_notification_for_event_respects_notification_opt_out() {
    let mut state = DashboardState::new_with_notification_options(NotificationOptions {
        desktop_notifications: false,
        ..NotificationOptions::default()
    });
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);

    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: None,
        channels: vec![positioned_text_channel_info(
            guild_id, channel_id, "general", 0,
        )],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(guild_id))
        },
    ]));
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}
