use std::str::FromStr;

use super::*;
use crate::discord::test_builders::{
    VoiceConnectionStatusChangedFixture, guild_create_event, voice_connection_status_changed_event,
};
use crate::discord::{
    AppCommand, StreamCaptureTarget, StreamCaptureTargetKind, VoiceParticipantPlaybackSettings,
    VoiceParticipantVolumePercent, VoiceScope, VoiceVolumePercent,
};
use crate::tui::keybindings::{KeyChord, OptionsCategoryShortcut, UiAction};
use crate::tui::state::{ChannelActionKind, popups::OptionsCategory};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn private_voice_state(kind: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(200)),
        name: "private call".to_owned(),
        ..ChannelInfo::test(Id::new(20), kind)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state
}

fn current_voice_stream_leader_label(state: &mut DashboardState) -> String {
    state.open_leader();
    state.push_leader_keymap_key(KeyChord::from_str("v").expect("voice prefix should parse"));
    state
        .leader_keymap_shortcuts()
        .into_iter()
        .find(|item| item.action == Some(UiAction::ToggleStream))
        .expect("voice stream shortcut is present")
        .label
}

fn complete_voice_audio_source_load(
    state: &mut DashboardState,
    inputs: &[(&str, &str)],
    outputs: &[(&str, &str)],
) {
    let commands = state.drain_pending_commands();
    let [AppCommand::LoadVoiceAudioSources { request_id }] = commands.as_slice() else {
        panic!("opening voice options should request audio sources");
    };
    state.push_effect(AppEvent::VoiceAudioSourcesLoaded {
        request_id: *request_id,
        inputs: inputs
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect(),
        outputs: outputs
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect(),
        error: None,
    });
}

#[test]
fn voice_options_show_push_to_talk_toggle_and_shortcut() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        push_to_talk: true,
        push_to_talk_shortcut: "control+F8".to_owned(),
        allow_microphone_transmit: true,
        ..VoiceOptions::default()
    });
    state.open_options_category(OptionsCategory::Voice);
    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );

    let items = state.display_option_items();

    assert_eq!(items[2].label, "Input source");
    assert_eq!(items[2].value.as_deref(), Some("System default"));
    assert_eq!(items[3].label, "Output source");
    assert_eq!(items[3].value.as_deref(), Some("System default"));
    assert_eq!(items[5].label, "Push to talk");
    assert!(items[5].enabled);
    assert_eq!(items[5].value, None);
    assert_eq!(items[6].value.as_deref(), Some("control+F8"));
    assert!(items[6].effective);
    assert!(!items[8].effective);
}

#[test]
fn voice_source_options_cycle_and_queue_updates_while_disconnected() {
    let mut state = DashboardState::new();
    state.open_options_category(OptionsCategory::Voice);
    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );
    state.move_option_down();
    state.move_option_down();

    state.adjust_selected_display_option(1);
    assert_eq!(state.voice_options().input_source.as_deref(), Some("mic-1"));
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceAudioSources {
            input_source: Some("mic-1".to_owned()),
            output_source: None,
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("speaker-1")
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceAudioSources {
            input_source: Some("mic-1".to_owned()),
            output_source: Some("speaker-1".to_owned()),
        }]
    );
}

#[test]
fn voice_option_toggles_queue_current_voice_state_update_when_joined() {
    let mut state = DashboardState::new();
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.open_options_category_picker();
    state.open_options_category_from_shortcut(OptionsCategoryShortcut::Voice);
    complete_voice_audio_source_load(&mut state, &[], &[]);

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
    state.move_option_down();
    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(state.voice_options().allow_microphone_transmit);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(state.voice_options().push_to_talk);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(!state.voice_options().noise_suppression);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
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
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(100);
    assert_eq!(
        state.voice_options().microphone_volume,
        VoiceVolumePercent::new(200)
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: VoiceVolumePercent::new(200),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(100);
    assert_eq!(
        state.voice_options().voice_output_volume,
        VoiceVolumePercent::new(200)
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: VoiceVolumePercent::new(200),
            voice_output_volume: VoiceVolumePercent::new(200),
        }]
    );
}

#[test]
fn unavailable_saved_voice_sources_fall_back_to_system_default_while_disconnected() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("missing-mic".to_owned()),
        output_source: Some("missing-speaker".to_owned()),
        ..VoiceOptions::default()
    });
    state.open_options_category(OptionsCategory::Voice);
    let loading_items = state.display_option_items();
    assert_eq!(
        loading_items[2].value.as_deref(),
        Some("Loading sources...")
    );
    assert_eq!(
        loading_items[3].value.as_deref(),
        Some("Loading sources...")
    );

    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );

    assert_eq!(state.voice_options().input_source, None);
    assert_eq!(state.voice_options().output_source, None);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceAudioSources {
            input_source: None,
            output_source: None,
        }]
    );
    let saved = state
        .take_options_save_request()
        .expect("normalized voice sources should be saved");
    assert_eq!(saved.voice.input_source, None);
    assert_eq!(saved.voice.output_source, None);
    let items = state.display_option_items();
    assert_eq!(items[2].value.as_deref(), Some("System default"));
    assert_eq!(items[3].value.as_deref(), Some("System default"));
}

