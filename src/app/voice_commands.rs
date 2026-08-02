use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::{
    DiscordClient,
    discord::{
        AppEvent, MicrophoneSensitivityDb, StreamCaptureTarget, VoiceAudioSettings,
        VoiceAudioSources, VoiceConnectionStatus, VoiceParticipantPlaybackSettings, VoiceScope,
        VoiceVolumePercent,
    },
    logging,
};

/// Join carries the whole audio configuration alongside the target channel, so
/// it travels as one value rather than a nine-argument call.
pub(super) struct JoinRequest {
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub input_source: Option<String>,
    pub output_source: Option<String>,
    pub allow_microphone_transmit: bool,
    pub noise_suppression: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
    pub participant_playback_settings: Vec<(Id<UserMarker>, VoiceParticipantPlaybackSettings)>,
}

pub(super) async fn join_channel(client: DiscordClient, request: JoinRequest) {
    let JoinRequest {
        scope,
        channel_id,
        self_mute,
        self_deaf,
        input_source,
        output_source,
        allow_microphone_transmit,
        noise_suppression,
        microphone_sensitivity,
        microphone_volume,
        voice_output_volume,
        participant_playback_settings,
    } = request;
    let audio_settings = VoiceAudioSettings {
        allow_microphone_transmit,
        noise_suppression,
        microphone_sensitivity,
        microphone_volume,
        voice_output_volume,
    };

    client.replace_voice_participant_playback_settings(participant_playback_settings);
    client.update_voice_audio_sources(VoiceAudioSources {
        input: input_source,
        output: output_source,
    });
    if let Err(message) = client.request_voice_join(scope, channel_id, self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: Some(channel_id),
                status: VoiceConnectionStatus::Failed,
                message: Some(message),
            })
            .await;
        return;
    }

    if let Err(message) = client.update_voice_capture_permission(scope, channel_id, audio_settings)
    {
        logging::error("app", &message);
        let _ = client.update_voice_capture_permission(
            scope,
            channel_id,
            VoiceAudioSettings {
                allow_microphone_transmit: false,
                ..audio_settings
            },
        );
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("Joined voice listen-only: {message}"),
            })
            .await;
    }
    client
        .publish_event(AppEvent::VoiceConnectionStatusChanged {
            scope,
            channel_id: Some(channel_id),
            status: VoiceConnectionStatus::Connecting,
            message: Some("Voice join requested".to_owned()),
        })
        .await;
}

pub(super) fn update_audio_sources(client: &DiscordClient, sources: VoiceAudioSources) {
    client.update_voice_audio_sources(sources);
}

pub(super) async fn load_voice_audio_sources(client: DiscordClient, request_id: u64) {
    let result = tokio::task::spawn_blocking(crate::discord::list_voice_audio_sources)
        .await
        .map_err(|error| format!("voice audio source task failed: {error}"))
        .and_then(|result| result);
    let (inputs, outputs, error) = match result {
        Ok(options) => {
            let (inputs, outputs) = options.into_parts();
            (inputs, outputs, None)
        }
        Err(error) => {
            logging::error("voice", &error);
            (Vec::new(), Vec::new(), Some(error))
        }
    };
    client
        .publish_event(AppEvent::VoiceAudioSourcesLoaded {
            request_id,
            inputs,
            outputs,
            error,
        })
        .await;
}

pub(super) async fn update_state(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    self_mute: bool,
    self_deaf: bool,
) {
    if let Err(message) = client.update_voice_state(scope, Some(channel_id), self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError { message })
            .await;
    }
}

pub(super) async fn update_capture_permission(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    settings: VoiceAudioSettings,
) {
    if let Err(message) = client.update_voice_capture_permission(scope, channel_id, settings) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError { message })
            .await;
    }
}

pub(super) fn update_participant_playback(
    client: &DiscordClient,
    user_id: Id<UserMarker>,
    settings: VoiceParticipantPlaybackSettings,
) {
    client.update_voice_participant_playback_settings(user_id, settings);
}

pub(super) async fn watch_stream(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    display_name: String,
) {
    if let Err(message) =
        client.request_stream_watch(scope, channel_id, user_id, display_name.clone())
    {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("Could not watch {display_name}'s stream: {message}"),
            })
            .await;
    }
}

pub(super) async fn load_stream_capture_targets(
    client: DiscordClient,
    request_id: crate::discord::StreamCaptureTargetsRequestId,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
) {
    let result = tokio::task::spawn_blocking(crate::discord::list_stream_capture_targets)
        .await
        .map_err(|error| format!("screen capture target task failed: {error}"))
        .and_then(|result| result);
    let (targets, error) = match result {
        Ok(targets) => (targets, None),
        Err(error) => {
            logging::error("stream", &error);
            (Vec::new(), Some(error))
        }
    };
    client
        .publish_event(AppEvent::StreamCaptureTargetsLoaded {
            request_id,
            scope,
            channel_id,
            targets,
            error,
        })
        .await;
}

pub(super) async fn start_stream(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    target: StreamCaptureTarget,
) {
    if let Err(message) = client.request_stream_start(scope, channel_id, target) {
        logging::error("stream", &message);
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("Could not start stream: {message}"),
            })
            .await;
    }
}

pub(super) async fn stop_stream(
    client: DiscordClient,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
) {
    if let Err(message) = client.request_stream_stop(scope, channel_id) {
        logging::error("stream", &message);
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("Could not stop stream: {message}"),
            })
            .await;
    }
}

pub(super) async fn leave_channel(
    client: DiscordClient,
    scope: VoiceScope,
    self_mute: bool,
    self_deaf: bool,
) {
    if let Err(message) = client.update_voice_state(scope, None, self_mute, self_deaf) {
        logging::error("app", &message);
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: None,
                status: VoiceConnectionStatus::Failed,
                message: Some(message),
            })
            .await;
    } else {
        client
            .publish_event(AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id: None,
                status: VoiceConnectionStatus::Disconnected,
                message: Some("Voice leave requested".to_owned()),
            })
            .await;
    }
}
