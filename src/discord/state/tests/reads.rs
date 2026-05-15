use super::*;

fn channel_with_last_message(channel_id: Id<ChannelMarker>, last_message_id: u64) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: Some(Id::new(last_message_id)),
        name: "general".to_owned(),
        kind: "text".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
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
                entries: vec![ReadStateInfo {
                    channel_id,
                    last_acked_message_id: Some(Id::new(last_acked_message_id)),
                    mention_count: 0,
                }],
            });
        }

        assert_eq!(state.channel_unread(channel_id), expected);
    }
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
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(100)),
            mention_count: 0,
        }],
    });

    state.apply_event(&AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(200),
        author_id: current_user_id,
        author: "me".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("sent from this account".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    assert_eq!(state.channel_ack_target(channel_id), None);
    assert_eq!(state.channel_unread_message_count(channel_id), 0);
}

#[test]
fn channel_with_pending_mentions_reports_mention_count() {
    let channel_id = Id::new(7);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
        channel_id, 200,
    )));
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(200)),
            mention_count: 3,
        }],
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
            ReadStateInfo {
                channel_id: first_channel_id,
                last_acked_message_id: Some(Id::new(200)),
                mention_count: 2,
            },
            ReadStateInfo {
                channel_id: second_channel_id,
                last_acked_message_id: Some(Id::new(300)),
                mention_count: 3,
            },
            ReadStateInfo {
                channel_id: third_channel_id,
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
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
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(100)),
            mention_count: 5,
        }],
    });
    assert_eq!(
        state.channel_unread(channel_id),
        ChannelUnreadState::Mentioned(5)
    );

    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(500),
        mention_count: 0,
    });

    assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    assert_eq!(
        state.channel_ack_target(channel_id),
        None,
        "fully-acked channels need no further ack"
    );
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
