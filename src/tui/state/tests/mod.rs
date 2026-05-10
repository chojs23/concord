mod fixtures;

use fixtures::*;

use crate::{
    config::{DisplayOptions, ImagePreviewQualityPreset},
    discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};
use unicode_width::UnicodeWidthStr;

use super::{
    ChannelActionKind, ChannelBranch, ChannelPaneEntry, DashboardState, FocusPane, GuildActionKind,
    GuildBranch, GuildPaneEntry, MessageActionKind, MessageState, message_rendered_height,
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, ChannelInfo, ChannelNotificationOverrideInfo,
    ChannelRecipientInfo, ChannelUnreadState, ChannelVisibilityStats, CustomEmojiInfo,
    DiscordState, ForumPostArchiveState, FriendStatus, GuildNotificationSettingsInfo, MemberInfo,
    MessageAttachmentUpload, MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo,
    MutualGuildInfo, NotificationLevel, PermissionOverwriteInfo, PermissionOverwriteKind,
    PresenceStatus, ReactionEmoji, ReactionInfo, ReactionUserInfo, ReactionUsersInfo,
    ReadStateInfo, ReplyInfo, RoleInfo, UserProfileInfo,
};

fn profile_info(user_id: u64, guild_nick: Option<&str>) -> UserProfileInfo {
    UserProfileInfo {
        user_id: Id::new(user_id),
        username: format!("user-{user_id}"),
        global_name: None,
        guild_nick: guild_nick.map(str::to_owned),
        role_ids: Vec::new(),
        avatar_url: None,
        bio: None,
        pronouns: None,
        mutual_guilds: Vec::<MutualGuildInfo>::new(),
        mutual_friends_count: 0,
        friend_status: FriendStatus::None,
        note: None,
    }
}

fn notification_message_event(channel_id: Id<ChannelMarker>, content: &str) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(50),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

fn direct_message_create_event(channel_id: Id<ChannelMarker>, message_id: u64) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello from dm".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

fn drain_debounced_read_ack(state: &mut DashboardState) -> Vec<AppCommand> {
    let deadline = state
        .next_read_ack_deadline()
        .expect("read ack should be scheduled");
    state.flush_due_read_acks(deadline);
    state.drain_pending_commands()
}

fn clear_scheduled_read_ack(state: &mut DashboardState) {
    if let Some(deadline) = state.next_read_ack_deadline() {
        state.flush_due_read_acks(deadline);
        state.drain_pending_commands();
    }
}

#[test]
fn tracks_current_user_from_ready() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    assert_eq!(state.current_user(), Some("neo"));
    assert_eq!(state.current_user_id, Some(Id::new(10)));
}

#[test]
fn gateway_error_stays_out_of_footer_state() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GatewayError {
        message: "boom".to_owned(),
    });

    assert_eq!(state.last_status(), None);
}

#[test]
fn desktop_notification_for_event_formats_eligible_guild_message() {
    let mut state = state_with_hidden_and_visible_channels();
    let channel_id = Id::new(3);
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }],
    });
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
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                channel_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                mute_end_time: None,
            }],
        }],
    });
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}

#[test]
fn desktop_notification_for_event_suppresses_active_channel() {
    let mut state = state_with_writable_channel();
    let channel_id = Id::new(2);
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }],
    });
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}

#[test]
fn desktop_notification_for_event_respects_display_opt_out() {
    let mut state = DashboardState::new_with_display_options(DisplayOptions {
        desktop_notifications: false,
        ..DisplayOptions::default()
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
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: Some(0),
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
    });
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(guild_id),
            message_notifications: Some(NotificationLevel::AllMessages),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }],
    });
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
}

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

#[test]
fn cycle_focus_uses_four_top_level_panes() {
    let mut state = DashboardState::new();

    assert_eq!(state.focus(), FocusPane::Guilds);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Channels);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Messages);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Members);
    state.cycle_focus();
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn loaded_messages_are_unselected_until_message_pane_is_focused() {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in 1..=2u64 {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.focused_message_selection(), None);

    while state.focus() != FocusPane::Messages {
        state.cycle_focus();
    }
    assert_eq!(state.focused_message_selection(), Some(0));
}

#[test]
fn startup_events_do_not_auto_open_direct_messages() {
    let channel_id: Id<ChannelMarker> = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
    state.push_event(AppEvent::MessageCreate {
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
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(state.selected_channel_id(), None);
    assert_eq!(state.selected_channel_state(), None);
    assert!(state.channel_pane_entries().is_empty());
    assert!(state.messages().is_empty());
}

#[test]
fn member_groups_use_roles_and_status_sorted_entries() {
    let guild_id = Id::new(1);
    let alice: Id<UserMarker> = Id::new(10);
    let bob: Id<UserMarker> = Id::new(20);
    let admin_role = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(2),
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
        members: vec![
            MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![admin_role],
            },
            MemberInfo {
                user_id: alice,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![admin_role],
            },
        ],
        presences: vec![(alice, PresenceStatus::Online), (bob, PresenceStatus::Idle)],
        roles: vec![RoleInfo {
            id: admin_role,
            name: "Admin".to_owned(),
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Admin");
    assert_eq!(groups[0].color, Some(0xFFAA00));
    assert_eq!(
        groups[0]
            .entries
            .iter()
            .map(|member| member.display_name())
            .collect::<Vec<_>>(),
        vec!["alice".to_owned(), "bob".to_owned()],
    );
}

#[test]
fn member_role_color_uses_highest_nonzero_role_color() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let low_role = Id::new(100);
    let zero_role = Id::new(101);
    let high_role = Id::new(102);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id,
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: vec![low_role, zero_role, high_role],
        }],
        presences: vec![(user_id, PresenceStatus::Online)],
        roles: vec![
            RoleInfo {
                id: low_role,
                name: "Low".to_owned(),
                color: Some(0x112233),
                position: 1,
                hoist: false,
                permissions: 0,
            },
            RoleInfo {
                id: zero_role,
                name: "Zero".to_owned(),
                color: Some(0),
                position: 99,
                hoist: false,
                permissions: 0,
            },
            RoleInfo {
                id: high_role,
                name: "High".to_owned(),
                color: Some(0x445566),
                position: 10,
                hoist: false,
                permissions: 0,
            },
        ],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    let member = state.flattened_members()[0];

    assert_eq!(state.member_role_color(member), Some(0x445566));
}

#[test]
fn member_role_color_breaks_equal_position_ties_by_role_id() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let older_role = Id::new(100);
    let newer_role = Id::new(200);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id,
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: vec![newer_role, older_role],
        }],
        presences: vec![(user_id, PresenceStatus::Online)],
        roles: vec![
            RoleInfo {
                id: newer_role,
                name: "Newer".to_owned(),
                color: Some(0x112233),
                position: 10,
                hoist: false,
                permissions: 0,
            },
            RoleInfo {
                id: older_role,
                name: "Older".to_owned(),
                color: Some(0x445566),
                position: 10,
                hoist: false,
                permissions: 0,
            },
        ],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    let member = state.flattened_members()[0];

    assert_eq!(state.member_role_color(member), Some(0x445566));
}

#[test]
fn member_groups_show_selected_group_dm_recipients() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
        recipients: Some(vec![
            ChannelRecipientInfo {
                user_id: Id::new(30),
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Idle),
            },
            ChannelRecipientInfo {
                user_id: Id::new(10),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            },
        ]),
        permission_overwrites: Vec::new(),
    }));

    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Members");
    assert_eq!(
        groups[0]
            .entries
            .iter()
            .map(|member| (member.display_name(), member.status()))
            .collect::<Vec<_>>(),
        vec![
            ("alice".to_owned(), PresenceStatus::Online),
            ("bob".to_owned(), PresenceStatus::Idle),
        ],
    );
}

#[test]
fn member_panel_title_separates_loaded_and_total_members() {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(100),
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id: Id::new(10),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: vec![(Id::new(10), PresenceStatus::Online)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    assert_eq!(state.member_panel_title(), "Members 1/100 loaded");
    assert_eq!(state.flattened_members().len(), 1);
}

#[test]
fn member_panel_title_stays_plain_without_guild_total_or_in_direct_messages() {
    let mut guild_state = DashboardState::new();
    guild_state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    guild_state.confirm_selected_guild();
    assert_eq!(guild_state.member_panel_title(), "Members");

    let mut dm_state = DashboardState::new();
    dm_state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(20),
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
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    dm_state.confirm_selected_guild();
    assert_eq!(dm_state.member_panel_title(), "Members");
}

#[test]
fn member_groups_split_role_online_and_offline_buckets() {
    let guild_id = Id::new(1);
    let admin_role = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(2),
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
        members: vec![
            MemberInfo {
                user_id: Id::new(10),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![admin_role],
            },
            MemberInfo {
                user_id: Id::new(11),
                display_name: "amy".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![admin_role],
            },
            MemberInfo {
                user_id: Id::new(20),
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(21),
                display_name: "ben".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        ],
        presences: vec![
            // Admin online, admin offline, no-role online, no-role offline
            (Id::new(10), PresenceStatus::Online),
            (Id::new(11), PresenceStatus::Offline),
            (Id::new(20), PresenceStatus::Idle),
            (Id::new(21), PresenceStatus::Offline),
        ],
        roles: vec![RoleInfo {
            id: admin_role,
            name: "Admin".to_owned(),
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(
        groups
            .iter()
            .map(|group| group.label.clone())
            .collect::<Vec<_>>(),
        vec![
            "Admin".to_owned(),
            "Online".to_owned(),
            "Offline".to_owned()
        ]
    );

    // Admin role group only carries the online admin (alice); the offline
    // admin (amy) belongs to the Offline bucket.
    let admin_names: Vec<_> = groups[0]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(admin_names, vec!["alice".to_owned()]);

    // Online group lists members with no hoisted role who aren't offline.
    let online_names: Vec<_> = groups[1]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(online_names, vec!["bob".to_owned()]);

    // Offline group merges everyone offline regardless of role.
    let offline_names: Vec<_> = groups[2]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(offline_names, vec!["amy".to_owned(), "ben".to_owned()]);
}

#[test]
fn member_groups_treat_idle_and_dnd_as_online() {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(2),
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
        members: vec![
            MemberInfo {
                user_id: Id::new(10),
                display_name: "idle".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(11),
                display_name: "dnd".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(12),
                display_name: "unknown".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        ],
        presences: vec![
            (Id::new(10), PresenceStatus::Idle),
            (Id::new(11), PresenceStatus::DoNotDisturb),
            // Unknown is treated as offline (Discord defaults to offline
            // when the gateway has not delivered a presence yet).
            (Id::new(12), PresenceStatus::Unknown),
        ],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Online");
    assert_eq!(groups[0].entries.len(), 2);
    assert_eq!(groups[1].label, "Offline");
    assert_eq!(groups[1].entries.len(), 1);
    assert_eq!(groups[1].entries[0].display_name(), "unknown");
}

#[test]
fn member_groups_show_selected_dm_recipient() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
            user_id: Id::new(10),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: Some(PresenceStatus::DoNotDisturb),
        }]),
        permission_overwrites: Vec::new(),
    }));

    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Members");
    assert_eq!(groups[0].entries.len(), 1);
    assert_eq!(groups[0].entries[0].display_name(), "alice");
    assert_eq!(groups[0].entries[0].status(), PresenceStatus::DoNotDisturb);
}

#[test]
fn emoji_picker_items_include_available_custom_emojis_for_selected_message_guild() {
    let state = state_with_custom_emojis();

    let items = state.emoji_reaction_items();

    assert_eq!(items.len(), 9);
    assert_eq!(items[0].emoji, ReactionEmoji::Unicode("👍".to_owned()));
    assert_eq!(items[8].label, "Party Time");
    assert_eq!(
        items[8].emoji,
        ReactionEmoji::Custom {
            id: Id::new(50),
            name: Some("party_time".to_owned()),
            animated: true,
        }
    );
}

