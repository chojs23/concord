use super::*;
use crate::discord::AppCommand;
use crate::tui::state::ActiveModalPopupKind;
use crate::tui::state::MessagePaneSource;
use crate::tui::state::ThreadActionKind;

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
    assert_eq!(state.focused_thread_card_selection(), Some(0));
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
    assert_eq!(
        post.applied_tags
            .iter()
            .map(|tag| tag.name.as_str())
            .collect::<Vec<_>>(),
        vec!["rust", "question"]
    );
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
fn forum_post_archive_update_moves_post_between_sections() {
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
            forum_thread_info(guild_id, forum_id, 30, "keep", Some(300), false),
            forum_thread_info(guild_id, forum_id, 31, "closing", Some(200), false),
        ],
        first_messages: Vec::new(),
        has_more: false,
    });

    let sections = |state: &DashboardState| {
        state
            .selected_forum_post_items()
            .iter()
            .map(|post| (post.label.clone(), post.archived))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        sections(&state),
        vec![("keep".to_owned(), false), ("closing".to_owned(), false)]
    );

    // A THREAD_UPDATE archiving "closing" must move it to the archived section,
    // not leave it sitting in place among the active posts.
    state.push_event(AppEvent::ChannelUpsert(forum_thread_info(
        guild_id,
        forum_id,
        31,
        "closing",
        Some(200),
        true,
    )));
    assert_eq!(
        sections(&state),
        vec![("keep".to_owned(), false), ("closing".to_owned(), true)]
    );
    let items = state.selected_forum_post_items();
    assert_eq!(items[1].section_label.as_deref(), Some("Archived posts"));

    // Unarchiving moves it back up into the active section.
    state.push_event(AppEvent::ChannelUpsert(forum_thread_info(
        guild_id,
        forum_id,
        31,
        "closing",
        Some(200),
        false,
    )));
    assert_eq!(
        sections(&state),
        vec![("keep".to_owned(), false), ("closing".to_owned(), false)]
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

    // Untagged posts are five rows each, so a height-10 viewport fits two cards
    // and the last full page shows the final two posts.
    assert_eq!(state.selected_forum_post(), 9);
    assert_eq!(state.message_scroll(), 8);
    assert_eq!(
        state
            .visible_thread_card_items()
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

fn thread_action_menu_state() -> DashboardState {
    let mut state = state_with_forum_channel_posts();
    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);
    state
}

/// Mark the selected forum post (thread 31) as joined/followed so the mute row
/// becomes available, mirroring what Discord echoes after a follow.
fn follow_selected_forum_post(state: &mut DashboardState) {
    let mut thread = forum_thread_info(
        Id::new(1),
        Id::new(20),
        31,
        "release notes",
        Some(301),
        false,
    );
    thread.current_user_joined_thread = Some(true);
    state.push_event(AppEvent::ChannelUpsert(thread));
}

#[test]
fn thread_action_menu_opens_with_permission_dimmed_items() {
    let mut state = thread_action_menu_state();

    assert!(state.open_selected_thread_actions());
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu));

    let items = state.selected_thread_action_items();
    assert_eq!(items.len(), 11);

    let enabled = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.enabled)
            .unwrap_or(false)
    };
    // Wired-up actions are selectable.
    assert!(enabled(ThreadActionKind::CopyLink));
    assert!(enabled(ThreadActionKind::ToggleFollow));
    assert!(enabled(ThreadActionKind::CopyId));
    // Muting is only offered once the post is followed; this fixture is not.
    assert!(!enabled(ThreadActionKind::ToggleMute));
    // Management actions require permission; this fixture's user is not the
    // owner or a moderator, so they are dimmed.
    assert!(!enabled(ThreadActionKind::Close));
    assert!(!enabled(ThreadActionKind::Lock));
    assert!(!enabled(ThreadActionKind::Edit));
    assert!(!enabled(ThreadActionKind::Delete));
}

#[test]
fn thread_actions_skipped_outside_forum_posts() {
    // A regular message channel has no forum post focused, so the trigger falls
    // through to the normal action contexts.
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    assert!(!state.open_selected_thread_actions());
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu));
}

