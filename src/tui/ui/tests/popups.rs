use super::*;
use crate::discord::FriendStatus;
use crate::discord::test_builders::{
    GuildCreateFixture, ReactionUsersLoadedFixture, guild_create_event, reaction_users_loaded_event,
};
use crate::tui::keybindings::{KeymapBindingSummary, OptionsCategoryShortcut};
use crate::tui::state::ReactionUsersPopupState;
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

    let description_background = Color::Red;
    let custom = theme::Theme::default().with_style(
        theme::HighlightGroup::Description,
        Style::default()
            .bg(description_background)
            .add_modifier(Modifier::DIM),
    );
    theme::with_test_theme(custom, || {
        let lines = options_popup_lines(&items, 1, items.len(), 0, 120);

        assert_eq!(lines[0].spans[1].content, "[ ] ");
        assert_eq!(lines[0].spans[4].style.bg, Some(description_background));
        assert_eq!(lines[1].spans[0].content, "› ");
        assert_eq!(lines[1].spans[1].content, "[x] ");
        assert_eq!(lines[1].spans[4].style.bg, Some(description_background));
        assert_eq!(lines[2].spans[1].content, "[balanced] ");
        assert!(lines[3].spans[1].content.contains("-100 dB"));
        assert!(lines[3].spans[3].content.contains("0 dB"));
        assert_eq!(lines.len(), 4);
    });
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
    let texts = line_texts_from_ratatui(&lines);
    assert_eq!(texts[4], "› [y] confirm");
    assert_eq!(texts[5], "  [n] cancel");
}

#[test]
fn message_pin_confirmation_lines_show_action_and_excerpt() {
    let pin_lines = message_pin_confirmation_lines(true, "neo", Some("pin this"), 80);
    assert_eq!(pin_lines[0].spans[0].content, "Pin this message?");
    let pin_texts = line_texts_from_ratatui(&pin_lines);
    assert_eq!(pin_texts[4], "› [y] confirm");
    assert_eq!(pin_texts[5], "  [n] cancel");

    let unpin_lines = message_pin_confirmation_lines(false, "neo", Some("unpin this"), 80);
    assert_eq!(unpin_lines[0].spans[0].content, "Unpin this message?");
    let unpin_texts = line_texts_from_ratatui(&unpin_lines);
    assert_eq!(unpin_texts[4], "› [y] confirm");
    assert_eq!(unpin_texts[5], "  [n] cancel");

    let remove_lines =
        message_remove_embeds_confirmation_lines("neo", Some("remove embeds from this"), 80);
    assert_eq!(
        remove_lines[0].spans[0].content,
        "Remove embeds from this message?"
    );
    let remove_texts = line_texts_from_ratatui(&remove_lines);
    assert_eq!(remove_texts[4], "› [y] confirm");
    assert_eq!(remove_texts[5], "  [n] cancel");
}

#[test]
fn quit_confirmation_lines_show_controls() {
    let lines = quit_confirmation_lines();

    assert_eq!(lines[0].spans[0].content, "Quit Concord?");
    assert_eq!(lines[1].spans[0].content, "");
    let texts = line_texts_from_ratatui(&lines);
    assert_eq!(texts[2], "› [y] confirm");
    assert_eq!(texts[3], "  [n] cancel");
    assert_eq!(lines[2].spans[0].content, "› ");
    assert_eq!(lines[3].spans[0].content, "  ");
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
    let background = Color::Rgb(12, 34, 56);
    let custom = theme::Theme::default().with_style(
        theme::HighlightGroup::Normal,
        Style::default().fg(Color::Reset).bg(background),
    );

    theme::with_test_theme(custom, || {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        let mut state = DashboardState::new();
        state.show_success_toast("Message copied", std::time::Instant::now());

        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
            })
            .expect("toast render should succeed");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 7)].symbol(), "┌");
        assert_eq!(buffer[(0, 7)].fg, Color::Green);
        assert_eq!(buffer[(1, 8)].symbol(), "M");
        assert_eq!(buffer[(1, 8)].fg, Color::Green);
        assert_eq!(buffer[(1, 8)].bg, background);
    });
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

    let lines = options_popup_lines(&items, 3, 2, 2, 120);
    let rendered = line_texts_from_ratatui(&lines).join("\n");

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
fn attachment_viewer_popup_uses_eighty_percent_of_frame_area() {
    let area = Rect::new(10, 5, 100, 40);

    let popup = attachment_viewer_popup(area, AttachmentViewerZoom::Default);
    let image_area = attachment_viewer_image_area(area, AttachmentViewerZoom::Default);

    assert_eq!(popup, Rect::new(20, 9, 80, 32));
    assert_eq!(image_area, Rect::new(21, 10, 78, 29));
}

