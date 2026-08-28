use super::*;
use crate::discord::{MessageHistoryAfterMode, MessageInteractionInfo};

#[test]
fn bounds_messages_per_channel() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(1);

    for id in [1, 2] {
        state.apply_event(&message_create_event(
            MessageCreateFixture::direct_message(channel_id, Id::new(id))
                .with_content(format!("message {id}")),
        ));
    }

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id.get(), 2);
}

#[test]
fn stores_message_kind_from_message_create() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        author: "mee6".to_owned(),
        author_is_bot: true,
        message_kind: MessageKind::new(20),
        interaction: Some(MessageInteractionInfo {
            user_id: Some(Id::new(30)),
            command_name: Some("anime search".to_owned()),
            ..MessageInteractionInfo::test("casey")
        }),
        content: Some(String::new()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].message_kind, MessageKind::new(20));
    assert!(messages[0].author_is_bot);
    assert_eq!(
        messages[0]
            .interaction
            .as_ref()
            .and_then(|info| info.command_name.as_deref()),
        Some("anime search")
    );
}

#[test]
fn duplicate_message_create_keeps_cached_payload_and_refreshes_kind() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        author_id,
        author: "Helper Bot".to_owned(),
        author_is_bot: true,
        interaction: Some(crate::discord::MessageInteractionInfo {
            user_id: Some(Id::new(77)),
            command_name: Some("help".to_owned()),
            ..crate::discord::MessageInteractionInfo::test("Alex")
        }),
        reply: Some(ReplyInfo {
            author_id: Some(Id::new(77)),
            content: Some("잘되는군".to_owned()),
            ..ReplyInfo::test("Alex")
        }),
        poll: Some(poll_info()),
        content: Some("cached".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    // Discord echoes an already-cached message back thin: no content, no reply
    // preview, no poll. The echo may refresh the kind but must not blank what
    // we already know, or the message visibly empties out on screen.
    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        author_id,
        author: "unknown".to_owned(),
        author_is_bot: false,
        interaction: Some(crate::discord::MessageInteractionInfo {
            user_id: Some(Id::new(77)),
            ..crate::discord::MessageInteractionInfo::test("unknown")
        }),
        reply: Some(ReplyInfo {
            author_id: Some(Id::new(77)),
            content: Some("잘되는군".to_owned()),
            ..ReplyInfo::test("unknown")
        }),
        message_kind: MessageKind::new(19),
        content: None,
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].author, "Helper Bot");
    assert!(messages[0].author_is_bot);
    assert_eq!(
        messages[0]
            .interaction
            .as_ref()
            .expect("interaction should remain cached"),
        &crate::discord::MessageInteractionInfo {
            user_id: Some(Id::new(77)),
            user: "Alex".to_owned(),
            command_name: Some("help".to_owned()),
        }
    );
    assert_eq!(messages[0].content.as_deref(), Some("cached"));
    assert_eq!(messages[0].message_kind, MessageKind::new(19));
    assert_eq!(
        messages[0]
            .reply
            .as_ref()
            .and_then(|reply| reply.content.as_deref()),
        Some("잘되는군")
    );
    assert_eq!(
        messages[0]
            .reply
            .as_ref()
            .map(|reply| reply.author.as_str()),
        Some("Alex")
    );
    assert_eq!(
        messages[0].poll.as_ref().map(|poll| poll.answers.len()),
        Some(2)
    );
}

#[test]
fn duplicate_message_create_adds_missing_mentions() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(
        MessageCreateFixture::direct_message(channel_id, message_id)
            .with_author_id(author_id)
            .with_content("hello <@10>"),
    ));
    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        author_id,
        content: Some("hello <@10>".to_owned()),
        mentions: vec![mention_info(10, "alice")],
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
}