#[test]
fn channel_pane_forum_post_thread_opens_thread_actions() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(30);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    // A followed forum post shows up as a thread under the forum in the pane.
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..forum_thread_info(
            guild_id,
            forum_id,
            thread_id.get(),
            "my post",
            Some(301),
            false,
        )
    }));

    state.focus_pane(FocusPane::Channels);
    // Forum is row 0, the nested post thread is row 1.
    state.move_down();
    assert_eq!(
        state
            .channel_pane_entries()
            .get(state.selected_channel())
            .and_then(|entry| entry.channel_id()),
        Some(thread_id),
    );

    // The shared action trigger opens the forum post menu, not the channel menu.
    state.open_leader_actions_for_focused_target();
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu));
    assert!(!state.is_channel_leader_action_active());
    // Mute is only enabled for a followed post, confirming the menu targets the
    // joined thread (id 30) rather than the forum channel.
    let items = state.selected_thread_action_items();
    assert!(
        items
            .iter()
            .find(|item| item.kind == ThreadActionKind::ToggleMute)
            .is_some_and(|item| item.enabled)
    );
}

#[test]
fn thread_action_copies_link_and_thread_id() {
    let mut state = thread_action_menu_state();
    state.open_selected_thread_actions();
    // "Copy link" is the sixth row.
    for _ in 0..5 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert_eq!(
        state.take_copy_text_request(),
        Some((
            "https://discord.com/channels/1/31".to_owned(),
            "Link copied"
        ))
    );

    state.open_selected_thread_actions();
    // "Copy thread ID" is the last row.
    for _ in 0..10 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert_eq!(
        state.take_copy_text_request(),
        Some(("31".to_owned(), "Thread ID copied"))
    );
}

#[test]
fn thread_action_shortcut_jumps_to_matching_row() {
    let mut state = thread_action_menu_state();
    state.open_selected_thread_actions();

    // The default "Copy thread ID" shortcut activates that row directly, even
    // though the selection still sits on the first row.
    let items = state.selected_thread_action_items();
    let copy_id_index = items
        .iter()
        .position(|item| item.kind == ThreadActionKind::CopyId)
        .expect("copy id row is present");
    let chord = state
        .key_bindings()
        .thread_action_shortcuts(&items, copy_id_index)[0];

    assert_eq!(state.activate_thread_action_shortcut(chord), None);
    assert_eq!(
        state.take_copy_text_request(),
        Some(("31".to_owned(), "Thread ID copied"))
    );
    assert!(!state.is_thread_action_menu_active());
}

#[test]
fn thread_action_mute_uses_duration_submenu() {
    let mut state = thread_action_menu_state();
    follow_selected_forum_post(&mut state);
    state.open_selected_thread_actions();
    // "Mute post" is the seventh row; selecting it opens the duration submenu.
    for _ in 0..6 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert!(state.is_thread_action_mute_duration_phase());

    // Picking a duration issues the thread mute command and closes the menu.
    let command = state.activate_selected_thread_action();
    assert!(matches!(
        command,
        Some(AppCommand::SetThreadMuted {
            channel_id,
            muted: true,
            ..
        }) if channel_id == Id::new(31)
    ));
    assert!(!state.is_thread_action_menu_active());
}

#[test]
fn thread_action_follow_toggle_emits_follow_command() {
    let mut state = thread_action_menu_state();
    state.open_selected_thread_actions();
    // "Follow post" is the second row; the fixture post is not followed.
    state.move_thread_action_down();
    let command = state.activate_selected_thread_action();
    assert!(matches!(
        command,
        Some(AppCommand::SetThreadFollowed {
            channel_id,
            followed: true,
            ..
        }) if channel_id == Id::new(31)
    ));
    assert!(!state.is_thread_action_menu_active());
}

#[test]
fn thread_action_disabled_item_is_noop() {
    let mut state = thread_action_menu_state();
    state.open_selected_thread_actions();
    // "Delete post" (tenth row) is disabled this round.
    for _ in 0..9 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    // The menu stays open since nothing happened.
    assert!(state.is_thread_action_menu_active());
}