#[test]
fn attachment_viewer_popup_large_uses_ninety_five_percent_of_frame_area() {
    let area = Rect::new(10, 5, 100, 40);

    let popup = attachment_viewer_popup(area, AttachmentViewerZoom::Large);

    assert_eq!(popup, Rect::new(12, 6, 95, 38));
}

#[test]
fn attachment_viewer_popup_fullscreen_uses_full_frame_area() {
    let frame_area = Rect::new(0, 0, 200, 60);

    let popup = attachment_viewer_popup(frame_area, AttachmentViewerZoom::Fullscreen);

    assert_eq!(popup, frame_area);
}

#[test]
fn user_profile_popup_keeps_name_on_terminal_foreground() {
    let profile = user_profile_info(10, "neo");
    let state = DashboardState::new();

    let lines = user_profile_popup_lines(&profile, &state, 40, PresenceStatus::Idle);

    assert_eq!(lines[0].spans[0].style.fg, None);
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
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id: Id::new(10),
            status: PresenceStatus::DoNotDisturb,
            activities: Vec::new(),
        },
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

    assert_eq!(
        display_label.spans[1].style.fg,
        theme::current()
            .style(theme::HighlightGroup::ActiveField)
            .fg
    );
    assert_eq!(display_value.spans[0].content, "  Neo Global");
    assert_eq!(
        display_value.spans[0].style,
        theme::current().style(theme::HighlightGroup::ActiveField)
    );
    assert_eq!(pronouns_value.spans[0].content, "  (empty)");
    assert!(
        pronouns_value.spans[0]
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
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
    assert_eq!(
        editing_label.spans[1].style.fg,
        theme::current().style(theme::HighlightGroup::Editing).fg
    );
    for value in "Neo Dirty".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();
    let dirty_lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::DoNotDisturb);
    let dirty_texts = line_texts_from_ratatui(&dirty_lines);

    assert!(dirty_texts.iter().any(|line| line == "Unsaved changes."));
    assert!(dirty_texts.iter().any(|line| line.contains("[s] save")));
    assert!(dirty_texts.iter().any(|line| line.contains("[c] cancel")));
    assert!(dirty_texts.iter().any(|line| line.contains("[o] sign out")));
    assert!(
        !dirty_texts
            .iter()
            .any(|line| line.contains("select/edit/commit"))
    );

    let narrow_lines = user_profile_popup_lines(&profile, &state, 24, PresenceStatus::DoNotDisturb);
    let narrow_texts = line_texts_from_ratatui(&narrow_lines);
    assert!(narrow_texts.iter().any(|line| line.contains("[s] save")));
    assert!(narrow_texts.iter().any(|line| line.contains("[c] cancel")));
    assert!(
        narrow_texts
            .iter()
            .any(|line| line.contains("[o] sign out"))
    );
    assert!(
        !narrow_texts
            .iter()
            .any(|line| line.contains("close/cancel"))
    );

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
    let selected_status = picker_lines
        .iter()
        .find(|line| line.to_string() == "› Idle")
        .expect("selected status row");
    assert_eq!(
        selected_status.spans[1].style.fg,
        presence_style(PresenceStatus::Idle).fg
    );
    assert_eq!(
        selected_status.spans[1].style.bg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .bg
    );
}

