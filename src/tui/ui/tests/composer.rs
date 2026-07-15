use super::*;
use crate::discord::MessageVerificationRestriction;

#[test]
fn verification_composer_text_explains_each_restriction() {
    let cases = [
        (
            MessageVerificationRestriction::MembershipScreening,
            "membership screening",
        ),
        (
            MessageVerificationRestriction::EmailVerificationRequired,
            "account email",
        ),
        (
            MessageVerificationRestriction::AccountTooNew {
                remaining_seconds: 30,
            },
            "account verification wait: 30s",
        ),
        (
            MessageVerificationRestriction::MemberTooNew {
                remaining_seconds: 60,
            },
            "server verification wait: 60s",
        ),
        (
            MessageVerificationRestriction::PhoneVerificationRequired,
            "account phone",
        ),
    ];

    for (restriction, expected) in cases {
        assert!(verification_composer_text("#general", restriction).contains(expected));
    }
}

#[test]
fn sync_view_heights_reserves_space_for_composer_height() {
    enum ExpectedHeight {
        Exact(usize),
        LessThan(usize),
    }

    let cases = [
        (String::new(), ExpectedHeight::Exact(14)),
        ("a\nb\nc".to_owned(), ExpectedHeight::Exact(12)),
        ("x".repeat(100), ExpectedHeight::LessThan(15)),
    ];

    for (input, expected) in cases {
        let mut state = DashboardState::new();
        for ch in input.chars() {
            state.push_composer_char(ch);
        }

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

        match expected {
            ExpectedHeight::Exact(height) => assert_eq!(state.message_view_height(), height),
            ExpectedHeight::LessThan(height) => assert!(state.message_view_height() < height),
        }
    }
}

#[test]
fn composer_prompt_line_count_uses_display_width_for_wide_chars() {
    assert_eq!(composer_prompt_line_count("漢字仮", 4), 2);
}

#[test]
fn composer_prompt_line_count_matches_prefixed_multiline_rendering() {
    let mut state = state_with_message();
    state.start_composer();
    for ch in "a\nbbbb".chars() {
        state.push_composer_char(ch);
    }

    let rendered = line_texts_from_ratatui(&composer_lines(&state, 5));

    assert_eq!(rendered, vec!["> a", "  bbb", "b"]);
    assert_eq!(composer_prompt_line_count(state.composer_input(), 5), 3);
    assert_eq!(composer_content_line_count(&state, 5), 3);
}

#[test]
fn composer_lines_show_saved_draft_when_not_composing() {
    let mut state = state_with_message();
    state.start_composer();
    for ch in "draft".chars() {
        state.push_composer_char(ch);
    }

    state.close_composer();

    assert_eq!(composer_text(&state, 80), "> draft");
    assert_eq!(
        line_texts_from_ratatui(&composer_lines(&state, 80)),
        vec!["> draft"]
    );
}

#[test]
fn composer_locks_use_the_same_red_hint_for_dm_and_server_channels() {
    let mut server = DashboardState::new();
    server.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            name: "general".to_owned(),
            ..ChannelInfo::test(Id::new(2), "GuildText")
        }],
        ..GuildCreateFixture::new(Id::new(1))
    }));
    server.confirm_selected_guild();
    server.confirm_selected_channel();
    server.push_event(empty_latest_message_history_loaded_event(Id::new(2)));

    let mut dm = DashboardState::new();
    dm.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        name: "alice".to_owned(),
        ..ChannelInfo::test(Id::new(20), "dm")
    }));
    dm.confirm_selected_guild();
    dm.confirm_selected_channel();
    dm.push_event(empty_latest_message_history_loaded_event(Id::new(20)));

    for state in [&server, &dm] {
        assert!(state.composer_lock().is_some());
        let lines = composer_lines(state, 120);
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.fg == Some(Color::Red))
        );
    }
}

