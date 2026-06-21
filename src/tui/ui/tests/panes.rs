use super::*;

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
fn header_shows_gateway_error_before_connected_account_is_ready() {
    let mut state = DashboardState::new();
    state.push_effect(AppEvent::GatewayError {
        message: "websocket closed: code=4004 reason=authentication failed".to_owned(),
    });

    let dump = render_dashboard_dump(120, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Concord - v"), "{header}");
    assert!(header.contains("Connection issue:"), "{header}");
    assert!(header.contains("websocket closed"), "{header}");
    assert!(!header.contains("Loading..."), "{header}");
}

#[test]
fn header_clears_gateway_error_after_connected_account_is_ready() {
    let mut state = DashboardState::new();
    state.push_effect(AppEvent::GatewayError {
        message: "websocket closed before READY".to_owned(),
    });
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Connected as muri"), "{header}");
    assert!(!header.contains("Connection issue:"), "{header}");
    assert!(!header.contains("Loading..."), "{header}");
}

#[test]
fn header_shows_connected_account() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            position: Some(0),
            name: "Lobby".to_owned(),
            ..ChannelInfo::test(Id::new(11), "GuildVoice")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_effect(AppEvent::VoiceConnectionStatusChanged {
        guild_id: Id::new(1),
        channel_id: Some(Id::new(11)),
        status: VoiceConnectionStatus::Connecting,
        message: None,
    });

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Concord - v"), "{header}");
    assert!(header.contains("Connected as muri"), "{header}");
    assert!(header.contains("Voice guild - Lobby"), "{header}");
    assert!(!header.contains("🔇"), "{header}");
    assert!(!header.contains("🎧"), "{header}");
    assert!(!header.contains("Loading..."), "{header}");
}

#[test]
fn header_shows_voice_status_icons_without_voice_connection() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        ..VoiceOptions::default()
    });
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });

    let dump = render_dashboard_dump(100, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(header.contains("Connected as muri"), "{header}");
    assert!(!header.contains("Voice "), "{header}");
    assert!(header.contains("🔇"), "{header}");
    assert!(header.contains("🎧"), "{header}");
}

#[test]
fn header_keeps_current_user_white_while_speaking() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| render_header(frame, frame.area(), &state))
        .expect("draw should succeed");
    let buffer = terminal.backend().buffer();
    let header = (0..buffer.area.width)
        .map(|col| buffer[(col, 0)].symbol().to_owned())
        .collect::<String>();
    let user_col = header.find("muri").expect("header should include user") as u16;
    assert_eq!(buffer[(user_col, 0)].fg, Color::White);

    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo::test(Id::new(1), Some(Id::new(11)), Id::new(10)),
    });
    state.push_event(AppEvent::VoiceSpeakingUpdate {
        guild_id: Id::new(1),
        channel_id: Id::new(11),
        user_id: Id::new(10),
        speaking: true,
    });
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| render_header(frame, frame.area(), &state))
        .expect("draw should succeed");
    let buffer = terminal.backend().buffer();
    let header = (0..buffer.area.width)
        .map(|col| buffer[(col, 0)].symbol().to_owned())
        .collect::<String>();
    let user_col = header.find("muri").expect("header should include user") as u16;
    assert_eq!(buffer[(user_col, 0)].fg, Color::White);
}