#[test]
fn custom_emoji_reaction_items_expose_cdn_image_url() {
    let state = state_with_custom_emojis();

    let items = state.emoji_reaction_items();

    assert_eq!(
        items[8].custom_image_url().as_deref(),
        Some("https://cdn.discordapp.com/emojis/50.gif")
    );
    assert_eq!(items[0].custom_image_url(), None);
}

#[test]
fn emoji_picker_items_include_custom_emojis_from_update_event() {
    let guild_id = Id::new(1);
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id,
        emojis: vec![CustomEmojiInfo {
            id: Id::new(60),
            name: "wave".to_owned(),
            animated: false,
            available: true,
        }],
    });

    let items = state.emoji_reaction_items();

    assert_eq!(items.len(), 9);
    assert_eq!(items[8].label, "Wave");
    assert_eq!(
        items[8].emoji,
        ReactionEmoji::Custom {
            id: Id::new(60),
            name: Some("wave".to_owned()),
            animated: false,
        }
    );
}

#[test]
fn emoji_picker_uses_channel_guild_when_selected_message_lacks_guild_id() {
    let mut state = state_with_custom_emojis();

    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("history message without guild".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let items = state.emoji_reaction_items();

    assert_eq!(items.len(), 9);
    assert_eq!(items[8].label, "Party Time");
}

#[test]
fn emoji_picker_items_stay_unicode_only_for_direct_messages() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
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
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
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

    assert_eq!(state.emoji_reaction_items().len(), 8);
}

#[test]
fn message_creation_keeps_viewport_on_latest() {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in 1..=3u64 {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }

    assert_eq!(state.selected_message(), 2);
}

#[test]
fn message_scroll_preserves_position_when_not_following() {
    let mut state = state_with_messages(5);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(6);

    assert_eq!(state.selected_message(), 4);
    assert!(state.message_auto_follow());

    state.move_up();
    assert_eq!(state.selected_message(), 3);
    assert!(!state.message_auto_follow());

    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(6),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("msg 6".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(state.selected_message(), 3);
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
    // Cursor moved up but the viewport still showed the latest, so the new
    // event engaged auto-scroll (without moving the cursor).
    assert!(state.message_auto_follow());
}

#[test]
fn user_sent_message_from_history_position_does_not_force_follow() {
    let me: Id<UserMarker> = Id::new(10);
    let mut state = state_with_messages(5);
    // Pretend the Ready event came through so the state knows who "we" are.
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(2);

    // Scroll up far enough that the latest message is no longer visible
    // and the cursor is parked on an older message.
    state.move_up();
    state.move_up();
    state.move_up();
    assert_eq!(state.selected_message(), 1);
    assert!(!state.message_auto_follow());

    let parked_message_id = state.messages()[state.selected_message()].id;

    // Simulate the REST send response arriving as a self-authored
    // MessageCreate. Auto-follow must NOT yank the cursor down — the user
    // was reading older history, and that intent outranks the convenience
    // of jumping to their own send.
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(99),
        author_id: me,
        author: "me".to_owned(),
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

    let messages = state.messages();
    assert_eq!(messages[state.selected_message()].id, parked_message_id);
    assert!(!state.message_auto_follow());
    assert_eq!(state.new_messages_marker_message_id(), None);
}

#[test]
fn image_preview_rows_keep_latest_message_visible_when_auto_following() {
    let mut state = state_with_image_messages(6, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(6);

    assert_eq!(state.message_scroll(), 0);

    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_scroll() > 0 || state.message_line_scroll() > 0);
    let selected_bottom = state
        .selected_message_rendered_row(200, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(200, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn image_preview_scrolloff_keeps_selected_message_visible() {
    let mut state = state_with_image_messages(8, &[5, 6, 7]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);

    while state.selected_message() > 3 {
        state.move_up();
    }
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert_eq!(state.following_message_rendered_rows(200, 16, 3, 3), 21);
    let selected_bottom = state
        .selected_message_rendered_row(200, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(200, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn video_attachment_does_not_reserve_image_preview_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("clip".to_owned()),
        mentions: Vec::new(),
        attachments: vec![video_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn explicit_newlines_increase_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("hello\nworld".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn wrapped_content_increases_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("abcdefghijkl".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 5, 16, 3), 5);
}

#[test]
fn rendered_mentions_affect_message_height() {
    let mut state = state_with_single_message_content("<@10><@10>");
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: MemberInfo {
            user_id: Id::new(10),
            display_name: "a".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    let message = state.messages()[0];

    assert_eq!(message_rendered_height(message, 5, 16, 3), 4);
    assert_eq!(state.message_base_line_count_for_width(message, 5), 2);
}

#[test]
fn forwarded_mentions_affect_height_from_source_channel_guild() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(2)),
        channel_id: Id::new(9),
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "source".to_owned(),
        kind: "GuildText".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(2),
        member: MemberInfo {
            user_id: Id::new(10),
            display_name: "a".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("<@10><@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            source_channel_id: Some(Id::new(9)),
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(state.message_base_line_count_for_width(&message, 7), 4);
}

#[test]
fn wide_content_increases_message_rendered_height_by_terminal_width() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("漢字仮名交じ".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 10, 16, 3), 4);
}

#[test]
fn discord_embed_rows_increase_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: vec![youtube_embed()],
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 80, 16, 3), 8);
}

#[test]
fn image_attachment_summary_reserves_text_row_before_preview() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("look".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
}

#[test]
fn five_image_album_rendered_height_lists_each_attachment_but_keeps_album_bounded() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("look".to_owned()),
        mentions: Vec::new(),
        attachments: (1..=5).map(image_attachment).collect(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 12);
}

#[test]
fn forwarded_image_attachment_reserves_preview_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![forwarded_snapshot(1)],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn forwarded_snapshot_wrapped_content_increases_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("abcdefghijkl".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 7, 16, 3), 10);
}

#[test]
fn forwarded_snapshot_wide_content_uses_terminal_width() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("漢字仮名交じ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 12, 16, 3), 9);
}

#[test]
fn forwarded_metadata_reserves_card_row() {
    let mut snapshot = forwarded_snapshot(1);
    snapshot.source_channel_id = Some(Id::new(2));
    snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![snapshot],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}

#[test]
fn forwarded_snapshot_embed_rows_increase_rendered_height() {
    let mut snapshot = forwarded_snapshot(1);
    snapshot.attachments.clear();
    snapshot.embeds = vec![youtube_embed()];
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![snapshot],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 10);
}

#[test]
fn non_default_message_kind_reserves_label_row() {
    let mut message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("reply body".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);

    message.message_kind = MessageKind::new(19);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn reply_preview_reserves_connector_row_without_extra_type_label() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::new(19),
        reference: None,
        reply: Some(ReplyInfo {
            author: "casey".to_owned(),
            content: Some("looks good".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
        }),
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("asdf".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn poll_message_reserves_question_and_answer_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(false)),
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}

#[test]
fn poll_message_body_counts_inside_card_height() {
    let mut message = height_test_message("Please vote");
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 10);
}

#[test]
fn wrapped_poll_message_body_counts_inside_card_height() {
    let mut message = height_test_message("abcdefghijkl");
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 10, 16, 3), 11);
}

#[test]
fn thread_created_message_reserves_system_card_rows() {
    let mut message = height_test_message("release notes");
    message.message_kind = MessageKind::new(18);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
}

#[test]
fn poll_result_message_reserves_result_card_rows() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(46);
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
}

#[test]
fn poll_result_message_counts_summed_answer_votes() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(46);
    let mut poll = poll_info(false);
    poll.total_votes = None;
    poll.answers[0].vote_count = Some(2);
    poll.answers[1].vote_count = Some(1);
    message.poll = Some(poll);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
}

#[test]
fn thread_starter_message_reserves_system_card_rows() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(21);
    message.reply = Some(ReplyInfo {
        author: "alice".to_owned(),
        content: Some("original topic".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
    });

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn multiselect_poll_message_uses_same_card_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(true)),
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}

#[test]
fn message_action_items_reflect_selected_message_capabilities() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(actions.iter().any(|action| {
        action.kind == MessageActionKind::ViewImage
            && action.label == "View image"
            && action.enabled
    }));
    assert!(!actions.iter().any(|action| action.label.contains("poll")));
}

#[test]
fn disabled_image_previews_hide_view_image_action() {
    let mut state = state_with_image_messages(1, &[1]);
    state.open_options_popup();
    state.toggle_selected_display_option();
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(
        !actions
            .iter()
            .any(|action| action.kind == MessageActionKind::ViewImage)
    );
}

#[test]
fn image_preview_quality_option_cycles_presets() {
    let mut state = DashboardState::new();
    state.open_options_popup();
    for _ in 0..3 {
        state.move_option_down();
    }

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::High
    );

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::Original
    );

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::Efficient
    );
}

#[test]
fn image_message_action_opens_image_viewer() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            embeds: vec![youtube_embed()],
            ..message_info(Id::new(2), 1)
        }],
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    state.move_message_action_down();

    let command = state.activate_selected_message_action();

    assert_eq!(command, None,);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_image_viewer_open());
    assert_eq!(
        state.selected_image_viewer_item(),
        Some(super::ImageViewerItem {
            index: 1,
            total: 1,
            filename: "embed-thumbnail".to_owned(),
            url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned(),
        })
    );
}

#[test]
fn image_viewer_navigation_clamps_and_downloads_current_image() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![image_attachment(10), image_attachment(11)],
            ..message_info(Id::new(2), 1)
        }],
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    state.move_message_action_down();
    state.activate_selected_message_action();

    state.move_image_viewer_previous();
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    state.move_image_viewer_next();
    state.move_image_viewer_next();
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    state.open_image_viewer_action_menu();
    let command = state.activate_selected_image_viewer_action();

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            url: "https://cdn.discordapp.com/image-11.png".to_owned(),
            filename: "image-11.png".to_owned(),
        })
    );
    assert!(state.is_image_viewer_open());
    assert!(!state.is_image_viewer_action_menu_open());
}

#[test]
fn image_viewer_download_uses_original_url_not_preview_proxy() {
    let mut state = state_with_messages(1);
    let mut attachment = image_attachment(10);
    attachment.url = "https://cdn.discordapp.com/original/photo.png".to_owned();
    attachment.proxy_url = concat!(
        "https://media.discordapp.net/attachments/1/10/photo.png",
        "?format=webp&width=160&height=90"
    )
    .to_owned();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![attachment],
            ..message_info(Id::new(2), 1)
        }],
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    state.move_message_action_down();
    state.activate_selected_message_action();
    state.open_image_viewer_action_menu();

    let command = state.activate_selected_image_viewer_action();

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            url: "https://cdn.discordapp.com/original/photo.png".to_owned(),
            filename: "image-10.png".to_owned(),
        })
    );
}

#[test]
fn normal_message_actions_do_not_include_poll_or_image_actions() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
        ]
    );
}

#[test]
fn focused_pane_horizontal_scroll_is_scoped_by_focus() {
    let mut state = state_with_many_channels(1);

    state.scroll_focused_pane_horizontal_right();
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 0);
    assert_eq!(state.member_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Channels);
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 1);

    state.focus_pane(FocusPane::Members);
    state.scroll_focused_pane_horizontal_right();
    state.scroll_focused_pane_horizontal_left();
    state.scroll_focused_pane_horizontal_left();
    assert_eq!(state.member_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Messages);
    state.scroll_focused_pane_horizontal_right();
    assert_eq!(state.guild_horizontal_scroll(), 2);
    assert_eq!(state.channel_horizontal_scroll(), 1);
    assert_eq!(state.member_horizontal_scroll(), 0);
}

