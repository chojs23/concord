use super::*;
use crate::discord::ids::marker::RoleMarker;
use crate::discord::{ForumPostDataInfo, ThreadMemberInfo};
use crate::tui::state::NotificationInboxItem;

fn current_user_thread_member(thread_id: Id<ChannelMarker>, muted: bool) -> ThreadMemberInfo {
    ThreadMemberInfo {
        thread_id: Some(thread_id),
        user_id: Some(Id::new(999)),
        join_timestamp: Some("2026-08-14T00:00:00.000Z".to_owned()),
        flags: Some(2),
        muted: Some(muted),
        mute_end_time: None,
        selected_time_window: None,
        member: None,
        presence: None,
        extra_fields: BTreeMap::new(),
    }
}

fn notification_inbox_channel_ids(state: &DashboardState) -> Vec<Id<ChannelMarker>> {
    state
        .notification_inbox_items()
        .into_iter()
        .filter_map(|item| match item {
            NotificationInboxItem::Unread(item) => Some(item.channel_id),
            NotificationInboxItem::Mention(_) => None,
        })
        .collect()
}

#[test]
fn notification_inbox_applies_display_order_at_each_scope() {
    // Private channels use their latest activity.
    {
        let older_channel_id = Id::new(10);
        let newer_channel_id = Id::new(20);
        let mut state = DashboardState::new();
        for channel in [
            ChannelInfo {
                last_message_id: Some(Id::new(100)),
                ..dm_channel_info(older_channel_id, "older")
            },
            ChannelInfo {
                last_message_id: Some(Id::new(200)),
                ..dm_channel_info(newer_channel_id, "newer")
            },
        ] {
            state.push_event(AppEvent::ChannelUpsert(channel));
        }

        state.open_notification_inbox();

        assert_eq!(
            notification_inbox_channel_ids(&state),
            vec![newer_channel_id, older_channel_id]
        );
    }

    // Guilds use the user's folder and sidebar order.
    {
        let first_guild_id = Id::new(1);
        let second_guild_id = Id::new(2);
        let first_channel_id = Id::new(101);
        let second_channel_id = Id::new(201);
        let mut state = DashboardState::new();
        for (guild_id, channel_id, name) in [
            (first_guild_id, first_channel_id, "first"),
            (second_guild_id, second_channel_id, "second"),
        ] {
            state.push_event(guild_create_event(
                guild_id,
                name,
                vec![ChannelInfo {
                    last_message_id: Some(Id::new(channel_id.get() + 1)),
                    ..text_channel_info(guild_id, channel_id, "general")
                }],
            ));
        }
        state.push_event(user_settings_update(vec![
            GuildFolder {
                id: None,
                name: None,
                color: None,
                guild_ids: vec![second_guild_id],
            },
            GuildFolder {
                id: None,
                name: None,
                color: None,
                guild_ids: vec![first_guild_id],
            },
        ]));

        state.open_notification_inbox();

        assert_eq!(
            notification_inbox_channel_ids(&state),
            vec![second_channel_id, first_channel_id]
        );
    }

    // Channels use their tree order within a guild.
    {
        let guild_id = Id::new(1);
        let first_root_id = Id::new(90);
        let category_id = Id::new(80);
        let first_child_id = Id::new(10);
        let thread_id = Id::new(60);
        let last_root_id = Id::new(20);
        let unread = |message_id| Some(Id::new(message_id));
        let mut state = DashboardState::new();
        state.push_event(crate::discord::test_builders::guild_create_event(
            GuildCreateFixture {
                guild_id,
                name: "guild".to_owned(),
                channels: vec![
                    ChannelInfo {
                        last_message_id: unread(101),
                        ..positioned_text_channel_info(guild_id, first_root_id, "first-root", 0)
                    },
                    category_channel_info(guild_id, category_id, "category", 1),
                    ChannelInfo {
                        last_message_id: unread(102),
                        ..child_text_channel_info(
                            guild_id,
                            first_child_id,
                            category_id,
                            "first-child",
                            0,
                        )
                    },
                    ChannelInfo {
                        last_message_id: unread(103),
                        ..thread_channel_info(guild_id, first_child_id, thread_id, "thread")
                    },
                    ChannelInfo {
                        last_message_id: unread(104),
                        ..positioned_text_channel_info(guild_id, last_root_id, "last-root", 2)
                    },
                ],
                current_user_thread_members: vec![current_user_thread_member(thread_id, false)],
                ..GuildCreateFixture::new(guild_id)
            },
        ));

        state.open_notification_inbox();

        assert_eq!(
            notification_inbox_channel_ids(&state),
            vec![first_root_id, first_child_id, thread_id, last_root_id]
        );
    }
}

