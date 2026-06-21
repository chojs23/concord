use super::*;
use crate::discord::AppCommand;
use crate::tui::state::ActiveModalPopupKind;
use crate::tui::state::MessagePaneSource;

#[test]
fn forum_channel_renders_loaded_posts_in_message_pane() {
    let mut state = state_with_forum_channel_posts();

    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ForumPosts {
            channel_id: Id::new(20)
        })
    );
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let mut preview =
        forum_preview_message(guild_id, Id::new(30), 30, "neo", "first message preview");
    preview.reactions = vec![ReactionInfo {
        count: 2,
        ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
    }];

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![ChannelInfo {
            owner_id: Some(Id::new(88)),
            position: Some(0),
            message_count: Some(1),
            member_count: None,
            total_message_sent: Some(1),
            ..forum_thread_info(guild_id, forum_id, 30, "welcome", None, false)
        }],
        first_messages: vec![preview],
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
    assert_eq!(post.last_activity_message_id, Some(Id::new(30)));
    assert_eq!(post.section_label.as_deref(), Some("Active posts"));
}

#[test]
fn forum_post_items_resolve_applied_tag_names() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();
    let mut forum = forum_channel_info(guild_id, forum_id);
    forum.available_tags = vec![
        ForumTagInfo {
            id: Id::new(101),
            name: "question".to_owned(),
            moderated: false,
            emoji_id: None,
            emoji_name: None,
        },
        ForumTagInfo {
            id: Id::new(102),
            name: "rust".to_owned(),
            moderated: false,
            emoji_id: None,
            emoji_name: None,
        },
    ];

    state.push_event(guild_create_event(guild_id, "guild", vec![forum]));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    let mut thread = forum_thread_info(guild_id, forum_id, 30, "welcome", None, false);
    thread.applied_tags = vec![Id::new(102), Id::new(101)];

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");
    assert_eq!(post.applied_tags, vec!["rust", "question"]);
}

#[test]
fn forum_post_preview_ignores_latest_message_when_starter_is_missing() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![forum_thread_info(
            guild_id,
            forum_id,
            30,
            "welcome",
            Some(300),
            false,
        )],
        first_messages: vec![forum_preview_message(
            guild_id,
            Id::new(30),
            300,
            "neo",
            "latest reply",
        )],
        has_more: false,
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");

    assert_eq!(post.preview_author, None);
    assert_eq!(post.preview_content, None);
    assert_eq!(post.last_activity_message_id, Some(Id::new(300)));
}

#[test]
fn forum_post_preview_uses_thread_creator_when_starter_is_missing() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(30);
    let owner_id = Id::new(88);
    let role_id = Id::<RoleMarker>::new(7);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: None,
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: vec![member_with_roles(owner_id, "neo", vec![role_id])],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            ..RoleInfo::test(role_id, "Maintainer")
        }],
        emojis: Vec::new(),
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![ChannelInfo {
            owner_id: Some(owner_id),
            ..forum_thread_info(
                guild_id,
                forum_id,
                thread_id.get(),
                "welcome",
                Some(300),
                false,
            )
        }],
        first_messages: vec![forum_preview_message(
            guild_id,
            thread_id,
            300,
            "latest-replier",
            "latest reply",
        )],
        has_more: false,
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");

    assert_eq!(post.preview_author_id, Some(owner_id));
    assert_eq!(post.preview_author.as_deref(), Some("neo"));
    assert_eq!(post.preview_author_color, Some(0xFFAA00));
    assert_eq!(
        post.preview_content.as_deref(),
        Some("original message deleted")
    );
    assert_eq!(post.last_activity_message_id, Some(Id::new(300)));
}

#[test]
fn forum_post_preview_shows_deleted_starter_with_author() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let mut deleted_starter = forum_preview_message(guild_id, Id::new(30), 30, "neo", "");
    deleted_starter.content = None;
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![forum_thread_info(
            guild_id,
            forum_id,
            30,
            "welcome",
            Some(300),
            false,
        )],
        first_messages: vec![deleted_starter],
        has_more: false,
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");

    assert_eq!(post.preview_author.as_deref(), Some("neo"));
    assert_eq!(
        post.preview_content.as_deref(),
        Some("original message deleted")
    );
}