#[test]
fn stores_rich_payload_fields_from_message_create() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        message_kind: MessageKind::new(19),
        reply: Some(ReplyInfo {
            content: Some("잘되는군".to_owned()),
            ..ReplyInfo::test("Alex")
        }),
        content: Some("asdf".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(21),
        author_id: Id::new(99),
        poll: Some(poll_info()),
        content: Some(String::new()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(22),
        author_id: Id::new(99),
        content: Some(String::new()),
        forwarded_snapshots: vec![snapshot_info("forwarded text")],
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    let reply = messages[0].reply.as_ref().expect("reply preview is cached");
    assert_eq!(reply.author, "Alex");
    assert_eq!(reply.content.as_deref(), Some("잘되는군"));
    assert_eq!(
        messages[1].poll.as_ref().map(|poll| poll.question.as_str()),
        Some("오늘 뭐 먹지?")
    );
    assert_eq!(messages[2].forwarded_snapshots.len(), 1);
    assert_eq!(
        messages[2].forwarded_snapshots[0].content.as_deref(),
        Some("forwarded text")
    );
}

#[test]
fn message_update_refreshes_cached_poll_results() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        poll: Some(poll_info()),
        content: Some(String::new()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    let mut updated_poll = poll_info();
    updated_poll.results_finalized = Some(true);
    updated_poll.answers[0].vote_count = Some(5);
    updated_poll.answers[1].vote_count = Some(3);
    state.apply_event(&message_update_event(
        channel_id,
        message_id,
        MessageUpdateEventFields {
            poll: Some(updated_poll),
            ..MessageUpdateEventFields::default()
        },
    ));

    let messages = state.messages_for_channel(channel_id);
    let poll = messages[0].poll.as_ref().expect("poll should stay cached");
    assert_eq!(poll.results_finalized, Some(true));
    assert_eq!(poll.answers[0].vote_count, Some(5));
    assert_eq!(poll.answers[1].vote_count, Some(3));
}

#[test]
fn message_update_applies_pin_state_to_the_cached_message() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id: Id::new(99),
        content: Some("Pinned from another client".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_update_event(
        channel_id,
        message_id,
        MessageUpdateEventFields {
            pinned: Some(true),
            ..MessageUpdateEventFields::default()
        },
    ));

    assert!(state.messages_for_channel(channel_id)[0].pinned);
    assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

    state.apply_event(&message_update_event(
        channel_id,
        message_id,
        MessageUpdateEventFields {
            pinned: Some(false),
            ..MessageUpdateEventFields::default()
        },
    ));

    assert!(!state.messages_for_channel(channel_id)[0].pinned);
    assert!(state.pinned_messages_for_channel(channel_id).is_empty());
}

#[test]
fn current_user_poll_vote_update_refreshes_cached_poll_counts() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        poll: Some(poll_info()),
        content: Some(String::new()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    state.apply_event(&current_user_poll_vote_update_event(
        CurrentUserPollVoteUpdateFixture {
            channel_id,
            message_id,
            answer_ids: vec![2],
        },
    ));
    let poll = state.messages_for_channel(channel_id)[0]
        .poll
        .as_ref()
        .expect("poll should be cached");
    assert_eq!(poll.answers[0].vote_count, Some(1));
    assert!(!poll.answers[0].me_voted);
    assert_eq!(poll.answers[1].vote_count, Some(2));
    assert!(poll.answers[1].me_voted);
    assert_eq!(poll.total_votes, Some(3));

    state.apply_event(&current_user_poll_vote_update_event(
        CurrentUserPollVoteUpdateFixture {
            channel_id,
            message_id,
            ..CurrentUserPollVoteUpdateFixture::new()
        },
    ));
    let poll = state.messages_for_channel(channel_id)[0]
        .poll
        .as_ref()
        .expect("poll should be cached");
    assert_eq!(poll.answers[0].vote_count, Some(1));
    assert!(!poll.answers[0].me_voted);
    assert_eq!(poll.answers[1].vote_count, Some(1));
    assert!(!poll.answers[1].me_voted);
    assert_eq!(poll.total_votes, Some(2));
}

#[test]
fn current_user_poll_vote_update_handles_missing_answer_counts() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();
    let mut poll = poll_info();
    poll.answers[1].vote_count = None;

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        poll: Some(poll),
        content: Some(String::new()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    state.apply_event(&current_user_poll_vote_update_event(
        CurrentUserPollVoteUpdateFixture {
            channel_id,
            message_id,
            answer_ids: vec![2],
        },
    ));

    let poll = state.messages_for_channel(channel_id)[0]
        .poll
        .as_ref()
        .expect("poll should be cached");
    assert_eq!(poll.answers[0].vote_count, Some(1));
    assert!(!poll.answers[0].me_voted);
    assert_eq!(poll.answers[1].vote_count, Some(1));
    assert!(poll.answers[1].me_voted);
    assert_eq!(poll.total_votes, Some(3));
}

