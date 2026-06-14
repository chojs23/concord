use super::*;
use crate::tui::keybindings::{KeymapBindingSummary, OptionsCategoryShortcut};
use crate::tui::ui::{downloads_popup_area, downloads_popup_lines};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;

#[test]
fn options_popup_lines_show_selected_toggle_state() {
    let items = vec![
        DisplayOptionItem {
            description: "Master switch.",
            ..DisplayOptionItem::test("Disable all image previews")
        },
        DisplayOptionItem {
            enabled: true,
            effective: true,
            description: "Message and profile avatars.",
            ..DisplayOptionItem::test("Show avatars")
        },
        DisplayOptionItem {
            enabled: true,
            value: Some("balanced".to_owned()),
            gauge_percent: Some(55),
            effective: true,
            description: "Attachment and embed previews.",
            ..DisplayOptionItem::test("Image preview quality")
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
fn quit_confirmation_lines_show_controls() {
    let lines = quit_confirmation_lines();

    assert_eq!(lines[0].spans[0].content, "Quit Concord?");
    assert_eq!(lines[1].spans[0].content, "");
    assert!(lines[2].spans[0].content.contains("Enter/y"));
    assert!(lines[2].spans[2].content.contains("Esc/n"));
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
fn downloads_popup_lines_show_percent_and_unknown_size() {
    let downloads = vec![
        AttachmentDownloadProgressView {
            id: AttachmentDownloadId::new(1),
            filename: "cat.png".to_owned(),
            downloaded_bytes: 50,
            total_bytes: Some(100),
        },
        AttachmentDownloadProgressView {
            id: AttachmentDownloadId::new(2),
            filename: "clip.mp4".to_owned(),
            downloaded_bytes: 1024,
            total_bytes: None,
        },
    ];

    let lines = downloads_popup_lines(&downloads, 48);
    let rendered = line_texts_from_ratatui(&lines).join("\n");

    assert!(rendered.contains("cat.png 50%"), "{rendered}");
    assert!(
        rendered.contains("clip.mp4 1.0 KiB downloaded"),
        "{rendered}"
    );
    assert_eq!(
        downloads_popup_area(Rect::new(5, 2, 60, 12), lines.len()).x,
        17
    );
}

#[test]
fn search_popup_message_results_show_sent_time() {
    let message_id = test_message_id_for_unix_millis(discord_epoch_unix_millis());
    let mut state = state_with_message_id(message_id, "seed");
    state.open_search_popup_for_focus(FocusPane::Messages);

    state.push_event(AppEvent::MessageSearchLoaded {
        page: MessageSearchPage {
            query: MessageSearchQuery {
                guild_id: Some(Id::new(1)),
                content: Some("needle".to_owned()),
                ..Default::default()
            },
            messages: vec![MessageInfo {
                guild_id: Some(Id::new(1)),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some("needle result".to_owned()),
                ..MessageInfo::test(Id::new(2), message_id)
            }],
            total_results: Some(1),
            has_more: false,
        },
    });

    let dump = render_dashboard_dump(120, 28, &mut state);
    let rendered = dump.join("\n");
    let expected_time = format_message_sent_time(message_id);

    assert!(
        rendered.contains(&format!("#general neo {expected_time}: needle result")),
        "{rendered}"
    );
}

#[test]
fn options_popup_lines_keep_selected_item_visible_when_clipped() {
    let items = vec![
        DisplayOptionItem {
            enabled: true,
            effective: true,
            description: "First.",
            ..DisplayOptionItem::test("Option 1")
        },
        DisplayOptionItem {
            enabled: true,
            effective: true,
            description: "Second.",
            ..DisplayOptionItem::test("Option 2")
        },
        DisplayOptionItem {
            enabled: true,
            effective: true,
            description: "Third.",
            ..DisplayOptionItem::test("Option 3")
        },
        DisplayOptionItem {
            enabled: true,
            effective: true,
            description: "Fourth.",
            ..DisplayOptionItem::test("Option 4")
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
    state.open_options_category_from_shortcut(OptionsCategoryShortcut::Notifications);

    let dump = render_dashboard_dump(100, 9, &mut state);
    let rendered = dump.join("\n");

    assert!(
        dump.iter()
            .any(|row| row.contains("›") && row.contains("Desktop notifications")),
        "{rendered}"
    );
}

#[test]
fn attachment_viewer_render_shows_download_hint_inside_popup() {
    let mut state = state_with_file_attachment_message();
    assert!(state.open_attachment_viewer_for_selected_message());

    let dump = render_dashboard_dump(100, 25, &mut state);
    let rendered = dump.join("\n");
    let hint_row = dump
        .iter()
        .find(|row| row.contains("[x] play") && row.contains("[d] download"))
        .expect("attachment viewer hint should render");

    assert!(rendered.contains("File: notes.txt"), "{rendered}");
    assert!(rendered.contains("Size: 42 B"), "{rendered}");
    assert!(hint_row.contains('│'), "{rendered}");
}

#[test]
fn attachment_viewer_popup_uses_eighty_percent_of_message_area() {
    let area = Rect::new(10, 5, 100, 40);

    let popup = attachment_viewer_popup(area, area, AttachmentViewerZoom::Default);
    let image_area = attachment_viewer_image_area(area, area, AttachmentViewerZoom::Default);

    assert_eq!(popup, Rect::new(20, 9, 80, 32));
    assert_eq!(image_area, Rect::new(21, 10, 78, 29));
}

#[test]
fn attachment_viewer_popup_large_uses_ninety_five_percent_of_message_area() {
    let area = Rect::new(10, 5, 100, 40);

    let popup = attachment_viewer_popup(area, area, AttachmentViewerZoom::Large);

    assert_eq!(popup, Rect::new(12, 6, 95, 38));
}

#[test]
fn attachment_viewer_popup_fullscreen_uses_full_frame_area() {
    let messages_area = Rect::new(10, 5, 100, 40);
    let frame_area = Rect::new(0, 0, 200, 60);

    let popup =
        attachment_viewer_popup(messages_area, frame_area, AttachmentViewerZoom::Fullscreen);

    assert_eq!(popup, frame_area);
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
fn current_user_profile_settings_render_contract() {
    let mut profile = user_profile_info(10, "neo");
    profile.global_name = Some("Neo Global".to_owned());
    profile.pronouns = None;
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(AppEvent::UserPresenceUpdate {
        user_id: Id::new(10),
        status: PresenceStatus::DoNotDisturb,
        activities: Vec::new(),
    });
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile: profile.clone(),
    });
    state.open_current_user_profile_popup();

    let lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::DoNotDisturb);
    let texts = line_texts_from_ratatui(&lines);
    assert_eq!(lines[0].spans[0].content, "Neo Global");

    let display_value = texts
        .iter()
        .position(|line| line.contains("Display name"))
        .and_then(|index| lines.get(index + 1))
        .expect("display name value should follow label");
    let display_label = texts
        .iter()
        .position(|line| line.contains("Display name"))
        .and_then(|index| lines.get(index))
        .expect("display name label should exist");
    let pronouns_value = texts
        .iter()
        .position(|line| line.contains("Pronouns"))
        .and_then(|index| lines.get(index + 1))
        .expect("pronouns value should follow label");
    let status_value = texts
        .iter()
        .position(|line| line.contains("Status"))
        .and_then(|index| lines.get(index + 1))
        .expect("status value should follow label");

    assert_eq!(display_label.spans[1].style.fg, Some(ACCENT));
    assert_eq!(display_value.spans[0].content, "  Neo Global");
    assert_eq!(display_value.spans[0].style, Style::default());
    assert_eq!(pronouns_value.spans[0].content, "  (empty)");
    assert_eq!(pronouns_value.spans[0].style.fg, Some(DIM));
    assert_eq!(status_value.spans[0].content, "  Do Not Disturb");
    assert_eq!(status_value.spans[0].style.fg, Some(Color::Red));

    let _ = state.start_or_commit_user_profile_edit();
    let editing_lines =
        user_profile_popup_lines(&profile, &state, 60, PresenceStatus::DoNotDisturb);
    let editing_texts = line_texts_from_ratatui(&editing_lines);
    let editing_label = editing_texts
        .iter()
        .position(|line| line.contains("Display name"))
        .and_then(|index| editing_lines.get(index))
        .expect("editing label should exist");
    assert_eq!(editing_label.spans[1].content, "Display name");
    assert_eq!(editing_label.spans[1].style.fg, Some(Color::Yellow));
    for value in "Neo Dirty".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();
    let dirty_lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::DoNotDisturb);
    let dirty_texts = line_texts_from_ratatui(&dirty_lines);

    assert!(
        dirty_texts
            .iter()
            .any(|line| line == "Unsaved changes. Press s to save.")
    );
    assert!(dirty_texts.iter().any(|line| line.contains("Enter select")));
    assert!(dirty_texts.iter().any(|line| line.contains(" · ")));
    assert!(
        !dirty_texts
            .iter()
            .any(|line| line.contains("select/edit/commit"))
    );

    let narrow_lines = user_profile_popup_lines(&profile, &state, 24, PresenceStatus::DoNotDisturb);
    let narrow_texts = line_texts_from_ratatui(&narrow_lines);
    let hint_start = narrow_texts
        .iter()
        .position(|line| line.contains("Esc close/cancel"))
        .expect("wrapped helper hint should start with Esc close/cancel");
    let wrapped_hint = narrow_texts[hint_start..].join(" ");
    assert!(wrapped_hint.contains("Esc close/cancel"));
    assert!(wrapped_hint.contains(" · "));
    assert!(wrapped_hint.contains("Enter select"));
    assert!(wrapped_hint.contains("s Save"));
    assert!(!wrapped_hint.contains("select/edit/commit"));

    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    let _ = state.start_or_commit_user_profile_edit();
    state.move_user_profile_status_picker_up();
    let picker_lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::DoNotDisturb);
    let picker_texts = line_texts_from_ratatui(&picker_lines);

    assert!(picker_texts.iter().any(|line| line.contains("Status")));
    assert!(picker_texts.iter().any(|line| line == "Choose status"));
    assert!(picker_texts.iter().any(|line| line == "› Idle"));
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
            state: Some("Coding hard".to_owned()),
            emoji: Some(ActivityEmoji {
                name: "🦀".to_owned(),
                id: None,
                animated: false,
            }),
            ..ActivityInfo::test(ActivityKind::Custom, "Custom Status")
        },
        ActivityInfo {
            details: Some("Bohemian Rhapsody".to_owned()),
            state: Some("Queen".to_owned()),
            ..ActivityInfo::test(ActivityKind::Listening, "Spotify")
        },
        ActivityInfo::test(ActivityKind::Playing, "Concord"),
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
            label: "Open thread".to_owned(),
            ..MessageActionItem::test(MessageActionKind::OpenThread)
        },
        MessageActionItem {
            label: "Show reacted users".to_owned(),
            enabled: false,
            ..MessageActionItem::test(MessageActionKind::ShowReactionUsers)
        },
        MessageActionItem {
            label: "Choose poll votes".to_owned(),
            ..MessageActionItem::test(MessageActionKind::OpenPollVotePicker)
        },
    ];

    let lines = message_action_menu_lines(&actions, 1);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "  [t] Open thread",
            "› [u] Show reacted users (unavailable)",
            "  [c] Choose poll votes",
        ]
    );
}

