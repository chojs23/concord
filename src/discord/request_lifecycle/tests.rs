use crate::discord::ids::Id;

use crate::discord::{
    AppEvent, ChannelInfo, ForumPostArchiveState, MemberInfo, MessageHistoryAfterMode,
    MessageHistoryLoadTarget, UserProfileInfo,
};

use crate::discord::test_builders::{
    ChannelPinsUpdateFixture, ForumPostsLoadFailedFixture, ForumPostsLoadedFixture,
    MessageHistoryAfterLoadedFixture, MessageHistoryLoadFailedFixture, MessageHistoryLoadedFixture,
    UserProfileLoadFailedFixture, channel_pins_update_event, forum_posts_load_failed_event,
    forum_posts_loaded_event, message_history_after_loaded_event,
    message_history_load_failed_event, message_history_loaded_event,
    user_profile_load_failed_event,
};

use super::{
    ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberBatchRequests,
    MemberListSubscriptionRequests, MemberListSubscriptionTarget, MemberRequests,
    MentionMemberSearchRequests, MentionMemberSearchTarget, PinnedMessageRequests,
    RequestLifecycle, ThreadPreviewRequests, UserNoteRequests, UserProfileRequests,
};

#[test]
fn history_request_is_sent_once_and_retries_failed_channel_after_reselect() {
    let mut requests = HistoryRequests::default();
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(requests.next(None, false), None);
    assert_eq!(requests.next(Some(first), false), Some(first));
    assert_eq!(requests.next(Some(first), false), None);
    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id: first,
            target: MessageHistoryLoadTarget::Latest,
            message: "temporary failure".to_owned(),
        },
    ));
    assert_eq!(requests.next(Some(first), false), None);
    assert_eq!(requests.next(Some(second), false), Some(second));
    assert_eq!(requests.next(Some(first), false), Some(first));

    let mut requests = HistoryRequests::default();
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(requests.next(Some(first), false), Some(first));
    requests.record_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id: first,
        ..MessageHistoryLoadedFixture::new()
    }));
    assert_eq!(requests.next(Some(first), true), None);
    assert_eq!(requests.next(Some(second), false), Some(second));
    assert_eq!(requests.next(Some(first), true), Some(first));
}

#[test]
fn pinned_message_request_is_on_demand_and_retries_failed_channel_after_reselect() {
    let mut requests = PinnedMessageRequests::default();
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(requests.next(None), None);
    assert_eq!(requests.next(Some(first)), Some(first));
    assert_eq!(requests.next(Some(first)), None);
    requests.record_event(&AppEvent::PinnedMessagesLoaded {
        channel_id: first,
        messages: Vec::new(),
    });
    assert_eq!(requests.next(Some(first)), None);
    assert_eq!(requests.next(Some(second)), Some(second));
    assert_eq!(requests.next(Some(first)), None);

    let mut requests = PinnedMessageRequests::default();
    assert_eq!(requests.next(Some(first)), Some(first));
    requests.record_event(&AppEvent::PinnedMessagesLoadFailed {
        channel_id: first,
        message: "temporary failure".to_owned(),
    });
    assert_eq!(requests.next(Some(first)), None);
    assert_eq!(requests.next(Some(second)), Some(second));
    assert_eq!(requests.next(Some(first)), Some(first));
}

#[test]
fn pinned_message_request_reloads_after_channel_pins_update() {
    let mut requests = PinnedMessageRequests::default();
    let channel_id = Id::new(1);

    assert_eq!(requests.next(Some(channel_id)), Some(channel_id));
    requests.record_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: Vec::new(),
    });
    assert_eq!(requests.next(Some(channel_id)), None);

    requests.record_event(&channel_pins_update_event(ChannelPinsUpdateFixture {
        channel_id,
        ..ChannelPinsUpdateFixture::new()
    }));

    assert_eq!(requests.next(Some(channel_id)), Some(channel_id));
}