#[test]
fn message_update_handles_mentions_tristate() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let cases = [
        (
            Vec::new(),
            Some(vec![mention_info(10, "alice")]),
            vec![mention_info(10, "alice")],
        ),
        (
            vec![mention_info(10, "alice")],
            None,
            vec![mention_info(10, "alice")],
        ),
        (
            vec![mention_info(10, "alice")],
            Some(Vec::new()),
            Vec::new(),
        ),
    ];

    for (initial_mentions, update_mentions, expected_mentions) in cases {
        let mut state = DiscordState::default();
        state.apply_event(&message_create_event(MessageCreateFixture {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            content: Some("hello <@10>".to_owned()),
            mentions: initial_mentions,
            ..MessageCreateFixture::test_fixture_default()
        }));
        state.apply_event(&message_update_event(
            channel_id,
            message_id,
            MessageUpdateEventFields {
                content: Some("hello".to_owned()),
                mentions: update_mentions,
                ..MessageUpdateEventFields::default()
            },
        ));

        assert_eq!(
            state.messages_for_channel(channel_id)[0].mentions,
            expected_mentions
        );
    }
}

#[test]
fn message_capabilities_preserve_overlapping_traits() {
    let mut message = message_state("hello");
    assert_eq!(message.capabilities(), Default::default());

    message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
    let capabilities = message.capabilities();
    assert!(capabilities.has_image);
    assert!(!capabilities.has_poll);

    message.poll = Some(poll_info());
    let capabilities = message.capabilities();
    assert!(capabilities.has_image);
    assert!(capabilities.has_poll);
}

#[test]
fn message_capabilities_expose_action_facets_for_chat_messages_only() {
    let mut message = message_state("system body");
    message.message_kind = MessageKind::new(19);
    message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
    message.poll = Some(poll_info());

    let capabilities = message.capabilities();
    assert!(capabilities.has_poll);
    assert!(capabilities.has_image);

    message.message_kind = MessageKind::new(7);
    message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
    message.poll = Some(poll_info());

    let capabilities = message.capabilities();
    assert!(!capabilities.has_poll);
    assert!(!capabilities.has_image);
}

#[test]
fn message_capabilities_track_reply_and_forwarded_traits() {
    let mut message = message_state("reply body");
    message.reply = Some(ReplyInfo {
        content: Some("original".to_owned()),
        ..ReplyInfo::test("neo")
    });
    message.forwarded_snapshots = vec![snapshot_info("forwarded")];

    let capabilities = message.capabilities();

    assert!(capabilities.is_reply);
    assert!(capabilities.is_forwarded);
}

#[test]
fn keeps_known_content_when_gateway_echo_has_no_content() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(30);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        content: None,
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&message_update_event(
        channel_id,
        message_id,
        MessageUpdateEventFields::default(),
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("hello"));
}

#[test]
fn merges_history_in_chronological_order() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(30),
        author_id: Id::new(99),
        content: Some("live".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 20, "history 20"),
            message_info(channel_id, 10, "history 10"),
        ],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
}

#[test]
fn history_merge_preserves_message_reference() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();
    let reference = MessageReferenceInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Some(Id::new(20)),
        ..MessageReferenceInfo::test(Id::new(30))
    };

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            reference: Some(reference.clone()),
            ..message_info(channel_id, 20, "history")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].reference, Some(reference));
}

#[test]
fn history_dedupes_and_preserves_known_content() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some("known".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            ..message_info(channel_id, 20, "")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("known"));
}

#[test]
fn pinned_messages_loaded_stay_out_of_normal_history() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "latest")],
    ));
    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 5, "old pin")],
    });

    assert_eq!(
        state
            .messages_for_channel(channel_id)
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![20]
    );
    assert_eq!(
        state
            .pinned_messages_for_channel(channel_id)
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![5]
    );
}

