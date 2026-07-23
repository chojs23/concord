use super::*;
use crate::discord::test_builders::{
    VoiceConnectionStatusChangedFixture, guild_create_event, voice_connection_status_changed_event,
};
use crate::discord::{
    AppCommand, VoiceParticipantPlaybackSettings, VoiceParticipantVolumePercent, VoiceScope,
    VoiceVolumePercent,
};
use crate::tui::keybindings::OptionsCategoryShortcut;
use crate::tui::state::ChannelActionKind;
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
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: VoiceVolumePercent::new(200),
            voice_output_volume: VoiceVolumePercent::new(200),
        }]
    );
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
fn voice_channel_action_emits_join_then_leave_command() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        allow_microphone_transmit: false,
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
            allow_microphone_transmit: false,
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
        allow_microphone_transmit: false,
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
                allow_microphone_transmit: false,
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
