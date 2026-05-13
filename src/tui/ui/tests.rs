use std::time::{SystemTime, UNIX_EPOCH};

use crate::discord::ids::{Id, marker::MessageMarker};
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use super::{
    ACCENT, DIM, DISCORD_EPOCH_MILLIS, ImagePreview, ImagePreviewState, MENTION_ORANGE,
    MemberEntry, READ_DIM, SELECTED_FORUM_POST_BORDER, SELECTED_MESSAGE_BORDER,
    SNOWFLAKE_TIMESTAMP_SHIFT, UNREAD_BRIGHT, channel_switcher_cursor_position,
    channel_switcher_lines, channel_unread_decoration, composer_content_line_count,
    composer_cursor_position, composer_lines, composer_lines_with_loaded_custom_emoji_urls,
    composer_prompt_line_count, composer_text, date_separator_line, debug_log_popup_lines,
    dm_presence_dot_span, emoji_picker_lines, emoji_reaction_picker_lines,
    emoji_reaction_picker_lines_for_width, filtered_emoji_reaction_picker_lines, focus_pane_at,
    format_message_sent_time, format_unix_millis_with_offset, forum_post_reaction_summary,
    forum_post_scrollbar_visible_count, forum_post_viewport_lines, inline_image_preview_area,
    inline_image_preview_row, member_display_label, member_name_style, message_action_menu_lines,
    message_author_style, message_body_custom_emoji_rows, message_item_lines,
    message_starts_new_day, message_viewport_lines, new_messages_notice_line, options_popup_lines,
    poll_vote_picker_lines, primary_activity_summary, reaction_users_popup_lines,
    reaction_users_visible_line_count, render_channels, render_guilds, selected_avatar_x_offset,
    selected_message_card_width, selected_message_content_x_offset, sync_view_heights,
    user_profile_popup_has_avatar, user_profile_popup_lines,
    user_profile_popup_lines_with_activities, user_profile_popup_text_geometry,
};
use crate::{
    config::DisplayOptions,
    discord::{
        ActivityEmoji, ActivityInfo, ActivityKind, AppEvent, AttachmentInfo, ChannelInfo,
        ChannelNotificationOverrideInfo, ChannelRecipientState, ChannelState, ChannelUnreadState,
        ChannelVisibilityStats, CustomEmojiInfo, EmbedInfo, FriendStatus, GuildMemberState,
        GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageAttachmentUpload,
        MessageInfo, MessageKind, MessageSnapshotInfo, MessageState, MutualGuildInfo,
        NotificationLevel, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
        ReactionUserInfo, ReactionUsersInfo, ReadStateInfo, ReplyInfo, RoleInfo, UserProfileInfo,
    },
    tui::{
        format::{TextHighlightKind, truncate_display_width, truncate_display_width_from},
        message_format::{
            MessageContentLine, format_message_content, format_message_content_lines,
            format_message_content_lines_with_loaded_custom_emoji_urls, lay_out_reaction_chips,
            mention_highlight_style, poll_box_border, poll_card_inner_width,
            reaction_line_test_spans, wrap_text_lines,
        },
        state::{
            ChannelSwitcherItem, ChannelThreadItem, DashboardState, DisplayOptionItem,
            EmojiPickerEntry, EmojiReactionItem, FocusPane, MessageActionItem, MessageActionKind,
            PollVotePickerItem,
        },
    },
};

#[test]
fn options_popup_lines_show_selected_toggle_state() {
    let items = vec![
        DisplayOptionItem {
            label: "Disable all image previews",
            enabled: false,
            value: None,
            effective: false,
            description: "Master switch.",
        },
        DisplayOptionItem {
            label: "Show avatars",
            enabled: true,
            value: None,
            effective: true,
            description: "Message and profile avatars.",
        },
        DisplayOptionItem {
            label: "Image preview quality",
            enabled: true,
            value: Some("balanced"),
            effective: true,
            description: "Attachment and embed previews.",
        },
    ];

    let lines = options_popup_lines(&items, 1, items.len(), 120);

    assert_eq!(lines[0].spans[1].content, "[ ] ");
    assert_eq!(lines[1].spans[0].content, "› ");
    assert_eq!(lines[1].spans[1].content, "[x] ");
    assert_eq!(lines[2].spans[1].content, "[balanced] ");
    assert_eq!(lines.len(), 3);
}

#[test]
fn options_popup_lines_keep_selected_item_visible_when_clipped() {
    let items = vec![
        DisplayOptionItem {
            label: "Option 1",
            enabled: true,
            value: None,
            effective: true,
            description: "First.",
        },
        DisplayOptionItem {
            label: "Option 2",
            enabled: true,
            value: None,
            effective: true,
            description: "Second.",
        },
        DisplayOptionItem {
            label: "Option 3",
            enabled: true,
            value: None,
            effective: true,
            description: "Third.",
        },
        DisplayOptionItem {
            label: "Option 4",
            enabled: true,
            value: None,
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
    state.open_options_popup();
    for _ in 0..5 {
        state.move_option_down();
    }

    let dump = render_dashboard_dump(100, 9, &mut state);
    let rendered = dump.join("\n");

    assert!(
        dump.iter()
            .any(|row| row.contains("›") && row.contains("Desktop notifications")),
        "{rendered}"
    );
}

#[test]
fn header_shows_available_update_version() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::UpdateAvailable {
        latest_version: "9.9.9".to_owned(),
    });

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Concord - v"), "{header}");
    assert!(header.contains("New version available: v9.9.9"), "{header}");
}

#[test]
fn header_shows_loading_before_connected_account_is_ready() {
    let mut state = DashboardState::new();

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Concord - v"), "{header}");
    assert!(header.contains("Loading..."), "{header}");
}

#[test]
fn header_shows_connected_account() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Concord - v"), "{header}");
    assert!(header.contains("Connected as muri"), "{header}");
    assert!(!header.contains("Loading..."), "{header}");
}

#[test]
fn channel_switcher_lines_show_search_and_grouped_selection() {
    let items = vec![
        ChannelSwitcherItem {
            channel_id: Id::new(1),
            guild_id: None,
            guild_name: None,
            group_label: "Direct Messages".to_owned(),
            parent_label: None,
            channel_label: "@alice".to_owned(),
            unread: ChannelUnreadState::Seen,
            unread_message_count: 0,
            search_name: "alice".to_owned(),
            depth: 0,
            group_order: 0,
            original_index: 0,
        },
        ChannelSwitcherItem {
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
            guild_name: Some("guild".to_owned()),
            group_label: "guild".to_owned(),
            parent_label: Some("Text".to_owned()),
            channel_label: "#general".to_owned(),
            unread: ChannelUnreadState::Seen,
            unread_message_count: 0,
            search_name: "general".to_owned(),
            depth: 1,
            group_order: 1,
            original_index: 1,
        },
    ];

    let lines = channel_switcher_lines(&items, 1, "gen", "gen".len(), 10, 40);

    assert_eq!(lines[0].spans[0].content, "🔎 ");
    assert_eq!(lines[0].spans[1].content, "gen");
    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("Direct Messages"))
    );
    assert!(lines.iter().any(|line| line.to_string().contains("guild")));
    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("Text / #general"))
    );
    assert!(!lines.iter().any(|line| line.to_string().contains("cursor")));
    assert!(
        !lines
            .iter()
            .any(|line| line.to_string().contains("type to filter"))
    );
}

#[test]
fn channel_switcher_lines_show_unread_badges_like_channel_pane() {
    let items = vec![ChannelSwitcherItem {
        channel_id: Id::new(1),
        guild_id: None,
        guild_name: None,
        group_label: "Direct Messages".to_owned(),
        parent_label: None,
        channel_label: "@new".to_owned(),
        unread: ChannelUnreadState::Unread,
        unread_message_count: 5,
        search_name: "new".to_owned(),
        depth: 0,
        group_order: 0,
        original_index: 0,
    }];

    let lines = channel_switcher_lines(&items, 0, "", 0, 10, 40);

    assert!(
        lines
            .iter()
            .any(|line| line.to_string().contains("(5) @new"))
    );
}

#[test]
fn selected_channel_switcher_unread_row_keeps_highlight() {
    let items = vec![ChannelSwitcherItem {
        channel_id: Id::new(1),
        guild_id: Some(Id::new(1)),
        guild_name: Some("guild".to_owned()),
        group_label: "guild".to_owned(),
        parent_label: None,
        channel_label: "#alerts".to_owned(),
        unread: ChannelUnreadState::Mentioned(2),
        unread_message_count: 0,
        search_name: "alerts".to_owned(),
        depth: 0,
        group_order: 0,
        original_index: 0,
    }];

    let lines = channel_switcher_lines(&items, 0, "", 0, 10, 40);
    let item_line = lines
        .iter()
        .find(|line| line.to_string().contains("#alerts"))
        .expect("selected channel row");
    let label = item_line.spans.last().expect("channel label span");

    assert_eq!(label.content, "#alerts");
    assert!(label.style.bg.is_some());
    assert_eq!(label.style.fg, Some(MENTION_ORANGE));
}

#[test]
fn channel_switcher_cursor_position_tracks_query_cursor() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    state.push_channel_switcher_char('g');
    state.push_channel_switcher_char('e');
    let right = channel_switcher_cursor_position(Rect::new(0, 0, 100, 20), &state)
        .expect("switcher cursor");

    state.move_channel_switcher_query_cursor_left();
    let left = channel_switcher_cursor_position(Rect::new(0, 0, 100, 20), &state)
        .expect("switcher cursor");

    assert_eq!(left.y, right.y);
    assert!(left.x < right.x);
}