#[test]
fn message_history_statuses_override_a_saved_draft() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(200)),
        name: "alice".to_owned(),
        ..ChannelInfo::test(Id::new(20), "dm")
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for ch in "draft".chars() {
        state.push_composer_char(ch);
    }

    assert_eq!(state.composer_lock(), Some(ComposerLock::LoadingMessages));
    let lines = composer_lines(&state, 120);
    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["loading messages in @alice..."]
    );
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg != Some(Color::Red))
    );

    state.push_event(AppEvent::MessageHistoryLoadFailed {
        channel_id: Id::new(20),
        target: crate::discord::MessageHistoryLoadTarget::Latest,
        message: "offline".to_owned(),
    });
    assert_eq!(state.composer_lock(), Some(ComposerLock::MessageLoadFailed));
    let lines = composer_lines(&state, 120);
    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["read-only · could not load messages in @alice. reopen it to retry"]
    );
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg == Some(Color::Red))
    );
}

#[test]
fn reply_composer_text_uses_original_reply_target_after_selection_changes() {
    let mut state = state_with_message();
    state.direct_reply_to_selected_message();
    push_message(&mut state, 2, "newer selected message");

    assert_eq!(
        state
            .selected_message_state()
            .and_then(|message| message.content.as_deref()),
        Some("newer selected message")
    );

    assert_eq!(composer_text(&state, 80), "reply to hello  @ on\n> ");
}

#[test]
fn reply_composer_hint_line_shows_dim_excerpt_and_semantic_ping_indicator() {
    let mut state = state_with_message();
    state.direct_reply_to_selected_message();

    let lines = composer_lines(&state, 80);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["reply to hello  @ on", "> "]
    );
    assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
    assert_eq!(
        lines[0].spans.last().unwrap().style.fg,
        theme::current()
            .style(theme::HighlightGroup::ReplyPingEnabled)
            .fg
    );
    assert_eq!(lines[1].spans[0].style.fg, None);

    state.toggle_ping_on_reply();
    let lines = composer_lines(&state, 80);
    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["reply to hello  @ off", "> "]
    );
    assert!(
        lines[0]
            .spans
            .last()
            .unwrap()
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn composer_border_title_tracks_message_mode() {
    let mut normal = state_with_message();
    normal.start_composer();
    let normal_rendered = render_dashboard_dump(80, 16, &mut normal).join("\n");

    let mut reply = state_with_message();
    reply.direct_reply_to_selected_message();
    let reply_rendered = render_dashboard_dump(80, 16, &mut reply).join("\n");

    let mut edit = state_with_message();
    edit.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    edit.direct_edit_selected_message();
    let edit_rendered = render_dashboard_dump(80, 16, &mut edit).join("\n");

    assert!(
        normal_rendered.contains("Message Input"),
        "{normal_rendered}"
    );
    assert!(reply_rendered.contains("Reply"), "{reply_rendered}");
    assert!(edit_rendered.contains("Edit Message"), "{edit_rendered}");

    let inactive = state_with_message();
    let custom = theme::Theme::default()
        .with_border_type(theme::BorderSurface::Composer, BorderType::Double)
        .with_style(
            theme::HighlightGroup::ComposerBorder,
            Style::default().fg(Color::Red),
        )
        .with_style(
            theme::HighlightGroup::ActiveComposerBorder,
            Style::default().fg(Color::Green),
        );
    theme::with_test_theme(custom, || {
        for (state, expected) in [(&normal, Color::Green), (&inactive, Color::Red)] {
            let backend = TestBackend::new(40, 4);
            let mut terminal = Terminal::new(backend).expect("test terminal should build");
            terminal
                .draw(|frame| render_composer(frame, frame.area(), state, &[]))
                .expect("composer should render");

            assert_eq!(terminal.backend().buffer()[(0, 0)].fg, expected);
            assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "╔");
        }
    });
}

