use super::*;

#[test]
fn options_popup_lines_show_selected_toggle_state() {
    let items = vec![
        DisplayOptionItem {
            label: "Disable all image previews",
            enabled: false,
            value: None,
            gauge_percent: None,
            effective: false,
            description: "Master switch.",
        },
        DisplayOptionItem {
            label: "Show avatars",
            enabled: true,
            value: None,
            gauge_percent: None,
            effective: true,
            description: "Message and profile avatars.",
        },
        DisplayOptionItem {
            label: "Image preview quality",
            enabled: true,
            value: Some("balanced".to_owned()),
            gauge_percent: Some(55),
            effective: true,
            description: "Attachment and embed previews.",
        },
    ];

    let lines = options_popup_lines(&items, 1, items.len(), 120);

    assert_eq!(lines[0].spans[1].content, "[ ] ");
    assert_eq!(lines[1].spans[0].content, "› ");
    assert_eq!(lines[1].spans[1].content, "[x] ");
    assert_eq!(lines[2].spans[1].content, "[balanced] ");
    assert!(lines[3].spans[1].content.contains("-100 dB"));
    assert!(lines[3].spans[3].content.contains("0 dB"));
    assert_eq!(lines.len(), 4);
}

#[test]
fn message_delete_confirmation_lines_show_controls_and_excerpt() {
    let lines = message_delete_confirmation_lines(
        "neo",
        Some("a very important message that should be deleted"),
        80,
    );

    assert_eq!(lines[0].spans[0].content, "Delete this message?");
    assert_eq!(lines[1].spans[0].content, "From: neo");
    assert!(lines[2].spans[0].content.contains("important message"));
    assert!(lines[4].spans[0].content.contains("Enter/y"));
    assert!(lines[4].spans[2].content.contains("Esc/n"));
}

#[test]
fn message_pin_confirmation_lines_show_action_and_excerpt() {
    let pin_lines = message_pin_confirmation_lines(true, "neo", Some("pin this"), 80);
    assert_eq!(pin_lines[0].spans[0].content, "Pin this message?");
    assert!(pin_lines[4].spans[1].content.contains("Pin message"));

    let unpin_lines = message_pin_confirmation_lines(false, "neo", Some("unpin this"), 80);
    assert_eq!(unpin_lines[0].spans[0].content, "Unpin this message?");
    assert!(unpin_lines[4].spans[1].content.contains("Unpin message"));
}

#[test]
fn toast_area_anchors_to_terminal_bottom_left() {
    let area = toast_area(Rect::new(5, 2, 40, 12), "Message copied");

    assert_eq!(area, Rect::new(5, 11, 16, 3));
}

#[test]
fn toast_line_truncates_to_available_width() {
    let line = toast_line("Message copied", 7);

    assert_eq!(line.spans[0].content, "Mess...");
}

#[test]
fn dashboard_renders_toast_at_bottom_left() {
    let mut state = DashboardState::new();
    state.show_success_toast("Message copied", std::time::Instant::now());

    let dump = render_dashboard_dump(40, 10, &mut state);
    let rendered = dump.join("\n");

    assert!(dump[7].starts_with("┌"), "{rendered}");
    assert!(dump[8].starts_with("│Message copied│"), "{rendered}");
    assert!(dump[9].starts_with("└"), "{rendered}");
}

#[test]
fn options_popup_lines_keep_selected_item_visible_when_clipped() {
    let items = vec![
        DisplayOptionItem {
            label: "Option 1",
            enabled: true,
            value: None,
            gauge_percent: None,
            effective: true,
            description: "First.",
        },
        DisplayOptionItem {
            label: "Option 2",
            enabled: true,
            value: None,
            gauge_percent: None,
            effective: true,
            description: "Second.",
        },
        DisplayOptionItem {
            label: "Option 3",
            enabled: true,
            value: None,
            gauge_percent: None,
            effective: true,
            description: "Third.",
        },
        DisplayOptionItem {
            label: "Option 4",
            enabled: true,
            value: None,
            gauge_percent: None,
            effective: true,
            description: "Fourth.",
        },
    ];

    let lines = options_popup_lines(&items, 3, 2, 120);
    let rendered = line_texts_from_ratatui(&lines).join("\n");

    assert!(!rendered.contains("Option 1"), "{rendered}");
    assert!(rendered.contains("Option 3"), "{rendered}");
    assert!(rendered.contains("› [x] Option 4"), "{rendered}");
}