#[test]
fn channel_switcher_search_line_windows_long_query_around_cursor() {
    let query = "abcdefghijklmnopqrstuvwxyz";

    let lines = channel_switcher_lines(&[], 0, query, query.len(), 10, 12);
    let rendered = lines[0].to_string();

    assert!(rendered.contains("uvwxyz"));
    assert!(!rendered.contains("abcdef"));
}

#[test]
fn channel_switcher_cursor_position_stays_on_search_row_for_long_query() {
    let mut state = DashboardState::new();
    state.open_channel_switcher();
    for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
        state.push_channel_switcher_char(ch);
    }

    let position =
        channel_switcher_cursor_position(Rect::new(0, 0, 40, 20), &state).expect("switcher cursor");

    assert_eq!(position.y, 2);
    assert!(position.x < 39);
}

#[test]
fn custom_emoji_markup_uses_id_fallback_when_disabled() {
    let message = message_with_content(Some("hello <:wave:42>".to_owned()));
    let state = DashboardState::new_with_display_options(DisplayOptions {
        show_custom_emoji: false,
        ..DisplayOptions::default()
    });

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(lines[0].text, "hello 42");
    assert!(lines[0].image_slots.is_empty());
}

#[test]
fn focus_pane_at_maps_dashboard_regions_and_ignores_non_panes() {
    let area = Rect::new(0, 0, 120, 20);
    let state = DashboardState::new();
    let cases = [
        (1, 1, Some(FocusPane::Guilds)),
        (21, 1, Some(FocusPane::Channels)),
        (50, 1, Some(FocusPane::Messages)),
        (100, 1, Some(FocusPane::Members)),
        (1, 0, None),
        (120, 1, None),
        (1, 20, None),
    ];

    for (x, y, expected) in cases {
        assert_eq!(focus_pane_at(area, &state, x, y), expected);
    }
}

#[test]
fn focus_pane_at_expands_messages_over_hidden_panes() {
    let area = Rect::new(0, 0, 120, 20);
    let mut state = DashboardState::new();

    state.toggle_pane_visibility(FocusPane::Channels);
    assert_eq!(
        focus_pane_at(area, &state, 21, 1),
        Some(FocusPane::Messages)
    );
    assert_eq!(focus_pane_at(area, &state, 95, 1), Some(FocusPane::Members));

    state.toggle_pane_visibility(FocusPane::Guilds);
    state.toggle_pane_visibility(FocusPane::Members);
    assert_eq!(focus_pane_at(area, &state, 1, 1), Some(FocusPane::Messages));
    assert_eq!(
        focus_pane_at(area, &state, 119, 1),
        Some(FocusPane::Messages)
    );
}

#[test]
fn focus_pane_at_uses_configured_pane_widths() {
    let state = DashboardState::new_with_display_options(DisplayOptions {
        server_width: 10,
        channel_list_width: 20,
        member_list_width: 15,
        ..DisplayOptions::default()
    });
    let area = Rect::new(0, 0, 100, 20);

    assert_eq!(focus_pane_at(area, &state, 9, 1), Some(FocusPane::Guilds));
    assert_eq!(
        focus_pane_at(area, &state, 10, 1),
        Some(FocusPane::Channels)
    );
    assert_eq!(
        focus_pane_at(area, &state, 30, 1),
        Some(FocusPane::Messages)
    );
    assert_eq!(focus_pane_at(area, &state, 85, 1), Some(FocusPane::Members));
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
fn reply_composer_text_uses_original_reply_target_after_selection_changes() {
    let mut state = state_with_message();
    state.open_selected_message_actions();
    state.activate_selected_message_action();
    push_message(&mut state, 2, "newer selected message");

    assert_eq!(
        state
            .selected_message_state()
            .and_then(|message| message.content.as_deref()),
        Some("newer selected message")
    );

    assert_eq!(composer_text(&state, 80), "reply to hello\n> ");
}

#[test]
fn reply_composer_hint_line_is_dim() {
    let mut state = state_with_message();
    state.open_selected_message_actions();
    state.activate_selected_message_action();

    let lines = composer_lines(&state, 80);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["reply to hello", "> "]
    );
    assert_eq!(lines[0].spans[0].style.fg, Some(DIM));
    assert_eq!(lines[1].spans[0].style.fg, None);
}

#[test]
fn composer_lines_show_pending_upload_above_input() {
    let mut state = state_with_message();
    state.start_composer();
    state.add_pending_composer_attachments(vec![MessageAttachmentUpload {
        path: "/tmp/cat.png".into(),
        filename: "cat.png".to_owned(),
        size_bytes: 2_048,
    }]);

    let lines = composer_lines(&state, 80);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["upload: cat.png (2.0 KiB)", "> "]
    );
    assert_eq!(lines[0].spans[0].style.fg, Some(ACCENT));
    assert_eq!(composer_content_line_count(&state, 80), 2);
}

#[test]
fn composer_lines_use_image_width_for_loaded_custom_emoji() {
    let mut state = state_with_message();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo {
            id: Id::new(60),
            name: "long_custom".to_owned(),
            animated: false,
            available: true,
        }],
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
    state.open_selected_message_actions();
    state.activate_selected_message_action();
    state.add_pending_composer_attachments(vec![MessageAttachmentUpload {
        path: "/tmp/cat.png".into(),
        filename: "cat.png".to_owned(),
        size_bytes: 2_048,
    }]);
    for value in "hi".chars() {
        state.push_composer_char(value);
    }

    assert_eq!(
        composer_cursor_position(Rect::new(10, 20, 20, 6), &state),
        Some(Position { x: 15, y: 23 })
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
            id: Id::new(50),
            name: "party_time".to_owned(),
            animated: true,
            available: true,
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
fn emoji_picker_lines_cross_out_unavailable_custom_emoji() {
    let lines = emoji_picker_lines(
        &[
            EmojiPickerEntry {
                emoji: "◆".to_owned(),
                shortcode: "gone".to_owned(),
                name: "custom emoji".to_owned(),
                wire_format: Some("<:gone:51>".to_owned()),
                available: false,
                custom_image_url: Some("https://cdn.discordapp.com/emojis/51.png".to_owned()),
            },
            EmojiPickerEntry {
                emoji: "❤️".to_owned(),
                shortcode: "heart".to_owned(),
                name: "red heart".to_owned(),
                wire_format: None,
                available: true,
                custom_image_url: None,
            },
            EmojiPickerEntry {
                emoji: "◆".to_owned(),
                shortcode: "party_time".to_owned(),
                name: "custom emoji".to_owned(),
                wire_format: Some("<:party_time:50>".to_owned()),
                available: true,
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
        !lines[2]
            .spans
            .last()
            .expect("custom emoji row should have a label span")
            .style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
    );
    assert_eq!(lines[2].spans[1].content.as_ref(), "   ");
}

#[test]
fn dashboard_renders_scrollbar_for_overflowing_composer_pickers() {
    let mut state = state_with_message();
    for index in 0..10 {
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: MemberInfo {
                user_id: Id::new(100 + index),
                display_name: format!("Scroll {index:02}"),
                username: Some(format!("scroll{index:02}")),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
    }
    state.start_composer();
    for ch in "@sc".chars() {
        state.push_composer_char(ch);
    }
    state.move_composer_mention_selection(9);

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains("Scroll 09"),
        "selected overflow mention candidate should stay visible:\n{rendered}"
    );
    assert!(
        !rendered.contains("@scroll00"),
        "picker should scroll away from the first row after selecting the bottom overflow candidate:\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "overflowing mention picker should render a scrollbar thumb:\n{rendered}"
    );

    let mut state = state_with_message();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: (0..10)
            .map(|index| CustomEmojiInfo {
                id: Id::new(100 + index),
                name: format!("overflow_{index:02}"),
                animated: false,
                available: true,
            })
            .collect(),
    });
    state.start_composer();
    for ch in ":ov".chars() {
        state.push_composer_char(ch);
    }
    state.move_composer_emoji_selection(9);

    let dump = render_dashboard_dump(100, 24, &mut state);
    let rendered = dump.join("\n");

    assert!(
        rendered.contains(":overflow_09:"),
        "selected overflow emoji candidate should stay visible:\n{rendered}"
    );
    assert!(
        !rendered.contains(":overflow_00:"),
        "picker should scroll away from the first row after selecting the bottom overflow candidate:\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "overflowing emoji picker should render a scrollbar thumb:\n{rendered}"
    );
}

#[test]
fn one_to_one_dm_carries_presence_in_dot() {
    let channel = channel_with_recipients("dm", &[PresenceStatus::DoNotDisturb]);

    let dot = dm_presence_dot_span(&channel).expect("1-on-1 DM should produce a presence dot");
    assert_eq!(dot.content.as_ref(), "● ");
    assert_eq!(dot.style.fg, Some(Color::Red));
}

#[test]
fn channel_unread_decoration_matches_unread_state() {
    let base = Style::default().fg(Color::White);
    let cases = [
        (ChannelUnreadState::Seen, None, Some(READ_DIM), false),
        (ChannelUnreadState::Unread, None, Some(UNREAD_BRIGHT), true),
        (
            ChannelUnreadState::Mentioned(3),
            Some(("(3) ", MENTION_ORANGE)),
            Some(MENTION_ORANGE),
            true,
        ),
        (
            ChannelUnreadState::Notified(3),
            Some(("(3) ", UNREAD_BRIGHT)),
            Some(UNREAD_BRIGHT),
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
fn server_pane_shows_guild_mention_badge() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(10)),
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(10)),
            mention_count: 2,
        }],
    });
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let server_rows = (0..buffer.area.height)
        .map(|row| {
            (0..20)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(server_rows.iter().any(|row| row.contains("(2)")));
}

#[test]
fn active_server_mention_badge_keeps_active_name_style() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(10)),
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.set_guild_view_height(20);
    assert!(state.select_visible_pane_row(FocusPane::Guilds, 1));
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            channel_id,
            last_acked_message_id: Some(Id::new(10)),
            mention_count: 2,
        }],
    });
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_guilds(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let mut checked = false;
    for row in 0..buffer.area.height {
        let text = (0..buffer.area.width)
            .map(|col| buffer[(col, row)].symbol().to_owned())
            .collect::<String>();
        if let Some(badge_col) = text.find("(2)") {
            let name_col = text[badge_col..]
                .find('g')
                .map(|offset| badge_col + offset)
                .expect("guild name starts with g after mention badge");
            assert_eq!(buffer[(badge_col as u16, row)].fg, MENTION_ORANGE);
            assert_eq!(buffer[(name_col as u16, row)].fg, Color::Green);
            assert!(
                buffer[(name_col as u16, row)]
                    .modifier
                    .contains(Modifier::BOLD)
            );
            checked = true;
            break;
        }
    }

    assert!(
        checked,
        "active guild row should include mention badge and guild name"
    );
}