#[test]
fn focused_pane_horizontal_scroll_stops_before_blank_labels() {
    let mut state = DashboardState::new();

    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(
        state.guild_horizontal_scroll(),
        "Direct Messages".width() - 1
    );

    let mut state = state_with_many_channels(1);
    state.focus_pane(FocusPane::Channels);
    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(state.channel_horizontal_scroll(), "channel 1".width() - 1);

    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    for _ in 0..100 {
        state.scroll_focused_pane_horizontal_right();
    }

    assert_eq!(state.member_horizontal_scroll(), "member 1".width() - 1);
}

#[test]
fn own_regular_message_actions_include_edit_and_delete() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::Edit,
            MessageActionKind::Delete,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
        ]
    );
}

#[test]
fn other_user_message_actions_do_not_include_edit_or_delete() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!actions.iter().any(|action| matches!(
        action.kind,
        MessageActionKind::Edit | MessageActionKind::Delete
    )));
}

#[test]
fn edit_message_action_prefills_composer_and_submits_edit_command() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    assert!(state.select_message_action_row(1));

    assert_eq!(state.activate_selected_message_action(), None);
    assert_eq!(state.composer_input(), "msg 1");
    state.push_composer_char('!');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::EditMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            content: "msg 1!".to_owned(),
        })
    );
    assert!(!state.is_composing());
}

#[test]
fn delete_message_action_submits_delete_command_for_own_message() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    assert!(state.select_message_action_row(2));

    assert_eq!(
        state.activate_selected_message_action(),
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn own_attachment_only_message_can_be_deleted_but_not_edited() {
    let mut state = state_with_message_ids([]);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: None,
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();

    let actions = state.selected_message_action_items();
    assert!(
        actions
            .iter()
            .any(|action| action.kind == MessageActionKind::Delete)
    );
    assert!(
        !actions
            .iter()
            .any(|action| action.kind == MessageActionKind::Edit)
    );
    assert!(state.select_message_action_row(1));
    assert_eq!(
        state.activate_selected_message_action(),
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn non_image_attachment_action_downloads_with_proxy_url_fallback() {
    let mut state = state_with_message_ids([]);
    let mut attachment = video_attachment(1);
    attachment.url.clear();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("clip".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![attachment],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();

    let actions = state.selected_message_action_items();
    assert!(actions.iter().any(|action| {
        action.kind == MessageActionKind::DownloadAttachment(0)
            && action.label == "Download clip-1.mp4"
    }));
    assert!(state.select_message_action_row(1));

    assert_eq!(
        state.activate_selected_message_action(),
        Some(AppCommand::DownloadAttachment {
            url: "https://media.discordapp.net/clip-1.mp4".to_owned(),
            filename: "clip-1.mp4".to_owned(),
        })
    );
}

#[test]
fn non_regular_message_actions_do_not_include_attachment_downloads() {
    let mut state = state_with_message_ids([]);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::new(7),
        reference: None,
        reply: None,
        poll: None,
        content: None,
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![video_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);

    assert!(
        !state
            .selected_message_action_items()
            .iter()
            .any(|action| matches!(action.kind, MessageActionKind::DownloadAttachment(_)))
    );
}

#[test]
fn channel_show_pinned_messages_action_enters_pinned_message_view() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let command = state.activate_selected_channel_action();

    assert!(matches!(
        command,
        Some(AppCommand::LoadPinnedMessages { channel_id }) if channel_id == Id::new(2)
    ));
    assert!(state.is_pinned_message_view());
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert!(!state.message_auto_follow());
}

#[test]
fn pinned_message_view_title_mentions_channel_and_pins() {
    let mut state = state_with_messages(1);

    assert_eq!(state.message_pane_title(), "#general");

    state.enter_pinned_message_view(Id::new(2));

    assert_eq!(state.message_pane_title(), "#general pinned messages");
}

#[test]
fn pinned_message_view_suppresses_unread_divider_and_banner() {
    let mut state = state_with_message_ids([1, 2, 3]);
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: Id::new(2),
            last_acked_message_id: Some(Id::new(1)),
            mention_count: 0,
        }],
    });
    state.activate_channel(Id::new(2));
    assert_eq!(state.unread_divider_message_index(), Some(1));
    assert!(state.unread_banner().is_some());

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(Id::new(2), 3)],
    });
    state.enter_pinned_message_view(Id::new(2));

    assert!(state.is_pinned_message_view());
    assert_eq!(state.unread_divider_message_index(), None);
    assert_eq!(state.unread_banner(), None);
    assert_eq!(state.message_extra_top_lines(0), 1);
}

#[test]
fn returning_from_pinned_message_view_restores_parent_message_window() {
    let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.move_up();
    state.move_up();
    let expected_selected = state.selected_message();
    let expected_scroll = state.message_scroll();
    let expected_line_scroll = state.message_line_scroll();

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(Id::new(2), 11)],
    });
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    assert!(state.return_from_pinned_message_view());

    assert!(!state.is_pinned_message_view());
    assert_eq!(state.selected_message(), expected_selected);
    assert_eq!(state.message_scroll(), expected_scroll);
    assert_eq!(state.message_line_scroll(), expected_line_scroll);
}

#[test]
fn pinned_message_view_does_not_request_older_history() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 11)],
    });
    state.enter_pinned_message_view(channel_id);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.messages().first().map(|message| message.id),
        Some(Id::new(11))
    );
    assert_eq!(state.next_older_history_command(), None);
}

#[test]
fn pinned_only_messages_stay_out_of_normal_history() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 5)],
    });

    assert_eq!(
        state
            .messages()
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![10, 11, 12]
    );

    state.enter_pinned_message_view(channel_id);
    assert_eq!(
        state.messages().first().map(|message| message.id),
        Some(Id::new(5))
    );
}

#[test]
fn pinned_only_messages_do_not_become_older_history_cursor() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 5)],
    });
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
}

#[test]
fn channel_change_exits_pinned_message_view() {
    let mut state = state_with_many_channels(2);
    state.confirm_selected_channel();
    state.enter_pinned_message_view(Id::new(1));
    assert!(state.is_pinned_message_view());

    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.confirm_selected_channel();

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert!(!state.is_pinned_message_view());
}

#[test]
fn guild_change_exits_pinned_message_view() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(2),
        name: "other guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.confirm_selected_guild();

    assert_eq!(state.selected_guild_id(), Some(Id::new(2)));
    assert_eq!(state.selected_channel_id(), None);
    assert!(!state.is_pinned_message_view());
}

#[test]
fn reaction_message_actions_use_single_reacted_users_item() {
    let mut state = state_with_reaction_message();
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
            MessageActionKind::ShowReactionUsers,
            MessageActionKind::RemoveReaction(0),
        ]
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.label == "Show reacted users")
            .count(),
        1
    );
    assert!(!actions.iter().any(|action| action.label == "Show 👍 users"));
}

#[test]
fn custom_emoji_action_label_uses_id_when_images_are_disabled() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            reactions: vec![ReactionInfo {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                count: 1,
                me: true,
            }],
            ..message_info(Id::new(2), 1)
        }],
    });
    state.open_options_popup();
    for _ in 0..4 {
        state.move_option_down();
    }
    state.toggle_selected_display_option();
    state.close_options_popup();
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(actions.iter().any(|action| {
        action.kind == MessageActionKind::RemoveReaction(0) && action.label == "Remove 50 reaction"
    }));
}

#[test]
fn show_reacted_users_action_loads_all_reaction_emojis() {
    let mut state = state_with_reaction_message();
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    for _ in 0..4 {
        state.move_message_action_down();
    }

    let command = state.activate_selected_message_action();

    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![
                ReactionEmoji::Unicode("👍".to_owned()),
                ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
            ],
        })
    );
    assert!(!state.is_message_action_menu_open());
}

#[test]
fn first_loaded_message_has_date_separator() {
    let state = state_with_message_ids([10, 11]);

    assert!(state.message_starts_new_day_at(0));
    assert_eq!(state.message_extra_top_lines(0), 1);
}

#[test]
fn incoming_message_while_scrolled_away_sets_new_messages_marker() {
    let mut state = state_with_messages(5);
    clear_scheduled_read_ack(&mut state);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();

    push_text_message(&mut state, 6, "new while reading older messages");

    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(6)));
    assert_eq!(state.new_messages_count(), 1);
    assert_eq!(state.message_extra_top_lines(5), 0);
    assert_eq!(state.channel_unread(Id::new(2)), ChannelUnreadState::Unread);
    assert!(state.next_read_ack_deadline().is_none());
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn new_messages_count_includes_messages_after_marker() {
    let mut state = state_with_messages(5);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();

    push_text_message(&mut state, 6, "first unread");
    push_text_message(&mut state, 7, "second unread");

    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(6)));
    assert_eq!(state.new_messages_count(), 2);
}

#[test]
fn viewport_scroll_away_from_latest_sets_new_messages_marker_even_when_cursor_is_latest() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let selected = state.selected_message();

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    push_text_message(&mut state, 11, "new while viewport is above latest");

    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(11)));
    assert_eq!(state.new_messages_count(), 1);
}

#[test]
fn new_messages_marker_clears_when_user_reaches_latest() {
    enum LatestAction {
        JumpBottom,
        ScrollViewportBottom,
        ScrollViewportDown,
    }

    for action in [
        LatestAction::JumpBottom,
        LatestAction::ScrollViewportBottom,
        LatestAction::ScrollViewportDown,
    ] {
        let mut state = state_with_messages(5);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(3);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.jump_top();
        push_text_message(&mut state, 6, "new while reading older messages");

        match action {
            LatestAction::JumpBottom => state.jump_bottom(),
            LatestAction::ScrollViewportBottom => state.scroll_message_viewport_bottom(),
            LatestAction::ScrollViewportDown => {
                for _ in 0..50 {
                    if state.new_messages_marker_message_id().is_none() {
                        break;
                    }
                    state.scroll_message_viewport_down();
                }
            }
        }

        assert_eq!(state.new_messages_marker_message_id(), None);
    }
}

#[test]
fn viewport_scroll_back_to_latest_re_engages_auto_follow_when_cursor_is_latest() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let selected = state.selected_message();

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    for _ in 0..50 {
        state.scroll_message_viewport_down();
    }

    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    push_text_message(&mut state, 11, "new while viewport is latest again");

    assert_eq!(state.messages()[state.selected_message()].id, Id::new(11));
    assert!(state.message_auto_follow());
}

#[test]
fn incoming_message_at_latest_does_not_set_new_messages_marker() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);

    push_text_message(&mut state, 3, "new while following latest");

    assert_eq!(state.new_messages_marker_message_id(), None);
}

#[test]
fn reaction_users_loaded_opens_popup_state() {
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            users: vec![ReactionUserInfo {
                user_id: Id::new(10),
                display_name: "neo".to_owned(),
            }],
        }],
    });

    assert!(state.is_reaction_users_popup_open());
    assert_eq!(state.last_status(), None);
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.reactions()[0].users[0].display_name.as_str()),
        Some("neo")
    );
}

#[test]
fn pinned_messages_loaded_does_not_update_status() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 1)],
    });

    assert_eq!(state.last_status(), None);
    assert_eq!(state.pinned_messages().len(), 1);
}

#[test]
fn reaction_users_popup_scroll_down_clamps_at_bottom() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            users: (1..=6)
                .map(|id| ReactionUserInfo {
                    user_id: Id::new(id),
                    display_name: format!("user-{id}"),
                })
                .collect(),
        }],
    });
    // 1 header + 6 users = 7 data lines. With a 3-line viewport the
    // furthest the user can scroll is 4.
    state.set_reaction_users_popup_view_height(3);

    for _ in 0..50 {
        state.scroll_reaction_users_popup_down();
    }
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(4)
    );

    // A single 'k' press should now move the scroll back, not be eaten by
    // the inflated counter.
    state.scroll_reaction_users_popup_up();
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(3)
    );
}