#[test]
fn notification_inbox_includes_only_eligible_unread_channels() {
    let private_channel_id = Id::new(10);
    let muted_guild_id = Id::new(1);
    let muted_guild_channel_id = Id::new(101);
    let guild_id = Id::new(2);
    let visible_channel_id = Id::new(201);
    let muted_channel_id = Id::new(202);
    let muted_category_id = Id::new(203);
    let muted_child_id = Id::new(204);
    let muted_thread_id = Id::new(205);
    let hidden_channel_id = Id::new(206);
    let voice_channel_id = Id::new(207);
    let stage_channel_id = Id::new(208);
    let unjoined_thread_id = Id::new(209);
    let archived_thread_id = Id::new(210);
    let unread = |message_id| Some(Id::new(message_id));
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: unread(100),
        ..dm_channel_info(private_channel_id, "private")
    }));
    state.push_event(guild_create_event(
        muted_guild_id,
        "muted-guild",
        vec![ChannelInfo {
            last_message_id: unread(200),
            ..positioned_text_channel_info(
                muted_guild_id,
                muted_guild_channel_id,
                "muted-guild-channel",
                0,
            )
        }],
    ));
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            guild_id,
            name: "visible-guild".to_owned(),
            channels: vec![
                ChannelInfo {
                    last_message_id: unread(300),
                    ..positioned_text_channel_info(guild_id, visible_channel_id, "visible", 0)
                },
                ChannelInfo {
                    last_message_id: unread(301),
                    ..positioned_text_channel_info(guild_id, muted_channel_id, "muted", 1)
                },
                category_channel_info(guild_id, muted_category_id, "muted-category", 2),
                ChannelInfo {
                    last_message_id: unread(302),
                    ..child_text_channel_info(
                        guild_id,
                        muted_child_id,
                        muted_category_id,
                        "muted-child",
                        0,
                    )
                },
                ChannelInfo {
                    last_message_id: unread(303),
                    ..thread_channel_info(guild_id, muted_child_id, muted_thread_id, "muted-thread")
                },
                ChannelInfo {
                    last_message_id: unread(304),
                    ..positioned_text_channel_info(guild_id, hidden_channel_id, "not-opted-in", 3)
                },
                ChannelInfo {
                    last_message_id: unread(305),
                    position: Some(4),
                    ..voice_channel_info(guild_id, voice_channel_id, "voice")
                },
                ChannelInfo {
                    kind: "stage".to_owned(),
                    last_message_id: unread(306),
                    ..positioned_text_channel_info(guild_id, stage_channel_id, "stage", 5)
                },
                ChannelInfo {
                    last_message_id: unread(307),
                    ..thread_channel_info(
                        guild_id,
                        visible_channel_id,
                        unjoined_thread_id,
                        "unjoined-thread",
                    )
                },
                ChannelInfo {
                    last_message_id: unread(308),
                    thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(true, false)),
                    ..thread_channel_info(
                        guild_id,
                        visible_channel_id,
                        archived_thread_id,
                        "archived-thread",
                    )
                },
            ],
            current_user_thread_members: vec![
                current_user_thread_member(muted_thread_id, false),
                current_user_thread_member(archived_thread_id, false),
            ],
            ..GuildCreateFixture::new(guild_id)
        },
    ));

    let private_settings = GuildNotificationSettingsInfo {
        muted: true,
        ..GuildNotificationSettingsInfo::test(None)
    };
    let muted_guild_settings = GuildNotificationSettingsInfo {
        muted: true,
        ..GuildNotificationSettingsInfo::test(Some(muted_guild_id))
    };
    let visible_guild_settings = GuildNotificationSettingsInfo {
        flags: 1 << 14,
        channel_overrides: vec![
            ChannelNotificationOverrideInfo {
                flags: 1 << 12,
                ..ChannelNotificationOverrideInfo::test(visible_channel_id)
            },
            ChannelNotificationOverrideInfo {
                muted: true,
                flags: 1 << 12,
                ..ChannelNotificationOverrideInfo::test(muted_channel_id)
            },
            ChannelNotificationOverrideInfo {
                muted: true,
                flags: 1 << 12,
                ..ChannelNotificationOverrideInfo::test(muted_category_id)
            },
            ChannelNotificationOverrideInfo {
                flags: 1 << 12,
                ..ChannelNotificationOverrideInfo::test(voice_channel_id)
            },
            ChannelNotificationOverrideInfo {
                flags: 1 << 12,
                ..ChannelNotificationOverrideInfo::test(stage_channel_id)
            },
        ],
        ..GuildNotificationSettingsInfo::test(Some(guild_id))
    };
    state.push_event(user_guild_settings_init(vec![
        private_settings,
        muted_guild_settings,
        visible_guild_settings,
    ]));

    state.open_notification_inbox();

    assert_eq!(
        notification_inbox_channel_ids(&state),
        vec![visible_channel_id]
    );
}