#[test]
fn message_action_menu_uses_numbered_shortcuts_for_duplicate_preferred_keys() {
    let actions = vec![
        MessageActionItem {
            label: "Show cat users".to_owned(),
            ..MessageActionItem::test(MessageActionKind::ShowReactionUsers)
        },
        MessageActionItem {
            label: "Show dog users".to_owned(),
            ..MessageActionItem::test(MessageActionKind::ShowReactionUsers)
        },
    ];

    let lines = message_action_menu_lines(&actions, 0);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [1] Show cat users", "  [2] Show dog users"]
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
            label: "Thumbs up".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("👍".to_owned()))
        },
        EmojiReactionItem {
            label: "Party".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Custom {
                id: Id::new(42),
                name: Some("party".to_owned()),
                animated: false,
            })
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
            label: "Thumbs up".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("👍".to_owned()))
        },
        EmojiReactionItem {
            label: "Heart".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("❤️".to_owned()))
        },
        EmojiReactionItem {
            label: "Joy".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("😂".to_owned()))
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
fn emoji_reaction_picker_marks_own_reactions_yellow() {
    let reactions = vec![
        EmojiReactionItem {
            label: "Thumbs up".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("👍".to_owned()))
        },
        EmojiReactionItem {
            label: "Heart".to_owned(),
            ..EmojiReactionItem::test(ReactionEmoji::Unicode("❤️".to_owned()))
        },
    ];
    let existing_reactions = vec![
        ReactionEmoji::Unicode("👍".to_owned()),
        ReactionEmoji::Unicode("❤️".to_owned()),
    ];
    let own_reactions = vec![ReactionEmoji::Unicode("❤️".to_owned())];

    let lines = emoji_reaction_picker_lines_with_own_reactions(
        &reactions,
        &existing_reactions,
        &own_reactions,
        1,
        10,
        &[],
    );

    assert_eq!(lines[0].spans[2].style.fg, None);
    assert_eq!(lines[1].spans[2].style.fg, Some(Color::Yellow));
    assert_eq!(lines[1].spans[2].style.bg, Some(Color::Rgb(40, 45, 90)));
}

