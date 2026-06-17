use super::*;
use crate::discord::MessageInteractionInfo;

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
        ..MessageCreateFixture::default()
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
fn duplicate_message_create_refreshes_message_kind() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(
        MessageCreateFixture::direct_message(channel_id, message_id)
            .with_author_id(author_id)
            .with_content("cached"),
    ));
    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        author_id,
        message_kind: MessageKind::new(19),
        content: None,
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("cached"));
    assert_eq!(messages[0].message_kind, MessageKind::new(19));
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
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
}

#[test]
fn stores_reply_preview_from_message_create() {
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
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages[0]
            .reply
            .as_ref()
            .map(|reply| reply.author.as_str()),
        Some("Alex")
    );
    assert_eq!(
        messages[0]
            .reply
            .as_ref()
            .and_then(|reply| reply.content.as_deref()),
        Some("잘되는군")
    );
}

#[test]
fn duplicate_message_create_preserves_cached_reply_preview() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        author_id,
        message_kind: MessageKind::new(19),
        reply: Some(ReplyInfo {
            content: Some("잘되는군".to_owned()),
            ..ReplyInfo::test("Alex")
        }),
        content: Some("asdf".to_owned()),
        ..MessageCreateFixture::default()
    }));
    let mut gateway_echo = MessageCreateFixture::direct_message(channel_id, message_id)
        .with_author_id(author_id)
        .without_content();
    gateway_echo.message_kind = MessageKind::new(19);
    state.apply_event(&message_create_event(gateway_echo));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]
            .reply
            .as_ref()
            .and_then(|reply| reply.content.as_deref()),
        Some("잘되는군")
    );
}

#[test]
fn stores_poll_payload_from_message_create() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        poll: Some(poll_info()),
        content: Some(String::new()),
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages[0].poll.as_ref().map(|poll| poll.question.as_str()),
        Some("오늘 뭐 먹지?")
    );
}

#[test]
fn duplicate_message_create_preserves_cached_poll_payload() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let message_id = Id::new(20);
    let author_id = Id::new(99);
    let mut state = DiscordState::default();

    let mut poll_message = MessageCreateFixture::direct_message(channel_id, message_id)
        .with_author_id(author_id)
        .with_content(String::new());
    poll_message.poll = Some(poll_info());
    state.apply_event(&message_create_event(poll_message));
    state.apply_event(&message_create_event(
        MessageCreateFixture::direct_message(channel_id, message_id)
            .with_author_id(author_id)
            .without_content(),
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].poll.as_ref().map(|poll| poll.answers.len()),
        Some(2)
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
        ..MessageCreateFixture::default()
    }));
    let mut updated_poll = poll_info();
    updated_poll.results_finalized = Some(true);
    updated_poll.answers[0].vote_count = Some(5);
    updated_poll.answers[1].vote_count = Some(3);
    state.apply_event(&AppEvent::MessageUpdate {
        guild_id: None,
        channel_id,
        message_id,
        poll: Some(updated_poll),
        content: None,
        sticker_names: None,
        mentions: None,
        mention_everyone: None,
        mention_roles: None,
        flags: None,
        attachments: AttachmentUpdate::Unchanged,
        embeds: None,
        edited_timestamp: None,
    });

    let messages = state.messages_for_channel(channel_id);
    let poll = messages[0].poll.as_ref().expect("poll should stay cached");
    assert_eq!(poll.results_finalized, Some(true));
    assert_eq!(poll.answers[0].vote_count, Some(5));
    assert_eq!(poll.answers[1].vote_count, Some(3));
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
        ..MessageCreateFixture::default()
    }));

    state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
        channel_id,
        message_id,
        answer_ids: vec![2],
    });
    let poll = state.messages_for_channel(channel_id)[0]
        .poll
        .as_ref()
        .expect("poll should be cached");
    assert_eq!(poll.answers[0].vote_count, Some(1));
    assert!(!poll.answers[0].me_voted);
    assert_eq!(poll.answers[1].vote_count, Some(2));
    assert!(poll.answers[1].me_voted);
    assert_eq!(poll.total_votes, Some(3));

    state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
        channel_id,
        message_id,
        answer_ids: Vec::new(),
    });
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
        ..MessageCreateFixture::default()
    }));

    state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
        channel_id,
        message_id,
        answer_ids: vec![2],
    });

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
            ..MessageCreateFixture::default()
        }));
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: None,
            mentions: update_mentions,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

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
        ..MessageCreateFixture::default()
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id,
        author_id,
        content: None,
        ..MessageCreateFixture::default()
    }));
    state.apply_event(&AppEvent::MessageUpdate {
        guild_id: None,
        channel_id,
        message_id,
        poll: None,
        content: None,
        sticker_names: None,
        mentions: None,
        mention_everyone: None,
        mention_roles: None,
        flags: None,
        attachments: AttachmentUpdate::Unchanged,
        embeds: None,
        edited_timestamp: None,
    });

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
        ..MessageCreateFixture::default()
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
        ..MessageCreateFixture::default()
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

    state.apply_event(&AppEvent::MessageDeleteBulk {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_ids: vec![Id::new(20), Id::new(30)],
    });

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
fn pinned_messages_loaded_mark_overlapping_normal_messages() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 20, "normal")],
    ));
    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 20, "normal")],
    });

    assert!(state.messages_for_channel(channel_id)[0].pinned);
    assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);
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
    state.apply_event(&AppEvent::MessagePinnedUpdate {
        channel_id,
        message_id: Id::new(20),
        pinned: true,
    });
    assert!(state.messages_for_channel(channel_id)[0].pinned);
    assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

    state.apply_event(&AppEvent::MessagePinnedUpdate {
        channel_id,
        message_id: Id::new(20),
        pinned: false,
    });
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

    state.apply_event(&AppEvent::ChannelPinsUpdate {
        guild_id: None,
        channel_id,
        last_pin_timestamp: Some("2026-05-25T12:34:56.000000+00:00".to_owned()),
    });

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
    state.apply_event(&AppEvent::MessageReactionAdd {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        user_id: Id::new(50),
        emoji: emoji.clone(),
    });

    let pinned = state.pinned_messages_for_channel(channel_id)[0];
    assert_eq!(pinned.reactions.len(), 1);
    assert_eq!(pinned.reactions[0].emoji, emoji);
    assert_eq!(pinned.reactions[0].count, 1);

    state.apply_event(&AppEvent::MessageReactionRemoveAll {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
    });
    assert!(
        state.pinned_messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}