#[test]
fn notification_inbox_loads_forum_post_titles_and_preserves_cached_role_colors() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let cached_thread_id = Id::new(31);
    let fallback_thread_id = Id::new(32);
    // Discord forum starter messages use the post thread's snowflake.
    let cached_message_id = Id::new(cached_thread_id.get());
    let author_role_id = Id::<RoleMarker>::new(7);
    let author_role_color = 0x11AA22;
    let mut state = DashboardState::new();
    let thread = |thread_id, name: &str, last_message_id, owner_id| ChannelInfo {
        last_message_id: Some(Id::new(last_message_id)),
        owner_id: Some(Id::new(owner_id)),
        ..thread_channel_info(guild_id, forum_id, thread_id, name)
    };
    let cached_thread = thread(cached_thread_id, "cached post", 301, 99);
    let fallback_thread = thread(fallback_thread_id, "title only", 302, 100);

    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![
                ChannelInfo {
                    kind: "forum".to_owned(),
                    last_message_id: Some(Id::new(302)),
                    ..text_channel_info(guild_id, forum_id, "forum")
                },
                cached_thread.clone(),
                fallback_thread.clone(),
            ],
            current_user_thread_members: vec![
                current_user_thread_member(cached_thread_id, false),
                current_user_thread_member(fallback_thread_id, false),
            ],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.push_event(AppEvent::GuildRoleUpsert {
        guild_id,
        role: RoleInfo {
            color: Some(author_role_color),
            position: 10,
            ..RoleInfo::test(author_role_id, "author")
        },
    });
    state.open_notification_inbox();

    let forum_request = state
        .drain_pending_commands()
        .into_iter()
        .find(|command| matches!(command, AppCommand::LoadForumPostData { .. }))
        .expect("the forum channel should load its post data");
    assert_eq!(
        forum_request,
        AppCommand::LoadForumPostData {
            guild_id,
            channel_id: forum_id,
            thread_ids: vec![cached_thread_id, fallback_thread_id],
        }
    );

    let mut cached_owner = member_with_username(Id::new(99), "Alice", "alice");
    cached_owner.role_ids = vec![author_role_id];
    cached_owner.role_ids_present = true;
    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: vec![cached_thread_id, fallback_thread_id],
        posts: vec![
            ForumPostDataInfo {
                thread_id: cached_thread_id,
                owner: Some(cached_owner),
                first_message: Some(MessageInfo {
                    guild_id: Some(guild_id),
                    channel_id: cached_thread_id,
                    message_id: cached_message_id,
                    author_id: Id::new(99),
                    author: "alice".to_owned(),
                    author_role_ids: vec![author_role_id],
                    content: Some("cached starter content".to_owned()),
                    ..MessageInfo::default()
                }),
                extra_fields: BTreeMap::new(),
            },
            ForumPostDataInfo {
                thread_id: fallback_thread_id,
                owner: Some(member_with_username(Id::new(100), "user-100", "user-100")),
                first_message: None,
                extra_fields: BTreeMap::new(),
            },
        ],
    });

    let mut requested_channel_ids = Vec::new();
    for _ in 0..2 {
        let (request_id, channel_id) = state
            .drain_pending_commands()
            .into_iter()
            .find_map(|command| match command {
                AppCommand::LoadInboxChannelHistory {
                    channel_id,
                    request_id,
                } => Some((request_id, channel_id)),
                _ => None,
            })
            .expect("an unread history request should be pending");
        requested_channel_ids.push(channel_id);
        state.apply_inbox_channel_messages_loaded(request_id, channel_id, &[]);
    }
    requested_channel_ids.sort();
    assert_eq!(
        requested_channel_ids,
        vec![cached_thread_id, fallback_thread_id]
    );
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id,
        member: crate::discord::MemberInfo {
            role_ids: vec![author_role_id],
            ..member_with_username(Id::new(99), "Guild Alice", "alice")
        },
    });

    let items = state
        .notification_inbox_items()
        .into_iter()
        .filter_map(|item| match item {
            NotificationInboxItem::Unread(item) => Some(item),
            NotificationInboxItem::Mention(_) => None,
        })
        .collect::<Vec<_>>();
    let forum = items
        .iter()
        .find(|item| item.channel_id == forum_id)
        .expect("forum channel should be in the inbox");
    assert_eq!(forum.messages.len(), 2);
    assert!(forum.messages.iter().any(|message| {
        message.author == "Guild Alice"
            && message.content == "cached post"
            && message.author_role_color == Some(author_role_color)
    }));
    assert!(
        forum
            .messages
            .iter()
            .any(|message| { message.author == "user-100" && message.content == "title only" })
    );

    let cached = items
        .iter()
        .find(|item| item.channel_id == cached_thread_id)
        .expect("cached post should be in the inbox");
    assert_eq!(cached.messages.len(), 1);
    assert_eq!(cached.messages[0].content, "cached starter content");
    assert_eq!(
        cached.messages[0].author_role_color,
        Some(author_role_color)
    );

    let fallback = items
        .iter()
        .find(|item| item.channel_id == fallback_thread_id)
        .expect("fallback post should be in the inbox");
    assert!(fallback.messages.is_empty());
    assert_eq!(fallback.fallback.as_deref(), Some("Post: title only"));
}