#[test]
fn missing_thread_preview_requests_exact_latest_message_until_loaded() {
    let mut state = state_with_thread_created_message();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(Id::new(2)),
        position: None,
        last_message_id: Some(Id::new(30)),
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

    assert_eq!(
        state.missing_thread_preview_load_requests(),
        vec![(Id::new(10), Id::new(30))]
    );

    state.push_event(AppEvent::ThreadPreviewLoaded {
        channel_id: Id::new(10),
        message: MessageInfo {
            content: Some("latest reply".to_owned()),
            ..message_info(Id::new(10), 30)
        },
    });
    let message = state.messages()[0];
    let summary = state
        .thread_summary_for_message(message)
        .expect("thread summary should resolve");

    assert_eq!(state.missing_thread_preview_load_requests(), Vec::new());
    assert_eq!(
        summary
            .latest_message_preview
            .map(|preview| (preview.author, preview.content)),
        Some(("neo".to_owned(), "latest reply".to_owned()))
    );
}

#[test]
fn thread_summary_suppresses_preview_when_channel_latest_is_newer_than_cache() {
    let mut state = state_with_thread_created_message();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(Id::new(2)),
        position: None,
        last_message_id: Some(Id::new(40)),
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
    state.push_event(AppEvent::ThreadPreviewLoaded {
        channel_id: Id::new(10),
        message: MessageInfo {
            content: Some("older cached reply".to_owned()),
            ..message_info(Id::new(10), 30)
        },
    });
    let message = state.messages()[0];
    let summary = state
        .thread_summary_for_message(message)
        .expect("thread summary should resolve");

    assert_eq!(summary.latest_message_id, Some(Id::new(40)));
    assert_eq!(summary.latest_message_preview, None);
    assert_eq!(
        state.missing_thread_preview_load_requests(),
        vec![(Id::new(10), Id::new(40))]
    );
}

#[test]
fn return_from_opened_thread_restores_scrolled_parent_message_window() {
    let mut state = state_with_thread_created_message_after_regular_message();
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(4);
    state.clamp_message_viewport_for_image_previews(16, 0, 0);
    state.scroll_message_viewport_top();
    for _ in 0..160 {
        state.scroll_message_viewport_down();
        if state.message_scroll() > 0 && state.message_line_scroll() > 0 {
            break;
        }
    }
    assert_eq!(state.selected_message(), 1);
    assert!(state.message_scroll() > 0);
    assert!(state.message_line_scroll() > 0);
    let expected_message_scroll = state.message_scroll();
    let expected_line_scroll = state.message_line_scroll();

    state.open_selected_message_actions();
    state.move_message_action_down();
    state.activate_selected_message_action();
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    assert!(state.return_from_opened_thread());

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), expected_message_scroll);
    assert_eq!(state.message_line_scroll(), expected_line_scroll);
}

fn state_with_thread_created_message_after_regular_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: parent_id,
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
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                parent_id: Some(parent_id),
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
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("older parent message ".repeat(20)),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::new(18),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        reply: None,
        poll: None,
        content: Some("release notes ".repeat(20)),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

#[test]
fn history_loaded_thread_created_message_opens_reference_thread_after_rename() {
    let mut state = state_with_thread_created_message();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            message_kind: MessageKind::new(18),
            reference: Some(MessageReferenceInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Some(Id::new(10)),
                message_id: None,
            }),
            pinned: false,
            reactions: Vec::new(),
            content: Some("old thread name".to_owned()),
            ..message_info(Id::new(2), 2)
        }],
    });
    state.focus_pane(FocusPane::Messages);
    state.jump_bottom();

    let actions = state.selected_message_action_items();
    assert!(
        actions
            .iter()
            .any(|action| action.kind == MessageActionKind::OpenThread)
    );

    state.open_selected_message_actions();
    state.move_message_action_down();
    state.activate_selected_message_action();

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
}

#[test]
fn start_composer_refused_in_read_only_channel() {
    let mut state = state_with_read_only_channel();
    state.start_composer();
    assert!(
        !state.is_composing(),
        "composer must not open when SEND_MESSAGES is denied"
    );
}

#[test]
fn submit_composer_drops_message_when_send_revoked_after_open() {
    // Open the composer with SEND_MESSAGES granted, type something, then
    // simulate a permission overwrite arriving that revokes SEND. Submit
    // must refuse rather than silently fire a request that would 403.
    let mut state = state_with_writable_channel();
    state.start_composer();
    state.push_composer_char('h');
    state.push_composer_char('i');
    assert!(state.is_composing());

    // Apply a CHANNEL_UPDATE that strips SEND_MESSAGES via a channel
    // overwrite on @everyone (role id == guild id == 1).
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        parent_id: None,
        position: Some(0),
        last_message_id: None,
        name: "general".to_owned(),
        kind: "GuildText".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: vec![PermissionOverwriteInfo {
            id: 1,
            kind: PermissionOverwriteKind::Role,
            allow: 0,
            deny: 0x800,
        }],
    }));
    assert_eq!(state.submit_composer(), None);
    assert!(!state.is_composing());
}

#[test]
fn active_channel_is_cleared_when_view_permission_is_revoked() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert!(state.is_composing());

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        parent_id: None,
        position: Some(0),
        last_message_id: None,
        name: "general".to_owned(),
        kind: "GuildText".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: vec![PermissionOverwriteInfo {
            id: 1,
            kind: PermissionOverwriteKind::Role,
            allow: 0,
            deny: 0x400,
        }],
    }));

    assert_eq!(state.selected_channel_id(), None);
    assert!(!state.is_composing());
    assert!(state.channel_pane_entries().is_empty());
}

#[test]
fn debug_channel_visibility_reports_active_guild_counts() {
    // The fixture's channel denies VIEW_CHANNEL on @everyone, so it
    // shows up in the hidden bucket.
    let state = state_with_view_denied_channel();
    let stats = state.debug_channel_visibility();
    assert_eq!(
        stats,
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
}

#[test]
fn typing_at_sign_at_start_opens_mention_picker() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');

    assert_eq!(state.composer_mention_query(), Some(""));
    assert!(!state.composer_mention_candidates().is_empty());
}

#[test]
fn typing_at_sign_after_letter_does_not_trigger_picker() {
    // `me@` should not open the picker — the user is mid-word, not
    // starting a fresh mention.
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    for ch in "me".chars() {
        state.push_composer_char(ch);
    }
    state.push_composer_char('@');

    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "me@");
}

#[test]
fn typing_after_at_filters_candidates_by_substring() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    assert_eq!(state.composer_mention_query(), Some("s"));
    let names: Vec<_> = state
        .composer_mention_candidates()
        .into_iter()
        .map(|entry| entry.display_name)
        .collect();
    assert!(
        names.iter().all(|name| name.to_lowercase().contains('s')),
        "expected only `s` matches, got {names:?}"
    );
    assert!(names.iter().any(|name| name == "Sally"));
    assert!(names.iter().any(|name| name == "Sammy"));
    assert!(!names.iter().any(|name| name == "Bob"));
}

#[test]
fn backspace_shrinks_query_then_closes_picker() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    state.pop_composer_char();
    assert_eq!(state.composer_mention_query(), Some(""));
    assert_eq!(state.composer_input(), "@");

    state.pop_composer_char();
    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "");
}

#[test]
fn confirm_inserts_display_name_and_submit_expands_to_wire_format() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');
    // First match (alphabetical within "starts_with s") is "Sally" (id 20).
    assert!(state.confirm_composer_mention());
    assert_eq!(state.composer_input(), "@Sally ");
    assert_eq!(state.composer_mention_query(), None);

    state.push_composer_char('h');
    state.push_composer_char('i');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<@20> hi".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn confirm_mention_in_middle_keeps_trailing_text() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    for value in "hello @sworld".chars() {
        state.push_composer_char(value);
    }
    for _ in 0.."world".len() {
        state.move_composer_cursor_left();
    }

    assert_eq!(state.composer_mention_query(), Some("s"));
    assert!(state.confirm_composer_mention());

    assert_eq!(state.composer_input(), "hello @Sally world");
    assert_eq!(state.composer_cursor_byte_index(), "hello @Sally ".len());
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "hello <@20> world".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn cancel_composer_clears_pending_attachments() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.confirm_selected_channel();
    state.start_composer();
    state.add_pending_composer_attachments(vec![MessageAttachmentUpload {
        path: "/tmp/cat.png".into(),
        filename: "cat.png".to_owned(),
        size_bytes: 2_048,
    }]);

    state.cancel_composer();

    assert_eq!(state.pending_composer_attachments(), &[]);
}

#[test]
fn pending_attachments_are_capped_at_upload_limit() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.confirm_selected_channel();
    state.start_composer();
    let attachments = (0..crate::discord::MAX_UPLOAD_ATTACHMENT_COUNT + 2)
        .map(|index| MessageAttachmentUpload {
            path: format!("/tmp/{index}.txt").into(),
            filename: format!("{index}.txt"),
            size_bytes: 1,
        })
        .collect();

    state.add_pending_composer_attachments(attachments);

    assert_eq!(
        state.pending_composer_attachments().len(),
        crate::discord::MAX_UPLOAD_ATTACHMENT_COUNT
    );
}

#[test]
fn move_selection_navigates_filtered_list() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');
    let candidates = state.composer_mention_candidates();
    assert!(candidates.len() >= 2);

    state.move_composer_mention_selection(1);
    assert_eq!(state.composer_mention_selected(), 1);

    state.move_composer_mention_selection(-5);
    assert_eq!(state.composer_mention_selected(), 0);
}

#[test]
fn cancel_picker_keeps_typed_text() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    state.cancel_composer_mention();
    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "@s");
}

#[test]
fn typing_footer_resolves_one_user_to_alias() {
    let mut state = state_with_writable_channel_and_members();
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(20),
    });

    assert_eq!(
        state.typing_footer_for_selected_channel(),
        Some("Sally is typing\u{2026}".to_owned())
    );
}

#[test]
fn typing_footer_excludes_current_user() {
    let mut state = state_with_writable_channel_and_members();
    // user_id 10 is the local user in the fixture's READY event.
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(10),
    });

    assert_eq!(state.typing_footer_for_selected_channel(), None);
}

#[test]
fn typing_footer_pluralizes_at_two_three_and_more_typers() {
    let mut state = state_with_writable_channel_and_members();
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(20),
    });
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(21),
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("two typers should produce a footer");
    // Newest typer first, so id 21 (Sammy) leads.
    assert_eq!(footer, "Sammy and Sally are typing\u{2026}");

    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(22),
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("three typers should produce a footer");
    assert_eq!(footer, "Bob, Sammy, and Sally are typing\u{2026}");

    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(23),
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("four typers should still produce a footer");
    assert_eq!(footer, "Several people are typing\u{2026}");
}

