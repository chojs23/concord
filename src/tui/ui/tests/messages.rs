use ratatui::style::Stylize;

use super::*;

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
            last_message_id: Some(Id::new(10)),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![ReadStateInfo {
            last_acked_message_id: Some(Id::new(10)),
            mention_count: 2,
            ..ReadStateInfo::test(channel_id)
        }],
    });
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), &mut state);
            super::super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
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
            last_message_id: Some(Id::new(10)),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
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
            last_acked_message_id: Some(Id::new(10)),
            mention_count: 2,
            ..ReadStateInfo::test(channel_id)
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
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: vec![MemberInfo {
            role_ids: vec![role_id],
            ..MemberInfo::test(author_id, "neo")
        }],
        presences: vec![(author_id, PresenceStatus::Online)],
        roles: vec![RoleInfo {
            color: Some(0x3366CC),
            position: 10,
            ..RoleInfo::test(role_id, "Blue")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(MessageCreateFixture {
        channel_id,
        message_id: Id::new(1),
        author_id,
        author: "fallback".to_owned(),
        author_is_bot: true,
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::default()
    }));

    let messages = state.messages();
    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::narrow_message_viewport_layout(40),
        &[],
    );

    assert_eq!(
        lines[1].spans[1].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
    assert_eq!(lines[1].spans[2].content.as_ref(), " [bot]");
    assert_eq!(lines[1].spans[2].style.fg, Some(Color::White));
    assert_eq!(lines[1].spans[2].style.bg, Some(Color::Rgb(88, 101, 242)));
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
    state.direct_open_selected_message_pin_confirmation();

    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessagePinConfirmation)
    );
}

#[test]
fn forum_post_reaction_summary_reserves_custom_emoji_image_slot() {
    let reactions = vec![ReactionInfo {
        me: true,
        ..ReactionInfo::test(ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: false,
        })
    }];

    assert_eq!(
        forum_post_reaction_summary(&reactions, 80).as_deref(),
        Some("[   1]")
    );
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
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: vec![MemberInfo {
            role_ids: vec![role_id],
            ..MemberInfo::test(author_id, "neo")
        }],
        presences: vec![(author_id, PresenceStatus::Online)],
        roles: vec![RoleInfo {
            color: Some(0x3366CC),
            position: 10,
            ..RoleInfo::test(role_id, "Blue")
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
        super::narrow_message_viewport_layout(40),
        &[],
    );

    assert_eq!(
        lines[1].spans[1].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
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
            "  ▎ A video description",
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
            "  ▎ A video description",
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
fn message_embed_renders_tweet_description_as_readable_text() {
    let mut message = message_with_content(Some(
        "Fx'ed that for you! https://www.fxtwitter.com/MikeReiss/status/2054582956438524124"
            .to_owned(),
    ));
    let mut embed = youtube_embed();
    embed.color = Some(0x6364ff);
    embed.provider_name = None;
    embed.author_name = Some("Mike Reiss (@MikeReiss)".to_owned());
    embed.title = None;
    embed.description = Some(
        "Patriots rookie Quintayvious Hutchins \\(seventh round, Boston College\\) was arraigned\\.\n\u{fe00}\n**[💬](https://x.com/intent/tweet?in_reply_to=1) 2 [🔁](https://x.com/intent/retweet?tweet_id=1) 11 [❤️](https://x.com/intent/like?tweet_id=1) 44 👁️ 12\\.4K **"
            .to_owned(),
    );
    embed.footer_text = Some("FxTwitter".to_owned());
    embed.url = Some("https://www.fxtwitter.com/MikeReiss/status/2054582956438524124".to_owned());
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 120);
    let texts = line_texts(&lines);

    assert!(texts.contains(&"  ▎ Mike Reiss (@MikeReiss)"));
    assert!(texts.contains(
        &"  ▎ Patriots rookie Quintayvious Hutchins (seventh round, Boston College) was arraigned."
    ));
    assert!(texts.contains(&"  ▎ 💬 2 🔁 11 ❤️ 44 👁️ 12.4K "));
    assert!(texts.contains(&"  ▎ FxTwitter"));
}

#[test]
fn message_embed_description_preserves_useful_link_destination() {
    let mut message = message_with_content(Some("read this".to_owned()));
    let mut embed = youtube_embed();
    embed.description = Some("See [docs](https://example.com/docs)".to_owned());
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 120);

    assert!(line_texts(&lines).contains(&"  ▎ See docs (https://example.com/docs)"));
}

