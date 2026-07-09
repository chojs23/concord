use super::*;
use crate::discord::test_builders::{
    GuildCreateFixture, MessageHistoryLoadedFixture, guild_create_event,
    message_history_loaded_event,
};

#[test]
fn channel_unread_decoration_matches_unread_state() {
    let base = Style::default().fg(Color::White);
    let cases = [
        (
            ChannelUnreadState::Seen,
            None,
            Some(theme::current().read_dim),
            false,
        ),
        (
            ChannelUnreadState::Unread,
            None,
            Some(theme::current().unread_bright),
            true,
        ),
        (
            ChannelUnreadState::Mentioned(3),
            Some(("(3) ", theme::current().mention)),
            Some(theme::current().mention),
            true,
        ),
        (
            ChannelUnreadState::Notified(3),
            Some(("(3) ", theme::current().unread_bright)),
            Some(theme::current().unread_bright),
            true,
        ),
    ];

    for (unread, expected_badge, expected_fg, expect_bold) in cases {
        let (badge, style) = channel_unread_decoration(unread, base, false);
        match expected_badge {
            Some((content, color)) => {
                let badge = badge.expect("unread state should include a count badge");
                assert_eq!(badge.content.as_ref(), content);
                assert_eq!(badge.style.fg, Some(color));
                assert!(badge.style.add_modifier.contains(Modifier::BOLD));
            }
            None => assert!(badge.is_none()),
        }
        assert_eq!(style.fg, expected_fg);
        assert_eq!(style.add_modifier.contains(Modifier::BOLD), expect_bold);
        if unread == ChannelUnreadState::Seen {
            assert!(!style.add_modifier.contains(Modifier::DIM));
        }
    }

    let active_base = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let (badge, style) =
        channel_unread_decoration(ChannelUnreadState::Mentioned(2), active_base, true);
    assert!(badge.is_none());
    assert_eq!(style, active_base);
}

#[test]
fn later_history_does_not_clear_loaded_pin_state() {
    let mut state = state_with_message();
    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(10, "mod", "important announcement", true)],
    });

    assert!(
        state
            .messages()
            .into_iter()
            .all(|message| message.id != Id::new(10))
    );

    state.push_event(message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id: Id::new(2),
        messages: vec![message_info(10, "mod", "important announcement", false)],
        ..MessageHistoryLoadedFixture::new()
    }));

    state.enter_pinned_message_view(Id::new(2));
    assert_eq!(state.messages().len(), 1);
    assert!(state.return_from_pinned_message_view());
    assert!(
        state
            .messages()
            .into_iter()
            .any(|message| message.id == Id::new(10) && message.pinned)
    );
}

#[test]
fn primary_activity_summary_prefers_game_over_custom_status() {
    let activities = vec![
        ActivityInfo::test(ActivityKind::Playing, "Concord"),
        ActivityInfo {
            state: Some("Coding hard".to_owned()),
            emoji: Some(ActivityEmoji {
                name: "🦀".to_owned(),
                id: None,
                animated: false,
            }),
            ..ActivityInfo::test(ActivityKind::Custom, "Custom Status")
        },
    ];

    assert_eq!(
        primary_activity_summary(&activities, &[]).map(|r| r.to_display_string()),
        Some("▶ Concord".to_owned())
    );
}

#[test]
fn primary_activity_summary_listening_includes_track_and_artist() {
    let activities = vec![ActivityInfo {
        details: Some("Bohemian Rhapsody".to_owned()),
        state: Some("Queen".to_owned()),
        ..ActivityInfo::test(ActivityKind::Listening, "Spotify")
    }];
    assert_eq!(
        primary_activity_summary(&activities, &[]).map(|r| r.to_display_string()),
        Some("♪ Spotify - Bohemian Rhapsody by Queen".to_owned())
    );
}

#[test]
fn primary_activity_summary_sanitizes_custom_status_emoji() {
    let activities = vec![ActivityInfo {
        state: Some("curse of rah".to_owned()),
        emoji: Some(ActivityEmoji {
            name: "⚜".to_owned(),
            id: None,
            animated: false,
        }),
        ..ActivityInfo::test(ActivityKind::Custom, "Custom Status")
    }];

    assert_eq!(
        primary_activity_summary(&activities, &[]).map(|render| render.to_display_string()),
        Some("? curse of rah".to_owned())
    );
}