#[test]
fn picker_matches_alias_with_multibyte_query() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('A');

    let candidates = state.composer_mention_candidates();
    assert!(
        candidates.iter().any(|entry| entry.display_name == "Alias"),
        "alias `Alias` must surface when typing `A`, got {:?}",
        candidates
            .iter()
            .map(|c| c.display_name.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn picker_matches_username_when_alias_does_not_contain_query() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('A');
    state.push_composer_char('l');

    let candidates = state.composer_mention_candidates();
    assert!(
        candidates
            .iter()
            .any(|entry| entry.username.as_deref() == Some("Alias123")),
        "username `Alias123` must match query `Al`, got {:?}",
        candidates
            .iter()
            .map(|c| (c.display_name.clone(), c.username.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn picker_ranks_alias_prefix_above_username_prefix() {
    // `s` should put display-name matches (Sally, Sammy) before any
    // username-only match. We don't have a username-only `s` match in the
    // fixture, but we still verify alias rows come first when both have
    // candidates.
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    let candidates = state.composer_mention_candidates();
    let names: Vec<_> = candidates.iter().map(|c| c.display_name.clone()).collect();
    assert!(
        names
            .first()
            .map(|name| name.starts_with('S'))
            .unwrap_or(false),
        "alias-prefix matches must lead the list, got {names:?}"
    );
}

#[test]
fn composer_sends_to_opened_thread_channel() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    state.move_message_action_down();
    state.activate_selected_message_action();

    state.start_composer();
    state.push_composer_char('h');
    state.push_composer_char('i');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(10),
            content: "hi".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn member_subscription_ranges_grow_with_viewport() {
    let mut state = state_with_thread_created_message();
    state.set_member_view_height(20);
    // Default scroll 0, viewport ends at 20 → bucket 0.
    assert_eq!(state.member_subscription_ranges(), vec![(0, 99)]);

    state.member_scroll = 100;
    state.member_view_height = 20;
    // Viewport ends at 120 → bucket 1, contiguous coverage.
    assert_eq!(
        state.member_subscription_ranges(),
        vec![(0, 99), (100, 199)]
    );

    state.member_scroll = 480;
    state.member_view_height = 30;
    // Viewport ends at 510 → bucket 5, anchor [0,99] plus the two buckets
    // around the visible end so we never exceed the four-range cap.
    assert_eq!(
        state.member_subscription_ranges(),
        vec![(0, 99), (400, 499), (500, 599)]
    );
}

#[test]
fn member_list_subscription_target_uses_active_channel_or_fallback() {
    let mut state = state_with_thread_created_message();
    // The fixture activates `general` (id=2) on guild=1.
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(2)))
    );

    // Switching the active channel to a thread must fall back to the
    // parent text channel — Discord rejects op-37 ranges against threads.
    state.activate_channel(Id::new(10));
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(2)))
    );
}

#[test]
fn member_list_subscription_fallback_skips_hidden_channels() {
    let state = state_with_hidden_and_visible_channels();

    assert_eq!(
        state.guild_member_list_channel(Id::new(1)),
        Some(Id::new(3))
    );
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(3)))
    );
}

#[test]
fn member_list_subscription_target_skips_active_voice_channel() {
    let mut state = state_with_hidden_and_visible_channels();
    state.activate_channel(Id::new(4));

    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(3)))
    );
}

#[test]
fn channel_pane_excludes_threads() {
    let state = state_with_thread_created_message();
    let entries = state.channel_pane_entries();
    let channel_ids: Vec<Id<ChannelMarker>> = entries
        .iter()
        .filter_map(|entry| match entry {
            ChannelPaneEntry::Channel { state, .. } => Some(state.id),
            ChannelPaneEntry::CategoryHeader { .. } => None,
        })
        .collect();
    assert!(channel_ids.contains(&Id::new(2)));
    assert!(!channel_ids.contains(&Id::new(10)));
}

#[test]
fn channel_switcher_groups_channels_and_filters_by_fuzzy_name() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(40),
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(100)),
        name: "alice".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(10),
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Text".to_owned(),
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
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(11),
                parent_id: Some(Id::new(10)),
                position: Some(0),
                last_message_id: None,
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
            ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(12),
                parent_id: Some(Id::new(10)),
                position: Some(1),
                last_message_id: None,
                name: "random".to_owned(),
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

    state.open_channel_switcher();
    let all_items = state.channel_switcher_items();
    assert_eq!(all_items[0].group_label, "Direct Messages");
    assert_eq!(all_items[1].group_label, "guild");
    assert_eq!(all_items[1].parent_label.as_deref(), Some("Text"));

    for ch in "gnrl".chars() {
        state.push_channel_switcher_char(ch);
    }
    let filtered = state.channel_switcher_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].channel_id, Id::new(11));
}

#[test]
fn channel_switcher_items_carry_unread_metadata() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: Id::new(40),
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(100)),
        name: "new".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: Id::new(40),
            last_acked_message_id: Some(Id::new(90)),
            mention_count: 0,
        }],
    });
    state.open_channel_switcher();

    let items = state.channel_switcher_items();

    assert_eq!(items[0].channel_id, Id::new(40));
    assert_eq!(items[0].unread, ChannelUnreadState::Unread);
}

#[test]
fn channel_switcher_query_edits_at_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "raXndom".chars() {
        state.push_channel_switcher_char(ch);
    }

    for _ in 0..5 {
        state.move_channel_switcher_query_cursor_left();
    }
    state.move_channel_switcher_query_cursor_right();
    state.pop_channel_switcher_char();

    assert_eq!(state.channel_switcher_query(), Some("random"));
    assert_eq!(
        state.channel_switcher_query_cursor_byte_index(),
        Some("ra".len())
    );
}

#[test]
fn channel_switcher_query_deletes_grapheme_before_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "e\u{301}x".chars() {
        state.push_channel_switcher_char(ch);
    }

    state.move_channel_switcher_query_cursor_left();
    state.pop_channel_switcher_char();

    assert_eq!(state.channel_switcher_query(), Some("x"));
    assert_eq!(state.channel_switcher_query_cursor_byte_index(), Some(0));
}

#[test]
fn channel_action_menu_lists_threads_for_selected_channel() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    assert!(state.is_channel_action_menu_open());
    let actions = state.selected_channel_action_items();
    assert_eq!(actions.len(), 4);
    assert_eq!(actions[0].kind, ChannelActionKind::LoadPinnedMessages);
    assert_eq!(actions[0].label, "Show pinned messages");
    assert!(actions[0].enabled);
    assert_eq!(actions[1].kind, ChannelActionKind::ShowThreads);
    assert!(actions[1].enabled);
    assert_eq!(actions[2].kind, ChannelActionKind::MarkAsRead);
    assert_eq!(actions[2].label, "Mark as read");
    assert_eq!(actions[3].kind, ChannelActionKind::ToggleMute);
    assert_eq!(actions[3].label, "Mute channel");

    state.move_channel_action_down();
    let command = state.activate_selected_channel_action();
    assert_eq!(command, None);
    assert!(state.is_channel_action_threads_phase());

    let threads = state.channel_action_thread_items();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].channel_id, Id::new(10));
    assert_eq!(threads[0].label, "release notes");
}

#[test]
fn mark_as_read_action_enablement_is_scoped_to_action_channel() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let unread_channel: Id<ChannelMarker> = Id::new(2);
    let read_channel: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: unread_channel,
                parent_id: None,
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                name: "unread".to_owned(),
                kind: "GuildText".to_owned(),
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
                channel_id: read_channel,
                parent_id: None,
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                name: "read".to_owned(),
                kind: "GuildText".to_owned(),
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
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: unread_channel,
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: read_channel,
                last_acked_message_id: Some(Id::new(30)),
                mention_count: 0,
            },
        ],
    });
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.activate_channel(unread_channel);
    assert_eq!(state.unread_divider_last_acked_id(), Some(Id::new(10)));

    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    let mark_as_read = actions
        .iter()
        .find(|action| action.kind == ChannelActionKind::MarkAsRead)
        .expect("channel actions include Mark as read");
    assert!(!mark_as_read.enabled);
}

#[test]
fn channel_action_menu_open_thread_activates_and_subscribes() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.move_channel_action_down();
    state.activate_selected_channel_action();
    let command = state.activate_selected_channel_action();

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
    assert!(!state.is_channel_action_menu_open());
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
        })
    );
}

#[test]
fn channel_action_menu_loads_pinned_messages_for_selected_channel() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let command = state.activate_selected_channel_action();

    assert_eq!(
        command,
        Some(AppCommand::LoadPinnedMessages {
            channel_id: Id::new(2),
        })
    );
    assert!(state.is_pinned_message_view());
    assert!(!state.is_channel_action_menu_open());
}

#[test]
fn guild_action_menu_lists_disabled_mark_server_read_when_guild_is_read() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    assert!(state.is_guild_action_menu_open());
    assert_eq!(state.guild_action_menu_title(), Some("guild 1".to_owned()));
    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert_eq!(actions[0].label, "Mark server as read");
    assert_eq!(actions[1].kind, GuildActionKind::ToggleMute);
    assert_eq!(actions[1].label, "Mute server");
    assert!(!actions[0].enabled);
    assert_eq!(state.activate_selected_guild_action(), None);
}

#[test]
fn guild_action_menu_marks_unread_server_channels_as_read() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(2),
                parent_id: None,
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                name: "unread-a".to_owned(),
                kind: "GuildText".to_owned(),
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
                channel_id: Id::new(3),
                parent_id: None,
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                name: "read".to_owned(),
                kind: "GuildText".to_owned(),
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
                channel_id: Id::new(4),
                parent_id: None,
                position: Some(2),
                last_message_id: Some(Id::new(40)),
                name: "unread-b".to_owned(),
                kind: "GuildText".to_owned(),
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
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(2),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(3),
                last_acked_message_id: Some(Id::new(30)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(4),
                last_acked_message_id: Some(Id::new(35)),
                mention_count: 0,
            },
        ],
    });
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert!(actions[0].enabled);

    let command = state.activate_selected_guild_action();

    assert_eq!(state.guild_unread(guild_id), ChannelUnreadState::Seen);
    assert!(!state.is_guild_action_menu_open());
    let Some(AppCommand::AckChannels { mut targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    targets.sort_by_key(|(channel_id, _)| channel_id.get());
    assert_eq!(
        targets,
        vec![(Id::new(2), Id::new(20)), (Id::new(4), Id::new(40))]
    );
}

#[test]
fn guild_action_menu_skips_hidden_channels_when_marking_server_read() {
    let mut state = state_with_hidden_and_visible_channels();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(2),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(3),
                last_acked_message_id: Some(Id::new(10)),
                mention_count: 0,
            },
        ],
    });
    state.push_event(notification_message_event(Id::new(2), "hidden"));
    state.push_event(notification_message_event(Id::new(3), "visible"));
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    assert_eq!(state.guild_action_menu_title(), Some("guild".to_owned()));

    let command = state.activate_selected_guild_action();

    let Some(AppCommand::AckChannels { targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    assert_eq!(targets, vec![(Id::new(3), Id::new(50))]);
    assert_ne!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
    assert_eq!(state.channel_unread(Id::new(3)), ChannelUnreadState::Seen);
}

#[test]
fn direct_messages_keep_placeholder_guild_action() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);
    state.move_up();
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, GuildActionKind::NoActionsYet);
    assert_eq!(actions[0].label, "No server actions yet");
    assert!(!actions[0].enabled);
}

#[test]
fn forum_channel_renders_loaded_posts_in_message_pane() {
    let mut state = state_with_forum_channel_posts();

    assert!(state.selected_channel_is_forum());
    assert!(state.messages().is_empty());
    assert_eq!(state.selected_message_history_channel_id(), None);
    assert_eq!(
        state.selected_forum_channel(),
        Some((Id::new(1), Id::new(20)))
    );
    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["release notes", "welcome"]
    );

    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);
    state.move_down();

    assert_eq!(state.selected_forum_post(), 1);
    assert_eq!(state.message_scroll(), 1);
    assert_eq!(state.focused_forum_post_selection(), Some(0));
}