#[test]
fn current_user_profile_settings_show_sign_out_action() {
    let profile = user_profile_info(10, "neo");
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile: profile.clone(),
    });
    state.open_current_user_profile_popup();

    let lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::Online);
    let texts = line_texts_from_ratatui(&lines);
    let sign_out_index = texts
        .iter()
        .position(|line| line.contains("[o] sign out"))
        .expect("sign-out action should render");

    assert_eq!(lines[sign_out_index].spans[2].style.fg, Some(Color::Red));
    assert_eq!(
        lines[sign_out_index].spans[1].style,
        theme::current().style(theme::HighlightGroup::Shortcut)
    );
    let rendered = texts.join(" ");
    assert!(rendered.contains("[o] sign out"));
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
fn user_profile_popup_lists_mutual_servers() {
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

    assert!(texts.iter().any(|line| line == "  • guild-1"));
    assert!(texts.iter().any(|line| line == "  • guild-3"));

    let custom = theme::Theme::default()
        .with_style(
            theme::HighlightGroup::RelationshipFriend,
            Style::default().fg(Color::Green),
        )
        .with_style(
            theme::HighlightGroup::RelationshipIncoming,
            Style::default().fg(Color::Yellow),
        )
        .with_style(
            theme::HighlightGroup::RelationshipOutgoing,
            Style::default().fg(Color::LightYellow),
        )
        .with_style(
            theme::HighlightGroup::RelationshipBlocked,
            Style::default().fg(Color::Red),
        )
        .with_style(
            theme::HighlightGroup::RelationshipNone,
            Style::default().fg(Color::Blue).add_modifier(Modifier::DIM),
        );
    theme::with_test_theme(custom, || {
        let cases = [
            (FriendStatus::Friend, Color::Green, false),
            (FriendStatus::IncomingRequest, Color::Yellow, false),
            (FriendStatus::OutgoingRequest, Color::LightYellow, false),
            (FriendStatus::Blocked, Color::Red, false),
            (FriendStatus::None, Color::Blue, true),
        ];
        for (relationship, expected, expect_dim) in cases {
            let mut profile = user_profile_info(10, "neo");
            profile.friend_status = relationship;
            let lines = user_profile_popup_lines(&profile, &state, 60, PresenceStatus::Online);
            let relationship = lines
                .iter()
                .flat_map(|line| &line.spans)
                .find(|span| span.content.starts_with('●'))
                .expect("relationship badge should render");
            assert_eq!(relationship.style.fg, Some(expected));
            assert_eq!(
                relationship.style.add_modifier.contains(Modifier::DIM),
                expect_dim
            );
        }
    });
}

