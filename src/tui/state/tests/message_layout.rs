use super::*;

// Absolute row counts only mean something next to a baseline, so the two
// plain-message rows come first and every other row should be read as the
// extra rows that feature costs.
#[test]
fn message_rendered_height_reserves_rows_for_each_message_feature() {
    for (name, message, width, expected) in [
        (
            "baseline, one content line",
            height_test_message("clip"),
            200,
            3,
        ),
        (
            "baseline, empty content still reserves its row",
            height_test_message(""),
            200,
            3,
        ),
        (
            "video attachment",
            MessageState {
                attachments: vec![video_attachment(1)],
                ..height_test_message("clip")
            },
            200,
            7,
        ),
        (
            "five image album",
            MessageState {
                attachments: (1..=5).map(image_attachment).collect(),
                ..height_test_message("look")
            },
            200,
            12,
        ),
        (
            "discord embed",
            MessageState {
                embeds: vec![youtube_embed()],
                ..height_test_message("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            },
            80,
            9,
        ),
        (
            "forwarded snapshot with an image",
            MessageState {
                forwarded_snapshots: vec![forwarded_snapshot(1)],
                ..height_test_message("")
            },
            200,
            8,
        ),
        (
            "forwarded snapshot with source metadata",
            MessageState {
                forwarded_snapshots: vec![MessageSnapshotInfo {
                    source_channel_id: Some(Id::new(2)),
                    timestamp: Some("2026-04-30T12:34:56.000000+00:00".to_owned()),
                    ..forwarded_snapshot(1)
                }],
                ..height_test_message("")
            },
            200,
            9,
        ),
        (
            "forwarded snapshot with an embed",
            MessageState {
                forwarded_snapshots: vec![MessageSnapshotInfo {
                    attachments: Vec::new(),
                    embeds: vec![youtube_embed()],
                    ..forwarded_snapshot(1)
                }],
                ..height_test_message("")
            },
            200,
            11,
        ),
        (
            "thread created system card",
            MessageState {
                message_kind: MessageKind::new(18),
                ..height_test_message("release notes")
            },
            200,
            9,
        ),
        (
            "thread starter system card",
            MessageState {
                message_kind: MessageKind::new(21),
                reply: Some(ReplyInfo {
                    content: Some("original topic".to_owned()),
                    ..ReplyInfo::test("alice")
                }),
                ..height_test_message("")
            },
            200,
            4,
        ),
        (
            "poll result card",
            MessageState {
                message_kind: MessageKind::new(46),
                poll: Some(poll_info(false)),
                ..height_test_message("")
            },
            200,
            6,
        ),
    ] {
        assert_eq!(
            message_rendered_height(&message, width, 16, 3),
            expected,
            "{name}"
        );
    }
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
fn relative_timestamp_refresh_clears_message_row_content_metrics_cache() {
    let mut state = state_with_single_message_content("<t:0:R>");
    let message = state.messages()[0];

    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.clear_message_row_content_metrics_cache();

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);
}

#[test]
fn message_row_content_metrics_cache_clears_on_discord_event() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    let message = state.messages()[0];

    let _ = state.message_row_metrics_at_with_selected_bottom(0, message, 5, 16, 3, true);
    assert_eq!(state.message_row_content_metrics_cache_len(), 1);

    state.push_event(message_update_event(
        Id::new(2),
        Id::new(1),
        MessageUpdateEventFields {
            content: Some("updated".to_owned()),
            edited_timestamp: Some("2026-01-01T00:00:00Z".to_owned()),
            ..MessageUpdateEventFields::default()
        },
    ));

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
            member: Some(member_with_username(
                Id::new(99),
                "voice nickname",
                "voice-user",
            )),
            ..voice_state(Id::new(1), None, Id::new(99))
        },
    });

    assert_eq!(state.message_row_content_metrics_cache_len(), 0);
}

#[test]
fn rendered_mentions_affect_message_height() {
    let mut state = state_with_single_message_content("<@10><@10>");
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: member_info(Id::new(10), "a"),
    });
    let message = state.messages()[0];

    assert_eq!(message_rendered_height(message, 5, 16, 3), 4);
    assert_eq!(state.message_base_line_count_for_width(message, 5), 2);
}

#[test]
fn forwarded_mentions_affect_height_from_source_channel_guild() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(text_channel_info(
        Id::new(2),
        Id::new(9),
        "source",
    )));
    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(2),
        member: member_info(Id::new(10), "a"),
    });
    let mut message = height_test_message("");
    message.forwarded_snapshots = vec![MessageSnapshotInfo {
        content: Some("<@10><@10>".to_owned()),
        source_channel_id: Some(Id::new(9)),
        ..MessageSnapshotInfo::test()
    }];

    assert_eq!(state.message_base_line_count_for_width(&message, 7), 4);
}

#[test]
fn non_default_message_kind_reserves_label_row() {
    let mut message = height_test_message("reply body");
    message.attachments = vec![image_attachment(1)];

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);

    message.message_kind = MessageKind::new(19);

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn reply_preview_reserves_connector_row_without_extra_type_label() {
    let mut message = height_test_message("asdf");
    message.message_kind = MessageKind::new(19);
    message.reply = Some(ReplyInfo {
        content: Some("looks good".to_owned()),
        ..ReplyInfo::test("casey")
    });
    message.attachments = vec![image_attachment(1)];

    assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
}

#[test]
fn message_height_grows_with_every_kind_of_extra_row() {
    let cases = [
        ("hello\nworld", 200, 4),
        ("abcdefghijkl", 5, 5),
        ("漢字仮名交じ", 10, 4),
    ];

    for (content, width, expected) in cases {
        let message = height_test_message(content);
        assert_eq!(
            message_rendered_height(&message, width, 16, 3),
            expected,
            "{content}"
        );
    }
}

#[test]
fn poll_card_height_covers_the_question_answers_and_wrapped_body() {
    let cases = [
        ("", 200, 9),
        ("Please vote", 200, 10),
        ("abcdefghijkl", 10, 11),
    ];

    for (content, width, expected) in cases {
        let mut message = height_test_message(content);
        message.poll = Some(poll_info(false));
        assert_eq!(
            message_rendered_height(&message, width, 16, 3),
            expected,
            "{content}"
        );
    }
}

#[test]
fn forwarded_snapshot_body_height_follows_the_content_width() {
    let cases = [("abcdefghijkl", 7, 10), ("漢字仮名交じ", 12, 9)];

    for (content, width, expected) in cases {
        let mut message = height_test_message("");
        message.forwarded_snapshots = vec![MessageSnapshotInfo {
            content: Some(content.to_owned()),
            attachments: vec![image_attachment(1)],
            ..MessageSnapshotInfo::test()
        }];

        assert_eq!(
            message_rendered_height(&message, width, 16, 3),
            expected,
            "{content}"
        );
    }
}