#[test]
fn header_labels_other_client_voice_connection() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "muri".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            position: Some(0),
            name: "Lobby".to_owned(),
            ..ChannelInfo::test(Id::new(11), "GuildVoice")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("other-client-voice-session".to_owned()),
            self_deaf: true,
            self_mute: true,
            ..VoiceStateInfo::test(Id::new(1), Some(Id::new(11)), Id::new(10))
        },
    });

    let dump = render_dashboard_dump(120, 10, &mut state);
    let header = dump.first().expect("dashboard render includes header");

    assert!(
        header.contains("Voice guild - Lobby (other client)"),
        "{header}"
    );
    assert!(header.contains("🔇"), "{header}");
    assert!(header.contains("🎧"), "{header}");
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
fn focus_pane_at_uses_persisted_pane_widths() {
    let state = DashboardState::new_with_options(
        DisplayOptions::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        UiStateOptions {
            server_width: 10,
            channel_list_width: 20,
            member_list_width: 15,
            ..Default::default()
        },
    );
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
fn mouse_target_at_maps_visible_message_action_rows() {
    let area = Rect::new(0, 0, 120, 20);
    let mut state = state_with_file_attachment_message();
    state.open_selected_message_actions();
    let action_count = state.selected_message_action_items().len();
    let last_row = action_count
        .checked_sub(1)
        .expect("message action menu has actions");
    let popup_height = action_count as u16 + 2;
    let first_action_y = 1 + (19 - popup_height) / 2 + 1;

    assert_eq!(
        mouse_target_at(area, &state, 46, first_action_y - 1),
        Some(MouseTarget::ModalBackdrop)
    );
    assert_eq!(
        mouse_target_at(area, &state, 46, first_action_y),
        Some(MouseTarget::PopupRow {
            target: PopupListTarget::MessageAction,
            row: 0,
        })
    );
    assert_eq!(
        mouse_target_at(area, &state, 46, first_action_y + last_row as u16),
        Some(MouseTarget::PopupRow {
            target: PopupListTarget::MessageAction,
            row: last_row,
        })
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
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: true,
            ..GuildNotificationSettingsInfo::test(Some(guild_id))
        },
    ]));
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
fn channel_pane_shows_voice_participants_under_voice_channel() {
    let guild_id = Id::new(1);
    let text_id = Id::new(9);
    let voice_id = Id::new(10);
    let empty_voice_id = Id::new(11);
    let alice = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                position: Some(0),
                name: "general".to_owned(),
                ..ChannelInfo::test(text_id, "GuildText")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                position: Some(2),
                name: "Lobby".to_owned(),
                ..ChannelInfo::test(voice_id, "GuildVoice")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                position: Some(1),
                name: "Empty".to_owned(),
                ..ChannelInfo::test(empty_voice_id, "GuildVoice")
            },
        ],
        members: vec![MemberInfo {
            username: Some("alice".to_owned()),
            ..MemberInfo::test(alice, "Alice")
        }],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            deaf: true,
            mute: true,
            self_stream: true,
            ..VoiceStateInfo::test(guild_id, Some(voice_id), alice)
        },
    });
    state.push_event(AppEvent::VoiceSpeakingUpdate {
        guild_id,
        channel_id: voice_id,
        user_id: alice,
        speaking: true,
    });
    state.push_effect(AppEvent::VoiceConnectionStatusChanged {
        guild_id,
        channel_id: Some(voice_id),
        status: VoiceConnectionStatus::Connecting,
        message: None,
    });
    state.confirm_selected_guild();
    state.set_channel_view_height(10);

    let backend = TestBackend::new(40, 9);
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
    let lobby_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, *row)].symbol().to_owned())
                .collect::<String>()
                .contains("Lobby")
        })
        .expect("populated voice row should render");
    let lobby_icon_col = (0..buffer.area.width)
        .find(|col| buffer[(*col, lobby_row)].symbol() == "🔊")
        .expect("populated voice row should use loud speaker icon");
    assert_eq!(buffer[(lobby_icon_col, lobby_row)].fg, Color::Cyan);
    let lobby_name_col = (0..buffer.area.width)
        .find(|col| buffer[(*col, lobby_row)].symbol() == "L")
        .expect("populated voice row should render channel name");
    assert_eq!(buffer[(lobby_name_col, lobby_row)].fg, Color::Yellow);

    let empty_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, *row)].symbol().to_owned())
                .collect::<String>()
                .contains("Empty")
        })
        .expect("empty voice row should render");
    let empty_icon_col = (0..buffer.area.width)
        .find(|col| buffer[(*col, empty_row)].symbol() == "🔈")
        .expect("empty voice row should use quiet speaker icon");
    assert_eq!(buffer[(empty_icon_col, empty_row)].fg, DIM);

    assert!(
        channel_rows.iter().any(|row| row.contains("Alice")),
        "{}",
        channel_rows.join("\n")
    );
    assert!(
        channel_rows.iter().any(|row| row.contains("🔴")),
        "{}",
        channel_rows.join("\n")
    );
    assert!(
        channel_rows.iter().any(|row| row.contains("Alice")
            && row.contains("🔴")
            && row.contains("🔇")
            && row.contains("🎧")
            && row.find("🔴") < row.find("🔇")
            && row.find("🔇") < row.find("🎧")),
        "{}",
        channel_rows.join("\n")
    );
    assert!(
        (0..buffer.area.height)
            .any(|row| (0..buffer.area.width).any(|col| buffer[(col, row)].symbol() == "🔴"))
    );
    let alice_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, *row)].symbol().to_owned())
                .collect::<String>()
                .contains("Alice")
        })
        .expect("participant row should render");
    let alice_col = (0..buffer.area.width)
        .find(|col| buffer[(*col, alice_row)].symbol() == "A")
        .expect("participant name should render");
    assert_eq!(buffer[(alice_col, alice_row)].fg, Color::Green);
    assert!(
        buffer[(alice_col, alice_row)]
            .modifier
            .contains(Modifier::BOLD)
    );

    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(1);
    state.scroll_focused_pane_viewport_down();
    state.scroll_focused_pane_viewport_down();
    let backend = TestBackend::new(40, 4);
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
    assert!(
        !channel_rows.iter().any(|row| row.contains("Alice")),
        "{}",
        channel_rows.join("\n")
    );
    let lobby_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, *row)].symbol().to_owned())
                .collect::<String>()
                .contains("Lobby")
        })
        .expect("voice row should be visible");
    let lobby_icon_col = (0..buffer.area.width)
        .find(|col| buffer[(*col, lobby_row)].symbol() == "🔊")
        .expect("populated voice row should keep loud speaker icon");
    assert_eq!(buffer[(lobby_icon_col, lobby_row)].fg, Color::Cyan);
}

