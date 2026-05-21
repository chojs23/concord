use super::*;

#[test]
fn video_attachment_does_not_reserve_image_preview_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("clip".to_owned()),
        mentions: Vec::new(),
        attachments: vec![video_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn explicit_newlines_increase_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("hello\nworld".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn wrapped_content_increases_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("abcdefghijkl".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 5, 16, 3), 5);
}

#[test]
fn message_row_content_metrics_cache_clears_on_display_option_toggle() {
    let mut state = state_with_single_message_content("<:party:1234>");
    let message = state.messages()[0];

    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.open_options_popup();
    for _ in 0..4 {
        state.move_option_down();
    }
    state.toggle_selected_display_option();

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);
}

#[test]
fn message_row_content_metrics_cache_clears_on_discord_event() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    let message = state.messages()[0];

    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.push_event(AppEvent::MessageUpdate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        poll: None,
        content: Some("updated".to_owned()),
        sticker_names: None,
        mentions: None,
        attachments: AttachmentUpdate::Unchanged,
        embeds: None,
        edited_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
    });

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);

    let message = state.messages()[0];
    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(Id::new(1)),
        profile: profile_info(99, Some("profile nickname")),
    });

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);

    let message = state.messages()[0];
    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id: Id::new(1),
            channel_id: None,
            user_id: Id::new(99),
            session_id: None,
            member: Some(MemberInfo {
                user_id: Id::new(99),
                display_name: "voice nickname".to_owned(),
                username: Some("voice-user".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }),
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        },
    });

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);
}

#[test]
fn rendered_mentions_affect_message_height() {
    let mut state = state_with_single_message_content("<@10><@10>");
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: MemberInfo {
            user_id: Id::new(10),
            display_name: "a".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    let message = state.messages()[0];

    assert_eq!(message_rendered_height(message, 5, 16, 3), 4);
    assert_eq!(state.message_base_line_count_for_width(message, 5), 2);
}

#[test]
fn forwarded_mentions_affect_height_from_source_channel_guild() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(2)),
        channel_id: Id::new(9),
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "source".to_owned(),
        kind: "GuildText".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(2),
        member: MemberInfo {
            user_id: Id::new(10),
            display_name: "a".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("<@10><@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            source_channel_id: Some(Id::new(9)),
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(state.message_base_line_count_for_width(&message, 7), 4);
}

#[test]
fn wide_content_increases_message_rendered_height_by_terminal_width() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("漢字仮名交じ".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 10, 16, 3), 4);
}

#[test]
fn discord_embed_rows_increase_message_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: vec![youtube_embed()],
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 80, 16, 3), 9);
}

#[test]
fn image_attachment_summary_reserves_text_row_before_preview() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("look".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
}

#[test]
fn five_image_album_rendered_height_lists_each_attachment_but_keeps_album_bounded() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("look".to_owned()),
        mentions: Vec::new(),
        attachments: (1..=5).map(image_attachment).collect(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 12);
}

#[test]
fn forwarded_image_attachment_reserves_preview_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![forwarded_snapshot(1)],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn forwarded_snapshot_wrapped_content_increases_rendered_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("abcdefghijkl".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 7, 16, 3), 10);
}

#[test]
fn forwarded_snapshot_wide_content_uses_terminal_width() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![MessageSnapshotInfo {
            content: Some("漢字仮名交じ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 12, 16, 3), 9);
}

#[test]
fn forwarded_metadata_reserves_card_row() {
    let mut snapshot = forwarded_snapshot(1);
    snapshot.source_channel_id = Some(Id::new(2));
    snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![snapshot],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}

#[test]
fn forwarded_snapshot_embed_rows_increase_rendered_height() {
    let mut snapshot = forwarded_snapshot(1);
    snapshot.attachments.clear();
    snapshot.embeds = vec![youtube_embed()];
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![snapshot],
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 11);
}

#[test]
fn non_default_message_kind_reserves_label_row() {
    let mut message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("reply body".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);

    message.message_kind = MessageKind::new(19);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn reply_preview_reserves_connector_row_without_extra_type_label() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::new(19),
        reference: None,
        reply: Some(ReplyInfo {
            author_id: None,
            author: "casey".to_owned(),
            content: Some("looks good".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
        }),
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some("asdf".to_owned()),
        mentions: Vec::new(),
        attachments: vec![image_attachment(1)],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn poll_message_reserves_question_and_answer_rows() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(false)),
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}

#[test]
fn poll_message_body_counts_inside_card_height() {
    let mut message = height_test_message("Please vote");
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 10);
}

#[test]
fn wrapped_poll_message_body_counts_inside_card_height() {
    let mut message = height_test_message("abcdefghijkl");
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 10, 16, 3), 11);
}

#[test]
fn thread_created_message_reserves_system_card_rows() {
    let mut message = height_test_message("release notes");
    message.message_kind = MessageKind::new(18);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
}

#[test]
fn poll_result_message_reserves_result_card_rows() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(46);
    message.poll = Some(poll_info(false));

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
}

#[test]
fn poll_result_message_counts_summed_answer_votes() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(46);
    let mut poll = poll_info(false);
    poll.total_votes = None;
    poll.answers[0].vote_count = Some(2);
    poll.answers[1].vote_count = Some(1);
    message.poll = Some(poll);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
}

#[test]
fn thread_starter_message_reserves_system_card_rows() {
    let mut message = height_test_message("");
    message.message_kind = MessageKind::new(21);
    message.reply = Some(ReplyInfo {
        author_id: None,
        author: "alice".to_owned(),
        content: Some("original topic".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
    });

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
}

#[test]
fn multiselect_poll_message_uses_same_card_height() {
    let message = MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(poll_info(true)),
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    };

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 9);
}