#[test]
fn message_action_menu_marks_selected_and_disabled_actions() {
    let actions = vec![
        MessageActionItem {
            label: "copy message".to_owned(),
            ..MessageActionItem::test(MessageActionKind::CopyContent)
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
            "  [y] copy message",
            "› [u] Show reacted users (unavailable)",
            "  [c] Choose poll votes",
        ]
    );

    let disabled_copy_keymap = KeymapOptions {
        message_actions: [("CopyMessage".to_owned(), KeymapBinding::disabled())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let lines = message_action_menu_lines_with_keymap_options(&actions, 0, &disabled_copy_keymap);

    assert_eq!(line_texts_from_ratatui(&lines)[0], "› [] copy message");
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

    let lines = emoji_reaction_picker_lines(&reactions, 1, 10, 0, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["  [1] 👍 Thumbs up", "› [2] :party: Party",]
    );
}

#[test]
fn emoji_reaction_picker_assigns_digit_shortcuts_by_position() {
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

    let lines = emoji_reaction_picker_lines(&reactions, 0, 10, 0, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["› [1] 👍 Thumbs up", "  [2] ❤️ Heart", "  [3] 😂 Joy"]
    );
}

#[test]
fn emoji_reaction_picker_uses_reaction_colors_and_selected_background() {
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
    let own_reactions = vec![ReactionEmoji::Unicode("❤️".to_owned())];

    let lines =
        emoji_reaction_picker_lines_with_own_reactions(&reactions, &own_reactions, 0, 10, &[]);

    assert_eq!(
        lines[0].spans[2].style.fg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .fg
    );
    assert_eq!(
        lines[1].spans[2].style.fg,
        theme::current()
            .style(theme::HighlightGroup::SelfReaction)
            .fg
    );
    assert_eq!(
        lines[0].spans[2].style.bg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .bg
    );
    assert_eq!(lines[1].spans[2].style.bg, None);
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
    assert_eq!(
        lines[0].spans[2].style.fg,
        theme::current().style(theme::HighlightGroup::Selection).fg
    );
    assert_eq!(
        lines[1].spans[2].style.bg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .bg
    );
}

#[test]
fn reaction_users_popup_lists_reactions() {
    // The selected row gets the `› ` marker. A custom emoji with no ready
    // thumbnail falls back to `:name:`.
    let popup = ReactionUsersPopupState::test_list(
        Id::new(2),
        Id::new(1),
        vec![
            (ReactionEmoji::Unicode("👍".to_owned()), 55),
            (
                ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                23,
            ),
        ],
    );

    let lines = reaction_users_popup_lines(&popup, 0, 10, 56);

    let trimmed = line_texts_from_ratatui(&lines)
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(trimmed, vec!["› 👍 55", "  :party: 23"]);
    assert_eq!(
        lines[0].spans[1].style.fg,
        theme::current()
            .style(theme::HighlightGroup::SelectedRow)
            .fg
    );
}

#[test]
fn reaction_list_reserves_image_cell_for_ready_custom_emoji() {
    let custom = ReactionEmoji::Custom {
        id: Id::new(50),
        name: Some("party".to_owned()),
        animated: false,
    };
    let popup =
        ReactionUsersPopupState::test_list(Id::new(2), Id::new(1), vec![(custom.clone(), 23)]);
    let url = custom
        .custom_image_url()
        .expect("custom emoji has a thumbnail url");

    // No thumbnail ready yet -> text fallback shows `:party:`.
    let fallback = reaction_list_lines_with_ready_urls(&popup, &[], 56);
    assert!(line_texts_from_ratatui(&fallback)[0].contains(":party:"));

    // Thumbnail ready -> the emoji cell is blanked so the overlaid image shows,
    // exactly like the message view and picker.
    let with_image = reaction_list_lines_with_ready_urls(&popup, std::slice::from_ref(&url), 56);
    assert!(!line_texts_from_ratatui(&with_image)[0].contains(":party:"));
}

#[test]
fn reaction_users_popup_scrolls_long_lists() {
    // View B: the reactors for one emoji, scrolled down by 3.
    let popup = ReactionUsersPopupState::test_viewing(
        Id::new(2),
        Id::new(1),
        vec![(
            ReactionEmoji::Unicode("👍".to_owned()),
            6,
            (1..=6)
                .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
                .collect(),
            None,
        )],
        0,
    );

    let lines = reaction_users_popup_lines(&popup, 3, 3, 56);

    let trimmed = line_texts_from_ratatui(&lines)
        .into_iter()
        .map(|line| line.trim_end().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(trimmed, vec!["  user-4", "  user-5", "  user-6"]);
}

#[test]
fn reaction_users_popup_buffer_renders_without_wrap_artifacts() {
    use crate::tui::keybindings::SelectionAction;

    let mut state = DashboardState::new();
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 5)]);
    // Drill into the reaction so the user list (with the long name) renders.
    state.activate_reaction_users_popup();
    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji,
        users: vec![
            ReactionUserInfo::test(Id::new(1), "갱생케가"),
            ReactionUserInfo::test(Id::new(2), "하나비"),
            ReactionUserInfo::test(Id::new(3), "슬기인뎅"),
            ReactionUserInfo::test(Id::new(4), "won"),
            ReactionUserInfo::test(Id::new(5), "파닥파닥( 40%..? )"),
        ],
        next_after: None,
        after: None,
    }));

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
        state.navigate_reaction_users_popup(SelectionAction::Next);
    }
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("second draw");
    for _ in 0..6 {
        state.navigate_reaction_users_popup(SelectionAction::Previous);
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
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 2)]);
    state.activate_reaction_users_popup();
    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji,
        users: vec![
            ReactionUserInfo::test(Id::new(1), "won"),
            ReactionUserInfo::test(Id::new(2), "파닥파닥( 40%..? )"),
        ],
        next_after: None,
        after: None,
    }));

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
    let popup = ReactionUsersPopupState::test_viewing(
        Id::new(2),
        Id::new(1),
        vec![(
            ReactionEmoji::Unicode("❤️".to_owned()),
            2,
            vec![
                ReactionUserInfo::test(Id::new(1), "won"),
                ReactionUserInfo::test(Id::new(2), "파닥파닥( 40%..? )"),
            ],
            None,
        )],
        0,
    );

    // Inner width that is narrower than the long Korean+ASCII display name
    // forces the popup logic to truncate. Without truncation, ratatui's
    // wrap would split the long name and the wrap continuation would bleed
    // onto adjacent rows.
    let lines = reaction_users_popup_lines(&popup, 0, 4, 12);

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
        0,
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

    // At scroll 10 the selected row 12 keeps rows 13 and 14 visible below it.
    let lines = emoji_reaction_picker_lines(&reactions, 12, 5, 10, &[]);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec![
            "      :emoji_10: Emoji 10",
            "      :emoji_11: Emoji 11",
            "›     :emoji_12: Emoji 12",
            "      :emoji_13: Emoji 13",
            "      :emoji_14: Emoji 14",
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
    state.open_focused_pane_actions();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Channel actions"), "{rendered}");
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
        "ToggleMute".to_owned(),
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
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Channels);
    state.open_leader();
    state.open_focused_pane_actions();

    let lines = channel_action_menu_lines_for_test(&state);
    let rendered = line_texts_from_ratatui(&lines).join("\n");

    assert!(rendered.contains("[Ctrl+u]"), "{rendered}");
    assert!(rendered.contains("Mute channel"), "{rendered}");
}