/// Forum post menu where the current user owns the guild, so the post
/// management actions (close/lock/pin/delete) are permitted. The single thread
/// (31, "release notes") is not archived/locked/pinned.
fn manageable_thread_action_menu_state() -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();
    // Build the forum + post before announcing the current user, so the guild
    // pane selection is not disturbed by a Direct Messages entry appearing.
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: Some(Id::new(10)),
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
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
            31,
            "release notes",
            Some(301),
            false,
        )],
        first_messages: Vec::new(),
        has_more: false,
    });
    // The guild owner is the current user, so post management is permitted.
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);
    state
}

#[test]
fn forum_post_management_actions_emit_commands_for_manager() {
    let mut state = manageable_thread_action_menu_state();
    state.open_selected_thread_actions();

    let items = state.selected_thread_action_items();
    let item = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .expect("action should exist")
            .clone()
    };
    // A manager sees all four wired-up management actions enabled, labelled for
    // the post's current (not archived/locked/pinned) state.
    let close = item(ThreadActionKind::Close);
    assert!(close.enabled && close.label == "Close post");
    let lock = item(ThreadActionKind::Lock);
    assert!(lock.enabled && lock.label == "Lock post");
    let pin = item(ThreadActionKind::Pin);
    assert!(pin.enabled && pin.label == "Pin post");
    assert!(item(ThreadActionKind::Delete).enabled);

    // Close -> archive command (second-row offset 2).
    for _ in 0..2 {
        state.move_thread_action_down();
    }
    assert!(matches!(
        state.activate_selected_thread_action(),
        Some(AppCommand::SetThreadArchived { channel_id, archived: true, .. })
            if channel_id == Id::new(31)
    ));
    assert!(!state.is_thread_action_menu_active());

    // Lock -> lock command (row offset 3).
    state.open_selected_thread_actions();
    for _ in 0..3 {
        state.move_thread_action_down();
    }
    assert!(matches!(
        state.activate_selected_thread_action(),
        Some(AppCommand::SetThreadLocked { channel_id, locked: true, .. })
            if channel_id == Id::new(31)
    ));

    // Pin -> pin command (row offset 8).
    state.open_selected_thread_actions();
    for _ in 0..8 {
        state.move_thread_action_down();
    }
    assert!(matches!(
        state.activate_selected_thread_action(),
        Some(AppCommand::SetThreadPinned { channel_id, pinned: true, .. })
            if channel_id == Id::new(31)
    ));
}

/// Open the action menu and activate "Delete post" (tenth row), leaving the
/// delete confirmation gate open. Asserts the gate replaced the menu and that
/// nothing was emitted yet.
fn open_forum_post_delete_gate(state: &mut DashboardState) {
    state.open_selected_thread_actions();
    for _ in 0..9 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert!(!state.is_thread_action_menu_active());
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation));
}

#[test]
fn forum_post_delete_uses_confirmation_before_emitting() {
    let mut state = manageable_thread_action_menu_state();

    // Cancelling the gate clears it without deleting.
    open_forum_post_delete_gate(&mut state);
    state.close_thread_delete_confirmation();
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation));

    // Confirming issues the delete and closes the gate.
    open_forum_post_delete_gate(&mut state);
    assert!(matches!(
        state.confirm_thread_delete(),
        Some(AppCommand::DeleteThread { channel_id, .. }) if channel_id == Id::new(31)
    ));
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation));
}

#[test]
fn forum_post_delete_requires_manage_permission_not_authorship() {
    // The post author (thread owner) without manage permission may close their
    // own post, but Delete (which removes the whole thread) is moderator-only;
    // Discord returns 403 otherwise.
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: Some(Id::new(88)), // the guild is owned by someone else
        channels: vec![forum_channel_info(guild_id, forum_id)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    // The current user authored the post but holds no manage permission.
    let mut thread = forum_thread_info(guild_id, forum_id, 31, "mine", Some(301), false);
    thread.owner_id = Some(Id::new(10));
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);

    state.open_selected_thread_actions();
    let items = state.selected_thread_action_items();
    let enabled = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.enabled)
            .unwrap_or(false)
    };
    // The author can close their own post...
    assert!(enabled(ThreadActionKind::Close));
    // ...but cannot delete it, nor use the moderator-only actions.
    assert!(!enabled(ThreadActionKind::Delete));
    assert!(!enabled(ThreadActionKind::Lock));
}