#[test]
fn offline_like_dm_status_uses_empty_dim_presence_marker() {
    for status in [PresenceStatus::Offline, PresenceStatus::Unknown] {
        let channel = channel_with_recipients("dm", &[status]);

        let dot = dm_presence_dot_span(&channel).expect("DM should still produce a dot");
        assert_eq!(dot.content.as_ref(), "○ ");
        assert_eq!(dot.style.fg, Some(Color::DarkGray));
    }
}

#[test]
fn wrapped_edited_marker_keeps_dim_italic_style() {
    let mut message = message_with_content(Some("hello".to_owned()));
    message.edited_timestamp = Some("2026-05-07T12:34:56.000000+00:00".to_owned());

    let lines = format_message_content_lines(&message, &DashboardState::new(), 5);

    assert_eq!(line_texts(&lines), vec!["hello", "(edited)"]);
    let marker = lines[1]
        .spans()
        .into_iter()
        .next()
        .expect("wrapped edited marker span should be present");
    assert_eq!(marker.style.fg, Some(theme::current().dim));
    assert!(marker.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn embed_text_emits_inline_emoji_slot_for_image_overlay() {
    let mut message = message_with_content(Some("see embed".to_owned()));
    let mut embed = youtube_embed();
    embed.title = Some("look <:party:99>!".to_owned());
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);
    let slots: Vec<_> = lines
        .iter()
        .flat_map(|line| line.image_slots.iter())
        .collect();

    assert!(!slots.is_empty());
    assert!(
        slots
            .iter()
            .any(|slot| slot.url == "https://cdn.discordapp.com/emojis/99.png")
    );
}

#[test]
fn non_default_message_type_adds_dim_label_line() {
    let mut message = message_with_attachment(Some("reply body".to_owned()), image_attachment());
    message.message_kind = MessageKind::new(19);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec!["↳ Reply", "reply body", "[image: cat.png] 640x480"]
    );
    assert_eq!(lines[0].style, Style::default().fg(theme::current().dim));
}