#[test]
fn composer_lines_show_pending_upload_rows_above_input() {
    let mut state = state_with_message();
    state.start_composer();
    state.add_pending_composer_attachments(vec![MessageAttachmentUpload::from_path(
        "/tmp/cat.png".into(),
        "cat.png".to_owned(),
        2_048,
    )]);

    let lines = composer_lines(&state, 80);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "upload: cat.png (2.0 KiB)",
            "────────────────────────────────────────────────────────────────────────────────",
            "",
            "",
            "",
            "",
            "",
            "",
            "────────────────────────────────────────────────────────────────────────────────",
            "> ",
        ]
    );
    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Yellow));
    assert_eq!(composer_content_line_count(&state, 80), 10);

    let mut processing = state_with_message();
    processing.start_composer();

    assert!(processing.begin_clipboard_paste());

    let processing_lines = composer_lines(&processing, 80);

    assert_eq!(
        line_texts_from_ratatui(&processing_lines),
        vec!["upload: ⠋ processing clipboard attachment...", "> "]
    );
    assert_eq!(processing_lines[0].spans[0].style.fg, Some(Color::Yellow));
    assert_eq!(composer_content_line_count(&processing, 80), 2);
}

#[test]
fn composer_lines_use_image_width_for_loaded_custom_emoji() {
    let mut state = state_with_message();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo::test(Id::new(60), "long_custom")],
    });
    state.start_composer();
    for ch in ":lo".chars() {
        state.push_composer_char(ch);
    }
    assert!(state.confirm_composer_emoji());
    for ch in "text".chars() {
        state.push_composer_char(ch);
    }

    let loading_lines = composer_lines_with_loaded_custom_emoji_urls(&state, 80, &[]);
    let loaded_lines = composer_lines_with_loaded_custom_emoji_urls(
        &state,
        80,
        &["https://cdn.discordapp.com/emojis/60.png".to_owned()],
    );

    assert_eq!(
        line_texts_from_ratatui(&loading_lines),
        vec!["> :long_custom: text"]
    );
    assert_eq!(line_texts_from_ratatui(&loaded_lines), vec![">    text"]);
}

#[test]
fn composer_cursor_position_tracks_input_cursor() {
    let mut state = state_with_message();
    state.start_composer();
    for value in "hello".chars() {
        state.push_composer_char(value);
    }
    state.move_composer_cursor_left();
    state.move_composer_cursor_left();

    assert_eq!(
        composer_cursor_position(Rect::new(10, 20, 20, 5), &state),
        Some(Position { x: 16, y: 21 })
    );
}

#[test]
fn composer_cursor_position_accounts_for_upload_and_reply_rows() {
    let mut state = state_with_message();
    state.direct_reply_to_selected_message();
    state.add_pending_composer_attachments(vec![MessageAttachmentUpload::from_path(
        "/tmp/cat.png".into(),
        "cat.png".to_owned(),
        2_048,
    )]);
    for value in "hi".chars() {
        state.push_composer_char(value);
    }

    assert_eq!(
        composer_cursor_position(Rect::new(10, 20, 20, 14), &state),
        Some(Position { x: 15, y: 31 })
    );
}

#[test]
fn dashboard_renders_emoji_picker_above_composer() {
    let mut state = state_with_message();
    state.start_composer();
    for ch in ":heart".chars() {
        state.push_composer_char(ch);
    }

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains(" emoji "),
        "emoji picker title should render above composer:\n{rendered}"
    );
    assert!(
        rendered.contains(":heart:"),
        "emoji picker should show matching shortcode:\n{rendered}"
    );

    let mut state = state_with_message();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(50), "party_time")
        }],
    });
    state.start_composer();
    for ch in ":pa".chars() {
        state.push_composer_char(ch);
    }

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains(":party_time:"),
        "custom emoji picker should show current guild custom emoji:\n{rendered}"
    );
}