#[test]
fn options_popup_render_keeps_selected_row_visible_when_short() {
    let mut state = DashboardState::new();
    state.open_options_category_picker();
    state.open_options_category_shortcut('n');

    let dump = render_dashboard_dump(100, 9, &mut state);
    let rendered = dump.join("\n");

    assert!(
        dump.iter()
            .any(|row| row.contains("›") && row.contains("Desktop notifications")),
        "{rendered}"
    );
}

#[test]
fn image_viewer_render_shows_download_hint_below_popup() {
    let mut state = state_with_message();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![image_attachment()],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    assert!(state.open_image_viewer_for_selected_message());

    let dump = render_dashboard_dump(100, 25, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("[d] download image"), "{rendered}");
}

#[test]
fn image_viewer_popup_uses_eighty_percent_of_message_area() {
    let area = Rect::new(10, 5, 100, 40);

    let popup = image_viewer_popup(area);
    let image_area = image_viewer_image_area(area);

    assert_eq!(popup, Rect::new(20, 9, 80, 32));
    assert_eq!(image_area, Rect::new(21, 10, 78, 29));
}

#[test]
fn user_profile_popup_styles_name_by_status() {
    let profile = user_profile_info(10, "neo");
    let state = DashboardState::new();

    let lines = user_profile_popup_lines(&profile, &state, 40, PresenceStatus::Idle);

    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(180, 140, 0)));
    assert!(
        lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn user_profile_popup_does_not_show_dm_hint_without_dm_context() {
    for (profile_name, current_user_id) in [("neo", 10), ("alice", 99)] {
        let profile = user_profile_info(10, profile_name);
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(current_user_id)),
        });

        let lines = user_profile_popup_lines(&profile, &state, 40, PresenceStatus::Online);
        let texts = line_texts_from_ratatui(&lines);

        assert!(!texts.iter().any(|line| line.contains("m send DM")));
    }
}

#[test]
fn user_profile_popup_avatar_gutter_matches_geometry_in_narrow_layouts() {
    let narrow_area = Rect::new(0, 0, 10, 20);
    let wide_area = Rect::new(0, 0, 80, 20);

    assert!(!user_profile_popup_has_avatar(narrow_area, true));
    assert_eq!(
        user_profile_popup_text_geometry(narrow_area, false),
        user_profile_popup_text_geometry(
            narrow_area,
            user_profile_popup_has_avatar(narrow_area, true),
        )
    );

    assert!(user_profile_popup_has_avatar(wide_area, true));
    assert_ne!(
        user_profile_popup_text_geometry(wide_area, false),
        user_profile_popup_text_geometry(wide_area, user_profile_popup_has_avatar(wide_area, true)),
    );
}

#[test]
fn user_profile_popup_renders_activity_section() {
    let profile = user_profile_info(10, "neo");
    let state = DashboardState::new();
    let activities = vec![
        ActivityInfo {
            kind: ActivityKind::Custom,
            name: "Custom Status".to_owned(),
            details: None,
            state: Some("Coding hard".to_owned()),
            url: None,
            application_id: None,
            emoji: Some(ActivityEmoji {
                name: "🦀".to_owned(),
                id: None,
                animated: false,
            }),
        },
        ActivityInfo {
            kind: ActivityKind::Listening,
            name: "Spotify".to_owned(),
            details: Some("Bohemian Rhapsody".to_owned()),
            state: Some("Queen".to_owned()),
            url: None,
            application_id: None,
            emoji: None,
        },
        ActivityInfo {
            kind: ActivityKind::Playing,
            name: "Concord".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        },
    ];

    let lines = user_profile_popup_lines_with_activities(
        &profile,
        &state,
        60,
        PresenceStatus::Online,
        &activities,
    );
    let texts = line_texts_from_ratatui(&lines);

    assert!(texts.iter().any(|line| line == "ACTIVITY"));
    assert!(texts.iter().any(|line| line == "🦀 Coding hard"));
    assert!(texts.iter().any(|line| line == "♪ Spotify"));
    assert!(texts.iter().any(|line| line == "Bohemian Rhapsody"));
    assert!(texts.iter().any(|line| line == "by Queen"));
    assert!(texts.iter().any(|line| line == "▶ Concord"));
}