#[test]
fn forum_post_request_is_sent_once_per_channel() {
    let mut requests = ForumPostRequests::default();
    let guild = Id::new(100);
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(requests.next(None), None);
    assert_eq!(
        requests.next(Some(target(guild, first, false))),
        Some((guild, first, ForumPostArchiveState::Active, 0))
    );
    assert_eq!(requests.next(Some(target(guild, first, false))), None);
    assert_eq!(
        requests.next(Some(target(guild, second, false))),
        Some((guild, second, ForumPostArchiveState::Active, 0))
    );
}

#[test]
fn forum_post_request_retries_failed_channel_after_reselect() {
    let mut requests = ForumPostRequests::default();
    let guild = Id::new(100);
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(
        requests.next(Some(target(guild, first, false))),
        Some((guild, first, ForumPostArchiveState::Active, 0))
    );
    requests.record_event(&forum_posts_load_failed_event(
        ForumPostsLoadFailedFixture {
            channel_id: first,
            archive_state: ForumPostArchiveState::Active,
            message: "temporary failure".to_owned(),
            ..ForumPostsLoadFailedFixture::new()
        },
    ));
    assert_eq!(requests.next(Some(target(guild, first, false))), None);
    assert_eq!(
        requests.next(Some(target(guild, second, false))),
        Some((guild, second, ForumPostArchiveState::Active, 0))
    );
    assert_eq!(
        requests.next(Some(target(guild, first, false))),
        Some((guild, first, ForumPostArchiveState::Active, 0))
    );
}

#[test]
fn forum_post_request_tracks_active_archived_and_server_offsets() {
    let mut requests = ForumPostRequests::default();
    let guild = Id::new(100);
    let channel = Id::new(1);

    assert_eq!(
        requests.next(Some(target(guild, channel, false))),
        Some((guild, channel, ForumPostArchiveState::Active, 0))
    );
    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Active,
        next_offset: 2,
        threads: vec![forum_post(channel, 10), forum_post(channel, 11)],
        has_more: true,
        ..ForumPostsLoadedFixture::new()
    }));

    assert_eq!(requests.next(Some(target(guild, channel, false))), None);
    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Active, 2))
    );
    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Active,
        offset: 2,
        next_offset: 3,
        threads: vec![forum_post(channel, 12)],
        ..ForumPostsLoadedFixture::new()
    }));

    assert_eq!(requests.next(Some(target(guild, channel, false))), None);
    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Archived, 0))
    );
    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Archived,
        next_offset: 2,
        threads: vec![forum_post(channel, 11), forum_post(channel, 12)],
        has_more: true,
        ..ForumPostsLoadedFixture::new()
    }));

    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Archived, 2))
    );

    let mut requests = ForumPostRequests::default();
    let channel = Id::new(2);

    assert_eq!(
        requests.next(Some(target(guild, channel, false))),
        Some((guild, channel, ForumPostArchiveState::Active, 0))
    );
    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Active,
        next_offset: 25,
        threads: vec![forum_post(channel, 10), forum_post(channel, 11)],
        has_more: true,
        ..ForumPostsLoadedFixture::new()
    }));

    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Active, 25))
    );
}

#[test]
fn archived_forum_posts_wait_for_the_active_search_to_drain() {
    let mut requests = ForumPostRequests::default();
    let guild = Id::new(100);
    let channel = Id::new(1);

    assert_eq!(
        requests.next(Some(target(guild, channel, false))),
        Some((guild, channel, ForumPostArchiveState::Active, 0))
    );
    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Active,
        next_offset: 25,
        threads: vec![forum_post(channel, 10)],
        has_more: true,
        ..ForumPostsLoadedFixture::new()
    }));

    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Active, 25))
    );
    assert_eq!(requests.next(Some(target(guild, channel, true))), None);

    requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
        channel_id: channel,
        archive_state: ForumPostArchiveState::Active,
        offset: 25,
        next_offset: 26,
        threads: vec![forum_post(channel, 11)],
        ..ForumPostsLoadedFixture::new()
    }));
    assert_eq!(
        requests.next(Some(target(guild, channel, true))),
        Some((guild, channel, ForumPostArchiveState::Archived, 0))
    );
}