#[test]
fn server_pane_shows_direct_message_unread_channel_count() {
    let state = state_with_unread_direct_messages();
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_guilds(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let server_rows = (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(server_rows.iter().any(|row| row.contains("(1)")));
}

#[test]
fn muted_server_name_is_dimmed() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(guild_id),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: true,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }],
    });
    state.set_guild_view_height(20);
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_guilds(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let mut checked = false;
    for row in 0..buffer.area.height {
        let text = (0..buffer.area.width)
            .map(|col| buffer[(col, row)].symbol().to_owned())
            .collect::<String>();
        if let Some(name_col) = text.find("guild") {
            assert!(
                buffer[(name_col as u16, row)]
                    .modifier
                    .contains(Modifier::DIM)
            );
            checked = true;
            break;
        }
    }

    assert!(checked, "muted guild row should render guild name");
}

#[test]
fn dm_channel_pane_shows_unread_channel_count_badge() {
    let mut state = state_with_unread_direct_messages();
    state.confirm_selected_guild();
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_channels(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let channel_rows = (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(channel_rows.iter().any(|row| row.contains("(1) @ new")));
}

#[test]
fn dm_channel_pane_shows_loaded_unread_message_count_badge() {
    let mut state = state_with_unread_direct_messages_with_loaded_unread_messages(5);
    state.confirm_selected_guild();
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_channels(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let channel_rows = (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(channel_rows.iter().any(|row| row.contains("(5) @ new")));
    assert!(!channel_rows.iter().any(|row| row.contains("(1) @ new")));
}

#[test]
fn muted_category_and_channel_names_are_dimmed() {
    let mut state = DashboardState::new();
    let guild_id = Id::new(1);
    let category_id = Id::new(10);
    let channel_id = Id::new(11);
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: category_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Text Channels".to_owned(),
                kind: "category".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: Some(category_id),
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(guild_id),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                channel_id: category_id,
                message_notifications: None,
                muted: true,
                mute_end_time: None,
            }],
        }],
    });
    state.set_channel_view_height(20);
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_channels(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let mut saw_category = false;
    let mut saw_channel = false;
    for row in 0..buffer.area.height {
        let text = (0..buffer.area.width)
            .map(|col| buffer[(col, row)].symbol().to_owned())
            .collect::<String>();
        if let Some(name_col) = text.find("Text Channels") {
            assert!(
                buffer[(name_col as u16, row)]
                    .modifier
                    .contains(Modifier::DIM)
            );
            saw_category = true;
        }
        if let Some(name_col) = text.find("general") {
            assert!(
                buffer[(name_col as u16, row)]
                    .modifier
                    .contains(Modifier::DIM)
            );
            saw_channel = true;
        }
    }

    assert!(
        saw_category,
        "muted category row should render category name"
    );
    assert!(
        saw_channel,
        "muted category child row should render channel name"
    );
}

#[test]
fn message_viewport_author_uses_resolved_role_color() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(99);
    let role_id = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: vec![MemberInfo {
            user_id: author_id,
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: vec![role_id],
        }],
        presences: vec![(author_id, PresenceStatus::Online)],
        roles: vec![RoleInfo {
            id: role_id,
            name: "Blue".to_owned(),
            color: Some(0x3366CC),
            position: 10,
            hoist: false,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(1),
        author_id,
        author: "fallback".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let messages = state.messages();
    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::message_viewport_layout(40, 80, 80, 16, 3),
        &[],
    );

    assert_eq!(
        lines[1].spans[1].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
}

#[test]
fn pinned_message_remains_selectable_for_unpin_action() {
    let mut state = state_with_message();
    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(10, "mod", "important announcement", true)],
    });
    state.enter_pinned_message_view(Id::new(2));
    state.jump_bottom();

    assert_eq!(
        state.selected_message_state().map(|message| message.pinned),
        Some(true)
    );
    state.open_selected_message_actions();

    assert!(state.selected_message_action_items().iter().any(|action| {
        action.kind == MessageActionKind::SetPinned(false) && action.label == "Unpin message"
    }));
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

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![message_info(10, "mod", "important announcement", false)],
    });

    assert_eq!(state.pinned_messages().len(), 1);
    assert!(
        state
            .messages()
            .into_iter()
            .any(|message| message.id == Id::new(10) && message.pinned)
    );
}

#[test]
fn forum_post_lines_render_title_author_and_preview() {
    let post = ChannelThreadItem {
        channel_id: Id::new(30),
        section_label: Some("Active posts".to_owned()),
        label: "A useful Rust crate".to_owned(),
        archived: false,
        locked: true,
        pinned: true,
        preview_author_id: Some(Id::new(99)),
        preview_author: Some("neo".to_owned()),
        preview_author_color: Some(0x3366CC),
        preview_content: Some("This crate solves a small but annoying problem".to_owned()),
        preview_reactions: vec![ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            count: 2,
            me: true,
        }],
        comment_count: Some(4),
        last_activity_message_id: Some(Id::new(30)),
    };

    let lines = forum_post_viewport_lines(&[post], Some(0), 80, false);
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.len(), 6);
    assert_eq!(texts[0].trim_end(), "Active posts");
    assert!(texts[1].starts_with("› ╭"));
    assert!(!texts[1].contains("Active posts"));
    assert!(texts.iter().all(|text| text.width() == 80));
    assert!(texts[2].contains("A useful Rust crate"));
    assert!(texts[2].contains("PINNED"));
    assert!(texts[3].contains("neo: This crate solves"));
    assert!(texts[4].contains("4 comments"));
    assert!(texts[4].contains("[👍 2]"));
    assert!(!texts[4].contains("pinned"));
    assert!(texts[4].contains("locked"));
    assert!(texts[5].starts_with("  ╰"));
    assert_eq!(lines[2].spans[2].style.fg, Some(Color::White));
    assert_eq!(lines[2].spans[3].style.fg, Some(Color::Yellow));
    assert_eq!(
        lines[3].spans[2].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
    assert_eq!(lines[3].spans[4].style.fg, Some(Color::White));
    assert_eq!(lines[4].spans[2].style.fg, Some(Color::White));
    assert_eq!(lines[4].spans[4].style.fg, Some(Color::Yellow));
    assert_eq!(lines[4].spans[6].style.fg, Some(Color::White));
    assert_eq!(lines[1].spans[1].style.fg, Some(SELECTED_FORUM_POST_BORDER));
    assert_eq!(lines[2].spans[1].style.fg, Some(SELECTED_FORUM_POST_BORDER));
    assert!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .all(|span| span.style.bg.is_none())
    );
}

#[test]
fn forum_post_reaction_summary_reserves_custom_emoji_image_slot() {
    let reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: false,
        },
        count: 1,
        me: true,
    }];

    assert_eq!(
        forum_post_reaction_summary(&reactions, 80).as_deref(),
        Some("[   1]")
    );
}

#[test]
fn forum_post_scrollbar_visible_count_uses_rendered_rows() {
    assert_eq!(forum_post_scrollbar_visible_count(10), 10);
    assert_eq!(forum_post_scrollbar_visible_count(0), 1);
}

#[test]
fn forum_post_lines_can_reserve_scrollbar_column() {
    let post = ChannelThreadItem {
        channel_id: Id::new(30),
        section_label: None,
        label: "A useful Rust crate".to_owned(),
        archived: false,
        locked: false,
        pinned: false,
        preview_author_id: Some(Id::new(99)),
        preview_author: Some("neo".to_owned()),
        preview_author_color: None,
        preview_content: Some("short preview".to_owned()),
        preview_reactions: Vec::new(),
        comment_count: Some(1),
        last_activity_message_id: Some(Id::new(30)),
    };

    let lines = forum_post_viewport_lines(
        &[post],
        Some(0),
        selected_message_card_width(80, true),
        false,
    );
    let texts = line_texts_from_ratatui(&lines);

    assert!(texts[0].starts_with("› ╭"));
    assert!(texts[0].ends_with("╮"));
    assert!(texts[1].ends_with("│"));
    assert!(texts[4].ends_with("╯"));
    assert!(texts.iter().all(|text| text.width() == 79));
}

