use super::*;
use crate::discord::{
    ActivityInfo, AppCommand, GlobalUserProfileUpdate, GuildUserProfileUpdate,
    MessageAttachmentUpload, ProfileAvatarUpload, UserProfileUpdate,
};
use crate::tui::state::UserProfileSettingsField;

#[test]
fn opening_profile_uses_cache_for_same_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile: profile_info(user_id.get(), Some("guild nick")),
    });

    assert_eq!(
        state.open_user_profile_popup(user_id, Some(guild_id)),
        Some(AppCommand::LoadUserProfile {
            user_id,
            guild_id: Some(guild_id),
        })
    );
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
        members: vec![member_info(user_id, "neo")],
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
        recipients: Some(vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::Idle),
            ..ChannelRecipientInfo::test(user_id, "neo")
        }]),
        ..dm_channel_info(Id::new(20), "neo")
    }));
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}

#[test]
fn user_profile_popup_status_uses_cached_presence_without_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        },
    });
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}

#[test]
fn user_profile_popup_status_prefers_cached_presence_over_unknown_recipient() {
    let user_id: Id<UserMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        },
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        recipients: Some(vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::Unknown),
            ..ChannelRecipientInfo::test(user_id, "test-user")
        }]),
        ..dm_channel_info(Id::new(20), "test-user")
    }));
    state.open_user_profile_popup(user_id, None);

    assert_eq!(state.user_profile_popup_status(), PresenceStatus::Idle);
}

#[test]
fn opening_current_user_profile_uses_active_user() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });

    assert_eq!(
        state.open_current_user_profile_popup(),
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(10),
            guild_id: None,
        })
    );
}

#[test]
fn profile_settings_save_dispatches_dirty_global_fields() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    for value in "Neo".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(
        state.save_user_profile_settings_command(),
        Some(AppCommand::UpdateUserProfile {
            update: UserProfileUpdate {
                user_id: Id::new(10),
                guild_id: None,
                global: GlobalUserProfileUpdate {
                    display_name: Some("Neo".to_owned()),
                    pronouns: None,
                    avatar: None,
                },
                guild: None,
            },
        })
    );
}

#[test]
fn profile_settings_sign_out_dispatches_command_and_status() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');

    assert_eq!(state.sign_out_command(), Some(AppCommand::SignOut));
    assert!(!state.is_user_profile_popup_editing());
    assert_eq!(state.user_profile_settings_status(), Some("Signing out..."));
}

#[test]
fn profile_settings_sign_out_ignores_other_profiles() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_user_profile_popup(Id::new(20), None);

    assert_eq!(state.sign_out_command(), None);
}

#[test]
fn profile_settings_text_editing_uses_cursor() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("hello world");
    state.move_user_profile_edit_cursor_word_left();
    state.insert_user_profile_edit_text("brave ");

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "hello brave world"
    );

    state.delete_previous_user_profile_edit_word();
    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "hello world"
    );

    state.move_user_profile_edit_cursor_home();
    state.insert_user_profile_edit_text("Neo ");
    state.move_user_profile_edit_cursor_end();
    state.pop_user_profile_edit_char();

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "Neo hello worl"
    );
}

#[test]
fn profile_settings_text_cursor_handles_graphemes() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("가🇰🇷나");
    state.move_user_profile_edit_cursor_left();
    state.pop_user_profile_edit_char();

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "가나"
    );
}

#[test]
fn profile_settings_dirty_count_ignores_unchanged_text_and_empty_avatar_path() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    let mut profile = profile_info(user_id.get(), None);
    profile.global_name = Some("Neo".to_owned());
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile,
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    let _ = state.start_or_commit_user_profile_edit();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("   ");
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(state.user_profile_settings_dirty_count(), 0);
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_save_dispatches_pasted_avatar_upload() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    assert!(
        state.set_user_profile_avatar_from_attachment(MessageAttachmentUpload::from_bytes(
            "avatar.png".to_owned(),
            vec![1, 2, 3]
        ),)
    );

    assert_eq!(
        state.save_user_profile_settings_command(),
        Some(AppCommand::UpdateUserProfile {
            update: UserProfileUpdate {
                user_id,
                guild_id: None,
                global: GlobalUserProfileUpdate {
                    display_name: None,
                    pronouns: None,
                    avatar: Some(ProfileAvatarUpload::from_bytes(
                        "avatar.png".to_owned(),
                        vec![1, 2, 3],
                    )),
                },
                guild: None,
            },
        })
    );
}

#[test]
fn profile_settings_status_picker_dispatches_presence_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::playing("Concord")],
        },
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::CurrentStatus),
        "Online"
    );

    let _ = state.start_or_commit_user_profile_edit();
    assert!(state.is_user_profile_status_picker_open());
    state.move_user_profile_status_picker_down();
    state.move_user_profile_status_picker_down();

    assert_eq!(
        state.activate_user_profile_status_picker(),
        Some(AppCommand::UpdateCurrentUserStatus {
            status: PresenceStatus::DoNotDisturb,
        })
    );
    assert!(!state.is_user_profile_status_picker_open());
    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::CurrentStatus),
        "Do Not Disturb"
    );
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_activity_edit_dispatches_presence_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    let _ = state.start_or_commit_user_profile_edit();
    for value in "Concord".chars() {
        state.push_user_profile_edit_char(value);
    }

    assert_eq!(
        state.start_or_commit_user_profile_edit(),
        Some(AppCommand::UpdateCurrentUserActivity {
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::playing("Concord")],
        })
    );
}

#[test]
fn profile_settings_ignore_non_current_user_profile() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_user_profile_popup(Id::new(20), None);

    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');

    assert!(!state.is_user_profile_popup_editing());
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_save_dispatches_guild_fields() {
    let user_id = Id::new(10);
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_user_profile_popup(user_id, Some(guild_id));
    state.switch_user_profile_settings_to_guild();

    let _ = state.start_or_commit_user_profile_edit();
    for value in "server neo".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(
        state.save_user_profile_settings_command(),
        Some(AppCommand::UpdateUserProfile {
            update: UserProfileUpdate {
                user_id,
                guild_id: Some(guild_id),
                global: GlobalUserProfileUpdate::default(),
                guild: Some(GuildUserProfileUpdate {
                    guild_id,
                    nickname: Some("server neo".to_owned()),
                    pronouns: None,
                }),
            },
        })
    );
}

#[test]
fn profile_settings_noop_edit_does_not_dispatch_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    let mut profile = profile_info(user_id.get(), None);
    profile.global_name = Some("Neo".to_owned());
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile,
    });
    state.open_user_profile_popup(user_id, None);

    let _ = state.start_or_commit_user_profile_edit();
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(state.save_user_profile_settings_command(), None);
    assert_eq!(
        state.user_profile_settings_status(),
        Some("No profile changes to save")
    );
}

#[test]
fn profile_reload_failure_after_save_clears_saving_state() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_current_user_profile_popup();
    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');
    let _ = state.start_or_commit_user_profile_edit();
    assert!(state.save_user_profile_settings_command().is_some());
    assert!(state.user_profile_settings_saving());

    state.push_event(AppEvent::UserProfileLoadFailed {
        user_id,
        guild_id: None,
        message: "reload failed".to_owned(),
    });

    assert!(!state.user_profile_settings_saving());
    assert_eq!(
        state.user_profile_settings_status(),
        Some("Save succeeded, but profile reload failed: reload failed"),
    );
}