#[test]
fn failed_voice_source_change_restores_the_active_sources() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("new-mic".to_owned()),
        output_source: Some("new-speaker".to_owned()),
        ..VoiceOptions::default()
    });

    state.push_effect(AppEvent::VoiceAudioSourcesApplyFailed {
        requested_input_source: Some("new-mic".to_owned()),
        requested_output_source: Some("new-speaker".to_owned()),
        active_input_source: Some("old-mic".to_owned()),
        active_output_source: Some("old-speaker".to_owned()),
        message: "Could not switch audio sources".to_owned(),
    });

    assert_eq!(
        state.voice_options().input_source.as_deref(),
        Some("old-mic")
    );
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("old-speaker")
    );
    let saved = state
        .take_options_save_request()
        .expect("restored active sources should be saved");
    assert_eq!(saved.voice.input_source.as_deref(), Some("old-mic"));
    assert_eq!(saved.voice.output_source.as_deref(), Some("old-speaker"));
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn stale_voice_source_failure_does_not_replace_a_newer_selection() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("newer-mic".to_owned()),
        output_source: Some("newer-speaker".to_owned()),
        ..VoiceOptions::default()
    });

    state.push_effect(AppEvent::VoiceAudioSourcesApplyFailed {
        requested_input_source: Some("failed-mic".to_owned()),
        requested_output_source: Some("failed-speaker".to_owned()),
        active_input_source: Some("old-mic".to_owned()),
        active_output_source: Some("old-speaker".to_owned()),
        message: "Could not switch audio sources".to_owned(),
    });

    assert_eq!(
        state.voice_options().input_source.as_deref(),
        Some("newer-mic")
    );
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("newer-speaker")
    );
    assert!(state.take_options_save_request().is_none());
}

#[test]
fn voice_channel_participant_audio_controls_persist() {
    let mut state = state_with_voice_channel_participant();
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(10);

    assert!(state.select_visible_pane_row(FocusPane::Channels, 2));
    assert_eq!(state.navigation.channels.list.selected, 2);
    assert_eq!(state.confirm_selected_channel_command(), None);
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        ),
        None
    );
    assert_eq!(
        state
            .voice_participant_audio_popup_view()
            .expect("participant audio popup should open")
            .settings,
        Default::default()
    );

    let volume_settings = VoiceParticipantPlaybackSettings {
        volume: VoiceParticipantVolumePercent::new(101),
        muted: false,
    };
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        ),
        Some(AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(20),
            settings: volume_settings,
        })
    );
    assert_eq!(
        crate::tui::input::handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),),
        None
    );
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        ),
        Some(AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(20),
            settings: VoiceParticipantPlaybackSettings {
                muted: true,
                ..volume_settings
            },
        })
    );
    let saved = state
        .take_ui_state_save_request()
        .expect("participant audio changes should request a state save");
    assert_eq!(saved.voice_participant_playback.len(), 1);
    assert_eq!(saved.voice_participant_playback[0].user_id, Id::new(20));
    assert_eq!(
        saved.voice_participant_playback[0].settings,
        VoiceParticipantPlaybackSettings {
            muted: true,
            ..volume_settings
        }
    );
}

#[test]
fn streaming_voice_participant_action_emits_watch_command_when_joined() {
    let mut state = state_with_voice_channel_participant();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("my-voice-session".to_owned()),
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(1))
        },
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            self_stream: true,
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(20))
        },
    });
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connected,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(10);

    assert!(state.select_visible_pane_row(FocusPane::Channels, 2));
    assert_eq!(state.confirm_selected_channel_command(), None);
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::WatchStream);
    assert!(actions[0].is_enabled());

    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        ),
        Some(AppCommand::WatchVoiceStream {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            user_id: Id::new(20),
            display_name: "Alice".to_owned(),
        })
    );
}

