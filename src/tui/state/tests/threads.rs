use super::*;
use crate::discord::{
    ArchivedThreadPageCursor, ArchivedThreadsPage, ForumPostDataInfo, ThreadGatewayInfo,
    ThreadListSyncInfo, ThreadMemberInfo, ThreadMemberListUpdateInfo,
};

fn forum_channel(guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        name: "forum".to_owned(),
        kind: "forum".to_owned(),
        ..ChannelInfo::test(channel_id, "forum")
    }
}

fn active_thread(
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
    thread_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        parent_id: Some(parent_id),
        name: name.into(),
        kind: "GuildPublicThread".to_owned(),
        last_message_id: Some(Id::new(thread_id.get() + 10_000)),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(thread_id, "GuildPublicThread")
    }
}

fn archived_thread(
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
    thread_id: Id<ChannelMarker>,
    name: impl Into<String>,
    archive_timestamp: &str,
) -> ChannelInfo {
    let mut thread = active_thread(guild_id, parent_id, thread_id, name);
    let metadata = thread
        .thread_metadata
        .as_mut()
        .expect("test thread should have metadata");
    metadata.archived = true;
    metadata.archive_timestamp = Some(archive_timestamp.to_owned());
    thread
}

fn current_user_thread_member(
    thread_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
) -> ThreadMemberInfo {
    ThreadMemberInfo {
        thread_id: Some(thread_id),
        user_id: Some(user_id),
        join_timestamp: Some("2026-08-14T00:00:00.000Z".to_owned()),
        flags: Some(0),
        muted: Some(false),
        mute_end_time: None,
        selected_time_window: None,
        member: None,
        presence: None,
        extra_fields: BTreeMap::new(),
    }
}

fn sidebar_thread_ids(state: &DashboardState) -> Vec<Id<ChannelMarker>> {
    state
        .channel_pane_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            ChannelPaneEntry::Thread { state, .. } => Some(state.id),
            _ => None,
        })
        .collect()
}

fn snowflake_for_unix_millis(unix_millis: u64) -> u64 {
    const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
    unix_millis.saturating_sub(DISCORD_EPOCH_MILLIS) << 22
}

#[test]
fn channel_tree_shows_only_joined_active_forum_posts_and_text_threads() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let text_id = Id::new(30);
    let joined_forum_ids = [Id::new(100), Id::new(101)];
    let joined_text_thread_id = Id::new(200);
    let current_user_id = Id::new(9);
    let mut state = DashboardState::new();

    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![
                forum_channel(guild_id, forum_id),
                text_channel_info(guild_id, text_id, "general"),
                active_thread(guild_id, forum_id, joined_forum_ids[0], "joined post 1"),
                active_thread(guild_id, forum_id, joined_forum_ids[1], "joined post 2"),
                active_thread(guild_id, text_id, joined_text_thread_id, "joined thread"),
            ],
            current_user_thread_members: vec![
                current_user_thread_member(joined_forum_ids[0], current_user_id),
                current_user_thread_member(joined_forum_ids[1], current_user_id),
                current_user_thread_member(joined_text_thread_id, current_user_id),
            ],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();

    assert_eq!(
        sidebar_thread_ids(&state),
        vec![
            joined_forum_ids[1],
            joined_forum_ids[0],
            joined_text_thread_id,
        ]
    );

    let all_forum_threads = (100..107)
        .map(|id| active_thread(guild_id, forum_id, Id::new(id), format!("post {id}")))
        .collect::<Vec<_>>();
    state.push_event(AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: Some(vec![forum_id]),
            threads: all_forum_threads,
            current_user_members: Some(
                joined_forum_ids
                    .into_iter()
                    .map(|thread_id| current_user_thread_member(thread_id, current_user_id))
                    .collect(),
            ),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        sidebar_thread_ids(&state),
        vec![
            joined_forum_ids[1],
            joined_forum_ids[0],
            joined_text_thread_id,
        ]
    );

    state.activate_channel(forum_id);
    assert_eq!(state.selected_forum_post_items().len(), 7);
}