#[test]
fn message_embed_description_preserves_escaped_emphasis_markers() {
    let mut message = message_with_content(Some("literal markers".to_owned()));
    let mut embed = youtube_embed();
    embed.description = Some("\\*\\*literal\\*\\* and **bold**".to_owned());
    message.embeds = vec![embed];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 120);

    assert!(line_texts(&lines).contains(&"  ▎ **literal** and bold"));
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
fn message_content_applies_supported_markdown_formatting() {
    let message = message_with_content(Some(
            "# Project Update\n## Highlights\n### Detail\nMessage body\n> Keep the layout calm\n>\nNext paragraph\n- First action\n* Alternate action\nUse **bold**, *italic*, ***both***, and `code` text\n```rust\nlet answer = 42;\n**not bold in code**\n```\nAfter\n```css\nTEST```\n\n```cs\nsadfasdf\n```\n\n```css\nzdasfffaewfewf\n\n```"
            .to_owned(),
    ));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec![
            "# Project Update",
            "## Highlights",
            "### Detail",
            "Message body",
            "▎ Keep the layout calm",
            "▎ ",
            "Next paragraph",
            "• First action",
            "• Alternate action",
            "Use bold, italic, both, and code text",
            "╭─ rust ───────────────╮",
            "│ let answer = 42;     │",
            "│ **not bold in code** │",
            "╰──────────────────────╯",
            "After",
            "╭─ css ╮",
            "│ TEST │",
            "╰──────╯",
            "",
            "╭─ cs ─────╮",
            "│ sadfasdf │",
            "╰──────────╯",
            "",
            "╭─ css ──────────╮",
            "│ zdasfffaewfewf │",
            "│                │",
            "╰────────────────╯",
        ]
    );

    assert_eq!(lines[0].style.fg, Some(ACCENT));
    assert!(lines[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(lines[1].style.add_modifier.contains(Modifier::BOLD));
    assert!(lines[1].style.add_modifier.contains(Modifier::UNDERLINED));
    assert!(lines[2].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(lines[4].style.fg, Some(DIM));
    assert_eq!(lines[14].style, Style::default());

    let h1_spans = lines[0].spans();
    assert_eq!(h1_spans[0].content.as_ref(), "# ");
    assert_eq!(h1_spans[0].style.fg, Some(DIM));

    let h2_spans = lines[1].spans();
    assert_eq!(h2_spans[0].content.as_ref(), "## ");
    assert_eq!(h2_spans[0].style.fg, Some(DIM));

    let h3_spans = lines[2].spans();
    assert_eq!(h3_spans[0].content.as_ref(), "### ");
    assert_eq!(h3_spans[0].style.fg, Some(DIM));

    let quote_spans = lines[4].spans();
    assert_eq!(quote_spans[0].content.as_ref(), "▎ ");
    assert_eq!(quote_spans[0].style.fg, Some(DIM));

    for line in [&lines[7], &lines[8]] {
        let bullet_spans = line.spans();
        assert_eq!(bullet_spans[0].content.as_ref(), "• ");
        assert_eq!(bullet_spans[0].style.fg, Some(DIM));
    }

    let inline_spans = lines[9].spans();
    let bold = inline_spans
        .iter()
        .find(|span| span.content == "bold")
        .expect("bold span should be present");
    assert!(bold.style.add_modifier.contains(Modifier::BOLD));

    let italic = inline_spans
        .iter()
        .find(|span| span.content == "italic")
        .expect("italic span should be present");
    assert!(italic.style.add_modifier.contains(Modifier::ITALIC));

    let bold_italic = inline_spans
        .iter()
        .find(|span| span.content == "both")
        .expect("bold italic span should be present");
    assert!(bold_italic.style.add_modifier.contains(Modifier::BOLD));
    assert!(bold_italic.style.add_modifier.contains(Modifier::ITALIC));

    let code = inline_spans
        .iter()
        .find(|span| span.content == "code")
        .expect("code span should be present");
    assert_eq!(code.style.fg, Some(Color::Rgb(255, 165, 0)));
    assert_eq!(code.style.bg, None);

    assert_eq!(lines[10].style.fg, Some(DIM));
    assert_eq!(lines[13].style.fg, Some(DIM));

    let code_line = lines[11].spans();
    assert_eq!(
        code_line,
        vec![
            ratatui::text::Span::from("│ ").fg(DIM),
            ratatui::text::Span::from("let").fg(Color::Rgb(180, 142, 173)),
            ratatui::text::Span::from(" answer ").fg(Color::Rgb(192, 197, 206)),
            ratatui::text::Span::from("=").fg(Color::Rgb(192, 197, 206)),
            ratatui::text::Span::from(" ").fg(Color::Rgb(192, 197, 206)),
            ratatui::text::Span::from("42").fg(Color::Rgb(208, 135, 112)),
            ratatui::text::Span::from(";").fg(Color::Rgb(192, 197, 206)),
            ratatui::text::Span::from("    ").dark_gray(),
            ratatui::text::Span::from(" │").fg(DIM)
        ]
    );

    let literal_code_line = lines[12].spans();
    assert_eq!(literal_code_line[1].content.as_ref(), "*");
    assert!(
        !literal_code_line
            .iter()
            .any(|span| span.style.add_modifier.contains(Modifier::BOLD))
    );

    let mut quote = message_with_content(Some("> hello <@10>".to_owned()));
    quote.mentions = vec![mention_info(10, "alice")];
    let quote_lines = format_message_content_lines(&quote, &DashboardState::new(), 200);
    let mention = quote_lines[0]
        .spans()
        .into_iter()
        .find(|span| span.content == "@alice")
        .expect("mention span should survive quote formatting");
    assert_eq!(
        mention.style.bg,
        mention_highlight_style(TextHighlightKind::OtherMention).bg
    );

    let emoji = message_with_content(Some("- <:party:99> party".to_owned()));
    let loaded_urls = vec!["https://cdn.discordapp.com/emojis/99.png".to_owned()];
    let emoji_lines = format_message_content_lines_with_loaded_custom_emoji_urls(
        &emoji,
        &DashboardState::new(),
        200,
        &loaded_urls,
    );
    assert_eq!(emoji_lines[0].image_slots[0].col, 2);
    assert_eq!(emoji_lines[0].image_slots[0].byte_start, "• ".len());

    let wrapped = message_with_content(Some("**abcdef**".to_owned()));
    let wrapped_lines = format_message_content_lines(&wrapped, &DashboardState::new(), 3);
    assert_eq!(line_texts(&wrapped_lines), vec!["abc", "def"]);
    assert!(
        wrapped_lines
            .iter()
            .all(|line| line.spans()[0].style.add_modifier.contains(Modifier::BOLD))
    );

    let mut mention = message_with_content(Some("**<@10>**".to_owned()));
    mention.mentions = vec![mention_info(10, "alice")];
    let mention_lines = format_message_content_lines(&mention, &DashboardState::new(), 200);
    let mention_span = mention_lines[0]
        .spans()
        .into_iter()
        .find(|span| span.content == "@alice")
        .expect("mention span should survive inline formatting");
    assert!(mention_span.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(
        mention_span.style.bg,
        mention_highlight_style(TextHighlightKind::OtherMention).bg
    );

    let emoji = message_with_content(Some("**<:party:99>**".to_owned()));
    let emoji_lines = format_message_content_lines_with_loaded_custom_emoji_urls(
        &emoji,
        &DashboardState::new(),
        200,
        &loaded_urls,
    );
    assert_eq!(line_texts(&emoji_lines), vec!["  "]);
    assert_eq!(emoji_lines[0].image_slots[0].col, 0);
    assert_eq!(emoji_lines[0].image_slots[0].byte_start, 0);

    let quote = message_with_content(Some("> **bold quote**".to_owned()));
    let quote_lines = format_message_content_lines(&quote, &DashboardState::new(), 200);
    let quote_span = quote_lines[0]
        .spans()
        .into_iter()
        .find(|span| span.content == "bold quote")
        .expect("inline bold span should survive quote formatting");
    assert_eq!(quote_span.style.fg, Some(DIM));
    assert!(quote_span.style.add_modifier.contains(Modifier::BOLD));

    let lines = format_message_content_lines(
        &message_with_content(Some("```\nabcdefghijkl\n```".to_owned())),
        &DashboardState::new(),
        9,
    );
    assert_eq!(
        line_texts(&lines),
        vec![
            "╭───────╮",
            "│ abcde │",
            "│ fghij │",
            "│ kl    │",
            "╰───────╯",
        ]
    );
    assert_eq!(lines[1].spans()[1].style.fg, Some(Color::White));

    let lines = format_message_content_lines(
        &message_with_content(Some("```".to_owned())),
        &DashboardState::new(),
        200,
    );
    assert_eq!(line_texts(&lines), vec!["```"]);
    assert_eq!(lines[0].style, Style::default());

    let lines = format_message_content_lines(
        &message_with_content(Some("```\none\n\nthree\n```".to_owned())),
        &DashboardState::new(),
        200,
    );
    assert_eq!(
        line_texts(&lines),
        vec![
            "╭───────╮",
            "│ one   │",
            "│       │",
            "│ three │",
            "╰───────╯",
        ]
    );

    let lines = format_message_content_lines(
        &message_with_content(Some("```\n漢字仮名交じ\n```".to_owned())),
        &DashboardState::new(),
        10,
    );
    assert_eq!(
        line_texts(&lines),
        vec!["╭────────╮", "│ 漢字仮 │", "│ 名交じ │", "╰────────╯",]
    );

    let lines = format_message_content_lines_with_loaded_custom_emoji_urls(
        &message_with_content(Some("```\n- not a bullet\n<:party:99>\n```".to_owned())),
        &DashboardState::new(),
        200,
        &loaded_urls,
    );
    assert_eq!(
        line_texts(&lines),
        vec![
            "╭────────────────╮",
            "│ - not a bullet │",
            "│ :party:        │",
            "╰────────────────╯",
        ]
    );
    assert!(lines.iter().all(|line| line.image_slots.is_empty()));
    assert_eq!(lines[1].spans()[1].style.fg, Some(Color::White));
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
        vec!["  oooo  neo 00:00", "  oooo  hello @server alias", ""]
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
        vec!["  oooo  neo 00:00", "  oooo  hello @alice", ""]
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
fn message_content_highlights_detected_urls() {
    let message = message_with_content(Some(
        "open https://thisis.com/a.test?with=querystrings#page now".to_owned(),
    ));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec!["open https://thisis.com/a.test?with=querystrings#page now"]
    );
    assert_eq!(
        lines[0].spans()[1].content.as_ref(),
        "https://thisis.com/a.test?with=querystrings#page"
    );
    assert_eq!(
        lines[0].spans()[1].style.fg,
        mention_highlight_style(TextHighlightKind::Url).fg
    );
    assert!(
        lines[0].spans()[1]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn message_content_highlights_markdown_link_urls() {
    let message = message_with_content(Some(
        "[Tweet](<https://x.com/i/status/2055068765671305537>)".to_owned(),
    ));

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);
    let spans = lines[0].spans();

    let url_span = spans
        .iter()
        .find(|span| span.content.as_ref() == "https://x.com/i/status/2055068765671305537")
        .expect("markdown link URL is rendered as its own span");

    assert!(url_span.style.add_modifier.contains(Modifier::UNDERLINED));
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
        vec!["  oooo  neo 00:00", "  oooo  ping @everyone", ""]
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
        vec!["  oooo  neo 00:00", "  oooo  @everyone hello @neo", ""]
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
        vec!["  oooo  neo 00:00", "  oooo  ping @here", ""]
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
        vec!["  oooo  neo 00:00", "  oooo  hello @moderators", ""]
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
        vec!["  oooo  neo 00:00", "  oooo  hello @everyone", ""]
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
fn thread_created_message_uses_cached_thread_details() {
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id =
        test_message_id_for_unix_millis(current_unix_millis().saturating_sub(10 * 60 * 1000));
    let latest_thread_message_id =
        test_message_id_for_unix_millis(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(message.channel_id),
        last_message_id: Some(latest_thread_message_id),
        name: "release notes".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(Id::new(10), "thread")
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
    message.id = test_message_id_for_unix_millis(now.saturating_sub(10 * 60 * 1000));
    let latest_thread_message_id =
        test_message_id_for_unix_millis(now.saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(message.channel_id),
        name: "release notes".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(Id::new(10), "thread")
    }));
    state.push_event(message_create_event(MessageCreateFixture {
        channel_id: Id::new(10),
        message_id: latest_thread_message_id,
        content: Some("latest reply".to_owned()),
        ..guild_message_create_fixture()
    }));

    let lines = format_message_content_lines(&message, &state, 200);
    let texts = line_texts(&lines);

    assert!(texts[2].contains("13 messages"));
    assert!(texts[3].contains("neo latest reply 2 minutes ago"));
}

#[test]
fn thread_created_message_falls_back_to_system_message_time() {
    let mut message = message_with_content(Some("release notes".to_owned()));
    message.message_kind = MessageKind::new(18);
    message.id =
        test_message_id_for_unix_millis(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(message.channel_id),
        name: "release notes".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(Id::new(10), "thread")
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
    message.id =
        test_message_id_for_unix_millis(current_unix_millis().saturating_sub(2 * 60 * 1000));
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(message.channel_id),
        name: "release notes".to_owned(),
        message_count: Some(12),
        total_message_sent: Some(14),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(true, true)),
        ..ChannelInfo::test(Id::new(10), "thread")
    }));

    let lines = format_message_content_lines(&message, &state, 200);

    assert!(line_texts(&lines)[3].contains("archived · locked"));
}

#[test]
fn thread_starter_message_uses_referenced_message_card() {
    let mut message = message_with_content(Some(String::new()));
    message.message_kind = MessageKind::new(21);
    message.reply = Some(ReplyInfo {
        content: Some("original topic".to_owned()),
        ..ReplyInfo::test("alice")
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(
        line_texts(&lines),
        vec!["Thread starter message", "╭─ alice : original topic"]
    );
}

#[test]
fn reply_preview_renders_known_user_mentions() {
    let mut message = message_with_content(Some("asdf".to_owned()));
    message.message_kind = MessageKind::new(19);
    message.reply = Some(ReplyInfo {
        content: Some("hello <@10>".to_owned()),
        ..ReplyInfo::test("neo")
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
        content: Some("hello <@10>".to_owned()),
        mentions: vec![mention_info(10, "alice")],
        ..ReplyInfo::test("neo")
    });

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
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
        count: 3,
        me: true,
        ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
    }];

    let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

    assert_eq!(line_texts(&lines), vec!["hello", "[👍 3]"]);
    let spans = lines[1].spans();
    assert_eq!(spans[0].content.as_ref(), "[👍 3]");
    assert_eq!(spans[0].style, Style::default().fg(Color::Yellow));
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
        name: "source".to_owned(),
        ..ChannelInfo::test(Id::new(9), "GuildText")
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
fn message_viewport_lines_reserve_rows_for_multiple_attachment_summaries() {
    let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
    message.attachments = image_attachments(2);
    let messages = [&message];

    let lines = message_viewport_lines(
        &messages,
        None,
        &DashboardState::new(),
        super::default_message_viewport_layout(),
        &[],
    );

    assert_eq!(lines.len(), 8);
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
        super::default_message_viewport_layout(),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.iter().filter(|text| text.contains("neo")).count(), 1);
    assert_eq!(texts[2], "  oooo  hello");
    assert_eq!(texts[3], "        follow-up");
}

#[test]
fn message_viewport_lines_start_new_author_group_after_time_gap() {
    let base = 1_743_465_600_000;
    let mut state = state_with_message_id(test_message_id_for_unix_millis(base), "hello");
    push_message_with_id(
        &mut state,
        test_message_id_for_unix_millis(base + 7 * 60 * 1000),
        "later follow-up",
    );
    state.jump_top();
    let messages = state.messages();

    let lines = message_viewport_lines(
        &messages,
        None,
        &state,
        super::default_message_viewport_layout(),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.iter().filter(|text| text.contains("neo")).count(), 2);
    assert!(state.message_starts_author_group_at(1));
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
        super::default_message_viewport_layout(),
        &[],
    );
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts[2], "  oooo  hello");
    assert_eq!(texts[3], "        [👍 1]");
    assert_eq!(texts[4], "        follow-up");
}