#[test]
fn forum_post_render_shows_scrollbar_when_posts_exceed_visible_cards() {
    let mut state = state_with_forum_posts(10);

    let dump = render_dashboard_dump(100, 20, &mut state);

    assert!(dump.iter().any(|line| line.contains('┃')));
}

#[test]
fn history_message_author_uses_channel_guild_for_role_color() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(99);
    let role_id = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: vec![MemberInfo {
            user_id: author_id,
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: vec![role_id],
        }],
        presences: vec![(author_id, PresenceStatus::Online)],
        roles: vec![RoleInfo {
            id: role_id,
            name: "Blue".to_owned(),
            color: Some(0x3366CC),
            position: 10,
            hoist: false,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(1),
            author_id,
            author: "fallback".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            ..MessageInfo::default()
        }],
    });

    let messages = state.messages();
    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::message_viewport_layout(40, 80, 80, 16, 3),
        &[],
    );

    assert_eq!(
        lines[1].spans[1].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
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
fn primary_activity_summary_prefers_game_over_custom_status() {
    let activities = vec![
        ActivityInfo {
            kind: ActivityKind::Playing,
            name: "Concord".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        },
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
    ];

    assert_eq!(
        primary_activity_summary(&activities, &[]).map(|r| r.to_display_string()),
        Some("▶ Concord".to_owned())
    );
}

#[test]
fn primary_activity_summary_listening_includes_track_and_artist() {
    let activities = vec![ActivityInfo {
        kind: ActivityKind::Listening,
        name: "Spotify".to_owned(),
        details: Some("Bohemian Rhapsody".to_owned()),
        state: Some("Queen".to_owned()),
        url: None,
        application_id: None,
        emoji: None,
    }];
    assert_eq!(
        primary_activity_summary(&activities, &[]).map(|r| r.to_display_string()),
        Some("♪ Spotify - Bohemian Rhapsody by Queen".to_owned())
    );
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
fn offline_like_dm_status_uses_empty_dim_presence_marker() {
    for status in [PresenceStatus::Offline, PresenceStatus::Unknown] {
        let channel = channel_with_recipients("dm", &[status]);

        let dot = dm_presence_dot_span(&channel).expect("DM should still produce a dot");
        assert_eq!(dot.content.as_ref(), "○ ");
        assert_eq!(dot.style.fg, Some(Color::DarkGray));
    }
}

#[test]
fn group_dm_has_no_presence_dot() {
    let channel = channel_with_recipients(
        "group-dm",
        &[PresenceStatus::Online, PresenceStatus::DoNotDisturb],
    );

    assert!(dm_presence_dot_span(&channel).is_none());
}

#[test]
fn reply_composer_line_count_includes_reply_hint() {
    let mut state = state_with_message();
    state.open_selected_message_actions();
    state.activate_selected_message_action();
    state.push_composer_char('h');
    state.push_composer_char('\n');
    state.push_composer_char('i');

    assert_eq!(composer_content_line_count(&state, 80), 3);
}

#[test]
fn image_attachment_replaces_empty_message_placeholder() {
    let message = message_with_attachment(Some(String::new()), image_attachment());

    assert_eq!(
        format_message_content(&message, 200),
        "[image: cat.png] 640x480"
    );
}

#[test]
fn attachment_summary_uses_own_accent_line_after_text_content() {
    let message = message_with_attachment(Some("look".to_owned()), image_attachment());
    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["look", "[image: cat.png] 640x480"]);
    assert_eq!(lines[1].style, Style::default().fg(ACCENT));
}

#[test]
fn edited_message_appends_dim_italic_marker_to_content() {
    let mut message = message_with_content(Some("hello".to_owned()));
    message.edited_timestamp = Some("2026-05-07T12:34:56.000000+00:00".to_owned());

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello (edited)"]);
    let marker = lines[0]
        .spans()
        .into_iter()
        .find(|span| span.content == " (edited)")
        .expect("edited marker span should be present");
    assert_eq!(marker.style.fg, Some(DIM));
    assert!(marker.style.add_modifier.contains(Modifier::ITALIC));
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
    assert_eq!(marker.style.fg, Some(DIM));
    assert!(marker.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn attachment_summary_renders_multiple_attachments_one_per_line() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.attachments.push(file_attachment());

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec!["look", "[image: cat.png] 640x480", "[file: notes.txt]"]
    );
    assert_eq!(lines[1].style, Style::default().fg(ACCENT));
    assert_eq!(lines[2].style, Style::default().fg(ACCENT));
}

#[test]
fn message_content_lines_render_discord_embed_preview() {
    let mut message = message_with_content(Some(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
    ));
    message.embeds = vec![youtube_embed()];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

    assert_eq!(
        line_texts(&lines),
        vec![
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "  ▎ YouTube",
            "  ▎ Example Video",
        ]
    );
    assert_eq!(lines[1].style.fg, Some(DIM));
    assert!(lines[2].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(lines[2].style.fg, Some(Color::Blue));
    let marker_spans = lines[1].spans();
    assert_eq!(marker_spans[0].content.as_ref(), "  ▎ ");
    assert_eq!(marker_spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
    assert!(
        !marker_spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn message_embed_hides_media_and_player_urls() {
    let mut message = message_with_content(Some("watch this".to_owned()));
    let mut embed = youtube_embed();
    embed.video_url = Some("https://www.youtube.com/embed/dQw4w9WgXcQ".to_owned());
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

    assert_eq!(
        line_texts(&lines),
        vec![
            "watch this",
            "  ▎ YouTube",
            "  ▎ Example Video",
            "  ▎ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ]
    );
}

#[test]
fn message_embed_url_underline_skips_marker() {
    let mut message = message_with_content(Some("watch this".to_owned()));
    let mut embed = youtube_embed();
    embed.description = None;
    embed.image_url = None;
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 80);
    let url_spans = lines[3].spans();

    assert_eq!(
        line_texts(&lines),
        vec![
            "watch this",
            "  ▎ YouTube",
            "  ▎ Example Video",
            "  ▎ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ]
    );
    assert_eq!(url_spans[0].content.as_ref(), "  ▎ ");
    assert_eq!(url_spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
    assert!(
        !url_spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
    assert_eq!(
        url_spans[1].content.as_ref(),
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    );
    assert!(
        url_spans[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
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
fn loaded_custom_emoji_message_uses_image_width() {
    let message = message_with_content(Some("<:long_custom:42>text".to_owned()));
    let loaded_urls = vec!["https://cdn.discordapp.com/emojis/42.png".to_owned()];

    for width in [200, 6] {
        let lines = format_message_content_lines_with_loaded_custom_emoji_urls(
            &message,
            &DashboardState::new(),
            width,
            &loaded_urls,
        );

        assert_eq!(line_texts(&lines), vec!["  text"]);
        assert_eq!(lines[0].image_slots[0].col, 0);
        assert_eq!(lines[0].image_slots[0].display_width, 2);
    }
}

#[test]
fn message_embed_does_not_repeat_body_url() {
    let mut message = message_with_content(Some(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
    ));
    let mut embed = youtube_embed();
    embed.title = None;
    embed.description = None;
    embed.image_url = None;
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

    assert_eq!(
        line_texts(&lines),
        vec!["https://www.youtube.com/watch?v=dQw4w9WgXcQ", "  ▎ YouTube"]
    );
}

#[test]
fn message_content_preserves_explicit_newlines() {
    let message = message_with_content(Some("hello\nworld".to_owned()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello", "world"]);
}

#[test]
fn message_content_wraps_long_lines_to_content_width() {
    let message = message_with_content(Some("abcdefghijkl".to_owned()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 5);

    assert_eq!(line_texts(&lines), vec!["abcde", "fghij", "kl"]);
}

#[test]
fn message_content_wraps_wide_characters_by_terminal_width() {
    let message = message_with_content(Some("漢字仮名交じ".to_owned()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 10);

    assert_eq!(line_texts(&lines), vec!["漢字仮名交", "じ"]);
}

#[test]
fn message_content_renders_known_user_mentions() {
    let message = message_with_content(Some("hello <@10>".to_owned()));
    let state = state_with_member(10, "alice");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["hello @alice"]);
}

#[test]
fn message_content_keeps_unknown_user_mentions_raw() {
    let message = message_with_content(Some("hello <@10>".to_owned()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello <@10>"]);
}

#[test]
fn message_content_renders_mentions_from_message_metadata() {
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "alice")];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello @alice"]);
}

#[test]
fn message_content_highlights_current_user_mentions() {
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "username")];
    let mut state = state_with_member(10, "server alias");
    state.push_event(AppEvent::Ready {
        user: "server alias".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   hello @server alias", ""]
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), "@server alias");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_highlights_other_user_mentions_with_softer_color() {
    // Discord still paints non-self mentions, just with a calmer tint than
    // the gold "you" highlight, so the user can tell whether they were the
    // one being pinged at a glance.
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "alice")];
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   hello @alice", ""]
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), "@alice");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::OtherMention).bg
    );
    assert_ne!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg,
        "other-user mentions must not look like a self-mention notification"
    );
}

#[test]
fn message_content_highlights_everyone_mentions_for_current_user() {
    let message = message_with_content(Some("ping @everyone".to_owned()));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   ping @everyone", ""]
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_highlights_mixed_everyone_and_direct_mentions_in_order() {
    let mut message = message_with_content(Some("@everyone hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "neo")];
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   @everyone hello @neo", ""]
    );
    assert_eq!(lines[1].spans[1].content.as_ref(), "@everyone");
    assert_eq!(lines[1].spans[3].content.as_ref(), "@neo");
    assert_eq!(
        lines[1].spans[1].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
    assert_eq!(
        lines[1].spans[3].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_highlights_here_mentions_for_current_user() {
    let message = message_with_content(Some("ping @here".to_owned()));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   ping @here", ""]
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), "@here");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_highlights_role_mentions_with_role_name() {
    let message = message_with_content(Some("hello <@&10>".to_owned()));
    let state = state_with_role(10, "moderators");

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   hello @moderators", ""]
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), "@moderators");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::OtherMention).bg
    );
}

#[test]
fn message_content_keeps_role_mentions_raw_without_guild_context() {
    let mut message = message_with_content(Some("hello <@&10>".to_owned()));
    message.guild_id = None;
    let state = state_with_role(10, "moderators");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["hello <@&10>"]);
}

#[test]
fn mention_like_display_name_does_not_duplicate_highlight_spans() {
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "everyone")];
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "everyone".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let lines = message_item_lines(
        message.author.clone(),
        message_author_style(None),
        "00:00".to_owned(),
        format_message_content_lines(&message, &state, 200),
        40,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   hello @everyone", ""]
    );
    assert_eq!(lines[1].spans.len(), 3);
    assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
    assert_eq!(
        lines[1].spans[2].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_prefers_cached_member_alias_over_mention_metadata() {
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info(10, "username")];
    let state = state_with_member(10, "server alias");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["hello @server alias"]);
}

#[test]
fn message_content_prefers_message_mention_nick_over_cached_member_name() {
    let mut message = message_with_content(Some("hello <@10>".to_owned()));
    message.mentions = vec![mention_info_with_nick(10, "server alias")];
    let state = state_with_member(10, "username");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["hello @server alias"]);
}

#[test]
fn message_content_does_not_split_grapheme_clusters() {
    let lines = wrap_text_lines("👨‍👩‍👧‍👦", 7);

    assert_eq!(lines, vec!["👨‍👩‍👧‍👦".to_owned()]);
}

#[test]
fn message_content_preserves_blank_lines() {
    let message = message_with_content(Some("one\n\nthree".to_owned()));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["one", "", "three"]);
}

#[test]
fn video_attachment_is_labeled_as_video() {
    let message = message_with_attachment(Some(String::new()), video_attachment());

    assert_eq!(
        format_message_content(&message, 200),
        "[video: clip.mp4] 1920x1080"
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
    assert_eq!(lines[0].style, Style::default().fg(DIM));
}

#[test]
fn user_join_message_type_uses_join_label() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(7);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["joined the server"]);
    assert_eq!(lines[0].style, Style::default().fg(DIM));
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
        assert_eq!(lines[0].style, Style::default().fg(ACCENT));
    }
}

