use std::sync::RwLock;

use super::*;
use crate::discord::{
    ChannelInfo, DiscordState, MemberInfo, PermissionOverwriteInfo, PermissionOverwriteKind,
    RoleInfo,
    test_builders::{GuildCreateFixture, guild_create_event},
};

const VIEW_CHANNEL: u64 = 1 << 10;
const CONNECT: u64 = 1 << 20;
const SPEAK: u64 = 1 << 21;
const USE_VOICE_ACTIVITY: u64 = 1 << 25;

fn voice_permissions(source_deny: u64, destination_deny: u64) -> RwLock<DiscordState> {
    let mut discord = DiscordState::default();
    discord.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    discord.apply_event(&guild_create_event(GuildCreateFixture {
        owner_id: Some(Id::new(99)),
        channels: [(10, source_deny), (20, destination_deny), (30, 0)]
            .into_iter()
            .map(|(id, deny)| ChannelInfo {
                guild_id: Some(Id::new(1)),
                permission_overwrites: vec![PermissionOverwriteInfo {
                    deny,
                    ..PermissionOverwriteInfo::test(1, PermissionOverwriteKind::Role)
                }],
                ..ChannelInfo::test(Id::new(id), "GuildVoice")
            })
            .collect(),
        members: vec![MemberInfo::test(Id::new(10), "me")],
        roles: vec![RoleInfo {
            permissions: VIEW_CHANNEL | CONNECT | SPEAK | USE_VOICE_ACTIVITY,
            ..RoleInfo::test(Id::new(1), "@everyone")
        }],
        ..GuildCreateFixture::new(Id::new(1))
    }));
    RwLock::new(discord)
}

fn connected_runtime(
    discord: &RwLock<DiscordState>,
    settings: VoiceAudioSettings,
) -> (
    VoiceRuntimeState,
    RwLock<Option<CurrentVoiceConnectionState>>,
) {
    let requested = CurrentVoiceConnectionState {
        self_mute: false,
        ..requested_voice()
    };
    let shared = RwLock::new(Some(requested));
    let mut runtime = VoiceRuntimeState::default();
    for event in [
        VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))),
        VoiceRuntimeEvent::Requested(Some(requested)),
        VoiceRuntimeEvent::AudioSettingsChanged(settings),
        VoiceRuntimeEvent::VoiceState(voice_state(10, Some(Id::new(10)))),
        VoiceRuntimeEvent::VoiceServer(voice_server()),
    ] {
        runtime.apply_shared(event, &shared, discord);
    }
    (runtime, shared)
}

#[test]
fn server_moves_apply_destination_capture_policy() {
    for (source_deny, destination_deny, source_enabled, destination_enabled) in [
        (SPEAK, 0, false, true),
        (0, SPEAK, true, false),
        (USE_VOICE_ACTIVITY, 0, false, true),
        (0, USE_VOICE_ACTIVITY, true, false),
    ] {
        let discord = voice_permissions(source_deny, destination_deny);
        let settings = VoiceAudioSettings {
            allow_microphone_transmit: true,
            noise_suppression: true,
            voice_output_volume: VoiceVolumePercent::new(65),
            ..VoiceAudioSettings::default()
        };
        let (mut runtime, shared) = connected_runtime(&discord, settings);
        assert_eq!(
            runtime
                .capture_gate()
                .expect("active capture gate")
                .capture_enabled,
            source_enabled
        );

        runtime.apply_shared(
            VoiceRuntimeEvent::VoiceState(voice_state(10, Some(Id::new(20)))),
            &shared,
            &discord,
        );

        let gate = runtime.capture_gate().expect("moved capture gate");
        assert_eq!(gate.capture_enabled, destination_enabled);
        assert_eq!(gate.transmit_enabled, destination_enabled);
        assert!(gate.noise_suppression);
        let requested = shared
            .read()
            .expect("requested voice lock")
            .expect("moved voice");
        assert_eq!(requested.channel_id, Id::new(20));
        assert_eq!(requested.allow_microphone_transmit, destination_enabled);
        assert_eq!(requested.voice_output_volume, settings.voice_output_volume);
    }
}

#[test]
fn audio_settings_follow_server_voice_changes_and_preserve_pending_joins() {
    for (destination, pending_join) in [
        (Some(Id::new(20)), false),
        (None, false),
        (Some(Id::new(20)), true),
        (None, true),
    ] {
        let discord = voice_permissions(0, 0);
        let settings = VoiceAudioSettings::default();
        let (mut runtime, shared) = connected_runtime(&discord, settings);
        let next_settings = VoiceAudioSettings {
            voice_output_volume: VoiceVolumePercent::new(35),
            ..settings
        };
        let mut pending = runtime.requested.expect("connected voice");
        pending.channel_id = Id::new(30);
        {
            // Match a client edit queued behind an unprocessed server event.
            let mut requested = shared.write().expect("requested voice lock");
            requested
                .as_mut()
                .expect("requested voice")
                .set_audio_settings(next_settings);
            if pending_join {
                *requested = Some(pending);
            }
        }

        runtime.apply_shared(
            VoiceRuntimeEvent::VoiceState(voice_state(10, destination)),
            &shared,
            &discord,
        );
        runtime.apply_shared(
            VoiceRuntimeEvent::AudioSettingsChanged(next_settings),
            &shared,
            &discord,
        );

        assert_eq!(runtime.requested.map(|voice| voice.channel_id), destination);
        let shared_target = shared
            .read()
            .expect("requested voice lock")
            .map(|voice| voice.channel_id);
        assert_eq!(
            shared_target,
            if pending_join {
                Some(pending.channel_id)
            } else {
                destination
            }
        );
        if destination.is_some() {
            assert_eq!(
                runtime.playback_gate().expect("moved playback gate").volume,
                next_settings.voice_output_volume
            );
        }
        if pending_join {
            runtime.apply_shared(
                VoiceRuntimeEvent::Requested(Some(pending)),
                &shared,
                &discord,
            );
            runtime.apply_shared(
                VoiceRuntimeEvent::VoiceState(voice_state(10, Some(pending.channel_id))),
                &shared,
                &discord,
            );
            assert_eq!(
                runtime.requested.map(|voice| voice.channel_id),
                Some(pending.channel_id)
            );
        }
    }
}