#[test]
fn bulk_delete_removes_messages_from_normal_and_pinned_caches() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "keep"),
            message_info(channel_id, 20, "delete"),
            message_info(channel_id, 30, "delete too"),
        ],
    ));
    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 20, "pinned delete")],
    });

    state.apply_event(&message_delete_bulk_event(MessageDeleteBulkFixture {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_ids: vec![Id::new(20), Id::new(30)],
    }));

    assert_eq!(
        state
            .messages_for_channel(channel_id)
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![10]
    );
    assert!(state.pinned_messages_for_channel(channel_id).is_empty());
}

#[test]
fn later_history_preserves_pin_state_from_pinned_cache() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 20, "pin")],
    });
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "pin")],
    ));

    assert!(state.messages_for_channel(channel_id)[0].pinned);
}

#[test]
fn pinned_messages_loaded_reconciles_normal_message_pin_flags() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            MessageInfo {
                pinned: true,
                ..message_info(channel_id, 20, "old pin")
            },
            MessageInfo {
                pinned: true,
                ..message_info(channel_id, 30, "current pin")
            },
        ],
    ));

    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 30, "current pin")],
    });

    let messages = state.messages_for_channel(channel_id);
    assert!(!messages[0].pinned);
    assert!(messages[1].pinned);
}

#[test]
fn message_pinned_update_updates_pinned_cache() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "normal")],
    ));
    state.apply_event(&message_pinned_update_event(MessagePinnedUpdateFixture {
        channel_id,
        message_id: Id::new(20),
        pinned: true,
    }));
    assert!(state.messages_for_channel(channel_id)[0].pinned);
    assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

    state.apply_event(&message_pinned_update_event(MessagePinnedUpdateFixture {
        channel_id,
        message_id: Id::new(20),
        ..MessagePinnedUpdateFixture::new()
    }));
    assert!(!state.messages_for_channel(channel_id)[0].pinned);
    assert!(state.pinned_messages_for_channel(channel_id).is_empty());
}

#[test]
fn channel_pins_update_invalidates_loaded_pinned_cache() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 20, "old pin")],
    });
    assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

    state.apply_event(&channel_pins_update_event(ChannelPinsUpdateFixture {
        channel_id,
        last_pin_timestamp: Some("2026-05-25T12:34:56.000000+00:00".to_owned()),
        ..ChannelPinsUpdateFixture::new()
    }));

    assert!(state.pinned_messages_for_channel(channel_id).is_empty());
}

#[test]
fn reaction_events_update_pinned_cache() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();
    let emoji = ReactionEmoji::Unicode("👍".to_owned());

    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 20, "pin")],
    });
    state.apply_event(&message_reaction_add_event(MessageReactionAddFixture {
        channel_id,
        message_id: Id::new(20),
        user_id: Id::new(50),
        emoji: emoji.clone(),
        ..MessageReactionAddFixture::new()
    }));

    let pinned = state.pinned_messages_for_channel(channel_id)[0];
    assert_eq!(pinned.reactions.len(), 1);
    assert_eq!(pinned.reactions[0].emoji, emoji);
    assert_eq!(pinned.reactions[0].count, 1);

    state.apply_event(&message_reaction_remove_all_event(
        MessageReactionRemoveAllFixture {
            channel_id,
            message_id: Id::new(20),
            ..MessageReactionRemoveAllFixture::new()
        },
    ));
    assert!(
        state.pinned_messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}

#[test]
fn history_merge_replaces_mentions_from_authoritative_history() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some("hello <@10>".to_owned()),
        mention_roles: vec![Id::new(30)],
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            mentions: vec![mention_info(10, "alice")],
            mention_roles: vec![Id::new(30)],
            ..message_info(channel_id, 20, "hello <@10>")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
    assert_eq!(messages[0].mention_roles, vec![Id::new(30)]);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "hello")],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert!(messages[0].mentions.is_empty());
    assert!(messages[0].mention_roles.is_empty());
}

#[test]
fn history_merge_preserves_richer_gateway_mention_display_name() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some("hello <@10> <@11>".to_owned()),
        mentions: vec![
            mention_info(10, "global alias"),
            mention_info(11, "unknown"),
        ],
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            mentions: vec![mention_info(10, "username"), mention_info(11, "recovered")],
            ..message_info(channel_id, 20, "hello <@10> <@11>")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages[0].mentions,
        vec![
            mention_info(10, "global alias"),
            mention_info(11, "recovered")
        ]
    );
}