fn target(
    guild_id: Id<crate::discord::ids::marker::GuildMarker>,
    channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
    should_load_more: bool,
) -> ForumPostRequestTarget {
    ForumPostRequestTarget {
        guild_id,
        channel_id,
        should_load_more,
    }
}

fn forum_post(
    forum_id: Id<crate::discord::ids::marker::ChannelMarker>,
    channel_id: u64,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(Id::new(100)),
        parent_id: Some(forum_id),
        name: format!("post {channel_id}"),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(Id::new(channel_id), "GuildPublicThread")
    }
}

fn subscription_target(bucket: u32) -> MemberListSubscriptionTarget {
    let ranges = if bucket == 0 {
        vec![(0, 99)]
    } else {
        vec![(0, 99), (bucket * 100, bucket * 100 + 99)]
    };
    MemberListSubscriptionTarget {
        guild_id: Id::new(1),
        channel_id: Id::new(2),
        bucket,
        ranges,
    }
}

fn user_profile(user_id: Id<crate::discord::ids::marker::UserMarker>) -> UserProfileInfo {
    UserProfileInfo::test(user_id, "neo")
}

#[test]
fn member_request_is_sent_once_per_active_guild() {
    let mut requests = MemberRequests::default();
    let first = Id::new(1);
    let second = Id::new(2);

    assert_eq!(requests.next(None), None);
    assert_eq!(requests.next(Some(first)), Some(first));
    assert_eq!(requests.next(Some(first)), None);
    assert_eq!(requests.next(Some(second)), Some(second));
    assert_eq!(requests.next(Some(first)), None);
}

#[test]
fn member_request_can_retry_after_remove() {
    let mut requests = MemberRequests::default();
    let guild_id = Id::new(1);

    assert_eq!(requests.next(Some(guild_id)), Some(guild_id));
    requests.remove(guild_id);

    assert_eq!(requests.next(Some(guild_id)), Some(guild_id));
}

#[test]
fn user_profile_request_dedupes_until_success_or_failure() {
    let mut requests = UserProfileRequests::default();
    let user_id = Id::new(10);
    let guild_id = Some(Id::new(1));

    assert!(requests.begin_request(user_id, guild_id));
    assert!(!requests.begin_request(user_id, guild_id));

    requests.record_event(&AppEvent::UserProfileLoaded {
        guild_id,
        profile: user_profile(user_id),
    });
    assert!(requests.begin_request(user_id, guild_id));

    requests.record_event(&user_profile_load_failed_event(
        UserProfileLoadFailedFixture {
            user_id,
            guild_id,
            message: "temporary failure".to_owned(),
        },
    ));
    assert!(requests.begin_request(user_id, guild_id));
}

#[test]
fn user_note_request_dedupes_until_success_or_failure() {
    let mut requests = UserNoteRequests::default();
    let user_id = Id::new(10);

    assert!(requests.begin_request(user_id));
    assert!(!requests.begin_request(user_id));

    requests.record_event(&AppEvent::UserNoteLoaded {
        user_id,
        note: Some("note".to_owned()),
    });
    assert!(requests.begin_request(user_id));

    requests.mark_failed(user_id);
    assert!(requests.begin_request(user_id));
}

#[test]
fn message_author_member_request_dedupes_until_member_arrives_or_ttl_expires() {
    let mut requests = MemberBatchRequests::default();
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let other_user_id = Id::new(20);
    let now = std::time::Instant::now();

    assert_eq!(
        requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
        vec![(guild_id, vec![user_id, other_user_id])]
    );
    assert_eq!(
        requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
        Vec::new()
    );

    requests.record_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            username: Some("neo".to_owned()),
            ..MemberInfo::test(user_id, "neo")
        },
    });
    assert_eq!(
        requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
        vec![(guild_id, vec![user_id])]
    );

    let retry_at = now + MemberBatchRequests::REQUEST_TTL + std::time::Duration::from_millis(1);
    assert_eq!(
        requests.next(vec![(guild_id, vec![other_user_id])], retry_at),
        vec![(guild_id, vec![other_user_id])]
    );
}