#[test]
fn forum_post_management_labels_reflect_thread_state() {
    let mut state = manageable_thread_action_menu_state();
    // Re-upsert the thread as archived + locked + pinned (flags PINNED = 1 << 1).
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(true, true)),
        flags: Some(1 << 1),
        ..forum_thread_info(
            Id::new(1),
            Id::new(20),
            31,
            "release notes",
            Some(301),
            true,
        )
    }));
    state.open_selected_thread_actions();

    let items = state.selected_thread_action_items();
    let label = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.label.clone())
            .expect("action should exist")
    };
    assert_eq!(label(ThreadActionKind::Close), "Reopen post");
    assert_eq!(label(ThreadActionKind::Lock), "Unlock post");
    assert_eq!(label(ThreadActionKind::Pin), "Unpin post");
}

#[test]
fn thread_edit_action_opens_popup_seeded_from_post() {
    let mut state = manageable_thread_action_menu_state();
    state.open_selected_thread_actions();
    // "Edit post" is the fifth row (offset 4) and enabled for the manager.
    for _ in 0..4 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ThreadEdit));

    let view = state
        .thread_edit_view()
        .expect("edit popup should expose a view");
    assert_eq!(view.title, "release notes");
    // The guild owner has manage permission, so slow mode is editable.
    assert!(view.can_set_slow_mode);
}

#[test]
fn thread_edit_submit_emits_edit_command_with_edited_title() {
    let mut state = manageable_thread_action_menu_state();
    state.open_thread_edit(Id::new(31));

    // Edit the title inline: Enter to edit, clear, type, Enter to commit.
    assert_eq!(state.activate_thread_edit(), None);
    state.clear_thread_edit_active_field();
    for ch in "renamed".chars() {
        state.push_thread_edit_char(ch);
    }
    assert_eq!(state.activate_thread_edit(), None);

    // Move to the submit cell and activate it.
    state.cycle_thread_edit_field_next(); // Tags
    state.cycle_thread_edit_field_next(); // SlowMode
    state.cycle_thread_edit_field_next(); // AutoArchive
    state.cycle_thread_edit_field_next(); // Submit
    let command = state.activate_thread_edit();

    assert!(matches!(
        command,
        Some(AppCommand::EditThread { channel_id, ref name, .. })
            if channel_id == Id::new(31) && name == "renamed"
    ));
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::ThreadEdit));
}

#[test]
fn thread_edit_slow_mode_is_locked_without_manage_permission() {
    // The default forum fixture has no current user with manage permission, so
    // the slow-mode selector is read-only and cycling it does nothing.
    let mut state = thread_action_menu_state();
    state.open_thread_edit(Id::new(31));

    let before = state
        .thread_edit_view()
        .expect("edit popup should expose a view");
    assert!(!before.can_set_slow_mode);

    // Focus the slow-mode selector and try to cycle it.
    state.cycle_thread_edit_field_next(); // Tags
    state.cycle_thread_edit_field_next(); // SlowMode
    state.cycle_thread_edit_selector(true);

    let after = state
        .thread_edit_view()
        .expect("edit popup should expose a view");
    assert_eq!(before.slow_mode_label, after.slow_mode_label);
}

#[test]
fn thread_edit_tag_picker_lists_parent_forum_tags() {
    // Available tags live on the parent forum, while the post (thread) only
    // carries its applied tags. The editor must resolve the picker from the
    // parent, otherwise it wrongly reports "no tags available".
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
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: Some(Id::new(10)),
        channels: vec![forum],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    let mut thread = forum_thread_info(guild_id, forum_id, 31, "release notes", Some(301), false);
    thread.applied_tags = vec![Id::new(102)];
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);

    state.open_thread_edit(Id::new(31));
    state.cycle_thread_edit_field_next(); // Title -> Tags
    // Opening the picker must succeed (not fall back to "no tags available").
    assert_eq!(state.activate_thread_edit(), None);
    assert!(state.is_thread_edit_tag_picker_active());

    let view = state
        .thread_edit_view()
        .expect("edit popup should expose a view");
    let names: Vec<&str> = view.tags.iter().map(|tag| tag.name.as_str()).collect();
    assert!(names.contains(&"rust") && names.contains(&"question"));
    // The post's currently applied tag is preselected.
    assert!(
        view.tags
            .iter()
            .any(|tag| tag.name == "rust" && tag.selected)
    );
}