#[test]
fn thread_created_message_uses_cached_thread_details() {
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(10 * 60 * 1000));
    let latest_thread_message_id =
        snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(message.channel_id),
        position: None,
        last_message_id: Some(latest_thread_message_id),
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(false),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let lines = format_message_content_lines(&message, &state, 200);
    let texts = line_texts(&lines);

    assert_eq!(texts[0], "neo started release notes thread.");
    assert!(texts[1].starts_with("  ╭"));
    assert!(texts[2].starts_with("  │ release notes"));
    assert!(texts[2].contains("12 messages"));
    assert!(texts[3].contains("2 minutes ago"));
    assert!(texts[4].starts_with("  ╰"));
    assert_eq!(lines[0].style, Style::default().fg(Color::White));
    assert_eq!(lines[3].style, Style::default().fg(DIM));
}

#[test]
fn thread_created_message_uses_cached_thread_message_when_last_id_missing() {
    let now = current_unix_millis();
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id = snowflake_for_unix_ms(now.saturating_sub(10 * 60 * 1000));
    let latest_thread_message_id = snowflake_for_unix_ms(now.saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(message.channel_id),
        position: None,
        last_message_id: None,
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(false),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        message_id: latest_thread_message_id,
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("latest reply".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let lines = format_message_content_lines(&message, &state, 200);
    let texts = line_texts(&lines);

    assert!(texts[2].contains("13 messages"));
    assert!(texts[3].contains("neo latest reply 2 minutes ago"));
}

#[test]
fn thread_created_message_falls_back_to_system_message_time() {
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(message.channel_id),
        position: None,
        last_message_id: None,
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(false),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let lines = format_message_content_lines(&message, &state, 200);
    let texts = line_texts(&lines);

    assert!(texts[2].contains("12 messages"));
    assert!(texts[3].contains("2 minutes ago"));
}

#[test]
fn thread_created_message_keeps_archived_and_locked_metadata() {
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(10),
        parent_id: Some(message.channel_id),
        position: None,
        last_message_id: None,
        name: "release notes".to_owned(),
        kind: "thread".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_archived: Some(true),
        thread_locked: Some(true),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));

    let lines = format_message_content_lines(&message, &state, 200);

    assert!(line_texts(&lines)[3].contains("archived · locked"));
}

#[test]
fn thread_starter_message_uses_referenced_message_card() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(21);
    message.reply = Some(ReplyInfo {
        author: "alice".to_owned(),
        content: Some("original topic".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec!["Thread starter message", "╭─ alice : original topic"]
    );
}

#[test]
fn poll_result_message_uses_result_card() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(46);
    message.poll = Some(PollInfo {
        question: "What should we eat?".to_owned(),
        answers: vec![PollAnswerInfo {
            answer_id: 1,
            text: "Soup".to_owned(),
            vote_count: Some(5),
            me_voted: false,
        }],
        allow_multiselect: false,
        results_finalized: Some(true),
        total_votes: Some(7),
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
        author: "casey".to_owned(),
        content: Some("looks good".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
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
    assert_eq!(lines[0].style, Style::default().fg(DIM));
}

#[test]
fn reply_preview_renders_known_user_mentions() {
    let mut message = message_with_content(Some("asdf".to_owned()));
    message.message_kind = MessageKind::new(19);
    message.reply = Some(ReplyInfo {
        author: "neo".to_owned(),
        content: Some("hello <@10>".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
    });
    let state = state_with_member(10, "alice");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
}

#[test]
fn reply_preview_renders_mentions_from_reply_metadata() {
    let mut message = message_with_content(Some("asdf".to_owned()));
    message.message_kind = MessageKind::new(19);
    message.reply = Some(ReplyInfo {
        author: "neo".to_owned(),
        content: Some("hello <@10>".to_owned()),
        sticker_names: Vec::new(),
        mentions: vec![mention_info(10, "alice")],
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
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
    assert_eq!(lines[2].style, Style::default().fg(DIM));
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
fn poll_message_body_highlights_mentions_inside_box() {
    let mut message = message_with_content(Some("<@10> please vote".to_owned()));
    message.mentions = vec![mention_info(10, "server alias")];
    message.poll = Some(poll_info(false));
    let mut state = state_with_member(10, "server alias");
    state.push_event(AppEvent::Ready {
        user: "server alias".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let lines = format_message_content_lines(&message, &state, 40);
    let spans = lines[2].spans();

    assert_eq!(spans[0].content.as_ref(), "│ ");
    assert_eq!(spans[1].content.as_ref(), "@server alias");
    assert_eq!(
        spans[1].style.bg,
        mention_highlight_style(TextHighlightKind::SelfMention).bg
    );
}

#[test]
fn message_content_renders_reaction_chips_below_message() {
    let mut message = message_with_content(Some("hello".to_owned()));
    message.reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 3,
        me: true,
    }];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello", "[👍 3]"]);
    let spans = lines[1].spans();
    assert_eq!(spans[0].content.as_ref(), "[👍 3]");
    assert_eq!(spans[0].style, Style::default().fg(Color::Yellow));
}

#[test]
fn lay_out_reaction_chips_unicode_only_emits_no_image_slots() {
    let reactions = vec![
        ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            count: 3,
            me: true,
        },
        ReactionInfo {
            emoji: ReactionEmoji::Unicode("❤".to_owned()),
            count: 1,
            me: false,
        },
    ];

    let layout = lay_out_reaction_chips(&reactions, 200);

    assert_eq!(layout.lines, vec!["[👍 3]  [❤ 1]"]);
    assert_eq!(layout.self_ranges.len(), 1);
    let spans = reaction_line_test_spans(&layout.lines[0], &layout.self_ranges, 0);
    assert_eq!(spans[0].content.as_ref(), "[👍 3]");
    assert_eq!(spans[0].style, Style::default().fg(Color::Yellow));
    assert_eq!(spans[1].style, Style::default().fg(ACCENT));
    assert!(layout.slots.is_empty());
}

#[test]
fn lay_out_reaction_chips_custom_emoji_reserves_image_slot() {
    let reactions = vec![
        ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            count: 2,
            me: false,
        },
        ReactionInfo {
            emoji: ReactionEmoji::Custom {
                id: Id::new(42),
                name: Some("party".to_owned()),
                animated: false,
            },
            count: 1,
            me: true,
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
            emoji: ReactionEmoji::Custom {
                id: Id::new(100 + i),
                name: Some(format!("e{i}")),
                animated: false,
            },
            count: i + 1,
            me: false,
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
fn message_action_menu_marks_selected_and_disabled_actions() {
    let actions = vec![
        MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "Reply".to_owned(),
            enabled: true,
        },
        MessageActionItem {
            kind: MessageActionKind::DownloadAttachment(0),
            label: "Download file".to_owned(),
            enabled: false,
        },
    ];

    let lines = message_action_menu_lines(&actions, 1);

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["  [r] Reply", "› [f] Download file (unavailable)",]
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
            super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
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
            super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
        })
        .expect("second draw");
    for _ in 0..6 {
        state.scroll_reaction_users_popup_up();
    }
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
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
fn forwarded_snapshot_attachment_replaces_empty_message_placeholder() {
    let message =
        message_with_forwarded_snapshot(forwarded_snapshot(Some(""), vec![image_attachment()]));

    assert_eq!(
        format_message_content(&message, 200),
        "↱ Forwarded │ [image: cat.png] 640x480"
    );
}

#[test]
fn forwarded_snapshot_content_appends_attachment_summary() {
    let message = message_with_forwarded_snapshot(forwarded_snapshot(
        Some("hello"),
        vec![image_attachment()],
    ));

    assert_eq!(
        format_message_content(&message, 200),
        "↱ Forwarded │ hello │ [image: cat.png] 640x480"
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
fn forwarded_snapshot_content_renders_known_user_mentions() {
    let message =
        message_with_forwarded_snapshot(forwarded_snapshot(Some("hello <@10>"), Vec::new()));
    let state = state_with_member(10, "alice");

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(line_texts(&lines), vec!["↱ Forwarded", "│ hello <@10>"]);
}

#[test]
fn forwarded_snapshot_content_uses_source_channel_guild_for_mentions() {
    let mut snapshot = forwarded_snapshot(Some("hello <@10>"), Vec::new());
    snapshot.source_channel_id = Some(Id::new(9));
    let message = message_with_forwarded_snapshot(snapshot);
    let mut state = state_with_member(10, "outer");
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
        member: member_info(10, "source"),
    });

    let lines = format_message_content_lines(&message, &state, 200);

    assert_eq!(
        line_texts(&lines),
        vec!["↱ Forwarded", "│ hello @source", "│ #source"]
    );
}

#[test]
fn forwarded_snapshot_content_renders_mentions_from_snapshot_metadata() {
    let mut snapshot = forwarded_snapshot(Some("hello <@10>"), Vec::new());
    snapshot.mentions = vec![mention_info(10, "alice")];
    let message = message_with_forwarded_snapshot(snapshot);

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["↱ Forwarded", "│ hello @alice"]);
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
            channel_id: Id::new(9),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
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
    assert_eq!(lines[2].style, Style::default().fg(DIM));
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
fn image_preview_rows_are_part_of_the_message_item() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("look".to_owned())],
        14,
        3,
        None,
        0,
    );

    assert_eq!(lines.len(), 6);
}

#[test]
fn message_viewport_lines_reserve_rows_for_multiple_attachment_summaries() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.attachments = image_attachments(2);
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );

    assert_eq!(lines.len(), 8);
}

#[test]
fn message_viewport_lines_put_reactions_below_image_preview_rows() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 3,
        me: true,
    }];
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );

    assert_eq!(lines.len(), 8);
    assert_eq!(line_texts_from_ratatui(&lines)[6], "   [👍 3]");
}

