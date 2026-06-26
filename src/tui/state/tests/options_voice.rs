use super::*;
use crate::discord::{AppCommand, VoiceScope};
use crate::tui::keybindings::OptionsCategoryShortcut;

#[test]
fn image_preview_quality_option_cycles_presets() {
    let mut state = DashboardState::new();
    state.open_options_popup();
    for _ in 0..3 {
        state.move_option_down();
    }

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::High
    );

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::Original
    );

    state.toggle_selected_display_option();
    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::Efficient
    );
}

#[test]
fn display_option_items_include_voice_state_controls() {
    let state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        allow_microphone_transmit: true,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });

    let items = state.display_option_items();

    assert_eq!(items.len(), 14);
    assert_eq!(items[8].label, "Voice muted");
    assert!(items[8].enabled);
    assert!(items[8].effective);
    assert_eq!(items[9].label, "Voice deafened");
    assert!(items[9].enabled);
    assert!(items[9].effective);
    assert_eq!(items[10].label, "Allow microphone transmit");
    assert!(items[10].enabled);
    assert!(items[10].effective);
    assert_eq!(items[11].label, "Microphone sensitivity");
    assert_eq!(items[11].value, Some("-30 dB".to_owned()));
    assert_eq!(items[11].gauge_percent, Some(70));
    assert!(items[11].effective);
    assert_eq!(items[12].label, "Microphone volume");
    assert_eq!(items[12].value, Some("100%".to_owned()));
    assert_eq!(items[12].gauge_percent, Some(100));
    assert!(items[12].effective);
    assert_eq!(items[13].label, "Voice volume");
    assert_eq!(items[13].value, Some("100%".to_owned()));
    assert_eq!(items[13].gauge_percent, Some(100));
    assert!(!items[13].effective);
}

#[test]
fn voice_option_toggles_queue_current_voice_state_update_when_joined() {
    let mut state = DashboardState::new();
    state.push_effect(AppEvent::VoiceConnectionStatusChanged {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Some(Id::new(11)),
        status: VoiceConnectionStatus::Connecting,
        message: None,
    });
    state.open_options_category_picker();
    state.open_options_category_from_shortcut(OptionsCategoryShortcut::Voice);

    state.toggle_selected_display_option();
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: false,
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(state.voice_options().allow_microphone_transmit);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(10);
    assert_eq!(
        state.voice_options().microphone_sensitivity.label(),
        "-20 dB"
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );
}

#[test]
fn voice_channel_participants_render_as_child_rows_and_are_skipped_by_selection() {
    let mut state = state_with_voice_channel_participant();
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(10);
    let entries = state.channel_pane_entries();

    assert!(matches!(
        &entries[1],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Middle,
            ..
        }
    ));
    assert!(matches!(
        &entries[2],
        ChannelPaneEntry::VoiceParticipant { participant, .. }
            if participant.display_name == "Alice"
    ));
    assert!(matches!(
        &entries[3],
        ChannelPaneEntry::Channel {
            branch: ChannelBranch::Last,
            ..
        }
    ));

    state.move_down();
    assert_eq!(state.navigation.channels.list.selected, 1);
    assert!(!state.select_visible_pane_row(FocusPane::Channels, 2));
    assert_eq!(state.navigation.channels.list.selected, 1);
    state.move_down();
    assert_eq!(state.navigation.channels.list.selected, 3);
}

#[test]
fn voice_channel_action_emits_join_then_leave_command() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        allow_microphone_transmit: false,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(guild_create_event(
        Id::new(1),
        "guild",
        vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
    ));
    state.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let command = state.activate_selected_channel_action();
    assert_eq!(
        command,
        Some(AppCommand::JoinVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
            allow_microphone_transmit: false,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        })
    );

    state.push_effect(AppEvent::VoiceConnectionStatusChanged {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Some(Id::new(11)),
        status: VoiceConnectionStatus::Connecting,
        message: None,
    });
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].enabled);
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    assert!(actions[1].enabled);

    state.select_channel_action_row(1);
    let command = state.activate_selected_channel_action();
    assert_eq!(
        command,
        Some(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: true,
            self_deaf: true,
        })
    );
}

#[test]
fn voice_direct_actions_toggle_state_and_leave_current_voice() {
    let mut state = DashboardState::new();
    state.push_effect(AppEvent::VoiceConnectionStatusChanged {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Some(Id::new(11)),
        status: VoiceConnectionStatus::Connecting,
        message: None,
    });

    state.toggle_voice_mute();
    assert!(state.voice_options().self_mute);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: false,
        }]
    );

    state.toggle_voice_deafen();
    assert!(state.voice_options().self_deaf);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
        }]
    );

    let command = state.leave_current_voice_channel_command();
    assert_eq!(
        command,
        Some(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: true,
            self_deaf: true,
        })
    );
}

#[test]
fn other_client_voice_state_shows_header_only() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        allow_microphone_transmit: false,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(guild_create_event(
        Id::new(1),
        "guild",
        vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
    ));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("other-client-voice-session".to_owned()),
            self_deaf: true,
            self_mute: true,
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(10))
        },
    });

    assert_eq!(
        state.active_voice_connection_label().as_deref(),
        Some("guild - Lobby (other client)")
    );
    assert!(!state.is_joined_voice_channel(Id::new(11)));

    state.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);

    state.open_options_popup();
    for _ in 0..6 {
        state.move_option_down();
    }
    state.toggle_selected_display_option();
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn voice_channel_join_action_requires_connect_permission() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild_id = Id::new(1);
    let voice_id = Id::new(11);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![voice_channel_info(guild_id, voice_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        presences: Vec::new(),
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL,
        )],
        emojis: Vec::new(),
    });
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].enabled);
    assert_eq!(state.activate_selected_channel_action(), None);
}