#[test]
fn forum_posts_loaded_event_populates_selected_forum_items() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: forum_id,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "announcements".to_owned(),
            kind: "forum".to_owned(),
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let mut preview =
        forum_preview_message(guild_id, Id::new(30), 300, "neo", "first message preview");
    preview.reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 2,
        me: false,
    }];

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        posts: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(30),
            parent_id: Some(forum_id),
            position: Some(0),
            last_message_id: None,
            name: "welcome".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: Some(1),
            total_message_sent: Some(1),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        preview_messages: vec![preview],
        has_more: false,
    });

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["welcome"]
    );
    let mut posts = state.selected_forum_post_items();
    let post = posts.remove(0);
    assert_eq!(post.preview_author_id, Some(Id::new(99)));
    assert_eq!(post.preview_author.as_deref(), Some("neo"));
    assert_eq!(
        post.preview_content.as_deref(),
        Some("first message preview")
    );
    assert_eq!(post.preview_reactions.len(), 1);
    assert_eq!(post.comment_count, Some(1));
    assert_eq!(post.last_activity_message_id, Some(Id::new(300)));
    assert_eq!(post.section_label.as_deref(), Some("Active posts"));
}

#[test]
fn missing_message_author_profile_requests_include_visible_forum_preview_authors() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        posts: vec![forum_thread_info(
            guild_id,
            forum_id,
            30,
            "welcome",
            Some(300),
            false,
        )],
        preview_messages: vec![forum_preview_message(
            guild_id,
            Id::new(30),
            300,
            "neo",
            "first message preview",
        )],
        has_more: false,
    });

    assert_eq!(
        state.missing_message_author_profile_requests(),
        vec![(Id::new(99), guild_id)]
    );

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile: profile_info(99, Some("neo")),
    });
    assert_eq!(state.missing_message_author_profile_requests(), Vec::new());
}

#[test]
fn forum_post_first_page_starts_cursor_at_top_and_next_page_appends() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        posts: vec![
            forum_thread_info(guild_id, forum_id, 30, "newest", Some(300), false),
            forum_thread_info(guild_id, forum_id, 31, "middle", Some(200), false),
        ],
        preview_messages: Vec::new(),
        has_more: true,
    });

    assert_eq!(state.selected_forum_post(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["newest", "middle"]
    );

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 2,
        next_offset: 3,
        posts: vec![forum_thread_info(
            guild_id,
            forum_id,
            32,
            "older",
            Some(100),
            false,
        )],
        preview_messages: Vec::new(),
        has_more: false,
    });

    assert_eq!(state.selected_forum_post(), 0);
    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["newest", "middle", "older"]
    );
}

#[test]
fn archived_forum_posts_render_after_active_posts_without_moving_shared_active_posts() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        posts: vec![
            forum_thread_info(guild_id, forum_id, 30, "active", Some(300), false),
            forum_thread_info(guild_id, forum_id, 31, "shared", Some(200), false),
        ],
        preview_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Archived,
        offset: 0,
        next_offset: 2,
        posts: vec![
            forum_thread_info(guild_id, forum_id, 31, "shared", Some(400), true),
            forum_thread_info(guild_id, forum_id, 32, "archived", Some(100), true),
        ],
        preview_messages: Vec::new(),
        has_more: false,
    });

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| {
                (
                    post.label.as_str(),
                    post.section_label.as_deref(),
                    post.archived,
                    post.last_activity_message_id,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("active", Some("Active posts"), false, Some(Id::new(300))),
            ("shared", None, false, Some(Id::new(200))),
            ("archived", Some("Archived posts"), true, Some(Id::new(100)),),
        ]
    );
}

#[test]
fn forum_posts_resort_by_last_message_id_when_server_index_is_stale() {
    // Discord's `/threads/search?sort_by=last_message_time` sometimes returns
    // posts out of strict timestamp order — its index lags behind real
    // activity. We re-sort by `last_message_id` (the snowflake encodes the
    // exact message timestamp) so the displayed order matches the official
    // client even when the API reply is stale.
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Posts arrive in the order Discord returned them (stale): the post with
    // the newest message id sits in the middle of the list.
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 3,
        posts: vec![
            forum_thread_info(guild_id, forum_id, 30, "stale-top", Some(100), false),
            forum_thread_info(guild_id, forum_id, 31, "newest-activity", Some(500), false),
            forum_thread_info(guild_id, forum_id, 32, "older", Some(200), false),
        ],
        preview_messages: Vec::new(),
        has_more: false,
    });

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["newest-activity", "older", "stale-top"]
    );
}

#[test]
fn forum_pinned_posts_float_to_top_preserving_relative_order() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Mirrors a real Discord response: posts arrive sorted by activity but a
    // pinned post sits in the middle, and the official client lifts it to the
    // top while keeping the rest in delivered order.
    let mut newest = forum_thread_info(guild_id, forum_id, 30, "newest", Some(300), false);
    newest.thread_pinned = Some(false);
    let mut pinned = forum_thread_info(guild_id, forum_id, 31, "pinned-post", Some(200), false);
    pinned.thread_pinned = Some(true);
    let mut middle = forum_thread_info(guild_id, forum_id, 32, "middle", Some(150), false);
    middle.thread_pinned = Some(false);
    let mut older = forum_thread_info(guild_id, forum_id, 33, "older", Some(100), false);
    older.thread_pinned = Some(false);

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 4,
        posts: vec![newest, pinned, middle, older],
        preview_messages: Vec::new(),
        has_more: false,
    });

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| (post.label.as_str(), post.pinned))
            .collect::<Vec<_>>(),
        vec![
            ("pinned-post", true),
            ("newest", false),
            ("middle", false),
            ("older", false),
        ]
    );
}

#[test]
fn activating_selected_forum_post_opens_thread_channel() {
    let mut state = state_with_forum_channel_posts();
    state.focus_pane(FocusPane::Messages);
    state.move_down();

    let command = state.activate_selected_message_pane_item();

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(30),
        })
    );
}

#[test]
fn forum_channel_does_not_start_parent_channel_composer() {
    let mut state = state_with_forum_channel_posts();

    assert!(!state.can_send_in_selected_channel());
    state.start_composer();

    assert!(!state.is_composing());
}

#[test]
fn forum_post_bottom_scroll_uses_last_full_page() {
    let mut state = state_with_many_forum_channel_posts(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(10);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);

    state.jump_bottom();

    assert_eq!(state.selected_forum_post(), 9);
    assert_eq!(state.message_scroll(), 8);
    assert_eq!(
        state
            .visible_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["post 2", "post 1"]
    );
}

#[test]
fn returning_from_forum_post_restores_parent_post_cursor() {
    let mut state = state_with_many_forum_channel_posts(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    state.jump_bottom();
    let expected_selected = state.selected_forum_post();
    let expected_scroll = state.message_scroll();

    state.activate_selected_message_pane_item();
    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));

    assert!(state.return_from_opened_thread());
    assert!(state.selected_channel_is_forum());
    assert_eq!(state.selected_forum_post(), expected_selected);
    assert_eq!(state.message_scroll(), expected_scroll);
}

#[test]
fn channel_action_menu_back_returns_to_actions_phase() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.move_channel_action_down();
    state.activate_selected_channel_action();
    assert!(state.is_channel_action_threads_phase());

    state.back_channel_action_menu();
    assert!(state.is_channel_action_menu_open());
    assert!(!state.is_channel_action_threads_phase());

    state.back_channel_action_menu();
    assert!(!state.is_channel_action_menu_open());
}

#[test]
fn poll_vote_actions_are_available_by_default() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
            MessageActionKind::VotePollAnswer(1),
            MessageActionKind::VotePollAnswer(2),
        ]
    );
    assert_eq!(actions[4].label, "Remove poll vote: Soup");
    assert_eq!(actions[5].label, "Vote poll: Noodles");
}

fn state_with_forum_channel_posts() -> DashboardState {
    state_with_many_forum_channel_posts(2)
}

fn forum_channel_info(guild_id: Id<GuildMarker>, forum_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        channel_id: forum_id,
        parent_id: None,
        position: Some(0),
        last_message_id: None,
        name: "announcements".to_owned(),
        kind: "forum".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }
}

fn forum_thread_info(
    guild_id: Id<GuildMarker>,
    forum_id: Id<ChannelMarker>,
    channel_id: u64,
    name: &str,
    last_message_id: Option<u64>,
    archived: bool,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        channel_id: Id::new(channel_id),
        parent_id: Some(forum_id),
        position: None,
        last_message_id: last_message_id.map(Id::<MessageMarker>::new),
        name: name.to_owned(),
        kind: "GuildPublicThread".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: Some(archived),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }
}

fn forum_preview_message(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    message_id: u64,
    author: &str,
    content: &str,
) -> MessageInfo {
    MessageInfo {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: author.to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageInfo::default()
    }
}

fn state_with_many_forum_channel_posts(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: forum_id,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "announcements".to_owned(),
            kind: "forum".to_owned(),
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Discord's `/threads/search` returns posts newest-first, so emit them in
    // reverse channel-id order to match what the live API would deliver.
    let posts: Vec<_> = (0..count)
        .rev()
        .map(|index| ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(30 + index),
            parent_id: Some(forum_id),
            position: Some(i32::try_from(index).expect("test index fits i32")),
            last_message_id: None,
            name: if count == 2 && index == 0 {
                "welcome".to_owned()
            } else if count == 2 && index == 1 {
                "release notes".to_owned()
            } else {
                format!("post {}", index + 1)
            },
            kind: "GuildPublicThread".to_owned(),
            message_count: Some(index + 1),
            total_message_sent: Some(index + 1),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
        .collect();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: posts.len(),
        posts,
        preview_messages: Vec::new(),
        has_more: false,
    });
    state
}

#[test]
fn message_action_items_keep_image_action_for_poll_messages() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::ViewImage,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
            MessageActionKind::VotePollAnswer(1),
            MessageActionKind::VotePollAnswer(2),
        ]
    );
}

#[test]
fn poll_vote_action_can_remove_existing_vote() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.open_selected_message_actions();
    for _ in 0..4 {
        state.move_message_action_down();
    }

    let command = state.activate_selected_message_action();

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: Vec::new(),
        })
    );
}

#[test]
fn multi_select_poll_action_opens_picker_and_submits_selected_answers() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(true)),
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let actions = state.selected_message_action_items();
    assert_eq!(actions[4].kind, MessageActionKind::OpenPollVotePicker);
    assert_eq!(actions[4].label, "Choose poll votes");

    state.open_selected_message_actions();
    for _ in 0..4 {
        state.move_message_action_down();
    }
    assert_eq!(state.activate_selected_message_action(), None);
    assert!(state.is_poll_vote_picker_open());
    assert_eq!(
        state.poll_vote_picker_items().map(|items| {
            items
                .iter()
                .map(|item| (item.answer_id, item.selected))
                .collect::<Vec<_>>()
        }),
        Some(vec![(1, true), (2, false)])
    );

    state.move_poll_vote_picker_down();
    state.toggle_selected_poll_vote_answer();
    let command = state.activate_poll_vote_picker();

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1, 2],
        })
    );
}

#[test]
fn message_scroll_uses_scrolloff() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);

    assert_eq!(state.message_scroll(), 5);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_message(), 9);
    assert_eq!(state.message_scroll(), 5);

    state.move_up();
    assert_eq!(state.selected_message(), 8);
    assert_eq!(state.message_scroll(), 5);
}

#[test]
fn message_auto_follow_keeps_latest_message_at_bottom_after_rendered_clamp() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);

    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_auto_follow());
    assert_eq!(state.selected_message(), 11);
    assert_eq!(state.message_scroll(), 9);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 4);
}

#[test]
fn message_selection_centers_selected_message_when_possible() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    for _ in 0..4 {
        state.move_up();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
    }

    assert_eq!(state.selected_message(), 7);
    assert_eq!(state.message_scroll(), 6);
    assert_eq!(state.message_line_scroll(), 1);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 2);
}

