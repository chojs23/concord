use super::*;
use crate::discord::notification::READ_STATE_MENTION_LOW_IMPORTANCE;

fn channel_with_last_message(channel_id: Id<ChannelMarker>, last_message_id: u64) -> ChannelInfo {
    ChannelInfo {
        last_message_id: Some(Id::new(last_message_id)),
        guild_id: Some(Id::new(1)),
        name: "general".to_owned(),
        ..channel_info(channel_id, "GuildText", Vec::new())
    }
}

#[test]
fn channel_unread_state_follows_ack_pointer() {
    let cases = [
        (100, None, ChannelUnreadState::Unread),
        (200, Some(150), ChannelUnreadState::Unread),
        (200, Some(200), ChannelUnreadState::Seen),
    ];

    for (latest_message_id, last_acked_message_id, expected) in cases {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id,
            latest_message_id,
        )));
        if let Some(last_acked_message_id) = last_acked_message_id {
            state.apply_event(&AppEvent::ReadStateInit {
                entries: vec![read_state_info(
                    channel_id,
                    Some(Id::new(last_acked_message_id)),
                    0,
                )],
            });
        }

        assert_eq!(state.channel_unread(channel_id), expected);
    }
}

#[test]
fn versioned_read_state_sync_merges_partial_and_clears_full_empty_snapshots() {
    let first = Id::new(7);
    let second = Id::new(8);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(first, Some(Id::new(10)), 1)],
    });

    state.apply_event(&AppEvent::ReadStateSync {
        entries: vec![read_state_info(second, Some(Id::new(20)), 2)],
        partial: true,
        version: Some(4),
    });
    assert!(state.notifications.read_states.contains_key(&first));
    assert!(state.notifications.read_states.contains_key(&second));
    assert_eq!(state.notifications.read_state_version, Some(4));

    state.apply_event(&AppEvent::ReadStateSync {
        entries: Vec::new(),
        partial: false,
        version: Some(5),
    });
    assert!(state.notifications.read_states.is_empty());
    assert!(state.notifications.non_channel_read_states.is_empty());
    assert_eq!(state.notifications.read_state_version, Some(5));
}

#[test]
fn acknowledgement_events_advance_the_gateway_version_but_local_ack_does_not() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ReadStateSync {
        entries: Vec::new(),
        partial: false,
        version: Some(10),
    });

    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(100),
        mention_count: Some(0),
        flags: None,
        last_viewed: None,
        version: Some(11),
    });
    state.apply_event(&AppEvent::FeatureReadStateAck {
        read_state_type: 2,
        resource_id: 1,
        entity_id: 20,
        version: 12,
    });
    assert_eq!(state.notifications.read_state_version, Some(12));

    state.apply_event(&AppEvent::ChannelPinsAck {
        channel_id,
        timestamp: "2026-08-12T00:00:00.000Z".to_owned(),
        version: 13,
    });
    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(101),
        mention_count: Some(0),
        flags: None,
        last_viewed: None,
        version: None,
    });
    assert_eq!(state.notifications.read_state_version, Some(13));
}

#[test]
fn typed_read_states_and_ack_metadata_remain_distinct_across_snapshots() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(7);
    let thread_id = Id::new(8);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        channels: vec![
            channel_with_last_message(channel_id, 100),
            guild_thread_channel(guild_id, thread_id, channel_id, "thread"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                last_acked_message_id: Some(Id::new(90)),
                last_pin_timestamp: Some("2026-07-24T00:00:00.000Z".to_owned()),
                flags: 5,
                last_viewed: Some(2_000),
                ..ReadStateInfo::test(channel_id)
            },
            ReadStateInfo {
                read_state_type: 1,
                last_acked_message_id: Some(Id::new(80)),
                badge_count: 3,
                ..ReadStateInfo::test(channel_id)
            },
        ],
    });

    let (channel_flags, last_viewed) = state.channel_ack_metadata(channel_id);
    let (thread_flags, _) = state.channel_ack_metadata(thread_id);
    assert_eq!(channel_flags, Some(1));
    assert_eq!(thread_flags, Some(3));
    assert!(last_viewed > 0);

    let restored = state
        .snapshot(crate::discord::SnapshotRevision::default())
        .to_state();
    let channel_read = restored
        .notifications
        .read_states
        .get(&channel_id)
        .expect("channel read state should survive");
    let non_channel_read = restored
        .notifications
        .non_channel_read_states
        .get(&(1, channel_id.get()))
        .expect("typed non-channel state should survive");
    assert_eq!(channel_read.flags, 5);
    assert_eq!(channel_read.last_viewed, Some(2_000));
    assert_eq!(
        channel_read.last_pin_timestamp.as_deref(),
        Some("2026-07-24T00:00:00.000Z")
    );
    assert_eq!(non_channel_read.last_acked_id, Some(80));
    assert_eq!(non_channel_read.badge_count, 3);
}