#[test]
fn channel_tree_hides_inactive_joined_threads_but_keeps_pinned_posts() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let current_user_id = Id::new(9);
    let now_millis: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_millis()
        .try_into()
        .expect("current timestamp should fit in u64");
    let recent_id = Id::new(snowflake_for_unix_millis(
        now_millis.saturating_sub(60 * 60 * 1_000),
    ));
    let stale_id = Id::new(snowflake_for_unix_millis(
        now_millis.saturating_sub(4 * 24 * 60 * 60 * 1_000),
    ));
    let pinned_id = Id::new(stale_id.get().saturating_add(1));

    let mut recent = active_thread(guild_id, forum_id, recent_id, "recent");
    let mut stale = active_thread(guild_id, forum_id, stale_id, "stale");
    let mut pinned = active_thread(guild_id, forum_id, pinned_id, "pinned");
    for thread in [&mut recent, &mut stale, &mut pinned] {
        thread.last_message_id = Some(Id::new(thread.channel_id.get()));
        thread
            .thread_metadata
            .as_mut()
            .expect("test thread has metadata")
            .auto_archive_duration = Some(4_320);
    }
    pinned.flags = Some(1 << 1);

    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![forum_channel(guild_id, forum_id), recent, stale, pinned],
            current_user_thread_members: [recent_id, stale_id, pinned_id]
                .into_iter()
                .map(|thread_id| current_user_thread_member(thread_id, current_user_id))
                .collect(),
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();

    let sidebar_ids = sidebar_thread_ids(&state);
    assert!(sidebar_ids.contains(&recent_id));
    assert!(sidebar_ids.contains(&pinned_id));
    assert!(!sidebar_ids.contains(&stale_id));
}

#[test]
fn inactive_sync_and_flags_remove_threads_without_promoting_unjoined_posts() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(100);
    let current_user_id = Id::new(9);
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![
                forum_channel(guild_id, forum_id),
                active_thread(guild_id, forum_id, thread_id, "post"),
            ],
            current_user_thread_members: vec![current_user_thread_member(
                thread_id,
                current_user_id,
            )],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);
    assert_eq!(sidebar_thread_ids(&state), vec![thread_id]);
    assert_eq!(state.selected_forum_post_items().len(), 1);

    state.push_event(AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: Some(vec![forum_id]),
            threads: Vec::new(),
            current_user_members: Some(Vec::new()),
            extra_fields: BTreeMap::new(),
        },
    });
    assert!(sidebar_thread_ids(&state).is_empty());
    assert!(state.selected_forum_post_items().is_empty());

    state.push_event(AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: Some(vec![forum_id]),
            threads: vec![active_thread(guild_id, forum_id, thread_id, "post")],
            current_user_members: Some(Vec::new()),
            extra_fields: BTreeMap::new(),
        },
    });
    assert!(sidebar_thread_ids(&state).is_empty());
    assert_eq!(state.selected_forum_post_items().len(), 1);

    let mut removed = active_thread(guild_id, forum_id, thread_id, "post");
    removed.flags = Some(1 << 2);
    state.push_event(AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: removed,
            current_user_member: None,
        },
        created: false,
    });
    assert!(sidebar_thread_ids(&state).is_empty());
    assert!(state.selected_forum_post_items().is_empty());
}

#[test]
fn post_data_hydrates_active_cards_without_changing_joined_sidebar_state() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let joined_id = Id::new(100);
    let unjoined_ids = [Id::new(101), Id::new(102)];
    let current_user_id = Id::new(9);
    let owner_id = Id::new(50);
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![
                forum_channel(guild_id, forum_id),
                active_thread(guild_id, forum_id, joined_id, "joined"),
                active_thread(guild_id, forum_id, unjoined_ids[0], "unjoined 1"),
                active_thread(guild_id, forum_id, unjoined_ids[1], "unjoined 2"),
            ],
            current_user_thread_members: vec![current_user_thread_member(
                joined_id,
                current_user_id,
            )],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);
    state.set_message_view_height(20);

    assert_eq!(sidebar_thread_ids(&state), vec![joined_id]);
    assert_eq!(
        state.selected_forum_post_data_target(),
        Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id: forum_id,
            thread_ids: vec![unjoined_ids[1], unjoined_ids[0], joined_id],
        })
    );

    let starter = MessageInfo {
        guild_id: Some(guild_id),
        author_id: owner_id,
        author: "Alice".to_owned(),
        content: Some("starter body".to_owned()),
        ..MessageInfo::test(unjoined_ids[0], Id::new(unjoined_ids[0].get()))
    };
    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: vec![unjoined_ids[1], unjoined_ids[0]],
        posts: vec![ForumPostDataInfo {
            thread_id: unjoined_ids[0],
            owner: Some(member_info(owner_id, "Alice")),
            first_message: Some(starter),
            extra_fields: BTreeMap::new(),
        }],
    });

    assert_eq!(sidebar_thread_ids(&state), vec![joined_id]);
    assert_eq!(
        state.selected_forum_post_data_target(),
        Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id: forum_id,
            thread_ids: vec![joined_id],
        })
    );
    let hydrated = state
        .selected_forum_post_items()
        .into_iter()
        .find(|post| post.channel_id == unjoined_ids[0])
        .expect("the hydrated active post should remain visible");
    assert_eq!(hydrated.preview_author.as_deref(), Some("Alice"));
    assert_eq!(hydrated.preview_content.as_deref(), Some("starter body"));
}