#[test]
fn user_profile_popup_omits_activity_section_when_empty() {
    let profile = user_profile_info(10, "neo");
    let state = DashboardState::new();
    let lines =
        user_profile_popup_lines_with_activities(&profile, &state, 60, PresenceStatus::Online, &[]);
    let texts = line_texts_from_ratatui(&lines);

    assert!(!texts.iter().any(|line| line == "ACTIVITY"));
}

#[test]
fn user_profile_popup_lists_mutual_servers_without_selection_marker() {
    let mut profile = user_profile_info(10, "neo");
    profile.mutual_guilds = (1_u64..=3)
        .map(|id| MutualGuildInfo {
            guild_id: Id::new(id),
            nick: None,
        })
        .collect();
    let state = DashboardState::new();
    let lines = user_profile_popup_lines(&profile, &state, 40, PresenceStatus::Online);
    let texts = line_texts_from_ratatui(&lines);

    // The popup no longer drives a per-row cursor. Every mutual entry gets a
    // uniform "  • name" prefix and the user navigates by scrolling.
    assert!(texts.iter().any(|line| line == "  • guild-1"));
    assert!(texts.iter().any(|line| line == "  • guild-3"));
    assert!(!texts.iter().any(|line| line.starts_with("› ")));
}

#[test]
fn message_action_menu_marks_selected_and_disabled_actions() {
    let actions = vec![
        MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "Reply".to_owned(),
            enabled: true,
        },
        MessageActionItem {
            kind: MessageActionKind::AddReaction,
            label: "Add reaction".to_owned(),
            enabled: true,
        },
        MessageActionItem {
            kind: MessageActionKind::DownloadAttachment(0),
            label: "Download file".to_owned(),
            enabled: false,
        },
        MessageActionItem {
            kind: MessageActionKind::SetPinned(true),
            label: "Pin message".to_owned(),
            enabled: true,
        },
    ];

    let lines = message_action_menu_lines(&actions, 2);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "  [R] Reply",
            "  [r] Add reaction",
            "› [f] Download file (unavailable)",
            "  [P] Pin message",
        ]
    );
}

#[test]
fn message_action_menu_uses_numbered_shortcuts_for_duplicate_preferred_keys() {
    let actions = vec![
        MessageActionItem {
            kind: MessageActionKind::Delete,
            label: "Delete message".to_owned(),
            enabled: true,
        },
        MessageActionItem {
            kind: MessageActionKind::Delete,
            label: "Download image".to_owned(),
            enabled: true,
        },
    ];

    let lines = message_action_menu_lines(&actions, 0);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [1] Delete message", "  [2] Download image"]
    );
}

#[test]
fn message_url_picker_truncates_fragment_urls_to_menu_width() {
    let urls = vec![super::super::MessageUrlItem {
        url: "https://thisis.com/a.test?with=querystrings#page".to_owned(),
        label: "https://thisis.com/a.test?with=querystrings#page".to_owned(),
    }];

    let lines = message_url_picker_lines_for_width(&urls, 0, 30);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [1] https://thisis.com/a...."]
    );
}

#[test]
fn emoji_reaction_picker_marks_selected_reaction() {
    let reactions = vec![
        EmojiReactionItem {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            label: "Thumbs up".to_owned(),
        },
        EmojiReactionItem {
            emoji: ReactionEmoji::Custom {
                id: Id::new(42),
                name: Some("party".to_owned()),
                animated: false,
            },
            label: "Party".to_owned(),
        },
    ];

    let lines = emoji_reaction_picker_lines(&reactions, 1, 10, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["  [1] 👍 Thumbs up", "› [2] :party: Party",]
    );
}