#[test]
fn message_selection_centers_with_line_offset_inside_previous_message() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    for id in 2..=5 {
        push_text_message(&mut state, id, &format!("msg {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 5);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 1);
}

#[test]
fn message_selection_keeps_top_when_next_message_is_already_visible() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    for id in 2..=5 {
        push_text_message(&mut state, id, &format!("msg {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 6);
}

#[test]
fn message_selection_centers_with_image_preview_height() {
    let mut state = state_with_image_messages(8, &[4]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    for _ in 0..3 {
        state.move_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
    }

    assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
    assert_eq!(state.selected_message_rendered_height(200, 16, 3), 7);
    assert_eq!(state.message_scroll(), 2);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 1);
}

#[test]
fn message_viewport_scrolls_by_rendered_line() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);

    state.scroll_message_viewport_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 1);
    assert_eq!(state.selected_message(), 0);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 3);
}

#[test]
fn viewport_scroll_moves_to_next_message_after_current_message() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 1);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message(), 0);
}

#[test]
fn focused_message_selection_returns_none_when_viewport_scrolled_past_selection() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    for _ in 0..6 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }

    assert_eq!(state.message_scroll(), 1);
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.focused_message_selection(), None);
}

#[test]
fn moving_cursor_to_first_message_resets_top_line_scroll() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    for _ in 0..2 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert!(state.message_line_scroll() > 0);

    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 0);
}

#[test]
fn jumping_to_first_message_resets_item_scroll_when_view_has_spare_rows() {
    let mut state = state_with_messages(20);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(20);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_scroll() > 0);

    state.jump_top();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn viewport_scrolls_by_rendered_line_when_selected_message_is_below_top() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message(), 0);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    let selected_bottom = state
        .selected_message_rendered_row(5, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(5, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn tall_message_clamp_keeps_next_selected_message_visible() {
    let mut state =
        state_with_single_message_content("abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    let selected_bottom = state
        .selected_message_rendered_row(5, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(5, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn viewport_scroll_up_enters_previous_long_message_at_last_line() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("next".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    for _ in 0..3 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }

    state.scroll_message_viewport_up();

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message(), 0);
}

#[test]
fn shared_scroll_helper_keeps_three_rows_below_cursor_when_scrolling_starts() {
    let height = 10;
    let scroll = super::clamp_list_scroll(7, 0, height, 20);

    assert_eq!(scroll, 1);
    assert_eq!(height - 1 - (7 - scroll), 3);
}

#[test]
fn shared_scroll_helper_moves_one_row_near_bottom() {
    let mut scroll = 0usize;

    for cursor in 0..20 {
        let next_scroll = super::clamp_list_scroll(cursor, scroll, 7, 20);
        assert!(
            next_scroll <= scroll.saturating_add(1),
            "cursor {cursor} moved scroll from {scroll} to {next_scroll}",
        );
        scroll = next_scroll;
    }
}

#[test]
fn guild_scroll_uses_scrolloff() {
    let mut state = state_with_many_guilds(8);
    state.focus_pane(FocusPane::Guilds);
    state.set_guild_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_guild(), 8);
    assert_eq!(state.guild_scroll(), 2);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_guild(), 6);
    assert_eq!(state.guild_scroll(), 2);

    state.move_up();
    assert_eq!(state.selected_guild(), 5);
    assert_eq!(state.guild_scroll(), 2);
}

#[test]
fn channel_scroll_uses_scrolloff() {
    let mut state = state_with_many_channels(8);
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_channel(), 7);
    assert_eq!(state.channel_scroll(), 1);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_channel(), 5);
    assert_eq!(state.channel_scroll(), 1);

    state.move_up();
    assert_eq!(state.selected_channel(), 4);
    assert_eq!(state.channel_scroll(), 1);
}

#[test]
fn member_scroll_uses_scrolloff() {
    let mut state = state_with_members(8);
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(7);

    state.jump_bottom();
    assert_eq!(state.selected_member(), 7);
    assert_eq!(state.member_scroll(), 2);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_member(), 5);
    assert_eq!(state.member_scroll(), 2);

    state.move_up();
    assert_eq!(state.selected_member(), 4);
    assert_eq!(state.member_scroll(), 2);
}

#[test]
fn viewport_scroll_does_not_move_list_pane_selection() {
    let mut guild_state = state_with_many_guilds(8);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(3);
    let selected_guild = guild_state.selected_guild();
    let guild_scroll = guild_state.guild_scroll();

    guild_state.scroll_focused_pane_viewport_down();
    guild_state.scroll_focused_pane_viewport_down();
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 2);
    assert_eq!(guild_state.focused_guild_selection(), None);

    guild_state.scroll_focused_pane_viewport_up();
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 1);

    let mut channel_state = state_with_many_channels(8);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(3);
    let selected_channel = channel_state.selected_channel();
    let channel_scroll = channel_state.channel_scroll();

    channel_state.scroll_focused_pane_viewport_down();
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll + 1);
    assert_eq!(channel_state.focused_channel_selection(), None);

    let mut member_state = state_with_members(8);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(3);
    let selected_member = member_state.selected_member();
    let member_scroll = member_state.member_scroll();

    member_state.scroll_focused_pane_viewport_down();
    member_state.scroll_focused_pane_viewport_down();
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll + 2);
    assert_eq!(member_state.focused_member_selection_line(), None);
}

#[test]
fn repeated_viewport_scroll_survives_view_height_sync() {
    let mut guild_state = state_with_many_guilds(12);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(4);
    let selected_guild = guild_state.selected_guild();
    let guild_scroll = guild_state.guild_scroll();
    for _ in 0..3 {
        guild_state.scroll_focused_pane_viewport_down();
        guild_state.set_guild_view_height(4);
    }
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 3);

    let mut channel_state = state_with_many_channels(12);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(4);
    let selected_channel = channel_state.selected_channel();
    let channel_scroll = channel_state.channel_scroll();
    for _ in 0..3 {
        channel_state.scroll_focused_pane_viewport_down();
        channel_state.set_channel_view_height(4);
    }
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll + 3);

    let mut member_state = state_with_members(12);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(4);
    let selected_member = member_state.selected_member();
    let member_scroll = member_state.member_scroll();
    for _ in 0..3 {
        member_state.scroll_focused_pane_viewport_down();
        member_state.set_member_view_height(4);
    }
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll + 3);
}

#[test]
fn viewport_scroll_survives_selection_clamp_after_events() {
    let mut guild_state = state_with_many_guilds(12);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(4);
    let selected_guild = guild_state.selected_guild();
    guild_state.scroll_focused_pane_viewport_down();
    guild_state.scroll_focused_pane_viewport_down();
    let guild_scroll = guild_state.guild_scroll();
    guild_state.push_event(AppEvent::StatusMessage {
        message: "tick".to_owned(),
    });
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll);
    let guild_snapshot = guild_state.discord.clone();
    guild_state.restore_discord_snapshot(guild_snapshot);
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll);

    let mut channel_state = state_with_many_channels(12);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(4);
    let selected_channel = channel_state.selected_channel();
    channel_state.scroll_focused_pane_viewport_down();
    channel_state.scroll_focused_pane_viewport_down();
    let channel_scroll = channel_state.channel_scroll();
    channel_state.push_event(AppEvent::StatusMessage {
        message: "tick".to_owned(),
    });
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll);
    let channel_snapshot = channel_state.discord.clone();
    channel_state.restore_discord_snapshot(channel_snapshot);
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll);

    let mut member_state = state_with_members(12);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(4);
    let selected_member = member_state.selected_member();
    member_state.scroll_focused_pane_viewport_down();
    member_state.scroll_focused_pane_viewport_down();
    let member_scroll = member_state.member_scroll();
    member_state.push_event(AppEvent::StatusMessage {
        message: "tick".to_owned(),
    });
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll);
    let member_snapshot = member_state.discord.clone();
    member_state.restore_discord_snapshot(member_snapshot);
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll);
}

#[test]
fn member_navigation_skips_over_activity_subrows() {
    let mut state = state_with_members(3);
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(20);

    state.push_event(AppEvent::PresenceUpdate {
        guild_id: Id::new(1),
        user_id: Id::new(2),
        status: PresenceStatus::Online,
        activities: vec![ActivityInfo {
            kind: ActivityKind::Playing,
            name: "Concord".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        }],
    });

    // Lines: 0 group header, 1 member 1, 2 member 2, 3 activity, 4 member 3.
    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);

    state.move_down();
    assert_eq!(state.selected_member(), 1);
    assert_eq!(state.selected_member_line(), 2);

    state.move_down();
    assert_eq!(state.selected_member(), 2);
    assert_eq!(state.selected_member_line(), 4);

    assert_eq!(state.member_line_count(), 5);
}

#[test]
fn member_half_page_scrolls_by_rendered_lines() {
    let mut state = state_with_grouped_members();
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(9);

    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);

    state.half_page_down();
    assert_eq!(state.selected_member(), 2);
    assert_eq!(state.selected_member_line(), 5);

    state.half_page_up();
    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);
}

#[test]
fn half_page_scrolls_all_list_panes() {
    let mut guild_state = state_with_many_guilds(8);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(9);
    guild_state.half_page_down();
    assert_eq!(guild_state.selected_guild(), 5);

    let mut channel_state = state_with_many_channels(8);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(9);
    channel_state.half_page_down();
    assert_eq!(channel_state.selected_channel(), 4);

    let mut member_state = state_with_members(8);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(9);
    member_state.half_page_down();
    assert_eq!(member_state.selected_member(), 4);
}

#[test]
fn message_half_page_up_disables_follow() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);

    state.half_page_up();

    assert_eq!(state.selected_message(), 5);
    assert!(!state.message_auto_follow());
}

#[test]
fn message_jump_bottom_re_engages_auto_follow() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);

    state.move_up();
    assert!(!state.message_auto_follow());

    state.jump_bottom();

    // Cursor is back on the latest message, so auto-follow turns on again
    // (sticky-bottom rule).
    assert_eq!(state.selected_message(), 9);
    assert!(state.message_auto_follow());
}

#[test]
fn message_half_page_down_re_engages_auto_follow_when_landing_on_last() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);

    state.half_page_down();
    assert!(state.message_auto_follow());

    state.move_up();
    assert!(!state.message_auto_follow());

    state.half_page_down();
    // Half-page-down moved the cursor back onto the latest message.
    assert!(state.message_auto_follow());
}

#[test]
fn history_load_preserves_manual_scroll_position_by_message_id() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.move_up();
    state.move_up();

    let selected_id = state.messages()[state.selected_message()].id;
    let scroll_id = state.messages()[state.message_scroll()].id;

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![message_info(channel_id, 5)],
    });

    assert_eq!(state.messages()[state.selected_message()].id, selected_id);
    assert_eq!(state.messages()[state.message_scroll()].id, scroll_id);
    assert!(!state.message_auto_follow());
}

#[test]
fn older_history_request_waits_for_loaded_page() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
    assert_eq!(state.next_older_history_command(), None);

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5)],
    });

    state.move_up();
    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(5)),
        })
    );
}

#[test]
fn older_history_request_advances_after_cache_limit_retention() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids(10..=209);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5)],
    });

    assert_eq!(
        state.messages().last().map(|message| message.id),
        Some(Id::new(209))
    );

    state.move_up();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(5)),
        })
    );
}

#[test]
fn empty_older_history_page_marks_cursor_exhausted() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: Vec::new(),
    });

    assert_eq!(state.next_older_history_command(), None);
}

#[test]
fn direct_messages_are_sorted_by_latest_message_id() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);
}

