use super::*;

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

    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(500),
        mention_count: 0,
    });
    state.apply_event(&AppEvent::MessageAck {
        channel_id,
        message_id: Id::new(100),
        mention_count: 5,
    });

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