#[test]
fn member_list_subscription_debounces_and_coalesces_bucket_updates() {
    let mut requests = MemberListSubscriptionRequests::default();
    let now = std::time::Instant::now();

    requests.set_target(Some(subscription_target(0)), now);
    assert_eq!(requests.pending_deadline(), None);

    requests.set_target(Some(subscription_target(1)), now);
    let first_deadline = requests
        .pending_deadline()
        .expect("bucket one should arm debounce");
    assert!(
        requests
            .next_due(first_deadline - std::time::Duration::from_millis(1))
            .is_none()
    );

    requests.set_target(
        Some(subscription_target(2)),
        now + std::time::Duration::from_millis(1),
    );
    let second_deadline = requests
        .pending_deadline()
        .expect("latest bucket should stay pending");
    let target = requests
        .next_due(second_deadline)
        .expect("latest bucket should be sent after debounce");
    assert_eq!(target.bucket, 2);
    assert_eq!(target.ranges, vec![(0, 99), (200, 299)]);

    requests.set_target(Some(subscription_target(2)), second_deadline);
    assert_eq!(requests.pending_deadline(), None);

    requests.set_target(Some(subscription_target(0)), second_deadline);
    assert!(requests.pending_deadline().is_some());
}

#[test]
fn mention_member_search_debounces_bounds_and_retries_queries() {
    let mut requests = MentionMemberSearchRequests::default();
    let guild_id = Id::new(1);
    let now = std::time::Instant::now();

    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: "A".to_owned(),
        }),
        now,
    );
    assert_eq!(requests.pending_deadline(), None);

    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: " Alice ".to_owned(),
        }),
        now,
    );
    let deadline = requests
        .pending_deadline()
        .expect("valid query should arm debounce");
    assert_eq!(
        requests.next_due(deadline - std::time::Duration::from_millis(1)),
        None
    );
    assert_eq!(
        requests.next_due(deadline),
        Some(MentionMemberSearchTarget {
            guild_id,
            query: "alice".to_owned(),
        })
    );

    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: "ALICE".to_owned(),
        }),
        now + std::time::Duration::from_secs(1),
    );
    assert_eq!(requests.pending_deadline(), None);

    let retry_at =
        deadline + MentionMemberSearchRequests::REQUEST_TTL + std::time::Duration::from_millis(1);
    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: "alice".to_owned(),
        }),
        retry_at,
    );
    assert!(requests.pending_deadline().is_some());

    let long_query = "A".repeat(MentionMemberSearchRequests::MAX_QUERY_CHARS + 10);
    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: long_query,
        }),
        retry_at + std::time::Duration::from_millis(1),
    );
    let deadline = requests
        .pending_deadline()
        .expect("long query should still search by capped prefix");
    let target = requests
        .next_due(deadline)
        .expect("capped query should be due");
    assert_eq!(
        target.query.chars().count(),
        MentionMemberSearchRequests::MAX_QUERY_CHARS
    );
    assert!(target.query.chars().all(|ch| ch == 'a'));

    let expanding_query = "İ".repeat(MentionMemberSearchRequests::MAX_QUERY_CHARS + 10);
    requests.set_target(
        Some(MentionMemberSearchTarget {
            guild_id,
            query: expanding_query,
        }),
        retry_at + std::time::Duration::from_millis(2),
    );
    let deadline = requests
        .pending_deadline()
        .expect("expanding query should still search by capped prefix");
    let target = requests
        .next_due(deadline)
        .expect("expanded lowercase query should be due");
    assert_eq!(
        target.query.chars().count(),
        MentionMemberSearchRequests::MAX_QUERY_CHARS
    );
}

#[test]
fn thread_preview_request_retries_after_failed_card_is_revisited() {
    let mut requests = ThreadPreviewRequests::default();
    let key = (Id::new(10), Id::new(30));

    assert_eq!(requests.next(vec![key]), vec![key]);
    requests.record_event(&AppEvent::ThreadPreviewLoadFailed {
        channel_id: key.0,
        message_id: key.1,
    });

    assert_eq!(requests.next(vec![key]), Vec::new());
    assert_eq!(requests.next(Vec::new()), Vec::new());
    assert_eq!(requests.next(vec![key]), vec![key]);
}