#[test]
fn thread_edit_tag_picker_keeps_custom_emoji_tags() {
    // Regression: a custom-emoji tag has `emoji_id` set and `emoji_name` null;
    // it must surface a CDN url plus a `:name:` fallback, not be dropped.
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();
    let mut forum = forum_channel_info(guild_id, forum_id);
    forum.available_tags = vec![
        ForumTagInfo {
            id: Id::new(101),
            name: "sparkles".to_owned(),
            moderated: false,
            emoji_id: Some(Id::new(77)),
            emoji_name: None,
        },
        ForumTagInfo {
            id: Id::new(102),
            name: "fire".to_owned(),
            moderated: false,
            emoji_id: None,
            emoji_name: Some("🔥".to_owned()),
        },
    ];
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        owner_id: Some(Id::new(10)),
        channels: vec![forum],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        // The tag payload omits the custom emoji name, so it is resolved from
        // the guild emoji cache by `emoji_id`.
        emojis: vec![CustomEmojiInfo::test(Id::new(77), "sparkle")],
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    let mut thread = forum_thread_info(guild_id, forum_id, 31, "release notes", Some(301), false);
    thread.applied_tags = vec![Id::new(101)];
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 1,
        threads: vec![thread],
        first_messages: Vec::new(),
        has_more: false,
    });
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.set_message_view_height(10);
    state.focus_pane(FocusPane::Messages);

    state.open_thread_edit(Id::new(31));
    let view = state
        .thread_edit_view()
        .expect("edit popup should expose a view");

    let custom = view
        .tags
        .iter()
        .find(|tag| tag.name == "sparkles")
        .expect("custom-emoji tag should not be dropped");
    assert_eq!(custom.unicode_emoji, None);
    assert_eq!(
        custom.custom_emoji_url.as_deref(),
        Some("https://cdn.discordapp.com/emojis/77.png")
    );
    assert_eq!(custom.custom_emoji_label.as_deref(), Some(":sparkle:"));

    let unicode = view
        .tags
        .iter()
        .find(|tag| tag.name == "fire")
        .expect("unicode-emoji tag should be present");
    assert_eq!(unicode.unicode_emoji.as_deref(), Some("🔥"));
    assert_eq!(unicode.custom_emoji_url, None);
}

#[test]
fn thread_notification_settings_disabled_when_not_followed() {
    // The default fixture is not followed, so "Notification settings" must be
    // disabled (it targets the thread-member settings endpoint, which requires
    // membership).
    let mut state = thread_action_menu_state();
    state.open_selected_thread_actions();

    let items = state.selected_thread_action_items();
    let notif = items
        .iter()
        .find(|item| item.kind == ThreadActionKind::NotificationSettings)
        .expect("notification settings action should exist");
    assert!(!notif.enabled);

    // After following, the item becomes enabled.
    state.close_thread_action_menu();
    follow_selected_forum_post(&mut state);
    state.open_selected_thread_actions();

    let items = state.selected_thread_action_items();
    let notif = items
        .iter()
        .find(|item| item.kind == ThreadActionKind::NotificationSettings)
        .expect("notification settings action should exist");
    assert!(notif.enabled);
}

#[test]
fn thread_notification_settings_submenu_emits_command() {
    // Activate "Notification settings" (row 7) on a followed post. The submenu
    // shows 3 radio rows; selecting "Nothing" (index 2) emits the command.
    let mut state = thread_action_menu_state();
    follow_selected_forum_post(&mut state);
    state.open_selected_thread_actions();

    // Navigate to "Notification settings" (row index 7).
    for _ in 0..7 {
        state.move_thread_action_down();
    }
    // Activating enters the submenu, returning None.
    assert_eq!(state.activate_selected_thread_action(), None);
    assert!(state.is_thread_action_notification_phase());

    // Pick "Nothing" (third row, index 2).
    state.move_thread_action_down();
    state.move_thread_action_down();
    let command = state.activate_selected_thread_action();
    assert!(matches!(
        command,
        Some(AppCommand::SetThreadNotificationLevel {
            channel_id,
            flags: 8,
            ..
        }) if channel_id == Id::new(31)
    ));
    assert!(!state.is_thread_action_menu_active());
}