#[test]
fn forum_post_preview_keeps_literal_unavailable_text() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![forum_thread_info(
            guild_id,
            forum_id,
            30,
            "welcome",
            Some(300),
            false,
        )],
        first_messages: vec![forum_preview_message(
            guild_id,
            Id::new(30),
            30,
            "neo",
            "<message content unavailable>",
        )],
        has_more: false,
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");

    assert_eq!(post.preview_author.as_deref(), Some("neo"));
    assert_eq!(
        post.preview_content.as_deref(),
        Some("<message content unavailable>")
    );
}

#[test]
fn forum_post_first_page_starts_cursor_at_top_and_next_page_appends() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        threads: vec![
            forum_thread_info(guild_id, forum_id, 30, "newest", Some(300), false),
            forum_thread_info(guild_id, forum_id, 31, "middle", Some(200), false),
        ],
        first_messages: Vec::new(),
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
        threads: vec![forum_thread_info(
            guild_id,
            forum_id,
            32,
            "older",
            Some(100),
            false,
        )],
        first_messages: Vec::new(),
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        threads: vec![
            forum_thread_info(guild_id, forum_id, 30, "active", Some(300), false),
            forum_thread_info(guild_id, forum_id, 31, "shared", Some(200), false),
        ],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Archived,
        offset: 0,
        next_offset: 2,
        threads: vec![
            forum_thread_info(guild_id, forum_id, 31, "shared", Some(400), true),
            forum_thread_info(guild_id, forum_id, 32, "archived", Some(100), true),
        ],
        first_messages: Vec::new(),
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Posts arrive in the order Discord returned them (stale): the post with
    // the newest message id sits in the middle of the list.
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 3,
        threads: vec![
            forum_thread_info(guild_id, forum_id, 30, "stale-top", Some(100), false),
            forum_thread_info(guild_id, forum_id, 31, "newest-activity", Some(500), false),
            forum_thread_info(guild_id, forum_id, 32, "older", Some(200), false),
        ],
        first_messages: Vec::new(),
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Mirrors a real Discord response: posts arrive sorted by activity but a
    // pinned post sits in the middle, and the official client lifts it to the
    // top while keeping the rest in delivered order.
    let mut newest = forum_thread_info(guild_id, forum_id, 30, "newest", Some(300), false);
    newest.flags = Some(0);
    let mut pinned = forum_thread_info(guild_id, forum_id, 31, "pinned-post", Some(200), false);
    pinned.flags = Some(1 << 1);
    let mut middle = forum_thread_info(guild_id, forum_id, 32, "middle", Some(150), false);
    middle.flags = Some(0);
    let mut older = forum_thread_info(guild_id, forum_id, 33, "older", Some(100), false);
    older.flags = Some(0);

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 4,
        threads: vec![newest, pinned, middle, older],
        first_messages: Vec::new(),
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![forum_thread_info(
            guild_id, forum_id, 30, "welcome", None, false,
        )],
        first_messages: Vec::new(),
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![welcome_thread.clone()],
        first_messages: Vec::new(),
        has_more: false,
    });

    let mut snapshot_state = DiscordState::default();
    snapshot_state.apply_event(&guild_create_event(
        guild_id,
        "guild",
        vec![
            forum_channel_info(guild_id, forum_id),
            welcome_thread,
            new_thread.clone(),
        ],
    ));
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
    let mut thread = forum_thread_info(
        guild_id,
        forum_id,
        thread_id.get(),
        "new post",
        Some(300),
        false,
    );
    thread.current_user_joined_thread = Some(true);

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id), thread],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(thread_id, Some(Id::new(299)), 0)],
    });

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Unread
    );
}

#[test]
fn forum_sidebar_unread_ignores_left_child_posts() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut left_thread = forum_thread_info(
        guild_id,
        forum_id,
        thread_id.get(),
        "left post",
        Some(300),
        false,
    );
    left_thread.current_user_joined_thread = Some(false);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id), left_thread],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(thread_id, Some(Id::new(299)), 0)],
    });

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Seen
    );
    assert_eq!(
        state.sidebar_guild_unread(guild_id),
        ChannelUnreadState::Seen
    );
}