#[test]
fn message_viewport_lines_group_consecutive_messages_by_author() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "follow-up");
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.iter().filter(|text| text.contains("neo")).count(), 1);
    assert_eq!(texts[2], "   hello");
    assert_eq!(texts[3], "   follow-up");
}

#[test]
fn message_viewport_lines_start_new_author_group_after_time_gap() {
    let base = 1_743_465_600_000;
    let mut state = state_with_message_id(snowflake_for_unix_ms(base), "hello");
    push_message_with_id(
        &mut state,
        snowflake_for_unix_ms(base + 7 * 60 * 1000),
        "later follow-up",
    );
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.iter().filter(|text| text.contains("neo")).count(), 2);
    assert!(state.message_starts_author_group_at(1));
}

#[test]
fn selected_grouped_continuation_uses_empty_top_border() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "follow-up");
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        Some(1),
        &state,
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert!(texts[3].starts_with("╭"));
    assert!(!texts[3].contains("follow-up"));
    assert!(texts[4].starts_with("│"));
    assert!(texts[4].contains("follow-up"));
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
fn message_viewport_lines_keep_reactions_below_reacted_grouped_message() {
    let mut state = state_with_message();
    state.push_event(AppEvent::MessageReactionAdd {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        user_id: Id::new(100),
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
    });
    push_message(&mut state, 2, "follow-up");
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts[2], "   hello");
    assert_eq!(texts[3], "   [👍 1]");
    assert_eq!(texts[4], "   follow-up");
}

#[test]
fn message_viewport_lines_reserve_bounded_rows_for_image_albums() {
    for (attachment_count, expected_lines, overflow_text) in [
        (3, 9, None),
        (4, 10, None),
        (5, 12, Some("   +1 more images")),
    ] {
        let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
        message.attachments = image_attachments(attachment_count);
        let messages = [&message];

        let lines = message_viewport_lines(
            &messages,
            None,
            &DashboardState::new(),
            super::message_viewport_layout(200, 80, 80, 16, 3),
            &[],
        );

        assert_eq!(lines.len(), expected_lines);
        if let Some(overflow_text) = overflow_text {
            assert!(line_texts_from_ratatui(&lines).contains(&overflow_text.to_owned()));
        }
    }
}

#[test]
fn embed_image_preview_rows_continue_embed_gutter() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("look".to_owned())],
        14,
        2,
        Some(0xff0000),
        0,
    );

    assert_eq!(line_texts_from_ratatui(&lines)[2], "     ▎ ");
    assert_eq!(lines[2].spans[1].style.fg, Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn text_only_message_item_has_header_and_content_rows() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("look".to_owned())],
        14,
        0,
        None,
        0,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["oo neo 00:00", "   look", ""]
    );
}

#[test]
fn message_item_lines_can_start_after_line_offset() {
    let lines = message_item_lines(
        "neo".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![
            MessageContentLine::plain("first".to_owned()),
            MessageContentLine::plain("second".to_owned()),
            MessageContentLine::plain("third".to_owned()),
        ],
        14,
        0,
        None,
        2,
    );

    assert_eq!(
        line_texts_from_ratatui(&lines),
        vec!["   second", "   third", ""]
    );
}

#[test]
fn message_item_header_uses_display_width_for_wide_author() {
    let ascii = message_item_lines(
        "bruised8".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("plain text".to_owned())],
        14,
        0,
        None,
        0,
    );
    let wide = message_item_lines(
        "漢字名".to_owned(),
        message_author_style(None),
        "00:00".to_owned(),
        vec![MessageContentLine::plain("plain text".to_owned())],
        14,
        0,
        None,
        0,
    );

    assert_eq!(line_texts_from_ratatui(&ascii)[0], "oo bruised8 00:00");
    assert_eq!(line_texts_from_ratatui(&wide)[0], "oo 漢字名 00:00");
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
        user_id: Id::new(10),
        display_name: "漢字仮名交じり文章".to_owned(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
        status: PresenceStatus::Online,
    };

    let label = member_display_label(MemberEntry::Guild(&member), 0, 12);

    assert_eq!(label, "漢字仮名...");
    assert!(label.width() <= 12);
}

#[test]
fn server_label_truncates_by_display_width() {
    let label = truncate_display_width("漢字仮名交じりサーバー", 12);

    assert_eq!(label, "漢字仮名...");
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
        user_id: Id::new(10),
        display_name: "long-member-name".to_owned(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
        status: PresenceStatus::Online,
    };

    let label = member_display_label(MemberEntry::Guild(&member), 5, 8);

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
    let member = GuildMemberState {
        user_id: Id::new(10),
        display_name: "neo".to_owned(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
        status: PresenceStatus::Offline,
    };

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
            user_id: Id::new(10),
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
            status,
        };

        let style = member_name_style(MemberEntry::Guild(&member), None, false);

        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::DIM));
    }
}