#[test]
fn poll_vote_picker_marks_selected_and_checked_answers() {
    let answers = vec![
        PollVotePickerItem {
            label: "Soup".to_owned(),
            selected: true,
            ..PollVotePickerItem::test(1)
        },
        PollVotePickerItem {
            label: "Noodles".to_owned(),
            ..PollVotePickerItem::test(2)
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
                users: vec![
                    ReactionUserInfo::test(Id::new(10), "neo"),
                    ReactionUserInfo::test(Id::new(11), "trinity"),
                ],
                ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
            },
            ReactionUsersInfo::test(ReactionEmoji::Custom {
                id: Id::new(50),
                name: Some("party".to_owned()),
                animated: false,
            }),
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
        users: (1..=6)
            .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
            .collect(),
        ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
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
                users: vec![
                    ReactionUserInfo::test(Id::new(1), "갱생케가"),
                    ReactionUserInfo::test(Id::new(2), "하나비"),
                    ReactionUserInfo::test(Id::new(3), "슬기인뎅"),
                    ReactionUserInfo::test(Id::new(4), "won"),
                ],
                ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
            },
            ReactionUsersInfo {
                users: vec![ReactionUserInfo::test(Id::new(5), "파닥파닥( 40%..? )")],
                ..ReactionUsersInfo::test(ReactionEmoji::Unicode("❤️".to_owned()))
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
            users: vec![
                ReactionUserInfo::test(Id::new(1), "won"),
                ReactionUserInfo::test(Id::new(2), "파닥파닥( 40%..? )"),
            ],
            ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
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
        users: vec![
            ReactionUserInfo::test(Id::new(1), "won"),
            ReactionUserInfo::test(Id::new(2), "파닥파닥( 40%..? )"),
        ],
        ..ReactionUsersInfo::test(ReactionEmoji::Unicode("❤️".to_owned()))
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
        label: "Party".to_owned(),
        ..EmojiReactionItem::test(ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: false,
        })
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
        label: "This Is A Very Long Server Emoji Name That Would Wrap".to_owned(),
        ..EmojiReactionItem::test(ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("this_is_a_very_long_server_emoji_name_that_would_wrap".to_owned()),
            animated: false,
        })
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
            label: format!("Emoji {index}"),
            ..EmojiReactionItem::test(ReactionEmoji::Custom {
                id: Id::new(100 + index),
                name: Some(format!("emoji_{index}")),
                animated: false,
            })
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
        label: "This goose".to_owned(),
        ..EmojiReactionItem::test(ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("this".to_owned()),
            animated: false,
        })
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

    let dump = render_dashboard_dump(160, 20, &mut state);
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
fn leader_popup_shows_keymap_entries_alongside_default_entries() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "StartComposer".to_owned(),
        crate::config::KeymapBinding::one("<leader>e"),
    );
    mappings.insert(
        "ReplyMessage".to_owned(),
        crate::config::KeymapBinding::one("<leader>m r"),
    );
    mappings.insert(
        "ChannelSwitcher".to_owned(),
        crate::config::KeymapBinding::one("<leader><C-w>"),
    );
    let mut state = DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        crate::config::KeymapOptions {
            leader: Some("space".to_owned()),
            groups: BTreeMap::new(),
            mappings,
            ..Default::default()
        },
        Default::default(),
    );
    state.open_leader();

    let dump = render_dashboard_dump(160, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("[e]"), "{rendered}");
    assert!(rendered.contains("start composer"), "{rendered}");
    assert!(rendered.contains("[m]"), "{rendered}");
    assert!(rendered.contains("prefix"), "{rendered}");
    assert!(rendered.contains("[Ctrl+w]"), "{rendered}");
    assert!(rendered.contains("Switch channels"), "{rendered}");
    assert!(rendered.contains("[a]"), "{rendered}");
    assert!(rendered.contains("Actions"), "{rendered}");
    assert!(rendered.contains("[o]"), "{rendered}");
    assert!(rendered.contains("Options"), "{rendered}");
}