#[test]
fn older_history_request_dedupes_and_tracks_exhausted_cursor() {
    let mut requests = RequestLifecycle::default();
    let channel_id = Id::new(10);
    let before = Id::new(30);

    assert!(requests.begin_older_history_request(channel_id, before));
    assert!(!requests.begin_older_history_request(channel_id, before));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Newer { after: Id::new(40) },
            message: "unrelated newer failure".to_owned(),
        },
    ));
    assert!(!requests.begin_older_history_request(channel_id, before));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Older {
                before: Id::new(31),
            },
            message: "stale older failure".to_owned(),
        },
    ));
    assert!(!requests.begin_older_history_request(channel_id, before));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Older { before },
            message: "temporary failure".to_owned(),
        },
    ));
    assert!(requests.begin_older_history_request(channel_id, before));

    requests.record_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        before: Some(before),
        ..MessageHistoryLoadedFixture::new()
    }));
    assert!(!requests.begin_older_history_request(channel_id, before));
    assert!(requests.begin_older_history_request(channel_id, Id::new(20)));
}

#[test]
fn newer_history_request_dedupes_and_tracks_exhausted_cursor() {
    let mut requests = RequestLifecycle::default();
    let channel_id = Id::new(10);
    let after = Id::new(30);

    assert!(requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));
    assert!(!requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Older {
                before: Id::new(20),
            },
            message: "unrelated older failure".to_owned(),
        },
    ));
    assert!(!requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Newer { after: Id::new(31) },
            message: "stale newer failure".to_owned(),
        },
    ));
    assert!(!requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));

    requests.record_event(&message_history_load_failed_event(
        MessageHistoryLoadFailedFixture {
            channel_id,
            target: MessageHistoryLoadTarget::Newer { after },
            message: "temporary failure".to_owned(),
        },
    ));
    assert!(requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));

    requests.record_event(&message_history_after_loaded_event(
        MessageHistoryAfterLoadedFixture {
            channel_id,
            after,
            mode: MessageHistoryAfterMode::GapFill,
            ..MessageHistoryAfterLoadedFixture::new()
        },
    ));
    assert!(!requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::GapFill
    ));
    assert!(requests.begin_history_after_request(
        channel_id,
        Id::new(31),
        MessageHistoryAfterMode::GapFill
    ));
}

#[test]
fn catch_up_history_request_dedupes_without_exhausting_empty_cursor() {
    let mut requests = RequestLifecycle::default();
    let channel_id = Id::new(10);
    let after = Id::new(30);

    assert!(requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::CatchUp
    ));
    assert!(!requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::CatchUp
    ));

    requests.record_event(&message_history_after_loaded_event(
        MessageHistoryAfterLoadedFixture {
            channel_id,
            after,
            mode: MessageHistoryAfterMode::CatchUp,
            ..MessageHistoryAfterLoadedFixture::new()
        },
    ));

    assert!(requests.begin_history_after_request(
        channel_id,
        after,
        MessageHistoryAfterMode::CatchUp
    ));
}

#[test]
fn read_ack_request_debounces_and_coalesces_by_channel() {
    let mut requests = RequestLifecycle::default();
    let now = std::time::Instant::now();
    let channel_id = Id::new(10);

    requests.schedule_read_ack(channel_id, Id::new(30), now);
    requests.schedule_read_ack(
        channel_id,
        Id::new(31),
        now + std::time::Duration::from_millis(1),
    );
    let deadline = requests
        .next_read_ack_deadline()
        .expect("read ack deadline should be armed");

    assert!(
        requests
            .flush_due_read_acks(deadline - std::time::Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        requests.flush_due_read_acks(deadline),
        vec![(channel_id, Id::new(31))]
    );
    assert_eq!(requests.next_read_ack_deadline(), None);
}