#[test]
fn poll_vote_updates_update_pinned_cache() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();
    let mut message = message_info(channel_id, 20, "poll");
    message.poll = Some(poll_info());

    state.apply_event(&AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message],
    });
    state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
        channel_id,
        message_id: Id::new(20),
        answer_ids: vec![2],
    });

    let poll = state.pinned_messages_for_channel(channel_id)[0]
        .poll
        .as_ref()
        .expect("pinned poll should stay cached");
    assert!(!poll.answers[0].me_voted);
    assert_eq!(poll.answers[0].vote_count, Some(1));
    assert!(poll.answers[1].me_voted);
    assert_eq!(poll.answers[1].vote_count, Some(2));
    assert_eq!(poll.total_votes, Some(3));
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
        ..MessageCreateFixture::default()
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
        content: Some("hello <@10>".to_owned()),
        mentions: vec![mention_info(10, "global alias")],
        ..MessageCreateFixture::default()
    }));
    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![MessageInfo {
            mentions: vec![mention_info(10, "username")],
            ..message_info(channel_id, 20, "hello <@10>")
        }],
    ));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].mentions, vec![mention_info(10, "global alias")]);
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
        ..MessageCreateFixture::default()
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
fn stores_forwarded_snapshots_from_message_create() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(20),
        author_id: Id::new(99),
        content: Some(String::new()),
        forwarded_snapshots: vec![snapshot_info("forwarded text")],
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].forwarded_snapshots.len(), 1);
    assert_eq!(
        messages[0].forwarded_snapshots[0].content.as_deref(),
        Some("forwarded text")
    );
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
        ..MessageCreateFixture::default()
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
            ..MessageCreateFixture::default()
        }));
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            attachments,
            embeds: None,
            edited_timestamp: None,
        });

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
fn older_history_preserves_existing_messages_when_message_limit_is_reached() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(3);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 11, "middle"),
            message_info(channel_id, 12, "new"),
        ],
    ));
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5, "older")],
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![5, 10, 11, 12]
    );
}

#[test]
fn older_history_is_bounded_by_extra_window() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(3);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 11, "middle"),
            message_info(channel_id, 12, "new"),
        ],
    ));
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![
            message_info(channel_id, 1, "older 1"),
            message_info(channel_id, 2, "older 2"),
            message_info(channel_id, 3, "older 3"),
            message_info(channel_id, 4, "older 4"),
            message_info(channel_id, 5, "older 5"),
        ],
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages.len(), 6);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 10]
    );
}