#[test]
fn emoji_reaction_picker_uses_qwerty_shortcuts_for_existing_reactions() {
    let reactions = vec![
        EmojiReactionItem {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            label: "Thumbs up".to_owned(),
        },
        EmojiReactionItem {
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
            label: "Heart".to_owned(),
        },
        EmojiReactionItem {
            emoji: ReactionEmoji::Unicode("😂".to_owned()),
            label: "Joy".to_owned(),
        },
    ];
    let existing_reactions = vec![
        ReactionEmoji::Unicode("👍".to_owned()),
        ReactionEmoji::Unicode("❤️".to_owned()),
    ];

    let lines =
        emoji_reaction_picker_lines_with_existing(&reactions, &existing_reactions, 0, 10, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [q] 👍 Thumbs up", "  [w] ❤️ Heart", "  [1] 😂 Joy"]
    );
}

#[test]
fn poll_vote_picker_marks_selected_and_checked_answers() {
    let answers = vec![
        PollVotePickerItem {
            answer_id: 1,
            label: "Soup".to_owned(),
            selected: true,
        },
        PollVotePickerItem {
            answer_id: 2,
            label: "Noodles".to_owned(),
            selected: false,
        },
    ];

    let lines = poll_vote_picker_lines(&answers, 1);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["  [1] [x] Soup", "› [2] [ ] Noodles"]
    );
}

#[test]
fn reaction_users_popup_groups_users_by_reaction() {
    let lines = reaction_users_popup_lines(
        &[
            ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                users: vec![
                    ReactionUserInfo {
                        user_id: Id::new(10),
                        display_name: "neo".to_owned(),
                    },
                    ReactionUserInfo {
                        user_id: Id::new(11),
                        display_name: "trinity".to_owned(),
                    },
                ],
            },
            ReactionUsersInfo {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                users: Vec::new(),
            },
        ],
        0,
        10,
        56,
    );

    let trimmed = line_texts_from_ratatui(&lines)
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        trimmed,
        vec![
            "👍 · 2 users",
            "  neo",
            "  trinity",
            ":party: · 0 users",
            "  no users found",
        ]
    );
}

#[test]
fn reaction_users_popup_scrolls_long_lists() {
    let reactions = vec![ReactionUsersInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        users: (1..=6)
            .map(|id| ReactionUserInfo {
                user_id: Id::new(id),
                display_name: format!("user-{id}"),
            })
            .collect(),
    }];

    let lines = reaction_users_popup_lines(&reactions, 3, 3, 56);

    let trimmed = line_texts_from_ratatui(&lines)
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(trimmed, vec!["  user-3", "  user-4", "  user-5",]);
}

#[test]
fn reaction_users_popup_buffer_renders_without_wrap_artifacts() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![
            ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                users: vec![
                    ReactionUserInfo {
                        user_id: Id::new(1),
                        display_name: "갱생케가".to_owned(),
                    },
                    ReactionUserInfo {
                        user_id: Id::new(2),
                        display_name: "하나비".to_owned(),
                    },
                    ReactionUserInfo {
                        user_id: Id::new(3),
                        display_name: "슬기인뎅".to_owned(),
                    },
                    ReactionUserInfo {
                        user_id: Id::new(4),
                        display_name: "won".to_owned(),
                    },
                ],
            },
            ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("❤️".to_owned()),
                users: vec![ReactionUserInfo {
                    user_id: Id::new(5),
                    display_name: "파닥파닥( 40%..? )".to_owned(),
                }],
            },
        ],
    });

    // Use a wide terminal so the popup's full POPUP_TARGET_WIDTH (58)
    // applies and line truncation should never trigger.
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("first draw");

    // Scroll the popup down past the long username, then back up. The
    // reported bug appeared after the long username was rendered and the user
    // scrolled up through earlier names. That is the diff path the popup must
    // survive without bleeding the wrap continuation onto
    // neighbouring rows.
    for _ in 0..6 {
        state.scroll_reaction_users_popup_down();
    }
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("second draw");
    for _ in 0..6 {
        state.scroll_reaction_users_popup_up();
    }
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("third draw");

    let buffer = terminal.backend().buffer();
    let dump = (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    // The reported artefact was the trailing fragment "? )" from
    // "파닥파닥( 40%..? )" appearing on rows that should hold a different
    // (shorter) name. After scrolling, count the number of rows whose
    // popup-content section ends with the long username's tail. Only the
    // single row that actually renders that user should match. Any other match
    // means wrap continuation bled across rows.
    let trailing_matches = dump.iter().filter(|line| line.contains("? )")).count();
    assert!(
        trailing_matches <= 1,
        "popup buffer contained '? )' fragment on {trailing_matches} rows; expected at most 1.\nDump:\n{}",
        dump.join("\n")
    );
}