#[test]
fn no_role_offline_member_name_is_white_and_dimmed() {
    let member = GuildMemberState {
        user_id: Id::new(10),
        display_name: "neo".to_owned(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
        status: PresenceStatus::Offline,
    };

    let style = member_name_style(MemberEntry::Guild(&member), None, false);

    assert_eq!(style.fg, Some(Color::White));
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn selected_bot_member_name_preserves_role_color_and_selection_style() {
    let member = GuildMemberState {
        user_id: Id::new(10),
        display_name: "bot".to_owned(),
        username: None,
        is_bot: true,
        avatar_url: None,
        role_ids: Vec::new(),
        status: PresenceStatus::Online,
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
        format_unix_millis_with_offset(DISCORD_EPOCH_MILLIS, kst),
        Some("09:00".to_owned())
    );
}

fn snowflake_for_unix_ms(unix_ms: u64) -> Id<MessageMarker> {
    let raw = (unix_ms - DISCORD_EPOCH_MILLIS) << SNOWFLAKE_TIMESTAMP_SHIFT;
    Id::new(raw.max(1))
}

fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis()
        .try_into()
        .expect("current unix millis should fit in u64")
}

#[test]
fn date_separator_appears_when_local_date_changes() {
    // 24h apart at noon UTC guarantees different local dates regardless of
    // the test runner's timezone.
    let day_one = snowflake_for_unix_ms(1_743_465_600_000); // 2026-04-01 00:00:00 UTC + 12h ≈ noon
    let day_two = snowflake_for_unix_ms(1_743_465_600_000 + 24 * 60 * 60 * 1000);

    assert!(message_starts_new_day(day_one, None));
    assert!(!message_starts_new_day(day_one, Some(day_one)));
    assert!(message_starts_new_day(day_two, Some(day_one)));
}

#[test]
fn date_separator_line_centers_label_within_full_width() {
    let id = snowflake_for_unix_ms(1_743_508_800_000); // arbitrary timestamp
    let line = date_separator_line(id, 30);
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(text.width(), 30);
    assert!(text.contains(' '));
    assert!(text.starts_with('─'));
    assert!(text.ends_with('─'));
    // The label is "YYYY-MM-DD" wrapped in spaces, so 12 chars.
    let label_chars = text.matches(char::is_numeric).count();
    assert_eq!(label_chars, 8);
}

#[test]
fn new_messages_notice_line_centers_count_within_full_width() {
    let line = new_messages_notice_line(3, 30);
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text.width(), 30);
    assert!(text.contains("↓ 3 new messages"));
    assert!(!text.contains('#'));
    assert!(!text.contains('│'));
    assert_eq!(line.spans[0].style.fg, Some(ACCENT));
    assert_eq!(line.spans[0].style.bg, None);
    assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn render_messages_shows_new_messages_notice_at_bottom_of_message_pane() {
    let mut state = state_with_message();
    push_message(&mut state, 2, "older second");
    push_message(&mut state, 3, "older third");
    push_message(&mut state, 4, "older fourth");
    state.focus_pane(FocusPane::Messages);
    state.jump_top();
    push_message(&mut state, 5, "first unread");
    push_message(&mut state, 6, "second unread");

    let dump = render_dashboard_dump(100, 24, &mut state);

    assert_notice_floats_at_list_bottom_above_composer(&dump, "2 new messages");
}

#[test]
fn render_messages_shows_new_messages_notice_after_viewport_scrolls_up() {
    let mut state = state_with_message();
    for id in 2..=10 {
        push_message(&mut state, id, &format!("older {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    push_message(&mut state, 11, "new after viewport scroll");

    let dump = render_dashboard_dump(100, 24, &mut state);

    assert_notice_floats_at_list_bottom_above_composer(&dump, "1 new messages");
}

#[test]
fn new_messages_notice_does_not_reserve_message_list_height() {
    let area = Rect::new(0, 0, 100, 24);
    let mut state = state_with_message();
    for id in 2..=30 {
        push_message(&mut state, id, &format!("older {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    sync_view_heights(area, &mut state);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let height_without_notice = state.message_view_height();

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    push_message(&mut state, 31, "first unread");
    sync_view_heights(area, &mut state);

    assert_eq!(state.new_messages_count(), 1);
    assert_eq!(state.message_view_height(), height_without_notice);
}

fn assert_notice_floats_at_list_bottom_above_composer(dump: &[String], label: &str) {
    let notice_row = dump
        .iter()
        .position(|line| line.contains(label))
        .expect("new messages notice should render");
    let composer_row = dump
        .iter()
        .position(|line| line.contains("Message Input"))
        .expect("composer should render");

    assert_eq!(
        notice_row.saturating_add(1),
        composer_row,
        "new messages notice should float on the message-list bottom above composer:\n{}",
        dump.join("\n")
    );
}

#[test]
fn message_viewport_lines_keep_rows_from_tall_following_message() {
    let mut selected = message_with_attachment(Some("selected".to_owned()), image_attachment());
    selected.attachments.clear();
    let mut tall_following = message_with_attachment(
        Some("abcdefghijklmnopqrstuvwx".to_owned()),
        image_attachment(),
    );
    tall_following.attachments.clear();
    let messages = [&selected, &tall_following];

    let visible_rows = message_viewport_lines(
        &messages,
        Some(0),
        &DashboardState::new(),
        super::message_viewport_layout(5, 80, 80, 16, 3),
        &[],
    )
    .into_iter()
    .take(5)
    .collect::<Vec<_>>();
    let visible_text = line_texts_from_ratatui(&visible_rows);
    let sent_time = format_message_sent_time(Id::new(1));

    assert!(visible_text[0].starts_with("╭─oo "));
    assert!(visible_text[0].contains(&sent_time));
    assert!(visible_text[1].contains("selected"));
    assert!(visible_text[2].starts_with("╰"));
    assert!(visible_text[3].starts_with("oo "));
    assert!(visible_text[3].ends_with(&sent_time));
    assert!(visible_text[4].ends_with("abcdefgh"));
}

#[test]
fn selected_message_uses_border_without_background() {
    let message = message_with_content(Some("abcdefghijkl".to_owned()));
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        Some(0),
        &DashboardState::new(),
        super::message_viewport_layout(5, 80, 80, 16, 3),
        &[],
    );
    let sent_time = format_message_sent_time(Id::new(1));

    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.len(), 3);
    assert!(texts[0].starts_with(&format!("╭─oo neo {sent_time}")));
    assert!(texts[0].ends_with("╮"));
    assert!(texts[0].contains(" ─"));
    assert!(texts[1].starts_with("│    abcdefghijkl"));
    assert!(texts[1].ends_with(" │"));
    assert!(texts[2].starts_with("╰"));
    assert!(texts[2].ends_with("╯"));
    assert!(texts.iter().all(|text| text.width() == 80));
    assert_eq!(lines[0].spans[0].style.fg, Some(SELECTED_MESSAGE_BORDER));
    assert_eq!(lines[1].spans[0].style.fg, Some(SELECTED_MESSAGE_BORDER));
    assert!(
        lines[1].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .all(|span| span.style.bg.is_none())
    );
}

#[test]
fn selected_message_right_border_can_reserve_scrollbar_column() {
    let message = message_with_content(Some("a".repeat(73)));
    let messages = [&message];

    let texts = line_texts_from_ratatui(&message_viewport_lines(
        &messages,
        Some(0),
        &DashboardState::new(),
        super::message_viewport_layout(40, 80, selected_message_card_width(80, true), 16, 3),
        &[],
    ));

    assert!(texts[0].starts_with("╭─oo"));
    assert!(texts[0].ends_with("╮"));
    assert!(texts[1].ends_with("│"));
    assert!(texts[2].ends_with("│"));
    assert!(texts[3].ends_with("╯"));
    assert!(texts.iter().all(|text| text.width() == 79));
}

#[test]
fn selected_message_avatar_moves_inside_border() {
    assert_eq!(selected_avatar_x_offset(Some(0), 0), 2);
    assert_eq!(selected_avatar_x_offset(Some(1), 0), 0);
}

#[test]
fn message_preview_rows_do_not_shrink_message_viewport() {
    let mut state = DashboardState::new();

    sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

    assert_eq!(state.message_view_height(), 14);
}

#[test]
fn inline_image_preview_slot_follows_image_message_content() {
    let area = Rect::new(10, 5, 80, 12);

    assert_eq!(
        inline_image_preview_area(area, 2, 0, 77, 4, None),
        Some(Rect::new(13, 8, 77, 4))
    );
}

#[test]
fn embed_image_preview_area_leaves_room_for_gutter() {
    let area = Rect::new(10, 5, 80, 12);

    assert_eq!(
        inline_image_preview_area(area, 2, 0, 77, 4, Some(0xff0000)),
        Some(Rect::new(17, 8, 73, 4))
    );
}

#[test]
fn selected_inline_image_preview_area_follows_bordered_message_content() {
    let area = Rect::new(10, 5, 80, 12);
    let selected_offset = selected_message_content_x_offset(true);

    assert_eq!(
        inline_image_preview_area(area, 2, selected_offset, 77, 4, None),
        Some(Rect::new(15, 8, 75, 4))
    );
}

#[test]
fn later_image_preview_slot_accounts_for_prior_preview_rows() {
    let area = Rect::new(10, 5, 80, 18);
    let messages = [
        message_with_attachment(Some("one".to_owned()), image_attachment()),
        message_with_attachment(Some("two".to_owned()), image_attachment()),
        message_with_attachment(Some("three".to_owned()), image_attachment()),
    ];
    let messages = messages.iter().collect::<Vec<_>>();
    let state = DashboardState::new();
    let row = inline_image_preview_row(&messages, &state, 2, 200, 0, 4);

    assert_eq!(row, 14);
    assert_eq!(
        inline_image_preview_area(area, row, 0, 77, 4, None),
        Some(Rect::new(13, 20, 77, 3))
    );
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
        inline_image_preview_area(area, row, 8, 8, 3, None),
        Some(Rect::new(21, 9, 8, 3))
    );
}

#[test]
fn inline_image_preview_row_ignores_reaction_footer_for_current_message() {
    let mut message = message_with_attachment(Some("one".to_owned()), image_attachment());
    message.reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 3,
        me: true,
    }];
    let messages = [&message];
    let state = DashboardState::new();

    assert_eq!(inline_image_preview_row(&messages, &state, 0, 200, 0, 0), 2);
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

#[test]
fn inline_image_preview_area_hides_preview_at_list_bottom() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(
        inline_image_preview_area(area, 3, 0, 77, 4, None),
        Some(Rect::new(13, 9, 77, 2))
    );
}

#[test]
fn inline_image_preview_area_clips_preview_at_list_top() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(
        inline_image_preview_area(area, -2, 0, 77, 4, None),
        Some(Rect::new(13, 5, 77, 3))
    );
}

#[test]
fn inline_image_preview_area_returns_none_when_preview_starts_below_list() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(inline_image_preview_area(area, 5, 0, 77, 4, None), None);
}

#[test]
fn inline_image_preview_area_returns_none_when_preview_ends_above_list() {
    let area = Rect::new(10, 5, 80, 6);

    assert_eq!(inline_image_preview_area(area, -5, 0, 77, 4, None), None);
}