#[test]
fn channel_pane_keeps_voice_participant_indicators_visible_after_name_truncation() {
    let guild_id = Id::new(1);
    let voice_id = Id::new(10);
    let alice = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            position: Some(0),
            name: "Lobby".to_owned(),
            ..ChannelInfo::test(voice_id, "GuildVoice")
        }],
        members: vec![MemberInfo {
            username: Some("some_really_long_voice_participant_name".to_owned()),
            display_name: "some_really_long_voice_participant_name".to_owned(),
            ..MemberInfo::test(alice, "some_really_long_voice_participant_name")
        }],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            deaf: true,
            mute: true,
            self_stream: true,
            ..VoiceStateInfo::test(guild_id, Some(voice_id), alice)
        },
    });
    state.confirm_selected_guild();
    state.set_channel_view_height(4);

    let backend = TestBackend::new(32, 5);
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
    let participant_row = channel_rows
        .iter()
        .find(|row| row.contains("🔴") || row.contains("🔇") || row.contains("🎧"))
        .expect("participant row should keep voice indicators visible");

    assert!(participant_row.contains("..."), "{participant_row}");
    assert!(participant_row.contains("🔴"), "{participant_row}");
    assert!(participant_row.contains("🔇"), "{participant_row}");
    assert!(participant_row.contains("🎧"), "{participant_row}");
    assert!(
        !participant_row.contains("participant_name"),
        "{participant_row}"
    );
}