#[test]
fn reaction_users_popup_buffer_stays_clean_in_narrow_terminal() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            users: vec![
                ReactionUserInfo {
                    user_id: Id::new(1),
                    display_name: "won".to_owned(),
                },
                ReactionUserInfo {
                    user_id: Id::new(2),
                    display_name: "파닥파닥( 40%..? )".to_owned(),
                },
            ],
        }],
    });

    // Narrow terminal that would force the popup down to a width where
    // the long name no longer fits without wrapping. Pre-truncation must
    // turn the long name into an ellipsis, never split it across rows.
    let dump = render_dashboard_dump(40, 25, &mut state);

    let trailing_matches = dump.iter().filter(|line| line.contains("? )")).count();
    assert!(
        trailing_matches <= 1,
        "popup buffer contained '? )' fragment on {trailing_matches} rows; expected at most 1.\nDump:\n{}",
        dump.join("\n")
    );
}

#[test]
fn reaction_users_popup_truncates_long_lines_to_fit_width() {
    let reactions = vec![ReactionUsersInfo {
        emoji: ReactionEmoji::Unicode("❤️".to_owned()),
        users: vec![
            ReactionUserInfo {
                user_id: Id::new(1),
                display_name: "won".to_owned(),
            },
            ReactionUserInfo {
                user_id: Id::new(2),
                display_name: "파닥파닥( 40%..? )".to_owned(),
            },
        ],
    }];

    // Inner width that is narrower than the long Korean+ASCII display name
    // forces the popup logic to truncate. Without truncation, ratatui's
    // wrap would split the long name and the wrap continuation would bleed
    // onto adjacent rows.
    let lines = reaction_users_popup_lines(&reactions, 0, 4, 12);

    for line in &lines {
        assert!(
            line.width() <= 12,
            "line {:?} exceeded inner width",
            line_texts_from_ratatui(std::slice::from_ref(line))
        );
    }
}

#[test]
fn reaction_users_popup_reserves_border_space_in_short_areas() {
    assert_eq!(reaction_users_visible_line_count(Rect::new(0, 0, 20, 5)), 1);
    assert_eq!(reaction_users_visible_line_count(Rect::new(0, 0, 20, 6)), 2);
    assert_eq!(
        reaction_users_visible_line_count(Rect::new(0, 0, 20, 40)),
        14
    );
}

#[test]
fn emoji_reaction_picker_reserves_space_for_loaded_custom_image() {
    let reactions = vec![EmojiReactionItem {
        emoji: ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: false,
        },
        label: "Party".to_owned(),
    }];

    let lines = emoji_reaction_picker_lines(
        &reactions,
        0,
        10,
        &["https://cdn.discordapp.com/emojis/42.png".to_owned()],
    );

    assert_eq!(line_texts_from_ratatui(&lines), vec!["› [1]    Party"]);
}

#[test]
fn emoji_reaction_picker_truncates_long_rows_to_inner_width() {
    let reactions = vec![EmojiReactionItem {
        emoji: ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("this_is_a_very_long_server_emoji_name_that_would_wrap".to_owned()),
            animated: false,
        },
        label: "This Is A Very Long Server Emoji Name That Would Wrap".to_owned(),
    }];

    let lines = emoji_reaction_picker_lines_for_width(&reactions, 0, 10, &[], 24);

    for line in &lines {
        assert!(
            line.width() <= 24,
            "line {:?} exceeded picker width",
            line_texts_from_ratatui(std::slice::from_ref(line))
        );
    }
    assert_eq!(
        line_texts_from_ratatui(&lines)[0].trim_end(),
        "› [1] :this_is_a_very..."
    );
}

#[test]
fn emoji_reaction_picker_windows_long_lists_around_selection() {
    let reactions = (0..15)
        .map(|index| EmojiReactionItem {
            emoji: ReactionEmoji::Custom {
                id: Id::new(100 + index),
                name: Some(format!("emoji_{index}")),
                animated: false,
            },
            label: format!("Emoji {index}"),
        })
        .collect::<Vec<_>>();

    let lines = emoji_reaction_picker_lines(&reactions, 12, 5, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "  [9] :emoji_8: Emoji 8",
            "  [0] :emoji_9: Emoji 9",
            "      :emoji_10: Emoji 10",
            "      :emoji_11: Emoji 11",
            "›     :emoji_12: Emoji 12",
        ]
    );
}

