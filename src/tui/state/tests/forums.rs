use super::*;

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
    // posts out of strict timestamp order because its index lags behind real
    // activity. We re-sort by `last_message_id` because the snowflake encodes the
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
fn forum_channel_upsert_inserts_new_thread_at_top_of_active_list() {
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
            guild_id, forum_id, 30, "welcome", None, false,
        )],
        preview_messages: Vec::new(),
        has_more: false,
    });

    state.push_event(AppEvent::ChannelUpsert(forum_thread_info(
        guild_id,
        forum_id,
        31,
        "brand-new",
        None,
        false,
    )));

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["brand-new", "welcome"]
    );

    // Re-emitting the same thread (e.g. via THREAD_LIST_SYNC) must not duplicate.
    state.push_event(AppEvent::ChannelUpsert(forum_thread_info(
        guild_id,
        forum_id,
        31,
        "brand-new",
        None,
        false,
    )));
    assert_eq!(state.selected_forum_post_items().len(), 2);
}

#[test]
fn forum_channel_upsert_effect_inserts_new_thread_after_snapshot_restore() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let welcome_thread = forum_thread_info(guild_id, forum_id, 30, "welcome", None, false);
    let new_thread = forum_thread_info(guild_id, forum_id, 31, "brand-new", None, false);
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
        posts: vec![welcome_thread.clone()],
        preview_messages: Vec::new(),
        has_more: false,
    });

    let mut snapshot_state = DiscordState::default();
    snapshot_state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            forum_channel_info(guild_id, forum_id),
            welcome_thread,
            new_thread.clone(),
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.restore_discord_snapshot(snapshot_state);
    state.push_effect(AppEvent::ChannelUpsert(new_thread.clone()));

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["brand-new", "welcome"]
    );

    state.push_effect(AppEvent::ChannelUpsert(new_thread));
    assert_eq!(state.selected_forum_post_items().len(), 2);
}

#[test]
fn forum_sidebar_unread_aggregates_unread_child_posts() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            forum_channel_info(guild_id, forum_id),
            forum_thread_info(
                guild_id,
                forum_id,
                thread_id.get(),
                "new post",
                Some(300),
                false,
            ),
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: thread_id,
            last_acked_message_id: Some(Id::new(299)),
            mention_count: 0,
        }],
    });

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Unread
    );
}

#[test]
fn forum_sidebar_unread_aggregates_child_notification_count() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            forum_channel_info(guild_id, forum_id),
            forum_thread_info(
                guild_id,
                forum_id,
                thread_id.get(),
                "new post",
                Some(299),
                false,
            ),
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
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
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id: thread_id,
            last_acked_message_id: Some(Id::new(299)),
            mention_count: 0,
        }],
    });
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: thread_id,
        message_id: Id::new(300),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("new post body".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Notified(1)
    );
    assert_eq!(
        state.sidebar_guild_unread(guild_id),
        ChannelUnreadState::Notified(1)
    );
}

#[test]
fn opening_forum_channel_marks_unread_child_posts_as_read() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();
    let mut forum = forum_channel_info(guild_id, forum_id);
    forum.last_message_id = Some(Id::new(200));

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            forum,
            forum_thread_info(
                guild_id,
                forum_id,
                thread_id.get(),
                "new post",
                Some(300),
                false,
            ),
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
                channel_id: forum_id,
                last_acked_message_id: Some(Id::new(199)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: thread_id,
                last_acked_message_id: Some(Id::new(299)),
                mention_count: 0,
            },
        ],
    });
    state.confirm_selected_guild();

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Unread
    );
    state.confirm_selected_channel();

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Seen
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::AckChannels {
            targets: vec![(forum_id, Id::new(200)), (thread_id, Id::new(300))]
        }]
    );
}

#[test]
fn hidden_forum_child_posts_are_not_listed_or_acked() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let public_thread_id = Id::new(31);
    let private_thread_id = Id::new(32);
    let mut private_thread = forum_thread_info(
        guild_id,
        forum_id,
        private_thread_id.get(),
        "private post",
        Some(400),
        false,
    );
    private_thread.kind = "GuildPrivateThread".to_owned();
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            forum_channel_info(guild_id, forum_id),
            forum_thread_info(
                guild_id,
                forum_id,
                public_thread_id.get(),
                "public post",
                Some(300),
                false,
            ),
            private_thread.clone(),
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        posts: vec![
            forum_thread_info(
                guild_id,
                forum_id,
                public_thread_id.get(),
                "public post",
                Some(300),
                false,
            ),
            private_thread,
        ],
        preview_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: public_thread_id,
                last_acked_message_id: Some(Id::new(299)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: private_thread_id,
                last_acked_message_id: Some(Id::new(399)),
                mention_count: 0,
            },
        ],
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    assert_eq!(
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| post.channel_id)
            .collect::<Vec<_>>(),
        vec![public_thread_id]
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::AckChannels {
            targets: vec![(public_thread_id, Id::new(300))]
        }]
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