#[test]
fn dashboard_renders_composer_pickers_across_composer_width() {
    let mut mention_state = state_with_message();
    mention_state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: MemberInfo {
            username: Some("candidate_visible_past_the_old_narrow_picker_limit".to_owned()),
            is_bot: true,
            ..MemberInfo::test(
                Id::new(101),
                "candidate visible past the old narrow picker limit",
            )
        },
    });
    mention_state.start_composer();
    for ch in "@candidate".chars() {
        mention_state.push_composer_char(ch);
    }
    let rendered = render_dashboard_dump(180, 24, &mut mention_state).join("\n");
    assert!(
        rendered.contains("past the old narrow picker limit"),
        "mention picker should use composer width for long labels:\n{rendered}"
    );

    let mut emoji_state = state_with_message();
    emoji_state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo::test(
            Id::new(50),
            "party_visible_past_the_old_narrow_picker_limit",
        )],
    });
    emoji_state.start_composer();
    for ch in ":party".chars() {
        emoji_state.push_composer_char(ch);
    }
    let rendered = render_dashboard_dump(180, 24, &mut emoji_state).join("\n");
    assert!(
        rendered.contains("past_the_old_narrow_picker_limit"),
        "emoji picker should use composer width for long labels:\n{rendered}"
    );

    let mut command_state = state_with_message();
    command_state.push_event(AppEvent::ApplicationCommandsLoaded {
        guild_id: Some(Id::new(1)),
        commands: vec![ApplicationCommandInfo {
            application_id: Id::new(200),
            version: "1".to_owned(),
            application_name: Some("LookupBot".to_owned()),
            description:
                "show details with a very long explanation visible past the old narrow picker limit"
                    .to_owned(),
            options: vec![ApplicationCommandOptionInfo {
                description: "item subcommand".to_owned(),
                ..ApplicationCommandOptionInfo::test(1, "item")
            }],
            raw: serde_json::json!({ "name": "lookup" }),
            ..ApplicationCommandInfo::test(Id::new(100), "lookup")
        }],
    });
    command_state.start_composer();
    for ch in "/lo".chars() {
        command_state.push_composer_char(ch);
    }
    let rendered = render_dashboard_dump(180, 24, &mut command_state).join("\n");
    assert!(
        rendered.contains("past the old narrow picker limit"),
        "command picker should use composer width for long descriptions:\n{rendered}"
    );
}