#[test]
fn leader_popup_expands_horizontally_for_many_keymap_entries() {
    let mut mappings = BTreeMap::new();
    for (action, key) in [
        ("StartComposer", "<leader>b"),
        ("OpenPaneFilter", "<leader>c"),
        ("FocusGuildPane", "<leader>d"),
        ("FocusChannelPane", "<leader>e"),
        ("FocusMessagePane", "<leader>f"),
        ("FocusMemberPane", "<leader>g"),
        ("CycleFocusNext", "<leader>h"),
        ("CycleFocusPrevious", "<leader>i"),
    ] {
        mappings.insert(
            action.to_owned(),
            crate::config::KeymapBinding {
                keys: vec![key.to_owned()],
                description: Some(format!("wide leader popup column label for {action}")),
            },
        );
    }
    mappings.insert(
        "CycleFocusPrevious".to_owned(),
        crate::config::KeymapBinding {
            keys: vec!["<leader>i".to_owned()],
            description: Some("RIGHTMOST_EXPANDED_POPUP_VISIBLE".to_owned()),
        },
    );
    let mut state = DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        crate::config::KeymapOptions {
            leader: Some("space".to_owned()),
            groups: BTreeMap::new(),
            mappings,
            ..Default::default()
        },
        Default::default(),
    );
    state.toggle_pane_visibility(FocusPane::Guilds);
    state.toggle_pane_visibility(FocusPane::Channels);
    state.toggle_pane_visibility(FocusPane::Members);
    state.open_leader();

    let dump = render_dashboard_dump(220, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains("RIGHTMOST_EXPANDED_POPUP_VISIBLE"),
        "{rendered}"
    );
}