#[test]
fn member_pane_keeps_normal_style_for_speaking_voice_members() {
    let guild_id = Id::new(1);
    let voice_id = Id::new(10);
    let alice = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            position: Some(0),
            name: "Lobby".to_owned(),
            ..ChannelInfo::test(voice_id, "GuildVoice")
        }],
        members: vec![MemberInfo {
            username: Some("alice".to_owned()),
            ..MemberInfo::test(alice, "Alice")
        }],
        presences: vec![(alice, PresenceStatus::Online)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.push_event(guild_member_list_counts_event(guild_id, 1));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo::test(guild_id, Some(voice_id), alice),
    });

    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| render_members(frame, frame.area(), &state, &[]))
        .expect("draw should succeed");
    let buffer = terminal.backend().buffer();
    let alice_cell = find_cell(buffer, "Alice").expect("member should render");
    assert_eq!(buffer[alice_cell].fg, Color::White);

    state.push_event(AppEvent::VoiceSpeakingUpdate {
        guild_id,
        channel_id: voice_id,
        user_id: alice,
        speaking: true,
    });
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| render_members(frame, frame.area(), &state, &[]))
        .expect("draw should succeed");
    let buffer = terminal.backend().buffer();
    let alice_cell = find_cell(buffer, "Alice").expect("member should render");
    assert_eq!(buffer[alice_cell].fg, Color::White);
}