#[test]
fn forum_post_data_target_advances_with_the_visible_viewport() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_ids = (100..108).map(Id::new).collect::<Vec<_>>();
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: std::iter::once(forum_channel(guild_id, forum_id))
                .chain(thread_ids.iter().map(|thread_id| {
                    active_thread(
                        guild_id,
                        forum_id,
                        *thread_id,
                        format!("post {}", thread_id.get()),
                    )
                }))
                .collect(),
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);
    state.set_message_view_height(13);

    let first_visible_ids = vec![thread_ids[7], thread_ids[6]];
    assert_eq!(
        state.selected_forum_post_data_target(),
        Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id: forum_id,
            thread_ids: first_visible_ids.clone(),
        })
    );

    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: first_visible_ids,
        posts: Vec::new(),
    });
    state.messages.selected_message = 2;
    state.messages.message_scroll = 2;

    assert_eq!(
        state.selected_forum_post_data_target(),
        Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id: forum_id,
            thread_ids: vec![thread_ids[5], thread_ids[4]],
        })
    );
}

#[test]
fn forum_starter_preview_distinguishes_loading_from_deleted() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(100);
    let owner_id = Id::new(50);
    let mut thread = active_thread(guild_id, forum_id, thread_id, "post");
    thread.owner_id = Some(owner_id);
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![forum_channel(guild_id, forum_id), thread],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);

    let loading = state
        .selected_forum_post_items()
        .into_iter()
        .find(|post| post.channel_id == thread_id)
        .expect("forum post should be visible while its starter loads");
    assert!(loading.preview_loading);
    assert_eq!(loading.preview_author.as_deref(), Some("user-50"));
    assert_eq!(loading.preview_content, None);

    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: vec![thread_id],
        posts: vec![ForumPostDataInfo {
            thread_id,
            owner: None,
            first_message: None,
            extra_fields: BTreeMap::new(),
        }],
    });

    let deleted = state
        .selected_forum_post_items()
        .into_iter()
        .find(|post| post.channel_id == thread_id)
        .expect("forum post should remain visible after post data loads");
    assert!(!deleted.preview_loading);
    assert_eq!(
        deleted.preview_content.as_deref(),
        Some("original message deleted")
    );
}

#[test]
fn forum_body_appends_archived_posts_without_adding_sidebar_rows() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let active_id = Id::new(100);
    let archived_ids = [Id::new(201), Id::new(200)];
    let current_user_id = Id::new(9);
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![
                forum_channel(guild_id, forum_id),
                active_thread(guild_id, forum_id, active_id, "active post"),
            ],
            current_user_thread_members: vec![current_user_thread_member(
                active_id,
                current_user_id,
            )],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);

    assert!(state.selected_forum_posts_loading());
    assert_eq!(
        state.selected_archived_thread_request_target(),
        Some(crate::discord::ArchivedThreadRequestTarget {
            guild_id,
            channel_id: forum_id,
            cursor: ArchivedThreadPageCursor::Initial,
        })
    );

    state.push_event(AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: None,
        page: ArchivedThreadsPage {
            threads: vec![
                archived_thread(
                    guild_id,
                    forum_id,
                    archived_ids[0],
                    "newer archived post",
                    "2026-08-14T02:00:00.000000+00:00",
                ),
                archived_thread(
                    guild_id,
                    forum_id,
                    archived_ids[1],
                    "older archived post",
                    "2026-08-14T01:00:00.000000+00:00",
                ),
            ],
            members: vec![current_user_thread_member(archived_ids[0], current_user_id)],
            has_more: false,
            next_before: Some("2026-08-14T01:00:00.000000+00:00".to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });

    assert!(!state.selected_forum_posts_loading());
    let items = state.selected_forum_post_items();
    assert_eq!(
        items.iter().map(|item| item.channel_id).collect::<Vec<_>>(),
        vec![active_id, archived_ids[0], archived_ids[1]]
    );
    assert_eq!(items[0].section_label.as_deref(), Some("Active posts"));
    assert_eq!(items[1].section_label.as_deref(), Some("Archived posts"));
    assert!(!items[0].archived);
    assert!(items[1].archived);
    assert!(items[2].archived);
    assert_eq!(sidebar_thread_ids(&state), vec![active_id]);
    assert!(state.discord.cache.thread_is_joined(archived_ids[0]));
    assert!(
        !state
            .discord
            .cache
            .thread_is_sidebar_active(archived_ids[0])
    );
    assert_eq!(
        state.selected_forum_post_data_target(),
        Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id: forum_id,
            thread_ids: vec![active_id],
        })
    );

    state.messages.selected_message = 1;
    assert_eq!(
        state.activate_selected_thread_card(),
        Some(AppCommand::SubscribeGuildChannel {
            guild_id,
            channel_id: archived_ids[0],
        })
    );
    assert_eq!(
        state.selected_channel_state().map(|channel| channel.id),
        Some(archived_ids[0])
    );
}

