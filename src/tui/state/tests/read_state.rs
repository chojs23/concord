use super::*;

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
            author_is_bot: false,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            interaction: None,
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