#[test]
fn chat_input_command_message_keeps_embed_text() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let user_id = Id::new(30);
    let role_id = Id::new(100);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: vec![MemberInfo {
            username: Some("casey".to_owned()),
            role_ids: vec![role_id],
            ..MemberInfo::test(user_id, "casey")
        }],
        roles: vec![RoleInfo {
            color: Some(0x3366CC),
            position: 10,
            ..RoleInfo::test(role_id, "Blue")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(20);
    message.interaction = Some(MessageInteractionInfo {
        user_id: Some(user_id),
        command_name: Some("anime search".to_owned()),
        ..MessageInteractionInfo::test("casey")
    });
    message.embeds = vec![youtube_embed()];

    let lines = format_message_content_lines(&message, &state, 80);

    assert_eq!(
        line_texts(&lines),
        vec![
            "┌ casey used /anime search",
            "  ▎ YouTube",
            "  ▎ Example Video",
            "  ▎ A video description",
            "  ▎ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ]
    );
    assert_eq!(lines[0].style, Style::default().fg(theme::current().dim));
    let spans = lines[0].spans();

    assert_eq!(spans[0].content.as_ref(), "┌ ");
    assert_eq!(spans[1].content.as_ref(), "casey");
    assert_eq!(spans[1].style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
    assert!(spans[1].style.add_modifier.contains(Modifier::DIM));
    assert_eq!(spans[2].content.as_ref(), " used ");
    assert_eq!(spans[2].style.fg, Some(theme::current().dim));
    assert_eq!(spans[3].content.as_ref(), "/anime search");
    assert_eq!(spans[3].style.fg, Some(Color::Rgb(88, 101, 242)));
    assert!(spans[3].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn user_join_message_type_uses_join_label() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(7);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["joined the server"]);
    assert_eq!(lines[0].style, Style::default().fg(theme::current().dim));
}

#[test]
fn boost_message_types_use_discord_like_copy() {
    for (kind, label) in [
        (8, "neo boosted the server"),
        (9, "neo boosted the server to Level 1"),
        (10, "neo boosted the server to Level 2"),
        (11, "neo boosted the server to Level 3"),
    ] {
        let mut message = message_with_content(Some(String::new()));
        message.message_kind = MessageKind::new(kind);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec![label]);
        assert_eq!(lines[0].style, Style::default().fg(theme::current().accent));
    }
}

#[test]
fn poll_result_message_uses_result_card() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(46);
    message.poll = Some(PollInfo {
        answers: vec![PollAnswerInfo {
            vote_count: Some(5),
            ..PollAnswerInfo::test(1, "Soup")
        }],
        results_finalized: Some(true),
        total_votes: Some(7),
        ..PollInfo::test("What should we eat?")
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec![
            "Poll results",
            "What should we eat?",
            "Winner: Soup with 5 votes",
            "7 total votes · Final results"
        ]
    );
}

#[test]
fn reply_message_uses_preview_instead_of_type_label() {
    let mut message = message_with_attachment(Some("message body".to_owned()), image_attachment());
    message.message_kind = MessageKind::new(19);
    message.reply = Some(ReplyInfo {
        content: Some("looks good".to_owned()),
        ..ReplyInfo::test("casey")
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec![
            "╭─ casey : looks good",
            "message body",
            "[image: cat.png] 640x480"
        ]
    );
    assert_eq!(lines[0].style, Style::default().fg(theme::current().dim));
}

#[test]
fn unsupported_message_type_uses_placeholder() {
    let mut message = message_with_attachment(Some("body".to_owned()), image_attachment());
    message.message_kind = MessageKind::new(255);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(lines[0].text, "<unsupported message type>");
}

#[test]
fn poll_message_replaces_empty_message_placeholder() {
    let mut message = message_with_content(Some(String::new()));
    message.poll = Some(poll_info(false));

    let width = 40;
    let lines = format_message_content_lines(&message, &DashboardState::new(), width);
    let texts = line_texts(&lines);

    assert_eq!(texts[0], poll_box_border('╭', '╮', width));
    assert_eq!(texts[1], poll_test_line("What should we eat?", width));
    assert_eq!(texts[2], poll_test_line("Select one answer", width));
    assert_eq!(texts[3], poll_test_line("  ◉ 1. Soup  2 votes  66%", width));
    assert_eq!(
        texts[4],
        poll_test_line("  ◯ 2. Noodles  1 votes  33%", width)
    );
    assert_eq!(
        texts[5],
        poll_test_line("3 votes · Results may still change", width)
    );
    assert_eq!(texts[6], poll_box_border('╰', '╯', width));
}

#[test]
fn poll_message_notes_multiselect() {
    let mut message = message_with_content(Some(String::new()));
    message.poll = Some(poll_info(true));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert!(lines[2].text.starts_with("│ Select one or more answers"));
    assert_eq!(lines[2].style, Style::default().fg(theme::current().dim));
}

#[test]
fn poll_message_places_body_inside_box() {
    let mut message = message_with_content(Some("Please vote <@10>".to_owned()));
    message.poll = Some(poll_info(false));
    let state = state_with_member(10, "alice");

    let lines = format_message_content_lines(&message, &state, 40);

    assert_eq!(lines[1].text, poll_test_line("What should we eat?", 40));
    assert_eq!(lines[2].text, poll_test_line("Please vote @alice", 40));
    assert!(lines[3].text.starts_with("│ Select one answer"));
}

#[test]
fn lay_out_reaction_chips_unicode_only_emits_no_image_slots() {
    let reactions = vec![
        ReactionInfo {
            count: 3,
            me: true,
            ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
        },
        ReactionInfo::test(ReactionEmoji::Unicode("❤".to_owned())),
    ];

    let layout = lay_out_reaction_chips(&reactions, 200);

    assert_eq!(layout.lines, vec!["[👍 3]  [❤ 1]"]);
    assert_eq!(layout.self_ranges.len(), 1);
    let spans = reaction_line_test_spans(&layout.lines[0], &layout.self_ranges, 0);
    assert_eq!(spans[0].content.as_ref(), "[👍 3]");
    assert_eq!(spans[0].style, Style::default().fg(Color::Yellow));
    assert_eq!(spans[1].style, Style::default().fg(theme::current().accent));
    assert!(layout.slots.is_empty());
}

#[test]
fn lay_out_reaction_chips_custom_emoji_reserves_image_slot() {
    let reactions = vec![
        ReactionInfo {
            count: 2,
            ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
        },
        ReactionInfo {
            me: true,
            ..ReactionInfo::test(ReactionEmoji::Custom {
                id: Id::new(42),
                name: Some("party".to_owned()),
                animated: false,
            })
        },
    ];

    let layout = lay_out_reaction_chips(&reactions, 200);

    // First line concatenates both chips with two spaces. The custom-emoji
    // chip reserves two cells of spaces in place of the textual `:name:`.
    assert_eq!(layout.lines, vec!["[👍 2]  [   1]"]);
    assert_eq!(layout.self_ranges.len(), 1);
    assert_eq!(layout.slots.len(), 1);
    let slot = &layout.slots[0];
    assert_eq!(slot.line, 0);
    // "[👍 2]" is 6 cells, plus "  " separator = 8 cells of preceding text.
    // Inside the chip "[" is 1 cell, so the image starts at col 8 + 1 = 9.
    assert_eq!(slot.col, 9);
    assert!(slot.url.contains("42.png"));
}

#[test]
fn lay_out_reaction_chips_wraps_at_chip_boundary() {
    let reactions = (0..3)
        .map(|i| ReactionInfo {
            count: i + 1,
            ..ReactionInfo::test(ReactionEmoji::Custom {
                id: Id::new(100 + i),
                name: Some(format!("e{i}")),
                animated: false,
            })
        })
        .collect::<Vec<_>>();

    // Each chip width: "[" + 2 placeholder spaces + " " + count + "]" = 6.
    // Two chips with separator = 6 + 2 + 6 = 14. Three would be 14 + 2 + 6 = 22.
    let layout = lay_out_reaction_chips(&reactions, 14);

    assert_eq!(layout.lines.len(), 2);
    // First two chips on line 0, third chip on line 1.
    assert_eq!(layout.slots.len(), 3);
    assert_eq!(layout.slots[0].line, 0);
    assert_eq!(layout.slots[1].line, 0);
    assert_eq!(layout.slots[2].line, 1);
    // Third chip starts at col 0 of the wrapped second line, image at col 1.
    assert_eq!(layout.slots[2].col, 1);
}

#[test]
fn forwarded_snapshot_replaces_empty_message_placeholder() {
    let message =
        message_with_forwarded_snapshot(forwarded_snapshot(Some("forwarded text"), Vec::new()));

    assert_eq!(
        format_message_content(&message, 200),
        "↱ Forwarded │ forwarded text"
    );
}

#[test]
fn forwarded_snapshot_content_wraps_after_prefix() {
    let message =
        message_with_forwarded_snapshot(forwarded_snapshot(Some("abcdefghijkl"), Vec::new()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 7);

    assert_eq!(
        line_texts(&lines),
        vec!["↱ Forwarded", "│ abcde", "│ fghij", "│ kl"]
    );
}

#[test]
fn forwarded_snapshot_content_wraps_wide_characters_after_prefix() {
    let message =
        message_with_forwarded_snapshot(forwarded_snapshot(Some("漢字仮名交じ"), Vec::new()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 12);

    assert_eq!(
        line_texts(&lines),
        vec!["↱ Forwarded", "│ 漢字仮名交", "│ じ"]
    );
}

#[test]
fn forwarded_snapshot_lines_include_channel_and_time() {
    let mut state = DashboardState::new();
    state.push_event(crate::discord::AppEvent::ChannelUpsert(
        crate::discord::ChannelInfo {
            guild_id: Some(Id::new(1)),
            name: "general".to_owned(),
            ..crate::discord::ChannelInfo::test(Id::new(9), "GuildText")
        },
    ));
    let mut snapshot = forwarded_snapshot(Some("hello"), Vec::new());
    snapshot.source_channel_id = Some(Id::new(9));
    snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
    let message = message_with_forwarded_snapshot(snapshot);

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(
        line_texts(&lines),
        vec!["↱ Forwarded", "│ hello", "│ #general · 12:34"]
    );
    assert_eq!(lines[2].style, Style::default().fg(theme::current().dim));
}

#[test]
fn forwarded_snapshot_renders_discord_embed_preview() {
    let mut snapshot = forwarded_snapshot(
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        Vec::new(),
    );
    snapshot.embeds = vec![youtube_embed()];
    let message = message_with_forwarded_snapshot(snapshot);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

    assert_eq!(
        line_texts(&lines),
        vec![
            "↱ Forwarded",
            "│ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "│   ▎ YouTube",
            "│   ▎ Example Video",
            "│   ▎ A video description",
        ]
    );
    let url_spans = lines[2].spans();
    assert_eq!(url_spans[0].content.as_ref(), "│ ");
    assert!(
        !url_spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
    assert_eq!(url_spans[1].content.as_ref(), "  ▎ ");
    assert_eq!(url_spans[1].style.fg, Some(Color::Rgb(255, 0, 0)));
    assert!(
        !url_spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn selected_grouped_continuation_stamps_time_on_border() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "follow-up");
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        Some(1),
        &state,
        super::default_message_viewport_layout(),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    let sent_time = format_message_sent_time(Id::new(2));
    assert!(texts[3].starts_with("╭"));
    assert!(texts[4].starts_with("│ "));
    assert!(texts[4].contains("follow-up"));
    assert!(!texts[4].contains(&sent_time));

    let border = texts
        .iter()
        .find(|line| line.starts_with("╰"))
        .expect("selected card bottom border");
    assert!(border.contains(&sent_time));
    assert!(border.ends_with("─╯"));
}

#[test]
fn selected_multiline_continuation_keeps_time_off_content_lines() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "alpha bravo charlie delta echo foxtrot golf");
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        Some(1),
        &state,
        super::selected_message_viewport_layout(20),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    let sent_time = format_message_sent_time(Id::new(2));
    let content_lines: Vec<&String> = texts.iter().filter(|line| line.starts_with("│ ")).collect();

    assert!(
        content_lines.len() >= 2,
        "content should wrap onto multiple lines"
    );
    assert!(content_lines.iter().all(|line| !line.contains(&sent_time)));

    let border = texts
        .iter()
        .find(|line| line.starts_with("╰"))
        .expect("selected card bottom border");
    assert!(border.contains(&sent_time));
    assert!(border.ends_with("─╯"));
}

#[test]
fn avatars_off_collapses_message_gutter() {
    let display = DisplayOptions {
        show_avatars: false,
        ..DisplayOptions::default()
    };
    let state = seed_channel_message(
        DashboardState::new_with_display_options(display),
        Id::new(1),
        "hello",
    );
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::default_message_viewport_layout(),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert!(!texts.iter().any(|line| line.contains("oooo")));
    let body = texts
        .iter()
        .find(|line| line.contains("hello"))
        .expect("body line renders");
    assert!(body.starts_with("  h"));
}

#[test]
fn grouped_continuation_custom_emoji_image_uses_body_row() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "<:long_custom:42>text");
    state.jump_top();
    let messages = state.messages();
    let rows = message_body_custom_emoji_rows(
        &messages,
        &state,
        200,
        None,
        &["https://cdn.discordapp.com/emojis/42.png".to_owned()],
        16,
        3,
    );

    assert_eq!(rows, vec![3]);
}

#[test]
fn shared_truncation_uses_display_width_for_wide_characters() {
    let author = truncate_display_width("漢字仮名交じり", 8);

    assert_eq!(author, "漢字...");
    assert_eq!(author.width(), 7);
}

#[test]
fn member_label_truncates_by_display_width() {
    let member = GuildMemberState {
        status: PresenceStatus::Online,
        ..GuildMemberState::test(Id::new(10), "漢字仮名交じり文章")
    };

    let label = member_display_label(MemberEntry::Guild(&member), &member.display_name, 0, 12);

    assert_eq!(label, "漢字仮名...");
    assert!(label.width() <= 12);
}

#[test]
fn member_label_sanitizes_ambiguous_width_emoji_before_truncating() {
    let member = GuildMemberState {
        status: PresenceStatus::Online,
        ..GuildMemberState::test(Id::new(10), "user ⚜ status")
    };

    let label = member_display_label(MemberEntry::Guild(&member), &member.display_name, 0, 12);

    assert_eq!(label, "user ? st...");
    assert!(label.width() <= 12);
}

#[test]
fn horizontal_truncation_skips_display_width_offset() {
    let label = truncate_display_width_from("abcdef", 2, 4);

    assert_eq!(label, "cdef");
}

#[test]
fn horizontal_truncation_respects_wide_character_boundaries() {
    let label = truncate_display_width_from("가나다abc", 2, 6);

    assert_eq!(label, "나...");
    assert!(label.width() <= 6);
}

#[test]
fn member_label_uses_horizontal_scroll_offset() {
    let member = GuildMemberState {
        status: PresenceStatus::Online,
        ..GuildMemberState::test(Id::new(10), "long-member-name")
    };

    let label = member_display_label(MemberEntry::Guild(&member), &member.display_name, 5, 8);

    assert_eq!(label, "membe...");
}

#[test]
fn channel_label_truncates_by_display_width_after_prefixes() {
    let branch_prefix = "├ ";
    let channel_prefix = "# ";
    let max_width = 14usize;
    let label_width = max_width
        .saturating_sub(branch_prefix.width())
        .saturating_sub(channel_prefix.width());
    let label = truncate_display_width("漢字仮名交じり", label_width);

    assert_eq!(label, "漢字仮...");
    assert!(branch_prefix.width() + channel_prefix.width() + label.width() <= max_width);
}

#[test]
fn offline_member_name_keeps_role_color_and_dims() {
    let member = GuildMemberState::test(Id::new(10), "neo");

    let style = member_name_style(MemberEntry::Guild(&member), Some(0x3366CC), false);

    assert_eq!(style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn no_role_member_name_stays_white_for_online_like_statuses() {
    for status in [
        PresenceStatus::Online,
        PresenceStatus::Idle,
        PresenceStatus::DoNotDisturb,
    ] {
        let member = GuildMemberState {
            status,
            ..GuildMemberState::test(Id::new(10), "neo")
        };

        let style = member_name_style(MemberEntry::Guild(&member), None, false);

        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::DIM));
    }
}

#[test]
fn no_role_offline_member_name_is_white_and_dimmed() {
    let member = GuildMemberState::test(Id::new(10), "neo");

    let style = member_name_style(MemberEntry::Guild(&member), None, false);

    assert_eq!(style.fg, Some(Color::White));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn selected_bot_member_name_preserves_role_color_and_selection_style() {
    let member = GuildMemberState {
        is_bot: true,
        status: PresenceStatus::Online,
        ..GuildMemberState::test(Id::new(10), "bot")
    };

    let style = member_name_style(MemberEntry::Guild(&member), Some(0x3366CC), true);

    assert_eq!(style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
    assert_eq!(style.bg, Some(Color::Rgb(24, 54, 65)));
    assert!(style.add_modifier.contains(Modifier::BOLD));
    assert!(style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn message_sent_time_formats_with_timezone_offset() {
    let kst = chrono::FixedOffset::east_opt(9 * 60 * 60).expect("KST offset should be valid");

    assert_eq!(
        format_unix_millis_with_offset(discord_epoch_unix_millis(), kst),
        Some("09:00".to_owned())
    );
}

#[test]
fn selected_message_media_moves_inside_border() {
    let message = message_with_content(Some("a".repeat(73)));
    let messages = [&message];

    let unselected = line_texts_from_ratatui(&message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::selected_message_viewport_layout(40),
        &[],
    ));
    let selected = line_texts_from_ratatui(&message_viewport_lines(
        &messages,
        Some(0),
        &DashboardState::new(),
        super::selected_message_viewport_layout(40),
        &[],
    ));

    assert_eq!(selected_message_content_x_offset(true), 0);
    let selected_content_col = selected[1]
        .split('a')
        .next()
        .expect("selected line contains content")
        .width();
    let unselected_content_col = unselected[1]
        .split('a')
        .next()
        .expect("unselected line contains content")
        .width();
    assert_eq!(selected_content_col, unselected_content_col);
}

#[test]
fn second_inline_preview_slot_uses_album_column_offset() {
    let area = Rect::new(10, 5, 80, 18);
    let mut message = message_with_attachment(Some("one".to_owned()), image_attachment());
    let mut second = image_attachment();
    second.id = Id::new(4);
    second.filename = "dog.png".to_owned();
    second.url = "https://cdn.discordapp.com/dog.png".to_owned();
    second.proxy_url = "https://media.discordapp.net/dog.png".to_owned();
    message.attachments.push(second);
    let messages = [&message];
    let state = DashboardState::new();
    let row = inline_image_preview_row(&messages, &state, 0, 200, 0, 0);

    assert_eq!(row, 3);
    assert_eq!(
        inline_image_preview_area(area, row, 8, 8, 3, None, MESSAGE_AVATAR_OFFSET),
        Some(Rect::new(26, 9, 8, 3))
    );
}

#[test]
fn forwarded_card_rows_push_inline_preview_slot_down() {
    let mut snapshot = forwarded_snapshot(Some("hello"), vec![image_attachment()]);
    snapshot.source_channel_id = Some(Id::new(9));
    snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
    let message = message_with_forwarded_snapshot(snapshot);
    let messages = [&message];
    let state = DashboardState::new();

    assert_eq!(inline_image_preview_row(&messages, &state, 0, 200, 0, 0), 4);
}