#[test]
fn emoji_reaction_picker_shows_active_filter() {
    let reactions = vec![EmojiReactionItem {
        emoji: ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("this".to_owned()),
            animated: false,
        },
        label: "This goose".to_owned(),
    }];

    let lines = filtered_emoji_reaction_picker_lines(&reactions, 0, 10, &[], "thi");

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [1] :this: This goose", "Filter /thi",]
    );
}

#[test]
fn leader_popup_renders_as_bottom_window() {
    let mut state = DashboardState::new();
    state.open_leader();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Leader"), "{rendered}");
    assert!(rendered.contains("[1]"), "{rendered}");
    assert!(rendered.contains("[2]"), "{rendered}");
    assert!(rendered.contains("[4]"), "{rendered}");
    assert!(rendered.contains("[a]"), "{rendered}");
    assert!(rendered.contains("toggle Servers"), "{rendered}");
    assert!(rendered.contains("toggle Channels"), "{rendered}");
    assert!(rendered.contains("toggle Members"), "{rendered}");
    assert!(rendered.contains("Actions"), "{rendered}");
}

#[test]
fn leader_action_popup_renders_focused_pane_actions() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Channels);
    state.open_leader();
    state.open_leader_actions_for_focused_target();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Channel Actions"), "{rendered}");
    assert!(rendered.contains("[p]"), "{rendered}");
    assert!(rendered.contains("Show pinned messages"), "{rendered}");
    assert!(rendered.contains("Show threads"), "{rendered}");
    assert!(rendered.contains("Mark as read"), "{rendered}");
}

#[test]
fn leader_action_popup_from_messages_hides_standalone_message_action_menu() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Messages);
    state.open_leader();
    state.open_leader_actions_for_focused_target();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Message Actions"), "{rendered}");
    assert!(rendered.contains("Reply"), "{rendered}");
    assert!(!rendered.contains("Message actions"), "{rendered}");
}

#[test]
fn leader_action_popup_from_guilds_uses_server_action_title() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Guilds);
    state.open_leader();
    state.open_leader_actions_for_focused_target();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Server Actions"), "{rendered}");
    assert!(rendered.contains("Mark server as read"), "{rendered}");
}

#[test]
fn leader_action_popup_from_members_uses_member_action_title() {
    let mut state = state_with_member(42, "Neo");
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Members);
    state.open_leader();
    state.open_leader_actions_for_focused_target();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Member Actions"), "{rendered}");
    assert!(rendered.contains("Show profile"), "{rendered}");
}

#[test]
fn debug_log_popup_shows_recent_errors() {
    let lines = debug_log_popup_lines(
        vec![
            "1 [ERROR] first: old".to_owned(),
            "2 [ERROR] second: recent".to_owned(),
        ],
        ChannelVisibilityStats {
            visible: 12,
            hidden: 3,
        },
        1,
        80,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "Channels: 12 visible · 3 hidden by permissions",
            "",
            "2 [ERROR] second: recent",
        ]
    );
}

#[test]
fn debug_log_popup_has_empty_state() {
    let lines = debug_log_popup_lines(Vec::new(), ChannelVisibilityStats::default(), 5, 80);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "Channels: 0 visible · 0 hidden by permissions",
            "",
            "No errors recorded in this process.",
        ]
    );
}

#[test]
fn debug_log_popup_wraps_long_detail_lines() {
    let lines = debug_log_popup_lines(
            vec!["42 [ERROR] history: load message history failed: Discord HTTP request failed; detail=Discord returned HTTP 403; api_error=Missing Access; response_body_bytes=99".to_owned()],
            ChannelVisibilityStats::default(),
            4,
            44,
        );
    let texts = line_texts_from_ratatui(&lines);
    let joined = texts.join("");

    assert!(
        joined.contains("detail=Discord returned HTTP 403"),
        "expected wrapped debug popup line to preserve HTTP detail: {texts:?}"
    );
}