#[test]
fn thread_notification_settings_marks_current_level_after_update() {
    // After a ThreadNotificationLevelUpdate event, the row matching the new
    // flags should have the [x] prefix and others should have [ ].
    let mut state = thread_action_menu_state();
    follow_selected_forum_post(&mut state);

    // Simulate the optimistic update setting flags to 2 (All messages).
    state.push_event(AppEvent::ThreadNotificationLevelUpdate {
        channel_id: Id::new(31),
        flags: 2,
    });

    state.open_selected_thread_actions();
    for _ in 0..7 {
        state.move_thread_action_down();
    }
    assert_eq!(state.activate_selected_thread_action(), None);
    assert!(state.is_thread_action_notification_phase());

    let items = state.selected_thread_notification_items();
    assert_eq!(items.len(), 3);
    assert!(
        items[0].label.starts_with("[x]"),
        "All messages should be marked current"
    );
    assert!(
        items[1].label.starts_with("[ ]"),
        "Only @mentions should not be marked"
    );
    assert!(
        items[2].label.starts_with("[ ]"),
        "Nothing should not be marked"
    );
}

#[test]
fn channel_pane_regular_thread_opens_thread_actions_without_pin() {
    let guild_id = Id::new(1);
    // `general` (id 11) is a normal text channel in the standard channel tree,
    // so a thread under it is a regular thread, not a forum post.
    let parent_id = Id::new(11);
    let thread_id = Id::new(30);
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        current_user_joined_thread: Some(true),
        ..thread_channel_info(guild_id, parent_id, thread_id, "design chat")
    }));

    state.focus_pane(FocusPane::Channels);
    // Walk down to the nested joined thread under `general`.
    for _ in 0..10 {
        let selected = state
            .channel_pane_entries()
            .get(state.selected_channel())
            .and_then(|entry| entry.channel_id());
        if selected == Some(thread_id) {
            break;
        }
        state.move_down();
    }
    assert_eq!(
        state
            .channel_pane_entries()
            .get(state.selected_channel())
            .and_then(|entry| entry.channel_id()),
        Some(thread_id),
    );

    // The shared action trigger opens the thread action menu, not the channel
    // menu.
    state.open_leader_actions_for_focused_target();
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu));
    assert!(!state.is_channel_leader_action_active());

    let items = state.selected_thread_action_items();
    let label = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.label.clone())
    };
    // Labels read "thread", not "post".
    assert_eq!(
        label(ThreadActionKind::Close),
        Some("Close thread".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::Lock),
        Some("Lock thread".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::Edit),
        Some("Edit thread".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::Delete),
        Some("Delete thread".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::ToggleMute),
        Some("Mute thread".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::ToggleFollow),
        Some("Unfollow thread".to_owned())
    );
    // Pin is forum-only, so it never appears for a regular thread.
    assert!(label(ThreadActionKind::Pin).is_none());
    // The remaining management/notification rows are still present.
    assert!(label(ThreadActionKind::NotificationSettings).is_some());
}

#[test]
fn thread_action_menu_keeps_pin_and_post_labels() {
    let mut state = manageable_thread_action_menu_state();
    state.open_selected_thread_actions();

    let items = state.selected_thread_action_items();
    let label = |kind: ThreadActionKind| {
        items
            .iter()
            .find(|item| item.kind == kind)
            .map(|item| item.label.clone())
    };
    // Forum posts keep the "post" noun and the forum-only Pin row.
    assert_eq!(
        label(ThreadActionKind::Close),
        Some("Close post".to_owned())
    );
    assert_eq!(
        label(ThreadActionKind::ToggleMute),
        Some("Mute post".to_owned())
    );
    assert_eq!(label(ThreadActionKind::Pin), Some("Pin post".to_owned()));
    assert_eq!(
        label(ThreadActionKind::Delete),
        Some("Delete post".to_owned())
    );
}