#[test]
fn pane_filters_keep_content_width_when_active() {
    let guild_id = Id::new(1);
    let matching_name = "abcdefghijklmnopqrstuvwxzy";
    let channels = (0..12)
        .map(|index| ChannelInfo {
            guild_id: Some(guild_id),
            position: Some(i32::try_from(index).expect("test index should fit i32")),
            name: if index == 0 {
                matching_name.to_owned()
            } else {
                format!("other-{index}")
            },
            ..ChannelInfo::test(Id::new(10 + index), "GuildText")
        })
        .collect();
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels,
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.open_channel_pane_filter();
    for value in matching_name.chars() {
        state.push_channel_pane_filter_char(value);
    }
    state.set_channel_view_height(10);

    let backend = TestBackend::new(32, 6);
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

    assert!(
        channel_rows.iter().any(|row| row.contains(matching_name)),
        "{}",
        channel_rows.join("\n")
    );
    assert!(
        !channel_rows.iter().any(|row| row.contains("┃")),
        "{}",
        channel_rows.join("\n")
    );

    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "This is Server 1".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.focus_pane(FocusPane::Guilds);
    state.set_guild_view_height(4);

    let normal_rows = rendered_guild_rows(&state, 20, 6);
    let normal_server_row = normal_rows
        .iter()
        .find(|row| row.contains("This"))
        .expect("server row should render")
        .clone();

    state.open_guild_pane_filter();
    state.set_guild_view_height(3);

    let filtered_rows = rendered_guild_rows(&state, 20, 6);
    let filtered_server_row = filtered_rows
        .iter()
        .find(|row| row.contains("This"))
        .expect("server row should render while filtering")
        .clone();

    assert_eq!(
        normal_server_row.replace('▸', " "),
        filtered_server_row.replace('▸', " "),
        "normal:\n{}\nfiltered:\n{}",
        normal_rows.join("\n"),
        filtered_rows.join("\n")
    );
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
                position: Some(0),
                name: "Text Channels".to_owned(),
                ..ChannelInfo::test(category_id, "category")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(category_id),
                position: Some(0),
                name: "general".to_owned(),
                ..ChannelInfo::test(channel_id, "text")
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.push_event(user_guild_settings_init(vec![
        GuildNotificationSettingsInfo {
            message_notifications: Some(NotificationLevel::OnlyMentions),
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                muted: true,
                ..ChannelNotificationOverrideInfo::test(category_id)
            }],
            ..GuildNotificationSettingsInfo::test(Some(guild_id))
        },
    ]));
    state.set_channel_view_height(20);
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render_channels(frame, frame.area(), &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    let header_text = (0..buffer.area.width)
        .map(|col| buffer[(col, 1)].symbol().to_owned())
        .collect::<String>();
    assert!(header_text.contains("guild"));

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
fn forum_post_lines_render_title_author_and_preview() {
    let post = ChannelThreadItem {
        section_label: Some("Active posts".to_owned()),
        label: "A useful Rust crate".to_owned(),
        locked: true,
        pinned: true,
        preview_author_id: Some(Id::new(99)),
        preview_author: Some("neo".to_owned()),
        preview_author_color: Some(0x3366CC),
        preview_content: Some("This crate solves a small but annoying problem".to_owned()),
        applied_tags: vec!["question".to_owned(), "rust".to_owned()],
        preview_reactions: vec![ReactionInfo {
            count: 2,
            me: true,
            ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
        }],
        comment_count: Some(4),
        new_message_count: 3,
        last_activity_message_id: Some(Id::new(30)),
        ..ChannelThreadItem::test(Id::new(30))
    };

    let lines = forum_post_viewport_lines(&[post], Some(0), 80, false);
    let texts = line_texts_from_ratatui(&lines);

    assert_eq!(texts.len(), 7);
    assert_eq!(texts[0].trim_end(), "Active posts");
    assert!(texts[1].starts_with("› ╭"));
    assert!(!texts[1].contains("Active posts"));
    assert!(texts.iter().all(|text| text.width() == 80));
    assert!(texts[2].contains("A useful Rust crate"));
    assert!(texts[2].contains("PINNED"));
    assert!(texts[3].contains("neo: This crate solves"));
    assert!(texts[4].contains("# question"));
    assert!(texts[4].contains("# rust"));
    assert!(texts[5].contains("4 comments"));
    assert!(texts[5].contains("3 new messages"));
    assert!(texts[5].contains("[👍 2]"));
    assert!(!texts[5].contains("pinned"));
    assert!(texts[5].contains("locked"));
    assert!(texts[6].starts_with("  ╰"));
    assert_eq!(lines[2].spans[2].style.fg, Some(Color::White));
    assert_eq!(lines[2].spans[3].style.fg, Some(Color::Yellow));
    assert_eq!(
        lines[3].spans[2].style.fg,
        Some(Color::Rgb(0x33, 0x66, 0xCC))
    );
    assert_eq!(lines[3].spans[4].style.fg, Some(Color::White));
    assert_eq!(lines[5].spans[2].style.fg, Some(Color::White));
    assert_eq!(lines[5].spans[4].style.fg, Some(Color::Yellow));
    assert_eq!(lines[5].spans[6].style.fg, Some(Color::Yellow));
    assert_eq!(lines[5].spans[8].style.fg, Some(Color::White));
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
fn forum_post_scrollbar_visible_count_uses_rendered_rows() {
    assert_eq!(forum_post_scrollbar_visible_count(10), 10);
    assert_eq!(forum_post_scrollbar_visible_count(0), 1);
}

#[test]
fn forum_post_lines_can_reserve_scrollbar_column() {
    let post = ChannelThreadItem {
        label: "A useful Rust crate".to_owned(),
        preview_author_id: Some(Id::new(99)),
        preview_author: Some("neo".to_owned()),
        preview_content: Some("short preview".to_owned()),
        comment_count: Some(1),
        last_activity_message_id: Some(Id::new(30)),
        ..ChannelThreadItem::test(Id::new(30))
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
    assert!(texts[5].ends_with("╯"));
    assert!(texts.iter().all(|text| text.width() == 79));
}

#[test]
fn forum_post_reaction_overlay_rows_skip_no_reaction_cards() {
    let first = ChannelThreadItem {
        label: "No reactions".to_owned(),
        ..ChannelThreadItem::test(Id::new(30))
    };
    let second = ChannelThreadItem {
        label: "Custom reaction".to_owned(),
        preview_reactions: vec![ReactionInfo::test(ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: false,
        })],
        ..ChannelThreadItem::test(Id::new(31))
    };

    assert_eq!(
        forum_post_reaction_rows_for_test(&[first, second], 80, 20),
        vec![10]
    );
}

#[test]
fn forum_post_render_shows_scrollbar_when_posts_exceed_visible_cards() {
    let mut state = state_with_forum_posts(10);

    let dump = render_dashboard_dump(100, 20, &mut state);

    assert!(dump.iter().any(|line| line.contains('┃')));
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
fn server_label_truncates_by_display_width() {
    let label = truncate_display_width("漢字仮名交じりサーバー", 12);

    assert_eq!(label, "漢字仮名...");
    assert!(label.width() <= 12);
}