#[test]
fn voice_channel_action_emits_join_then_leave_command() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        input_source: None,
        output_source: None,
        allow_microphone_transmit: false,
        push_to_talk: false,
        push_to_talk_shortcut: "F8".to_owned(),
        noise_suppression: true,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
        ..GuildCreateFixture::new(Id::new(1))
    }));
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
            input_source: None,
            output_source: None,
            allow_microphone_transmit: false,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
            participant_playback_settings: Vec::new(),
        })
    );

    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].is_enabled());
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    assert!(actions[1].is_enabled());

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
fn joined_voice_channel_can_select_a_stream_target_and_stop_sharing() {
    let me = Id::new(10);
    let guild_id = Id::new(1);
    let channel_id = Id::new(11);
    let target = StreamCaptureTarget {
        kind: StreamCaptureTargetKind::Window,
        id: 7,
        title: "Window: Terminal".to_owned(),
    };
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(Id::new(99)),
        channels: vec![voice_channel_info(guild_id, channel_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_CONNECT | PERM_STREAM,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("voice-session".to_owned()),
            ..voice_state(guild_id, Some(channel_id), me)
        },
    });
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(guild_id),
            channel_id: Some(channel_id),
            status: VoiceConnectionStatus::Connected,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    assert_eq!(
        current_voice_stream_leader_label(&mut state),
        "Share screen"
    );
    assert_eq!(
        state.toggle_current_voice_stream_command(),
        Some(AppCommand::LoadStreamCaptureTargets {
            request_id: crate::discord::StreamCaptureTargetsRequestId::new(0),
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    assert!(actions[2].is_enabled());
    assert_eq!(actions[2].kind, ChannelActionKind::ToggleStream);
    assert_eq!(actions[2].label, "Share screen");
    state.select_channel_action_row(2);
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::LoadStreamCaptureTargets {
            request_id: crate::discord::StreamCaptureTargetsRequestId::new(1),
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );

    state.push_effect(AppEvent::StreamCaptureTargetsLoaded {
        request_id: crate::discord::StreamCaptureTargetsRequestId::new(0),
        scope: VoiceScope::Guild(guild_id),
        channel_id,
        targets: vec![target.clone()],
        error: None,
    });
    assert!(!state.is_channel_action_stream_target_phase());

    state.push_effect(AppEvent::StreamCaptureTargetsLoaded {
        request_id: crate::discord::StreamCaptureTargetsRequestId::new(1),
        scope: VoiceScope::Guild(guild_id),
        channel_id,
        targets: vec![target.clone()],
        error: None,
    });
    assert!(state.is_channel_action_stream_target_phase());
    assert_eq!(
        state.selected_stream_capture_targets(),
        std::slice::from_ref(&target)
    );
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::StartVoiceStream {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
            target,
        })
    );
    assert_eq!(
        state
            .toast_message()
            .expect("screen share preparing toast is visible")
            .text,
        "Preparing screen share..."
    );

    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("voice-session".to_owned()),
            self_stream: true,
            ..voice_state(guild_id, Some(channel_id), me)
        },
    });
    assert_eq!(
        current_voice_stream_leader_label(&mut state),
        "Stop sharing"
    );
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert!(actions[2].is_enabled());
    assert_eq!(actions[2].kind, ChannelActionKind::ToggleStream);
    assert_eq!(actions[2].label, "Stop sharing");
    state.select_channel_action_row(2);
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::StopVoiceStream {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );
}

#[test]
fn voice_direct_actions_toggle_state_and_leave_current_voice() {
    let mut state = DashboardState::new();
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));

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
        input_source: None,
        output_source: None,
        allow_microphone_transmit: false,
        push_to_talk: false,
        push_to_talk_shortcut: "F8".to_owned(),
        noise_suppression: false,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
        ..GuildCreateFixture::new(Id::new(1))
    }));
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
}

#[test]
fn voice_join_action_reflects_scope_permissions_and_participation() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild_id = Id::new(1);
    let voice_id = Id::new(11);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![voice_channel_info(guild_id, voice_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].is_enabled());
    assert_eq!(actions[0].disabled_reason(), Some("Connect required"));
    assert_eq!(state.activate_selected_channel_action(), None);

    for kind in ["dm", "group-dm"] {
        let mut state = private_voice_state(kind);
        assert_eq!(
            state.composer_lock(),
            Some(ComposerLock::LoadingMessages),
            "{kind}"
        );
        let join = &state.selected_channel_action_items()[0];
        assert!(join.is_enabled(), "{kind}");
        assert_eq!(join.disabled_reason(), None, "{kind}");
        assert_eq!(
            state.activate_selected_channel_action(),
            Some(AppCommand::JoinVoiceChannel {
                scope: VoiceScope::Private(Id::new(20)),
                channel_id: Id::new(20),
                self_mute: false,
                self_deaf: false,
                input_source: None,
                output_source: None,
                allow_microphone_transmit: false,
                noise_suppression: true,
                microphone_sensitivity: Default::default(),
                microphone_volume: Default::default(),
                voice_output_volume: Default::default(),
                participant_playback_settings: Vec::new(),
            }),
            "{kind}"
        );
    }

    let me = Id::new(10);
    let guild_id = Id::new(1);
    let voice_id = Id::new(11);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(Id::new(99)),
        channels: vec![voice_channel_info(guild_id, voice_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_CONNECT,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    apply_incomplete_community_onboarding(&mut state, guild_id, me);
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    let action = |kind| {
        actions
            .iter()
            .find(|action| action.kind == kind)
            .expect("channel action should exist")
    };
    assert!(!action(ChannelActionKind::JoinVoice).is_enabled());
    assert_eq!(
        action(ChannelActionKind::JoinVoice).disabled_reason(),
        Some("onboarding incomplete")
    );
    assert!(action(ChannelActionKind::ToggleMute).is_enabled());
}