#[test]
fn forum_archive_pagination_uses_the_last_archive_timestamp_near_the_end() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let first_before = "2026-08-14T01:00:00.000000+00:00";
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![forum_channel(guild_id, forum_id)],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(forum_id);
    state.push_event(AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: None,
        page: ArchivedThreadsPage {
            threads: vec![archived_thread(
                guild_id,
                forum_id,
                Id::new(200),
                "archived",
                first_before,
            )],
            members: Vec::new(),
            has_more: true,
            next_before: Some(first_before.to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        state.selected_archived_thread_request_target(),
        Some(crate::discord::ArchivedThreadRequestTarget {
            guild_id,
            channel_id: forum_id,
            cursor: ArchivedThreadPageCursor::Before(first_before.to_owned()),
        })
    );

    state.push_event(AppEvent::ArchivedThreadsLoaded {
        guild_id,
        channel_id: forum_id,
        before: Some(first_before.to_owned()),
        page: ArchivedThreadsPage {
            threads: vec![archived_thread(
                guild_id,
                forum_id,
                Id::new(199),
                "older archived",
                "2026-08-14T00:00:00.000000+00:00",
            )],
            members: Vec::new(),
            has_more: false,
            next_before: Some("2026-08-14T00:00:00.000000+00:00".to_owned()),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(state.selected_archived_thread_request_target(), None);
}

#[test]
fn selected_thread_member_snapshot_drives_member_pane_without_joining_thread() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(100);
    let alice_id = Id::new(50);
    let bob_id = Id::new(51);
    let mut state = DashboardState::new();
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            channels: vec![
                forum_channel(guild_id, forum_id),
                active_thread(guild_id, forum_id, thread_id, "post"),
            ],
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.confirm_selected_guild();
    state.activate_channel(thread_id);

    assert_eq!(
        state.thread_member_list_subscription_target(),
        Some((guild_id, thread_id))
    );
    assert!(state.is_member_list_loading());

    state.push_event(AppEvent::ThreadMemberListUpdate {
        update: ThreadMemberListUpdateInfo {
            guild_id,
            channel_id: thread_id,
            members: vec![
                ThreadMemberInfo {
                    member: Some(member_info(alice_id, "Alice")),
                    presence: Some(crate::discord::PresenceEventFields {
                        user_id: alice_id,
                        status: PresenceStatus::Online,
                        activities: Vec::new(),
                    }),
                    ..current_user_thread_member(thread_id, alice_id)
                },
                ThreadMemberInfo {
                    member: Some(member_info(bob_id, "Bob")),
                    ..current_user_thread_member(thread_id, bob_id)
                },
            ],
            extra_fields: BTreeMap::new(),
        },
    });

    assert!(!state.is_member_list_loading());
    let mut member_ids = state
        .members_grouped()
        .into_iter()
        .flat_map(|group| group.entries)
        .map(|member| member.user_id())
        .collect::<Vec<_>>();
    member_ids.sort_unstable();
    assert_eq!(member_ids, vec![alice_id, bob_id]);
    assert!(sidebar_thread_ids(&state).is_empty());
    assert!(!state.discord.cache.thread_is_joined(thread_id));
}