#[test]
fn mention_picker_selection_keeps_presence_foreground_and_base_background() {
    let entries = vec![
        MentionPickerEntry {
            target: MentionPickerTarget::User(Id::new(101)),
            display_name: "Unselected User".to_owned(),
            username: Some("unselected".to_owned()),
            status: PresenceStatus::Online,
            is_bot: false,
            role_color: None,
        },
        MentionPickerEntry {
            target: MentionPickerTarget::User(Id::new(102)),
            display_name: "Selected Bot".to_owned(),
            username: Some("selected".to_owned()),
            status: PresenceStatus::Offline,
            is_bot: true,
            role_color: None,
        },
        MentionPickerEntry {
            target: MentionPickerTarget::Role(Id::new(103)),
            display_name: "Uncolored Role".to_owned(),
            username: None,
            status: PresenceStatus::Unknown,
            is_bot: false,
            role_color: None,
        },
        MentionPickerEntry {
            target: MentionPickerTarget::Role(Id::new(104)),
            display_name: "Colored Role".to_owned(),
            username: None,
            status: PresenceStatus::Unknown,
            is_bot: false,
            role_color: Some(0x3366CC),
        },
    ];
    let custom = theme::Theme::default().with_style(
        theme::HighlightGroup::MentionPickerRole,
        Style::default().fg(Color::LightMagenta),
    );

    theme::with_test_theme(custom, || {
        let lines = mention_picker_lines_for_test(&entries, 1, 80);

        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Green));
        assert_eq!(lines[0].spans[3].style.fg, None);
        assert_eq!(lines[0].spans[4].style.fg, None);
        assert_eq!(
            lines[1].spans[3].style.fg,
            theme::current()
                .style(theme::HighlightGroup::SelectedRow)
                .fg
        );
        assert!(
            lines[1].spans[3]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(
            lines[1].spans[1].style.fg,
            presence_style(PresenceStatus::Offline).fg
        );
        assert_eq!(
            lines[1].spans[1].style.bg,
            theme::current().style(theme::HighlightGroup::Normal).bg
        );
        assert!(
            lines[1].spans[1]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(lines[1].spans[4].style.fg, None);
        assert_eq!(
            lines[1].spans[5].style.fg,
            theme::current()
                .style(theme::HighlightGroup::SelectedRow)
                .fg
        );
        assert_eq!(lines[2].spans[1].style.fg, Some(Color::LightMagenta));
        assert_eq!(lines[2].spans[3].style.fg, Some(Color::LightMagenta));
        assert_eq!(
            lines[3].spans[1].style.fg,
            Some(Color::Rgb(0x33, 0x66, 0xCC))
        );
        assert_eq!(
            lines[3].spans[3].style.fg,
            Some(Color::Rgb(0x33, 0x66, 0xCC))
        );

        let selected_role = mention_picker_lines_for_test(&entries, 3, 80);
        assert_eq!(
            selected_role[3].spans[3].style.fg,
            Some(Color::Rgb(0x33, 0x66, 0xCC))
        );
        assert_eq!(
            selected_role[3].spans[3].style.bg,
            theme::current()
                .style(theme::HighlightGroup::SelectedRow)
                .bg
        );
        assert!(
            selected_role[3].spans[3]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    });
}

#[test]
fn emoji_picker_lines_cross_out_unavailable_custom_emoji() {
    let lines = emoji_picker_lines(
        &[
            EmojiPickerEntry {
                emoji: "◆".to_owned(),
                shortcode: "gone".to_owned(),
                name: "custom emoji".to_owned(),
                wire_format: Some("<:gone:51>".to_owned()),
                available: false,
                available_as_link: false,
                custom_image_url: Some("https://cdn.discordapp.com/emojis/51.png".to_owned()),
            },
            EmojiPickerEntry {
                emoji: "❤️".to_owned(),
                shortcode: "heart".to_owned(),
                name: "red heart".to_owned(),
                wire_format: None,
                available: true,
                available_as_link: false,
                custom_image_url: None,
            },
            EmojiPickerEntry {
                emoji: "◆".to_owned(),
                shortcode: "party_time".to_owned(),
                name: "custom emoji".to_owned(),
                wire_format: Some("<:party_time:50>".to_owned()),
                available: true,
                available_as_link: true,
                custom_image_url: Some("https://cdn.discordapp.com/emojis/50.png".to_owned()),
            },
        ],
        0,
        40,
        &[
            "https://cdn.discordapp.com/emojis/51.png".to_owned(),
            "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        ],
        true,
    );

    assert!(
        lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert_eq!(lines[0].spans[1].content.as_ref(), "   ");
    assert!(
        !lines[1].spans[3]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert!(
        !lines[2].spans[2]
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert_eq!(lines[2].spans[1].content.as_ref(), "   ");
    assert_eq!(lines[2].spans[3].content.as_ref(), " - ");
    assert_eq!(
        lines[2].spans[4].content.as_ref(),
        "available as image link"
    );
    assert!(lines[2].spans[4].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn dashboard_renders_scrollbar_for_overflowing_composer_pickers() {
    let mut state = state_with_message();
    for index in 0..10 {
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: MemberInfo {
                username: Some(format!("scroll{index:02}")),
                ..MemberInfo::test(Id::new(100 + index), format!("Scroll {index:02}"))
            },
        });
    }
    state.start_composer();
    for ch in "@sc".chars() {
        state.push_composer_char(ch);
    }
    state.move_active_composer_picker_selection(9);

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains("Scroll 09"),
        "selected overflow mention candidate should stay visible:\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "overflowing mention picker should render a scrollbar thumb:\n{rendered}"
    );

    let mut state = state_with_message();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: (0..10)
            .map(|index| {
                CustomEmojiInfo::test(Id::new(100 + index), format!("overflow_{index:02}"))
            })
            .collect(),
    });
    state.start_composer();
    for ch in ":ov".chars() {
        state.push_composer_char(ch);
    }
    state.move_active_composer_picker_selection(9);

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains(":overflow_09:"),
        "selected overflow emoji candidate should stay visible:\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "overflowing emoji picker should render a scrollbar thumb:\n{rendered}"
    );
}

#[test]
fn reply_composer_line_count_includes_reply_hint() {
    let mut state = state_with_message();
    state.direct_reply_to_selected_message();
    state.push_composer_char('h');
    state.push_composer_char('\n');
    state.push_composer_char('i');

    assert_eq!(composer_content_line_count(&state, 80), 3);
}