#[test]
fn forum_sidebar_unread_aggregates_child_notification_count() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();
    let mut thread = forum_thread_info(
        guild_id,
        forum_id,
        thread_id.get(),
        "new post",
        Some(299),
        false,
    );
    thread.current_user_joined_thread = Some(true);

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id), thread],
    ));
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::AllMessages),
            ..GuildNotificationSettingsInfo::test(Some(guild_id))
        },
    ]));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(thread_id, Some(Id::new(299)), 0)],
    });
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id: thread_id,
        message_id: Id::new(300),
        author_id: Id::new(99),
        content: Some("new post body".to_owned()),
        ..guild_message_create_fixture()
    }));

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
fn opening_forum_channel_keeps_child_posts_unread_until_post_opens() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();
    let mut thread = forum_thread_info(
        guild_id,
        forum_id,
        thread_id.get(),
        "new post",
        Some(300),
        false,
    );
    thread.current_user_joined_thread = Some(true);

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id), thread.clone()],
    ));
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(thread_id, Some(Id::new(299)), 0)],
    });
    state.confirm_selected_guild();

    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Unread
    );
    state.confirm_selected_channel();
    let commands = state.drain_pending_commands();

    assert!(commands.is_empty());
    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Unread
    );

    state.focus_pane(FocusPane::Messages);
    let subscribe = state.activate_selected_message_pane_item();
    let commands = state.drain_pending_commands();
    apply_optimistic_ack_commands(&mut state, &commands);

    assert_eq!(
        subscribe,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id,
            channel_id: thread_id,
        })
    );
    assert_eq!(
        commands,
        vec![AppCommand::AckChannel {
            channel_id: thread_id,
            message_id: Id::new(300),
        }]
    );
    assert_eq!(
        state.sidebar_channel_unread(forum_id),
        ChannelUnreadState::Seen
    );
}

#[test]
fn forum_post_items_show_loaded_new_message_count() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(31);
    let mut state = DashboardState::new();
    let mut thread = forum_thread_info(
        guild_id,
        forum_id,
        thread_id.get(),
        "new post",
        Some(301),
        false,
    );
    thread.current_user_joined_thread = Some(true);

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(thread_id, Some(Id::new(299)), 0)],
    });
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: thread_id,
        before: None,
        messages: vec![
            forum_preview_message(guild_id, thread_id, 300, "neo", "first new comment"),
            forum_preview_message(guild_id, thread_id, 301, "neo", "second new comment"),
        ],
    });

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .next()
        .expect("forum post should be visible");
    assert_eq!(post.new_message_count, 2);
}

#[test]
fn hidden_forum_child_posts_are_not_listed_when_forum_opens() {
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
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
    ));
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        threads: vec![
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
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(public_thread_id, Some(Id::new(299)), 0),
            read_state_info(private_thread_id, Some(Id::new(399)), 0),
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
    assert!(state.drain_pending_commands().is_empty());
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
fn forum_channel_starts_new_post_overlay() {
    let mut state = state_with_forum_channel_posts();

    assert!(!state.can_send_in_selected_channel());
    state.start_composer();

    assert!(!state.is_composing());
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ForumPostComposer));
}

#[test]
fn forum_post_bottom_scroll_uses_last_full_page() {
    let mut state = state_with_many_forum_channel_posts(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(10);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);

    state.jump_bottom();

    assert_eq!(state.selected_forum_post(), 9);
    assert_eq!(state.message_scroll(), 9);
    assert_eq!(
        state
            .visible_forum_post_items()
            .iter()
            .map(|post| post.label.as_str())
            .collect::<Vec<_>>(),
        vec!["post 1"]
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
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ForumPosts {
            channel_id: Id::new(20)
        })
    );
    assert_eq!(state.selected_forum_post(), expected_selected);
    assert_eq!(state.message_scroll(), expected_scroll);
}

#[test]
fn poll_vote_actions_are_available_by_default() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        ..guild_message_create_fixture()
    }));

    let actions = state.selected_message_action_items();
    let poll_action = actions
        .iter()
        .find(|action| action.kind == MessageActionKind::OpenPollVotePicker)
        .expect("poll action should exist");

    assert_eq!(poll_action.label, "choose poll votes");
    assert!(poll_action.enabled);
}