#[test]
fn live_message_after_older_history_keeps_newer_window() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(4);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 10, "old"),
            message_info(channel_id, 11, "middle"),
            message_info(channel_id, 12, "new"),
        ],
    ));
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5, "older")],
    });
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(13),
        author_id: Id::new(99),
        content: Some("newest".to_owned()),
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![10, 11, 12, 13]
    );
}

#[test]
fn newer_history_gap_is_recorded_shrunk_and_closed() {
    let channel_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DiscordState::new(3);

    state.apply_event(&latest_history_loaded(
        channel_id,
        vec![
            message_info(channel_id, 100, "newer 100"),
            message_info(channel_id, 101, "newer 101"),
        ],
    ));
    state.apply_event(&AppEvent::MessageHistoryAroundLoaded {
        channel_id,
        message_id: Id::new(11),
        messages: vec![
            message_info(channel_id, 10, "around 10"),
            message_info(channel_id, 11, "around 11"),
            message_info(channel_id, 12, "around 12"),
        ],
    });
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(12)),
        Some(Id::new(100))
    );

    state.apply_event(&AppEvent::MessageHistoryAfterLoaded {
        channel_id,
        after: Id::new(12),
        messages: vec![
            message_info(channel_id, 13, "gap 13"),
            message_info(channel_id, 14, "gap 14"),
            message_info(channel_id, 15, "gap 15"),
            message_info(channel_id, 16, "gap 16"),
        ],
        has_more: true,
    });
    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![13, 14, 15, 16, 100, 101]
    );
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(16)),
        Some(Id::new(100))
    );

    state.apply_event(&AppEvent::MessageHistoryAfterLoaded {
        channel_id,
        after: Id::new(16),
        messages: vec![
            message_info(channel_id, 17, "gap 17"),
            message_info(channel_id, 100, "upper 100"),
        ],
        has_more: false,
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![14, 15, 16, 17, 100, 101]
    );
    assert_eq!(
        state.message_history_gap_after(channel_id, Id::new(17)),
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
        ..MessageCreateFixture::default()
    }));

    state.apply_event(&AppEvent::CurrentUserReactionAdd {
        channel_id,
        message_id,
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
    });
    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 1);
    assert!(message.reactions[0].me);

    state.apply_event(&AppEvent::CurrentUserReactionRemove {
        channel_id,
        message_id,
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
    });
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
        ..MessageCreateFixture::default()
    }));

    state.apply_event(&AppEvent::MessageReactionAdd {
        guild_id: None,
        channel_id,
        message_id,
        user_id: Id::new(50),
        emoji: emoji.clone(),
    });
    state.apply_event(&AppEvent::MessageReactionAdd {
        guild_id: None,
        channel_id,
        message_id,
        user_id: Id::new(51),
        emoji: emoji.clone(),
    });

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(message.reactions[0].count, 2);
    assert!(!message.reactions[0].me);

    state.apply_event(&AppEvent::MessageReactionRemove {
        guild_id: None,
        channel_id,
        message_id,
        user_id: Id::new(50),
        emoji,
    });

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
        ..MessageCreateFixture::default()
    }));

    state.apply_event(&AppEvent::CurrentUserReactionAdd {
        channel_id,
        message_id,
        emoji: emoji.clone(),
    });
    state.apply_event(&AppEvent::MessageReactionAdd {
        guild_id: None,
        channel_id,
        message_id,
        user_id: current_user_id,
        emoji: emoji.clone(),
    });
    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions[0].count, 1);
    assert!(message.reactions[0].me);

    state.apply_event(&AppEvent::MessageReactionAdd {
        guild_id: None,
        channel_id,
        message_id,
        user_id: Id::new(50),
        emoji: emoji.clone(),
    });
    state.apply_event(&AppEvent::CurrentUserReactionRemove {
        channel_id,
        message_id,
        emoji: emoji.clone(),
    });
    state.apply_event(&AppEvent::MessageReactionRemove {
        guild_id: None,
        channel_id,
        message_id,
        user_id: current_user_id,
        emoji,
    });

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

    state.apply_event(&AppEvent::MessageReactionRemoveEmoji {
        guild_id: None,
        channel_id,
        message_id,
        emoji: thumbs_up,
    });

    let message = state.messages_for_channel(channel_id)[0];
    assert_eq!(message.reactions.len(), 1);
    assert_eq!(
        message.reactions[0].emoji,
        ReactionEmoji::Unicode("🎉".to_owned())
    );

    state.apply_event(&AppEvent::MessageReactionRemoveAll {
        guild_id: None,
        channel_id,
        message_id,
    });

    assert!(
        state.messages_for_channel(channel_id)[0]
            .reactions
            .is_empty()
    );
}