#[test]
fn history_merge_clears_reactions_from_authoritative_history() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            reactions: vec![ReactionInfo {
                count: 2,
                me: true,
                ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
            }],
            ..message_info(channel_id, 20, "hello")
        }],
    ));
    assert_eq!(state.messages_for_channel(channel_id)[0].reactions.len(), 1);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            reactions: Vec::new(),
            ..message_info(channel_id, 20, "hello")
        }],
    ));

    assert!(
        state.messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}

#[test]
fn stores_and_merges_message_attachments() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some(String::new()),
        attachments: vec![attachment_info(1, "cat.png", "image/png")],
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            attachments: Vec::new(),
            ..message_info(channel_id, 20, "")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].attachments.len(), 1);
    assert_eq!(messages[0].attachments[0].filename, "cat.png");
}

#[test]
fn history_merge_preserves_existing_forwarded_snapshots() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some(String::new()),
        forwarded_snapshots: vec![snapshot_info("live snapshot")],
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "")],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages[0].forwarded_snapshots[0].content.as_deref(),
        Some("live snapshot")
    );
}

#[test]
fn message_update_handles_attachment_update_tristate() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let cases = [
        (AttachmentUpdate::Unchanged, 1),
        (AttachmentUpdate::Replace(Vec::new()), 0),
    ];

    for (attachments, expected_len) in cases {
        let mut state = DiscordState::default();
        state.apply_event(&message_create_event(MessageCreateFixture {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            content: Some(String::new()),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            ..MessageCreateFixture::test_fixture_default()
        }));
        state.apply_event(&message_update_event(
            channel_id,
            Id::new(20),
            MessageUpdateEventFields {
                attachments,
                ..MessageUpdateEventFields::default()
            },
        ));

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].attachments.len(), expected_len);
        if expected_len == 1 {
            assert_eq!(messages[0].attachments[0].filename, "cat.png");
        }
    }
}

#[test]
fn history_respects_message_limit_after_merge() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(2);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 20, "middle"),
            message_info(channel_id, 30, "new"),
        ],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![20, 30]
    );
}

#[test]
fn segmented_history_keeps_live_messages_reachable_while_browsing_older_pages() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(6);

    state.apply_event(&latest_history_loaded(
        channel_id,
        (100..=105)
            .map(|id| message_info(channel_id, id, &format!("live {id}")))
            .collect(),
    ));
    state.apply_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        before: Some(Id::new(100)),
        messages: (1..=7)
            .map(|id| message_info(channel_id, id, &format!("older {id}")))
            .collect(),
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(106),
        author_id: Id::new(99),
        content: Some("new live message".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    assert_eq!(
        state
            .messages_for_channel(channel_id)
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 101, 102, 103, 104, 105, 106]
    );
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(6)),
        Some(Id::new(101))
    );

    state.apply_event(&message_history_after_loaded_event(
        MessageHistoryAfterLoadedFixture {
            channel_id,
            after: Id::new(6),
            messages: (7..=10)
                .map(|id| message_info(channel_id, id, &format!("gap {id}")))
                .collect(),
            has_more: true,
            mode: MessageHistoryAfterMode::GapFill,
        },
    ));
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(8)),
        Some(Id::new(101))
    );

    state.apply_event(&message_history_after_loaded_event(
        MessageHistoryAfterLoadedFixture {
            channel_id,
            after: Id::new(8),
            messages: (9..=12)
                .map(|id| message_info(channel_id, id, &format!("gap {id}")))
                .collect(),
            has_more: true,
            mode: MessageHistoryAfterMode::GapFill,
        },
    ));
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(10)),
        Some(Id::new(101))
    );

    state.apply_event(&message_history_after_loaded_event(
        MessageHistoryAfterLoadedFixture {
            channel_id,
            after: Id::new(10),
            messages: vec![
                message_info(channel_id, 11, "gap 11"),
                message_info(channel_id, 12, "gap 12"),
                message_info(channel_id, 101, "live boundary"),
            ],
            mode: MessageHistoryAfterMode::GapFill,
            ..MessageHistoryAfterLoadedFixture::new()
        },
    ));

    assert_eq!(
        state
            .messages_for_channel(channel_id)
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![7, 8, 9, 10, 11, 12, 101, 102, 103, 104, 105, 106]
    );
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(12)),
        None
    );
}

