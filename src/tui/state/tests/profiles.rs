use super::*;

#[test]
fn opening_profile_uses_cache_for_same_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile: profile_info(user_id.get(), Some("guild nick")),
    });

    assert_eq!(state.open_user_profile_popup(user_id, Some(guild_id)), None);
    assert_eq!(
        state
            .user_profile_popup_data()
            .and_then(|profile| profile.guild_nick.as_deref()),
        Some("guild nick")
    );
}

#[test]
fn opening_profile_refetches_when_cached_for_different_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let cached_guild: Id<GuildMarker> = Id::new(1);
    let popup_guild: Id<GuildMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(cached_guild),
        profile: profile_info(user_id.get(), Some("cached nick")),
    });

    assert_eq!(
        state.open_user_profile_popup(user_id, Some(popup_guild)),
        Some(AppCommand::LoadUserProfile {
            user_id,
            guild_id: Some(popup_guild),
        })
    );
    assert!(state.user_profile_popup_data().is_none());
}

#[test]
fn user_profile_load_failure_marks_open_popup_failed() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.open_user_profile_popup(user_id, Some(guild_id));
    state.push_event(AppEvent::UserProfileLoadFailed {
        user_id,
        guild_id: Some(guild_id),
        message: "network failed".to_owned(),
    });

    assert_eq!(
        state.user_profile_popup_load_error(),
        Some("network failed")
    );
}

#[test]
fn user_profile_load_failure_ignores_stale_popup() {
    let user_id: Id<UserMarker> = Id::new(10);
    let open_guild: Id<GuildMarker> = Id::new(1);
    let stale_guild: Id<GuildMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.open_user_profile_popup(user_id, Some(open_guild));
    state.push_event(AppEvent::UserProfileLoadFailed {
        user_id,
        guild_id: Some(stale_guild),
        message: "stale failure".to_owned(),
    });

    assert_eq!(state.user_profile_popup_load_error(), None);
}

#[test]
fn user_profile_popup_status_uses_cached_guild_member_status() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id,
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: vec![(user_id, PresenceStatus::DoNotDisturb)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.open_user_profile_popup(user_id, Some(guild_id));

    assert_eq!(
        state.user_profile_popup_status(),
        PresenceStatus::DoNotDisturb
    );
}

#[test]
fn user_profile_popup_status_uses_dm_recipient_status_without_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(20),
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "neo".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id,
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: Some(PresenceStatus::Idle),
        }]),
        permission_overwrites: Vec::new(),
    }));
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}

#[test]
fn user_profile_popup_status_uses_cached_presence_without_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserPresenceUpdate {
        user_id,
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}

#[test]
fn user_profile_popup_status_prefers_cached_presence_over_unknown_recipient() {
    let user_id: Id<UserMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserPresenceUpdate {
        user_id,
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(20),
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
            status: Some(PresenceStatus::Unknown),
        }]),
        permission_overwrites: Vec::new(),
    }));
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}
