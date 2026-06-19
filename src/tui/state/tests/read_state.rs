use super::*;

#[test]
fn direct_message_unread_count_counts_unread_channels() {
    let mut state = state_with_direct_messages();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(10), Some(Id::new(100)), 0),
            read_state_info(Id::new(20), Some(Id::new(100)), 0),
            read_state_info(Id::new(30), None, 5),
        ],
    });

    assert_eq!(state.direct_message_unread_count(), 1);
}

#[test]
fn background_channel_message_updates_unread_without_scheduling_ack() {
    let mut state = state_with_direct_messages();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(10), Some(Id::new(100)), 0),
            read_state_info(Id::new(20), Some(Id::new(200)), 0),
        ],
    });
    state.push_effect(AppEvent::ActivateChannel {
        channel_id: Id::new(20),
    });
    assert!(state.drain_pending_commands().is_empty());

    state.push_event(direct_message_create_event(Id::new(10), 101));

    assert_eq!(state.direct_message_unread_count(), 1);
    assert_ne!(state.channel_unread(Id::new(10)), ChannelUnreadState::Seen);
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn active_channel_read_state_coalesces_when_new_messages_arrive_at_latest() {
    {
        let mut state = state_with_direct_messages();
        state.push_event(AppEvent::ReadStateInit {
            entries: vec![
                read_state_info(Id::new(10), Some(Id::new(100)), 0),
                read_state_info(Id::new(20), Some(Id::new(200)), 0),
            ],
        });
        state.push_effect(AppEvent::ActivateChannel {
            channel_id: Id::new(20),
        });
        assert!(state.drain_pending_commands().is_empty());

        state.push_event(direct_message_create_event(Id::new(20), 201));
        let scheduled = state.drain_pending_commands();
        apply_optimistic_ack_commands(&mut state, &scheduled);
        state.push_event(direct_message_create_event(Id::new(20), 202));
        let next_scheduled = state.drain_pending_commands();
        apply_optimistic_ack_commands(&mut state, &next_scheduled);

        assert_eq!(state.direct_message_unread_count(), 0);
        assert_eq!(state.channel_unread(Id::new(20)), ChannelUnreadState::Seen);
        assert_eq!(
            scheduled,
            vec![AppCommand::ScheduleAckChannel {
                channel_id: Id::new(20),
                message_id: Id::new(201),
            }]
        );
        assert_eq!(
            next_scheduled,
            vec![AppCommand::ScheduleAckChannel {
                channel_id: Id::new(20),
                message_id: Id::new(202),
            }]
        );
    }

    {
        let mut state = state_with_writable_channel();
        state.push_event(user_guild_settings_init(vec![
            GuildNotificationSettingsInfo {
                message_notifications: Some(NotificationLevel::AllMessages),
                ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
            },
        ]));

        state.push_event(notification_message_event(Id::new(2), "hello"));
        let scheduled = drain_debounced_read_ack(&mut state);
        apply_optimistic_ack_commands(&mut state, &scheduled);

        assert_eq!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
        assert_eq!(
            scheduled,
            vec![AppCommand::ScheduleAckChannel {
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
            entries: vec![read_state_info(Id::new(2), Some(Id::new(1)), 0)],
        });
        state.activate_channel(Id::new(2));
        state.set_message_view_height(10);
        assert_eq!(state.unread_divider_message_index(), Some(1));
        assert!(state.unread_banner().is_some());
        state.drain_pending_commands();

        state.push_event(message_create_event(MessageCreateFixture {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(4),
            author_id: Id::new(10),
            author: "me".to_owned(),
            content: Some("sent while reading latest".to_owned()),
            ..guild_message_create_fixture()
        }));

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
            read_state_info(Id::new(10), Some(Id::new(100)), 0),
            read_state_info(Id::new(20), Some(Id::new(100)), 0),
        ],
    });
    state.push_event(latest_history_loaded(
        Id::new(20),
        (101..=105)
            .map(|message_id| MessageInfo {
                guild_id: None,
                ..message_info(Id::new(20), message_id)
            })
            .collect(),
    ));

    assert_eq!(state.channel_unread_message_count(Id::new(20)), 5);
    assert_eq!(state.direct_message_unread_count(), 1);
}