#[test]
fn current_user_reaction_events_update_cached_reaction_summary() {
    let mut state = DiscordState::default();
    let channel_id = Id::new(2);
    let message_id = Id::new(1);
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    state.apply_event(&current_user_reaction_add_event(
        CurrentUserReactionAddFixture {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        },
    ));
    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 1);
    assert!(message.reactions[0].me);

    state.apply_event(&current_user_reaction_remove_event(
        CurrentUserReactionRemoveFixture {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        },
    ));
    assert!(
        state.messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}

#[test]
fn gateway_reaction_events_update_cached_reaction_summary() {
    let mut state = DiscordState::default();
    let channel_id = Id::new(2);
    let message_id = Id::new(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    state.apply_event(&message_reaction_add_event(MessageReactionAddFixture {
        channel_id,
        message_id,
        user_id: Id::new(50),
        emoji: emoji.clone(),
        ..MessageReactionAddFixture::new()
    }));
    state.apply_event(&message_reaction_add_event(MessageReactionAddFixture {
        channel_id,
        message_id,
        user_id: Id::new(51),
        emoji: emoji.clone(),
        ..MessageReactionAddFixture::new()
    }));

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 2);
    assert!(!message.reactions[0].me);

    state.apply_event(&message_reaction_remove_event(
        MessageReactionRemoveFixture {
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji,
            ..MessageReactionRemoveFixture::new()
        },
    ));

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 1);
    assert!(!message.reactions[0].me);
}

#[test]
fn current_user_gateway_reaction_events_reconcile_optimistic_updates() {
    let mut state = DiscordState::default();
    let channel_id = Id::new(2);
    let message_id = Id::new(1);
    let current_user_id = Id::new(7);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    state.apply_event(&current_user_reaction_add_event(
        CurrentUserReactionAddFixture {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        },
    ));
    state.apply_event(&message_reaction_add_event(MessageReactionAddFixture {
        channel_id,
        message_id,
        user_id: current_user_id,
        emoji: emoji.clone(),
        ..MessageReactionAddFixture::new()
    }));
    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions[0].count, 1);
    assert!(message.reactions[0].me);

    state.apply_event(&message_reaction_add_event(MessageReactionAddFixture {
        channel_id,
        message_id,
        user_id: Id::new(50),
        emoji: emoji.clone(),
        ..MessageReactionAddFixture::new()
    }));
    state.apply_event(&current_user_reaction_remove_event(
        CurrentUserReactionRemoveFixture {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        },
    ));
    state.apply_event(&message_reaction_remove_event(
        MessageReactionRemoveFixture {
            channel_id,
            message_id,
            user_id: current_user_id,
            emoji,
            ..MessageReactionRemoveFixture::new()
        },
    ));

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 1);
    assert!(!message.reactions[0].me);
}

#[test]
fn gateway_reaction_clear_events_update_cached_reaction_summary() {
    let mut state = DiscordState::default();
    let channel_id = Id::new(2);
    let message_id = Id::new(1);
    let thumbs_up = ReactionEmoji::Unicode("👍".to_owned());
    let party = ReactionEmoji::Unicode("🎉".to_owned());
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            reactions: vec![
                ReactionInfo {
                    count: 2,
                    me: true,
                    ..ReactionInfo::test(thumbs_up.clone())
                },
                ReactionInfo::test(party),
            ],
            ..message_info(channel_id, message_id.get(), "hello")
        }],
    ));

    state.apply_event(&message_reaction_remove_emoji_event(
        MessageReactionRemoveEmojiFixture {
            channel_id,
            message_id,
            emoji: thumbs_up,
            ..MessageReactionRemoveEmojiFixture::new()
        },
    ));

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(
        message.reactions[0].emoji,
        ReactionEmoji::Unicode("🎉".to_owned())
    );

    state.apply_event(&message_reaction_remove_all_event(
        MessageReactionRemoveAllFixture {
            channel_id,
            message_id,
            ..MessageReactionRemoveAllFixture::new()
        },
    ));

    assert!(
        state.messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}