#[test]
fn message_viewport_lines_reserve_bounded_rows_for_image_albums() {
    for (attachment_count, expected_lines, overflow_text) in [
        (3, 9, None),
        (4, 10, None),
        (5, 12, Some("        +1 more images")),
    ] {
        let mut message = message_with_attachment(Some("look".to_owned()), image_attachment());
        message.attachments = image_attachments(attachment_count);
        let messages = [&message];

        let lines = message_viewport_lines(
            &messages,
            None,
            &DashboardState::new(),
            super::default_message_viewport_layout(),
            &[],
        );

        assert_eq!(lines.len(), expected_lines);
        if let Some(overflow_text) = overflow_text {
            assert!(line_texts_from_ratatui(&lines).contains(&overflow_text.to_owned()));
        }
    }
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
        vec!["  oooo  neo 00:00", "  oooo  look", ""]
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
        vec!["        second", "        third", ""]
    );
}

#[test]
fn message_item_header_uses_display_width_for_wide_author() {
    let ascii = message_item_lines(
        "alice".to_owned(),
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

    assert_eq!(line_texts_from_ratatui(&ascii)[0], "  oooo  alice 00:00");
    assert_eq!(line_texts_from_ratatui(&wide)[0], "  oooo  漢字名 00:00");
}

#[test]
fn date_separator_appears_when_local_date_changes() {
    // 24h apart at noon UTC guarantees different local dates regardless of
    // the test runner's timezone.
    let day_one = test_message_id_for_unix_millis(1_743_465_600_000); // 2026-04-01 00:00:00 UTC + 12h ≈ noon
    let day_two = test_message_id_for_unix_millis(1_743_465_600_000 + 24 * 60 * 60 * 1000);

    assert!(message_starts_new_day(day_one, None));
    assert!(!message_starts_new_day(day_one, Some(day_one)));
    assert!(message_starts_new_day(day_two, Some(day_one)));
}

#[test]
fn date_separator_line_centers_label_within_full_width() {
    let id = test_message_id_for_unix_millis(1_743_508_800_000); // arbitrary timestamp
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
        super::narrow_message_viewport_layout(5),
        &[],
    )
    .into_iter()
    .take(5)
    .collect::<Vec<_>>();
    let visible_text = line_texts_from_ratatui(&visible_rows);
    let sent_time = format_message_sent_time(Id::new(1));

    assert!(visible_text[0].starts_with("╭─oooo  "));
    assert!(visible_text[0].contains(&sent_time));
    assert!(visible_text[1].starts_with("│ oooo  selected"));
    assert!(visible_text[2].starts_with("╰"));
    assert!(visible_text[3].starts_with("  oooo  "));
    assert!(visible_text[3].ends_with(&sent_time));
    assert!(visible_text[4].ends_with("abcdefgh"));
}

#[test]
fn message_preview_rows_do_not_shrink_message_viewport() {
    let mut state = DashboardState::new();

    sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

    assert_eq!(state.message_view_height(), 14);
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
        super::default_message_viewport_layout(),
        &[],
    );

    assert!(line_texts_from_ratatui(&lines).contains(&"        +2 more images".to_owned()));
}