#[test]
fn current_user_message_create_keeps_channel_seen() {
    let channel_id = Id::new(7);
    let current_user_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 100,
    )));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(channel_id, Some(Id::new(100)), 0)],
    });
    {
        let read = state
            .notifications_mut()
            .read_states
            .get_mut(&channel_id)
            .expect("read state exists");
        read.mention_count = 3;
        read.notification_count = 2;
        read.flags |= READ_STATE_MENTION_LOW_IMPORTANCE;
    }

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(200),
        author_id: current_user_id,
        author: "me".to_owned(),
        content: Some("sent from this account".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    assert_eq!(state.channel_ack_target(channel_id), None);
    assert_eq!(state.channel_unread_message_count(channel_id), 0);
    let read = state
        .notifications
        .read_states
        .get(&channel_id)
        .expect("read state exists");
    assert_eq!(read.mention_count, 0);
    assert_eq!(read.notification_count, 0);
    assert_eq!(read.flags & READ_STATE_MENTION_LOW_IMPORTANCE, 0);
}

#[test]
fn message_ack_without_mention_count_retains_mentions_and_flags() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            last_acked_message_id: Some(Id::new(100)),
            mention_count: 3,
            flags: READ_STATE_MENTION_LOW_IMPORTANCE,
            ..ReadStateInfo::test(channel_id)
        }],
    });

    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(200),
        mention_count: None,
        flags: None,
        last_viewed: None,
        version: None,
    });

    let read = state
        .notifications
        .read_states
        .get(&channel_id)
        .expect("read state exists");
    assert_eq!(read.last_acked_message_id, Some(Id::new(200)));
    assert_eq!(read.mention_count, 3);
    assert_eq!(
        read.flags & READ_STATE_MENTION_LOW_IMPORTANCE,
        READ_STATE_MENTION_LOW_IMPORTANCE
    );
}

#[test]
fn channel_with_pending_mentions_reports_mention_count() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 200,
    )));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(channel_id, Some(Id::new(200)), 3)],
    });

    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Mentioned(3)
    );
}

#[test]
fn guild_unread_sums_channel_mentions_before_plain_unread() {
    let first_channel_id = Id::new(7);
    let second_channel_id = Id::new(8);
    let third_channel_id = Id::new(9);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        first_channel_id,
        200,
    )));
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        second_channel_id,
        300,
    )));
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        third_channel_id,
        400,
    )));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(first_channel_id, Some(Id::new(200)), 2),
            read_state_info(second_channel_id, Some(Id::new(300)), 3),
            read_state_info(third_channel_id, Some(Id::new(100)), 0),
        ],
    });

    assert_eq!(
        state.guild_unread(Id::new(1)),
        ChannelUnreadState::Mentioned(5)
    );
}

#[test]
fn message_ack_clears_outstanding_mentions_and_advances_pointer() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 500,
    )));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(channel_id, Some(Id::new(100)), 5)],
    });
    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Mentioned(5)
    );

    state.apply_event(&message_ack_event(MessageAckFixture {
        channel_id,
        message_id: Id::new(500),
        ..MessageAckFixture::new()
    }));

    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    assert_eq!(
        state.channel_ack_target(channel_id),
        None,
        "fully-acked channels need no further ack"
    );
}

#[test]
fn stale_message_ack_does_not_reopen_unread_state() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 500,
    )));
    state.apply_event(&user_guild_settings_init(vec![notification_settings(
        guild_id,
        NotificationLevel::AllMessages,
    )]));
    state.apply_event(&latest_history_loaded(
        channel_id,
        (101..=105)
            .map(|message_id| MessageInfo {
                guild_id: Some(guild_id),
                ..message_info(channel_id, message_id, "hello")
            })
            .collect(),
    ));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![read_state_info(channel_id, Some(Id::new(100)), 0)],
    });
    assert_eq!(state.channel_unread_message_count(channel_id), 5);

    state.apply_event(&message_ack_event(MessageAckFixture {
        channel_id,
        message_id: Id::new(500),
        ..MessageAckFixture::new()
    }));
    state.apply_event(&message_ack_event(MessageAckFixture {
        channel_id,
        message_id: Id::new(100),
        mention_count: 5,
    }));

    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    assert_eq!(state.channel_unread_message_count(channel_id), 0);
    assert_eq!(state.channel_ack_target(channel_id), None);
}

#[test]
fn channel_ack_target_returns_latest_when_unread() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 500,
    )));

    // No ack pointer at all -> ack target is the channel's last message.
    assert_eq!(state.channel_ack_target(channel_id), Some(Id::new(500)));
}