#[test]
fn leader_popup_shows_non_leader_prefix_title_and_description() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "ChannelSwitcher".to_owned(),
        crate::config::KeymapBinding {
            keys: vec!["<C-w>f".to_owned()],
            description: Some("find channel".to_owned()),
        },
    );
    let mut state = DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        crate::config::KeymapOptions {
            leader: None,
            groups: BTreeMap::new(),
            mappings,
            ..Default::default()
        },
        Default::default(),
    );
    let prefix = vec![
        state
            .key_bindings()
            .keymap_chord_for_event(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
    ];
    state.open_keymap_prefix(prefix);

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("<C-w>"), "{rendered}");
    assert!(rendered.contains("[f]"), "{rendered}");
    assert!(rendered.contains("find channel"), "{rendered}");
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
fn leader_action_popup_renders_modified_action_shortcut_labels() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut channel_actions = BTreeMap::new();
    channel_actions.insert(
        "MuteChannel".to_owned(),
        crate::config::KeymapBinding::one("<C-u>"),
    );
    let mut state = DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        crate::config::KeymapOptions {
            channel_actions,
            ..Default::default()
        },
        Default::default(),
    );
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Channels);
    state.open_leader();
    state.open_leader_actions_for_focused_target();

    let lines = leader_action_lines_for_test(&state);
    let rendered = line_texts_from_ratatui(&lines).join("\n");

    assert!(rendered.contains("[Ctrl+u]"), "{rendered}");
    assert!(rendered.contains("Mute channel"), "{rendered}");
}

#[test]
fn leader_action_popup_dims_disabled_channel_actions() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Channels);
    state.open_leader();
    state.open_leader_actions_for_focused_target();
    let lines = leader_action_lines_for_test(&state);

    assert_eq!(lines[0].spans[2].content, "Join voice");
    assert_eq!(lines[0].spans[2].style.fg, Some(DIM));
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
fn leader_action_popup_for_empty_panes_does_not_fall_back_to_root_keymap() {
    for pane in [FocusPane::Channels, FocusPane::Messages, FocusPane::Members] {
        let mut state = DashboardState::new();
        state.focus_pane(pane);
        state.open_leader();
        state.open_leader_actions_for_focused_target();

        let dump = render_dashboard_dump(120, 20, &mut state);
        let rendered = dump.join("\n");

        assert!(
            rendered.contains("No actions available"),
            "{pane:?}: {rendered}"
        );
        assert!(!rendered.contains("[o] Options"), "{pane:?}: {rendered}");
    }
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

#[test]
fn keymap_popup_lines_show_help_content() {
    let help_lines = keymap_help_popup_lines(vec![
        KeymapBindingSummary {
            scope: "keymap",
            action: "ReplyMessage".to_owned(),
            keys: vec!["n".to_owned()],
        },
        KeymapBindingSummary {
            scope: "keymap.composer",
            action: "Submit".to_owned(),
            keys: vec!["<Enter>".to_owned()],
        },
    ]);

    assert_eq!(help_lines[0].spans[0].content, "[keymap]");
    assert_eq!(help_lines[1].spans[0].content, "[n] ");
    assert!(help_lines[1].spans[1].content.contains("ReplyMessage"));
    assert_eq!(help_lines[3].spans[0].content, "[keymap.composer]");
    assert_eq!(help_lines[4].spans[0].content, "[<Enter>] ");
    assert!(help_lines[4].spans[1].content.contains("Submit"));
}