#[test]
fn inline_album_overflow_marker_is_visible() {
    let mut state = state_with_message();
    let dump = render_dashboard_dump_with_previews(
        120,
        20,
        &mut state,
        vec![ImagePreview {
            viewer: false,
            message_index: 0,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: 16,
            preview_height: 3,
            preview_overflow_count: 2,
            accent_color: None,
            state: ImagePreviewState::Loading {
                filename: "image-4.png".to_owned(),
            },
        }],
    );

    assert!(
        dump.iter().any(|line| line.contains("+2")),
        "dashboard dump did not contain overflow overlay marker:\n{}",
        dump.join("\n")
    );
}

#[test]
fn message_viewport_lines_render_overflow_marker_as_text_fallback() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.attachments = image_attachments(6);
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::message_viewport_layout(200, 80, 80, 16, 3),
        &[],
    );

    assert!(line_texts_from_ratatui(&lines).contains(&"   +2 more images".to_owned()));
}

fn render_dashboard_dump(width: u16, height: u16, state: &mut DashboardState) -> Vec<String> {
    render_dashboard_dump_with_previews(width, height, state, Vec::new())
}

fn render_dashboard_dump_with_previews(
    width: u16,
    height: u16,
    state: &mut DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), state);
            super::render(frame, state, image_previews, Vec::new(), Vec::new(), None);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

fn message_with_attachment(content: Option<String>, attachment: AttachmentInfo) -> MessageState {
    MessageState {
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
        content,
        mentions: Vec::new(),
        attachments: vec![attachment],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    }
}

fn message_with_content(content: Option<String>) -> MessageState {
    let mut message = message_with_attachment(content, image_attachment());
    message.attachments.clear();
    message
}

fn youtube_embed() -> EmbedInfo {
    EmbedInfo {
        color: Some(0xff0000),
        provider_name: Some("YouTube".to_owned()),
        author_name: None,
        title: Some("Example Video".to_owned()),
        description: Some("A video description".to_owned()),
        fields: Vec::new(),
        footer_text: None,
        url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        thumbnail_proxy_url: None,
        thumbnail_width: Some(480),
        thumbnail_height: Some(360),
        image_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        image_proxy_url: None,
        image_width: Some(480),
        image_height: Some(360),
        video_url: None,
    }
}

fn state_with_message() -> DashboardState {
    state_with_message_id(Id::new(1), "hello")
}

fn state_with_message_id(message_id: Id<MessageMarker>, content: &str) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id,
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_forum_posts(post_count: usize) -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: forum_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "forum".to_owned(),
            kind: "GuildForum".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);

    let posts: Vec<_> = (0..post_count)
        .map(|index| {
            let id = 100 + u64::try_from(index).expect("post index should fit u64");
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(id),
                parent_id: Some(forum_id),
                position: None,
                last_message_id: Some(Id::new(10_000 + id)),
                name: format!("post {index}"),
                kind: "GuildPublicThread".to_owned(),
                message_count: Some(0),
                total_message_sent: Some(1),
                thread_archived: Some(false),
                thread_locked: Some(false),
                thread_pinned: Some(false),
                recipients: None,
                permission_overwrites: Vec::new(),
            }
        })
        .collect();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 0,
        next_offset: posts.len(),
        posts,
        preview_messages: Vec::new(),
        has_more: false,
    });
    state
}

fn state_with_unread_direct_messages() -> DashboardState {
    let mut state = DashboardState::new();
    for (channel_id, name, last_message_id) in [
        (Id::new(10), "old", Some(Id::new(100))),
        (Id::new(20), "new", Some(Id::new(200))),
        (Id::new(30), "empty", None),
    ] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id,
            name: name.to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
    }
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                channel_id: Id::new(10),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
            ReadStateInfo {
                channel_id: Id::new(20),
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            },
        ],
    });
    state
}

fn state_with_unread_direct_messages_with_loaded_unread_messages(count: u64) -> DashboardState {
    let mut state = state_with_unread_direct_messages();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(20),
        before: None,
        messages: (0..count)
            .map(|offset| MessageInfo {
                guild_id: None,
                channel_id: Id::new(20),
                message_id: Id::new(101 + offset),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                pinned: false,
                reactions: Vec::new(),
                content: Some(format!("dm {offset}")),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
                ..MessageInfo::default()
            })
            .collect(),
    });
    state
}

fn push_message(state: &mut DashboardState, message_id: u64, content: &str) {
    push_message_with_id(state, Id::new(message_id), content);
}

fn push_message_with_id(state: &mut DashboardState, message_id: Id<MessageMarker>, content: &str) {
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id,
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
}

fn message_info(message_id: u64, author: &str, content: &str, pinned: bool) -> MessageInfo {
    MessageInfo {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: author.to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned,
        reactions: Vec::new(),
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageInfo::default()
    }
}

fn message_with_forwarded_snapshot(snapshot: MessageSnapshotInfo) -> MessageState {
    MessageState {
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
    }
}

fn poll_info(allow_multiselect: bool) -> PollInfo {
    PollInfo {
        question: "What should we eat?".to_owned(),
        answers: vec![
            PollAnswerInfo {
                answer_id: 1,
                text: "Soup".to_owned(),
                vote_count: Some(2),
                me_voted: true,
            },
            PollAnswerInfo {
                answer_id: 2,
                text: "Noodles".to_owned(),
                vote_count: Some(1),
                me_voted: false,
            },
        ],
        allow_multiselect,
        results_finalized: Some(false),
        total_votes: Some(3),
    }
}

fn forwarded_snapshot(
    content: Option<&str>,
    attachments: Vec<AttachmentInfo>,
) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: content.map(str::to_owned),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments,
        embeds: Vec::new(),
        source_channel_id: None,
        timestamp: None,
    }
}

fn state_with_member(user_id: u64, display_name: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![member_info(user_id, display_name)],
        presences: vec![(Id::new(user_id), PresenceStatus::Online)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state
}

fn state_with_role(role_id: u64, name: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: Id::new(role_id),
            name: name.to_owned(),
            color: None,
            position: 1,
            hoist: false,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state
}

fn member_info(user_id: u64, display_name: &str) -> MemberInfo {
    MemberInfo {
        user_id: Id::new(user_id),
        display_name: display_name.to_owned(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
    }
}

fn user_profile_info(user_id: u64, username: &str) -> UserProfileInfo {
    UserProfileInfo {
        user_id: Id::new(user_id),
        username: username.to_owned(),
        global_name: None,
        guild_nick: None,
        role_ids: Vec::new(),
        avatar_url: None,
        bio: None,
        pronouns: None,
        mutual_guilds: Vec::<MutualGuildInfo>::new(),
        mutual_friends_count: 0,
        friend_status: FriendStatus::None,
        note: None,
    }
}

fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
    MentionInfo {
        user_id: Id::new(user_id),
        guild_nick: None,
        display_name: display_name.to_owned(),
    }
}

fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
    MentionInfo {
        user_id: Id::new(user_id),
        guild_nick: Some(nick.to_owned()),
        display_name: nick.to_owned(),
    }
}

fn channel_with_recipients(kind: &str, statuses: &[PresenceStatus]) -> ChannelState {
    ChannelState {
        id: Id::new(10),
        guild_id: None,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "alice".to_owned(),
        kind: kind.to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: statuses
            .iter()
            .enumerate()
            .map(|(index, status)| ChannelRecipientState {
                user_id: Id::new(100 + u64::try_from(index).expect("index should fit u64")),
                display_name: format!("recipient {index}"),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: *status,
            })
            .collect(),
        permission_overwrites: Vec::new(),
    }
}

fn line_texts(lines: &[MessageContentLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn poll_test_line(text: &str, width: usize) -> String {
    let inner_width = poll_card_inner_width(width);
    let padding = inner_width.saturating_sub(text.width());
    format!("│ {text}{} │", " ".repeat(padding))
}

fn line_texts_from_ratatui(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn image_attachment() -> AttachmentInfo {
    AttachmentInfo {
        id: Id::new(3),
        filename: "cat.png".to_owned(),
        url: "https://cdn.discordapp.com/cat.png".to_owned(),
        proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
        content_type: Some("image/png".to_owned()),
        size: 2048,
        width: Some(640),
        height: Some(480),
        description: None,
    }
}

fn image_attachments(count: u64) -> Vec<AttachmentInfo> {
    (0..count)
        .map(|index| {
            let id = 3 + index;
            let mut attachment = image_attachment();
            attachment.id = Id::new(id);
            attachment.filename = format!("image-{id}.png");
            attachment.url = format!("https://cdn.discordapp.com/image-{id}.png");
            attachment.proxy_url = format!("https://media.discordapp.net/image-{id}.png");
            attachment
        })
        .collect()
}

fn video_attachment() -> AttachmentInfo {
    AttachmentInfo {
        id: Id::new(4),
        filename: "clip.mp4".to_owned(),
        url: "https://cdn.discordapp.com/clip.mp4".to_owned(),
        proxy_url: "https://media.discordapp.net/clip.mp4".to_owned(),
        content_type: Some("video/mp4".to_owned()),
        size: 78_364_758,
        width: Some(1920),
        height: Some(1080),
        description: None,
    }
}

fn file_attachment() -> AttachmentInfo {
    AttachmentInfo {
        id: Id::new(5),
        filename: "notes.txt".to_owned(),
        url: "https://cdn.discordapp.com/notes.txt".to_owned(),
        proxy_url: "https://media.discordapp.net/notes.txt".to_owned(),
        content_type: Some("text/plain".to_owned()),
        size: 42,
        width: None,
        height: None,
        description: None,
    }
}