#[test]
fn direct_message_unread_count_counts_unread_channels() {
    let mut state = state_with_direct_messages();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(10),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(20),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(30),
                last_acked_message_id: None,
                mention_count: 5,
            },
        ],
    });

    assert_eq!(state.direct_message_unread_count(), 1);
}

#[test]
fn background_channel_message_updates_unread_without_scheduling_ack() {
    let mut state = state_with_direct_messages();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(10),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(20),
                last_acked_message_id: Some(Id::new(200)),
                mention_count: 0,
            },
        ],
    });
    state.push_effect(AppEvent::ActivateChannel {
        channel_id: Id::new(20),
    });
    assert!(state.drain_pending_commands().is_empty());

    state.push_event(direct_message_create_event(Id::new(10), 101));

    assert_eq!(state.direct_message_unread_count(), 1);
    assert_ne!(state.channel_unread(Id::new(10)), ChannelUnreadState::Seen);
    assert!(state.next_read_ack_deadline().is_none());
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn active_channel_read_state_coalesces_when_new_messages_arrive_at_latest() {
    {
        let mut state = state_with_direct_messages();
        state.push_event(AppEvent::ReadStateInit {
            entries: vec![
                ReadStateInfo {
                    channel_id: Id::new(10),
                    last_acked_message_id: Some(Id::new(100)),
                    mention_count: 0,
                },
                ReadStateInfo {
                    channel_id: Id::new(20),
                    last_acked_message_id: Some(Id::new(200)),
                    mention_count: 0,
                },
            ],
        });
        state.push_effect(AppEvent::ActivateChannel {
            channel_id: Id::new(20),
        });
        assert!(state.drain_pending_commands().is_empty());

        state.push_event(direct_message_create_event(Id::new(20), 201));
        let first_deadline = state
            .next_read_ack_deadline()
            .expect("active message should schedule read ack");
        state.push_event(direct_message_create_event(Id::new(20), 202));

        assert_eq!(state.direct_message_unread_count(), 0);
        assert_eq!(state.channel_unread(Id::new(20)), ChannelUnreadState::Seen);
        assert_eq!(state.next_read_ack_deadline(), Some(first_deadline));
        assert!(state.drain_pending_commands().is_empty());
        state.flush_due_read_acks(first_deadline);
        assert_eq!(
            state.drain_pending_commands(),
            vec![AppCommand::AckChannel {
                channel_id: Id::new(20),
                message_id: Id::new(202),
            }]
        );
    }

    {
        let mut state = state_with_writable_channel();
        state.push_event(AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![GuildNotificationSettingsInfo {
                guild_id: Some(Id::new(1)),
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: false,
                mute_end_time: None,
                suppress_everyone: false,
                suppress_roles: false,
                channel_overrides: Vec::new(),
            }],
        });

        state.push_event(notification_message_event(Id::new(2), "hello"));

        assert_eq!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
        assert!(state.drain_pending_commands().is_empty());
        assert_eq!(
            drain_debounced_read_ack(&mut state),
            vec![AppCommand::AckChannel {
                channel_id: Id::new(2),
                message_id: Id::new(50),
            }]
        );
    }

    {
        let mut state = state_with_message_ids([1, 2, 3]);
        state.push_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        });
        state.push_event(AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id: Id::new(2),
                last_acked_message_id: Some(Id::new(1)),
                mention_count: 0,
            }],
        });
        state.activate_channel(Id::new(2));
        state.set_message_view_height(10);
        assert_eq!(state.unread_divider_message_index(), Some(1));
        assert!(state.unread_banner().is_some());
        state.drain_pending_commands();

        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(4),
            author_id: Id::new(10),
            author: "me".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("sent while reading latest".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
        assert_eq!(state.unread_divider_message_index(), None);
        assert_eq!(state.unread_banner(), None);
        assert_eq!(state.unread_divider_last_acked_id(), None);
        assert!(state.drain_pending_commands().is_empty());
    }
}

#[test]
fn channel_unread_message_count_counts_loaded_messages_after_ack() {
    let mut state = state_with_direct_messages();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(10),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(20),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
        ],
    });
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(20),
        before: None,
        messages: (101..=105)
            .map(|message_id| MessageInfo {
                guild_id: None,
                ..message_info(Id::new(20), message_id)
            })
            .collect(),
    });

    assert_eq!(state.channel_unread_message_count(Id::new(20)), 5);
    assert_eq!(state.direct_message_unread_count(), 1);
}

#[test]
fn direct_message_selection_waits_for_channel_confirmation() {
    let mut state = state_with_direct_messages();

    state.confirm_selected_guild();
    assert_eq!(state.selected_channel_id(), None);

    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
}

#[test]
fn activate_channel_effect_moves_direct_message_cursor_to_target() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    assert_eq!(state.selected_channel(), 0);

    state.push_effect(AppEvent::ActivateChannel {
        channel_id: Id::new(30),
    });

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(state.selected_channel(), 2);
}

#[test]
fn direct_message_sorting_uses_channel_id_fallback() {
    let mut state = DashboardState::new();
    for (channel_id, name) in [(Id::new(10), "older-id"), (Id::new(30), "newer-id")] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: name.to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
    }
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["newer-id", "older-id"]);
}

#[test]
fn restoring_discord_snapshot_recovers_missed_guilds_and_direct_messages() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let guild_channel_id: Id<ChannelMarker> = Id::new(2);
    let dm_channel_id: Id<ChannelMarker> = Id::new(20);
    let mut snapshot = DiscordState::default();
    snapshot.apply_event(&AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    snapshot.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: guild_channel_id,
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
    });
    snapshot.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id: dm_channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(200)),
        name: "alice".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let mut state = DashboardState::new();
    state.restore_discord_snapshot(snapshot);

    assert_eq!(state.last_status(), None);
    assert_eq!(state.current_user(), Some("neo"));
    assert_eq!(state.current_user_id, Some(Id::new(10)));
    assert_eq!(state.guild_pane_entries().len(), 2);

    state.confirm_selected_guild();
    assert_eq!(state.selected_guild_id(), Some(guild_id));
    assert_eq!(channel_entry_names(&state), vec!["general"]);

    state.selected_guild = 0;
    state.confirm_selected_guild();
    assert_eq!(channel_entry_names(&state), vec!["alice"]);
}

#[test]
fn direct_message_cursor_stays_on_same_channel_after_recency_sort() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    assert_eq!(state.selected_channel(), 1);
    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);

    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id: Id::new(30),
        message_id: Id::new(300),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("new empty dm".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(channel_entry_names(&state), vec!["empty", "new", "old"]);
    assert_eq!(state.selected_channel(), 2);
}

#[test]
fn channel_tree_groups_category_children() {
    let state = state_with_channel_tree();
    let entries = state.channel_pane_entries();

    assert!(matches!(
        entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: false,
            ..
        }
    ));
    assert!(matches!(
        entries[1],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Middle,
            ..
        }
    ));
    assert!(matches!(
        entries[2],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Last,
            ..
        }
    ));
}

#[test]
fn selected_channel_category_can_be_closed_and_opened() {
    let mut state = state_with_channel_tree();

    assert_eq!(state.channel_pane_entries().len(), 3);
    assert_eq!(state.selected_channel_id(), None);

    state.close_selected_channel_category();
    let closed_entries = state.channel_pane_entries();
    assert_eq!(closed_entries.len(), 1);
    assert!(matches!(
        closed_entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: true,
            ..
        }
    ));

    state.open_selected_channel_category();
    assert_eq!(state.channel_pane_entries().len(), 3);
}

#[test]
fn selected_channel_child_can_close_parent_category() {
    let mut state = state_with_channel_tree();
    state.selected_channel = 1;

    state.toggle_selected_channel_category();
    let entries = state.channel_pane_entries();
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0],
        ChannelPaneEntry::CategoryHeader {
            collapsed: true,
            ..
        }
    ));
}

#[test]
fn moving_guild_cursor_does_not_activate_guild() {
    let mut state = state_with_two_guilds();
    state.focus_pane(FocusPane::Guilds);

    state.confirm_selected_guild();
    let active_guild = state.selected_guild_id();
    assert!(active_guild.is_some());

    state.move_down();
    assert_eq!(state.selected_guild, 2);
    assert_eq!(state.selected_guild_id(), active_guild);

    state.confirm_selected_guild();
    assert_ne!(state.selected_guild_id(), active_guild);
}

#[test]
fn active_guild_entry_tracks_confirmed_guild() {
    let mut state = state_with_two_guilds();
    state.focus_pane(FocusPane::Guilds);

    {
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[0]));
        assert!(!state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.confirm_selected_guild();
    {
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[0]));
        assert!(state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.move_down();
    {
        let entries = state.guild_pane_entries();
        assert!(state.is_active_guild_entry(&entries[1]));
        assert!(!state.is_active_guild_entry(&entries[2]));
    }

    state.confirm_selected_guild();
    let entries = state.guild_pane_entries();
    assert!(!state.is_active_guild_entry(&entries[1]));
    assert!(state.is_active_guild_entry(&entries[2]));
}

#[test]
fn moving_channel_cursor_does_not_activate_channel() {
    let mut state = state_with_channel_tree();
    let random_id = Id::new(12);
    state.focus_pane(FocusPane::Channels);

    assert_eq!(state.selected_channel_id(), None);

    state.move_down();
    state.move_down();
    assert_eq!(state.selected_channel, 2);
    assert_eq!(state.selected_channel_id(), None);

    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(random_id));
}

#[test]
fn active_channel_entry_tracks_confirmed_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    {
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[0]));
        assert!(!state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.move_down();
    state.confirm_selected_channel();
    {
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[0]));
        assert!(state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.move_down();
    {
        let entries = state.channel_pane_entries();
        assert!(state.is_active_channel_entry(&entries[1]));
        assert!(!state.is_active_channel_entry(&entries[2]));
    }

    state.confirm_selected_channel();
    let entries = state.channel_pane_entries();
    assert!(!state.is_active_channel_entry(&entries[1]));
    assert!(state.is_active_channel_entry(&entries[2]));
}

#[test]
fn selected_folder_can_be_closed_and_opened() {
    let mut state = state_with_folder(Some(42));

    assert_eq!(state.guild_pane_entries().len(), 4);
    state.close_selected_folder();
    let closed_entries = state.guild_pane_entries();
    assert_eq!(closed_entries.len(), 2);
    assert!(matches!(
        closed_entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));

    state.open_selected_folder();
    let open_entries = state.guild_pane_entries();
    assert_eq!(open_entries.len(), 4);
    assert!(matches!(
        open_entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: false,
            ..
        }
    ));
}

#[test]
fn folder_children_use_middle_and_last_branches() {
    let state = state_with_folder(Some(42));

    let entries = state.guild_pane_entries();
    assert!(matches!(
        entries[2],
        GuildPaneEntry::Guild {
            branch: GuildBranch::Middle,
            ..
        }
    ));
    assert!(matches!(
        entries[3],
        GuildPaneEntry::Guild {
            branch: GuildBranch::Last,
            ..
        }
    ));
}

#[test]
fn folder_without_id_can_be_closed() {
    let mut state = state_with_folder(None);

    state.close_selected_folder();
    let entries = state.guild_pane_entries();
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));
}

#[test]
fn selected_folder_child_can_close_parent() {
    let mut state = state_with_folder(Some(42));
    state.selected_guild = 2;

    state.toggle_selected_folder();
    let entries = state.guild_pane_entries();
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader {
            collapsed: true,
            ..
        }
    ));
}

fn channel_entry_names(state: &DashboardState) -> Vec<&str> {
    state
        .channel_pane_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            ChannelPaneEntry::Channel { state, .. } => Some(state.name.as_str()),
            ChannelPaneEntry::CategoryHeader { .. } => None,
        })
        .collect()
}