#[test]
fn notification_inbox_preserves_discord_mention_order_across_pages() {
    let mut state = DashboardState::new();
    state.open_notification_inbox();
    let request_id = state
        .drain_pending_commands()
        .into_iter()
        .find_map(|command| match command {
            AppCommand::LoadInboxMentions { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("opening the inbox requests mentions");
    let mention = |message_id| MessageInfo {
        channel_id: Id::new(1),
        message_id: Id::new(message_id),
        author: "alice".to_owned(),
        content: Some("@neo hello".to_owned()),
        ..MessageInfo::default()
    };

    state.apply_inbox_mentions_loaded(request_id, None, &[mention(400), mention(300)], true);
    state.apply_inbox_mentions_loaded(
        request_id,
        Some(Id::new(300)),
        &[mention(250), mention(200), mention(300)],
        false,
    );
    state.switch_notification_inbox_tab(crate::tui::keybindings::SelectionAction::Next);

    let message_ids = state
        .notification_inbox_items()
        .into_iter()
        .filter_map(|item| match item {
            NotificationInboxItem::Mention(item) => Some(item.message_id),
            NotificationInboxItem::Unread(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        message_ids,
        [400, 300, 250, 200].map(Id::<MessageMarker>::new)
    );
}

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
fn active_channel_suppresses_both_desktop_notification_and_sound() {
    let mut state = state_with_writable_channel();
    let channel_id = Id::new(2);
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
        },
    ]));
    let event = notification_message_event(channel_id, "hello");

    // The user is already looking at this channel, so neither channel fires.
    assert!(state.desktop_notification_for_event(&event).is_none());
    assert!(!state.notification_sound_for_event(&event));
}

#[test]
fn notification_sound_for_event_respects_notification_opt_out() {
    let mut state = state_with_hidden_and_visible_channels();
    let channel_id = Id::new(3);
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
        },
    ]));
    state.options.notification_options.desktop_notifications = false;
    let event = notification_message_event(channel_id, "hello");

    assert!(state.desktop_notification_for_event(&event).is_none());
    assert!(!state.notification_sound_for_event(&event));
}