#[test]
fn leader_action_popup_selection_overrides_disabled_dim() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Channels);
    state.open_leader();
    state.open_focused_pane_actions();
    let lines = channel_action_menu_lines_for_test(&state);

    assert_eq!(lines[0].spans[2].content, "Join voice (unavailable)");
    assert!(!lines[0].spans[2].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn leader_action_popup_from_messages_uses_message_action_title() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Messages);
    state.open_leader();
    state.open_focused_pane_actions();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Message actions"), "{rendered}");
}

#[test]
fn leader_action_popup_from_guilds_uses_server_action_title() {
    let mut state = state_with_message();
    state.focus_pane(FocusPane::Guilds);
    state.open_leader();
    state.open_focused_pane_actions();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Server actions"), "{rendered}");
    assert!(rendered.contains("Mark server as read"), "{rendered}");
}

#[test]
fn folder_settings_popup_renders_name_and_color_inputs() {
    let mut state = state_with_folder_settings();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Folder Settings"), "{rendered}");
    assert!(rendered.contains("Name:"), "{rendered}");
    assert!(rendered.contains("folder"), "{rendered}");
    assert!(rendered.contains("Color code:"), "{rendered}");
    assert!(rendered.contains("#00AAFF"), "{rendered}");
    assert!(rendered.contains("[s] submit"), "{rendered}");
    assert!(rendered.contains("[c] cancel"), "{rendered}");
    assert!(!rendered.contains("[Enter] select"), "{rendered}");
    assert!(!rendered.contains("[Esc] close/cancel"), "{rendered}");

    let inactive = folder_settings_input_line_for_test(false);
    assert!(inactive.spans[1].style.add_modifier.contains(Modifier::DIM));
    assert!(inactive.spans[2].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn leader_action_popup_from_members_uses_member_action_title() {
    let mut state = state_with_member(42, "Neo");
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Members);
    state.open_leader();
    state.open_focused_pane_actions();

    let dump = render_dashboard_dump(120, 20, &mut state);
    let rendered = dump.join("\n");

    assert!(rendered.contains("Member actions"), "{rendered}");
    assert!(rendered.contains("Show profile"), "{rendered}");
}

#[test]
fn focused_pane_actions_on_empty_panes_open_nothing() {
    for pane in [FocusPane::Channels, FocusPane::Messages, FocusPane::Members] {
        let mut state = DashboardState::new();
        state.focus_pane(pane);
        state.open_leader();
        state.open_focused_pane_actions();

        assert_eq!(state.active_modal_popup_kind(), None, "{pane:?}");
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
    let summaries = vec![
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
    ];
    let help_lines = keymap_help_popup_lines(summaries.clone());

    assert_eq!(help_lines[0].spans[0].content, "[keymap]");
    assert_eq!(help_lines[1].spans[0].content, "[n] ");
    assert!(help_lines[1].spans[1].content.contains("ReplyMessage"));
    assert_eq!(help_lines[3].spans[0].content, "[keymap.composer]");
    assert_eq!(help_lines[4].spans[0].content, "[<Enter>] ");
    assert!(help_lines[4].spans[1].content.contains("Submit"));

    let custom = theme::Theme::default().with_style(
        theme::HighlightGroup::Shortcut,
        Style::default().fg(Color::LightMagenta),
    );
    theme::with_test_theme(custom, || {
        let help_lines = keymap_help_popup_lines(summaries);
        assert_eq!(help_lines[1].spans[0].style.fg, Some(Color::LightMagenta));
        assert_eq!(help_lines[4].spans[0].style.fg, Some(Color::LightMagenta));

        let confirmation_lines = quit_confirmation_lines();
        assert_eq!(
            confirmation_lines[3].spans[1].style.fg,
            Some(Color::LightMagenta)
        );
    });
}
