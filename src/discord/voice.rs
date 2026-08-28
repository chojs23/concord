#[cfg(test)]
use std::num::NonZeroU16;
use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "voice-playback")]
mod audio_buffer;
mod audio_runtime;
mod broadcast;
#[cfg(feature = "stream-broadcast")]
mod capture;
#[cfg(not(feature = "stream-broadcast"))]
#[path = "voice/capture_disabled.rs"]
mod capture;
mod capture_cancellation;
mod dave;
mod devices;
mod gateway;
mod info;
mod levels;
mod media;
#[cfg(any(test, feature = "voice-playback"))]
mod microphone;
#[cfg(feature = "voice-playback")]
mod noise;
mod opus;
mod outbound;
mod playback;
mod preview;
mod rtp;
mod runtime;
mod state;
mod stream;
mod system_audio;

pub(crate) use capture::list_stream_capture_targets;
pub(crate) use devices::{VoiceAudioSourceOptions, VoiceAudioSources, list_voice_audio_sources};
#[cfg(all(feature = "voice-playback", not(test)))]
use gateway::voice_speaking_payload;
#[cfg(test)]
use gateway::*;
#[cfg(not(test))]
use gateway::{run_voice_gateway_session, send_voice_binary, send_voice_text};
pub use info::{
    StreamCaptureTarget, StreamCaptureTargetKind, StreamCreateInfo, StreamDeleteInfo,
    StreamServerInfo, StreamUpdateInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo,
};
#[cfg(all(feature = "voice-playback", target_os = "linux", not(test)))]
use microphone::log_captured_alsa_errors;
#[cfg(all(feature = "voice-playback", not(test)))]
use microphone::run_voice_udp_transmit;
#[cfg(test)]
use microphone::*;
pub(crate) use preview::StreamPreviewUploader;
#[cfg(test)]
use runtime::{VoiceRuntimeAction, VoiceRuntimeState};
pub(crate) use runtime::{forward_app_event, run_voice_runtime};
pub(in crate::discord) use state::StreamState;
pub(in crate::discord) use state::VoiceState;
pub use state::{CurrentVoiceConnectionState, VoiceAudioSettings, VoiceParticipantState};

#[cfg(feature = "voice-playback")]
use self::opus::VoiceDecodedAudioOutput;
use self::opus::VoiceOpusDecode;
#[cfg(any(test, feature = "voice-playback"))]
use self::opus::VoiceOpusEncode;
#[cfg(test)]
use self::opus::mix_voice_decoded_samples;
use self::outbound::VoiceOutboundSendBlockReason;
#[cfg(any(test, feature = "voice-playback"))]
use self::outbound::{VoiceOutboundSendEvent, VoiceOutboundSendOutcome, VoiceOutboundSendState};
#[cfg(test)]
use ::opus::{Channels, Decoder as OpusDecoder, SampleRate as OpusSampleRate};
#[cfg(all(test, feature = "voice-playback"))]
use audio_buffer::{VoiceAudioBuffer, VoiceAudioOutputStats};
use audio_runtime::VoiceAudioRuntime;
use dave::{VoiceDaveState, VoiceMediaPayload, voice_speaking_microphone_active};
#[cfg(test)]
use dave::{VoiceSpeakingState, looks_like_dave_media_frame};
#[cfg(feature = "voice-playback")]
use playback::VoiceAudioOutput;
#[cfg(test)]
use playback::VoicePlaybackPlayoutBuffer;
use playback::{VoicePlaybackFrame, VoicePlaybackGate};
#[cfg(test)]
use playback::{VoicePlaybackPostProcess, VoicePlayoutFrame};
#[cfg(all(test, feature = "voice-playback"))]
use playback::{apply_voice_playback_gain_and_limit, write_voice_output_frame};
#[cfg(any(test, feature = "voice-playback"))]
use rtp::VoiceOutboundRtpState;
use rtp::{
    RtpHeader, VoiceRtpDecryptor, VoiceRtpEncryptor, looks_like_rtcp_packet, parse_rtp_header,
    rtcp_sender_ssrc,
};

#[cfg(test)]
use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
#[cfg(test)]
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, StreamTrait};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
#[cfg(feature = "voice-playback")]
use std::sync::Mutex as StdMutex;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::logging;
pub use levels::{
    MicrophoneSensitivityDb, VoiceParticipantPlaybackSettings, VoiceParticipantVolumePercent,
    VoiceVolumePercent,
};

use super::{client::AppEventPublisher, events::AppEvent, gateway::GatewayCommand};

const VOICE_GATEWAY_VERSION: u8 = 9;
const VOICE_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(feature = "stream-broadcast"))]
const STREAM_BROADCAST_FEATURE_DISABLED: &str =
    "stream broadcasting requires the stream-broadcast feature";

pub(crate) fn ensure_stream_broadcast_available() -> Result<(), String> {
    #[cfg(feature = "stream-broadcast")]
    {
        Ok(())
    }
    #[cfg(not(feature = "stream-broadcast"))]
    {
        Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
    }
}

const VOICE_RESUME_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const VOICE_CONNECTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const VOICE_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
const UDP_DISCOVERY_PACKET_LEN: usize = 74;
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const UDP_PING_PACKET_LEN: usize = 8;
const UDP_PING_INTERVAL: Duration = Duration::from_secs(5);
const RTP_HEADER_MIN_LEN: usize = 12;
const RTP_VERSION: u8 = 2;
const DISCORD_VOICE_PAYLOAD_TYPE: u8 = 0x78;
const DISCORD_STREAM_VIDEO_PAYLOAD_TYPE: u8 = 103;
const DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE: u8 = 104;
const LOCAL_STREAM_AUDIO_PAYLOAD_TYPE: u8 = 111;
const LOCAL_STREAM_VIDEO_PAYLOAD_TYPE: u8 = 96;
const RTP_HEADER_EXTENSION_BYTES: usize = 4;
const RTP_EXTENSION_WORD_BYTES: usize = 4;
const RTP_AEAD_TAG_BYTES: usize = 16;
const RTP_AEAD_NONCE_SUFFIX_BYTES: usize = 4;
const RTCP_MIN_PACKET_BYTES: usize = 4;
const RTCP_SENDER_SSRC_OFFSET: usize = 4;
const RTCP_SENDER_SSRC_BYTES: usize = 4;
const DAVE_MIN_SUPPLEMENTAL_BYTES: usize = 11;
const DAVE_MAGIC_MARKER: [u8; 2] = [0xfa, 0xfa];
const DISCORD_VOICE_SAMPLE_RATE: u32 = 48_000;
const DISCORD_VOICE_CHANNELS: u16 = 2;
#[cfg(feature = "voice-playback")]
const DISCORD_VOICE_CHANNELS_USIZE: usize = DISCORD_VOICE_CHANNELS as usize;
// These outbound helpers are intentionally not wired into the runtime yet.
// They let tests prove packet shapes before any live transmit path is added.
#[allow(dead_code)]
const DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL: usize = 960;
#[allow(dead_code)]
const DISCORD_OPUS_20MS_STEREO_SAMPLES: usize =
    DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL * DISCORD_VOICE_CHANNELS as usize;
#[cfg(any(test, feature = "voice-playback"))]
const DISCORD_OPUS_FRAME_DURATION: Duration = Duration::from_millis(20);
#[allow(dead_code)]
const DISCORD_OPUS_TIMESTAMP_INCREMENT: u32 = DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL as u32;
#[allow(dead_code)]
const DISCORD_OPUS_SILENCE_FRAME: [u8; 3] = [0xf8, 0xff, 0xfe];
#[allow(dead_code)]
const DISCORD_TRAILING_SILENCE_FRAMES: usize = 5;
#[allow(dead_code)]
const OPUS_MAX_ENCODED_FRAME_BYTES: usize = 4000;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_PCM_FRAME_QUEUE: usize = 16;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_MAX_LIVE_FRAMES: usize = 3;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_MAX_FRAME_AGE: Duration = Duration::from_millis(60);
#[cfg(feature = "voice-playback")]
const VOICE_TRANSMIT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(feature = "voice-playback")]
const VOICE_MIC_PREFERRED_BUFFER_FRAMES: u32 = 480;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_GATE_HANGOVER_FRAMES: u8 = 8;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_OVERLOAD_RECOVERY_FRAMES: u8 = 8;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES: u8 = 12;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES: usize = 8;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES: usize = DISCORD_OPUS_20MS_STEREO_SAMPLES / 20;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES: usize = DISCORD_OPUS_20MS_STEREO_SAMPLES / 8;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_HANDLING_NOISE_DELTA: i32 = 42_000;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA: i32 = 32_000;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_IMPULSE_DELTA: i32 = 36_000;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_ATTENUATION_GAIN: f32 = 0.35;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_HANDLING_NOISE_GAIN: f32 = 0.0;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_OVERLOAD_TRANSIENT_GAIN: f32 = 0.03;
#[cfg(feature = "voice-playback")]
const VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN: f32 = 0.15;
#[allow(dead_code)]
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_TRANSMIT_BOOST_GAIN: f32 = 1.5;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_SOFT_LIMIT_THRESHOLD: f32 = 0.85;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_SOFT_LIMIT_CEILING: f32 = 0.95;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_SOFT_LIMIT_CURVE: f32 = 4.0;
const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;
const VOICE_PLAYBACK_FRAME_QUEUE: usize = 256;
#[cfg(test)]
const VOICE_PLAYBACK_FRAME_DURATION: Duration = Duration::from_millis(20);
const VOICE_PLAYBACK_POLL_DURATION: Duration = Duration::from_millis(10);
const VOICE_OUTPUT_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);
const VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL: usize = 480;
#[cfg(feature = "voice-playback")]
const VOICE_TRANSMIT_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);
const VOICE_PLAYBACK_JITTER_BUFFER_DELAY: Duration = Duration::from_millis(60);
const VOICE_PLAYBACK_MAX_BUFFERED_FRAMES_PER_SSRC: usize = 32;
const VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES: usize = 5;
#[cfg(feature = "voice-playback")]
const VOICE_OUTPUT_UNDERRUN_FADE_MILLIS: u32 = 5;

/// Keeps boosted capture and playback samples bounded without flattening every
/// peak to the same hard-clipped value.
#[cfg(any(test, feature = "voice-playback"))]
fn soft_limit_voice_sample(sample: f32) -> f32 {
    let magnitude = sample.abs();
    if magnitude <= VOICE_SOFT_LIMIT_THRESHOLD {
        return sample;
    }

    let excess = (magnitude - VOICE_SOFT_LIMIT_THRESHOLD) / (1.0 - VOICE_SOFT_LIMIT_THRESHOLD);
    let shaped = VOICE_SOFT_LIMIT_THRESHOLD
        + (VOICE_SOFT_LIMIT_CEILING - VOICE_SOFT_LIMIT_THRESHOLD)
            * (1.0 - 1.0 / (1.0 + VOICE_SOFT_LIMIT_CURVE * excess));
    sample.signum() * shaped.min(VOICE_SOFT_LIMIT_CEILING)
}

const VOICE_OUTPUT_LOW_PASS_CUTOFF_HZ: f32 = 8_000.0;
#[cfg(feature = "voice-playback")]
const VOICE_AUDIO_OUTPUT_QUEUE: usize = 64;
#[cfg(feature = "voice-playback")]
const VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES: u64 = DISCORD_VOICE_SAMPLE_RATE as u64 * 60 / 1_000;
#[cfg(feature = "voice-playback")]
const VOICE_PULSE_OUTPUT_BUFFER_FRAMES: u32 = 2_400;
const AEAD_AES256_GCM_RTPSIZE: &str = "aead_aes256_gcm_rtpsize";
const AEAD_XCHACHA20_POLY1305_RTPSIZE: &str = "aead_xchacha20_poly1305_rtpsize";
const VOICE_REMOTE_SPEAKING_TTL: Duration = Duration::from_millis(500);
const VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL: Duration = Duration::from_millis(250);

const VOICE_OP_READY: u8 = 2;
const VOICE_OP_HEARTBEAT: u8 = 3;
const VOICE_OP_SESSION_DESCRIPTION: u8 = 4;
const VOICE_OP_SPEAKING: u8 = 5;
const VOICE_OP_HEARTBEAT_ACK: u8 = 6;
const VOICE_OP_RESUME: u8 = 7;
const VOICE_OP_HELLO: u8 = 8;
const VOICE_OP_RESUMED: u8 = 9;
const VOICE_OP_CLIENTS_CONNECT: u8 = 11;
const VOICE_OP_VIDEO: u8 = 12;
const VOICE_OP_CLIENT_DISCONNECT: u8 = 13;
const VOICE_OP_SESSION_UPDATE: u8 = 14;
const VOICE_OP_MEDIA_SINK_WANTS: u8 = 15;
const VOICE_OP_CLIENT_FLAGS: u8 = 18;
const VOICE_OP_CLIENT_PLATFORM: u8 = 20;
const VOICE_OP_DAVE_PREPARE_TRANSITION: u8 = 21;
const VOICE_OP_DAVE_EXECUTE_TRANSITION: u8 = 22;
const VOICE_OP_DAVE_TRANSITION_READY: u8 = 23;
const VOICE_OP_DAVE_PREPARE_EPOCH: u8 = 24;
const VOICE_OP_DAVE_MLS_EXTERNAL_SENDER: u8 = 25;
const VOICE_OP_DAVE_MLS_KEY_PACKAGE: u8 = 26;
const VOICE_OP_DAVE_MLS_PROPOSALS: u8 = 27;
const VOICE_OP_DAVE_MLS_COMMIT_WELCOME: u8 = 28;
const VOICE_OP_DAVE_MLS_ANNOUNCE_COMMIT_TRANSITION: u8 = 29;
const VOICE_OP_DAVE_MLS_WELCOME: u8 = 30;
const VOICE_OP_DAVE_MLS_INVALID_COMMIT_WELCOME: u8 = 31;

type VoiceGatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type VoiceWriter = Arc<Mutex<futures::stream::SplitSink<VoiceGatewayStream, WsMessage>>>;
type VoiceReader = futures::stream::SplitStream<VoiceGatewayStream>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VoiceRuntimeEvent {
    Requested(Option<CurrentVoiceConnectionState>),
    ManualRetry(CurrentVoiceConnectionState),
    AudioSourcesChanged(VoiceAudioSources),
    AudioSourcesApplyFailed {
        connection_id: u64,
        generation: u64,
        requested_sources: VoiceAudioSources,
        active_sources: VoiceAudioSources,
        message: String,
    },
    #[cfg(feature = "voice-playback")]
    PushToTalkEnabledChanged(bool),
    #[cfg(feature = "voice-playback")]
    PushToTalkPressed(bool),
    ReplaceParticipantPlaybackSettings(Vec<(Id<UserMarker>, VoiceParticipantPlaybackSettings)>),
    UpdateParticipantPlaybackSettings {
        user_id: Id<UserMarker>,
        settings: VoiceParticipantPlaybackSettings,
    },
    CurrentUserReady(Option<Id<UserMarker>>),
    VoiceState(VoiceStateInfo),
    VoiceServer(VoiceServerInfo),
    WatchStreamRequested(StreamWatchRequest),
    WatchStreamCancelled {
        stream_key: String,
    },
    StreamCreate(StreamCreateInfo),
    StreamServer(StreamServerInfo),
    StreamDelete(StreamDeleteInfo),
    StreamConnectionEstablished {
        connection_id: u64,
        stream_key: String,
    },
    StreamConnectionEnded {
        connection_id: u64,
        stream_key: String,
        outcome: VoiceConnectionEnd,
    },
    BroadcastStreamRequested(StreamBroadcastRequest),
    BroadcastStreamCaptureReady {
        request_id: u64,
        stream_key: String,
    },
    BroadcastStreamCaptureFailed {
        request_id: u64,
        stream_key: String,
        error: String,
    },
    #[cfg(test)]
    BroadcastStreamCancelled {
        stream_key: String,
    },
    BroadcastStreamStopRequested {
        stream_key: String,
    },
    BroadcastStreamConnectionEstablished {
        connection_id: u64,
        stream_key: String,
    },
    BroadcastStreamConnectionStable {
        connection_id: u64,
        stream_key: String,
    },
    BroadcastStreamConnectionEnded {
        connection_id: u64,
        stream_key: String,
        outcome: VoiceConnectionEnd,
    },
    ConnectionEstablished {
        connection_id: u64,
    },
    ConnectionEnded {
        connection_id: u64,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: String,
        endpoint: String,
        outcome: VoiceConnectionEnd,
    },
    Shutdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VoiceAudioSourceSelection {
    generation: u64,
    sources: VoiceAudioSources,
}

#[derive(Debug, Eq, PartialEq)]
struct VoiceAudioSourcesApplyOutcome {
    active_sources: VoiceAudioSources,
    error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamWatchRequest {
    pub(crate) stream_key: String,
    pub(crate) scope: VoiceScope,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) owner_id: Id<UserMarker>,
    pub(crate) display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamBroadcastRequest {
    pub(crate) stream_key: String,
    pub(crate) scope: VoiceScope,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) target: StreamCaptureTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VoiceConnectionEnd {
    Reconnect,
    Stop,
}

#[derive(Clone)]
pub(crate) struct VoiceStatusPublisher {
    events: AppEventPublisher,
}

#[derive(Clone)]
struct VoiceGatewaySession {
    connection_id: u64,
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    session_id: String,
    endpoint: String,
    token: String,
}

impl VoiceStatusPublisher {
    pub(crate) fn new(events: AppEventPublisher) -> Self {
        Self { events }
    }

    async fn publish(
        &self,
        session: &VoiceGatewaySession,
        status: VoiceConnectionStatus,
        message: impl Into<String>,
    ) {
        self.events
            .publish(AppEvent::VoiceConnectionStatusChanged {
                scope: session.scope,
                channel_id: Some(session.channel_id),
                status,
                message: Some(message.into()),
            })
            .await;
    }

    async fn publish_speaking(
        &self,
        session: &VoiceGatewaySession,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) {
        self.events
            .publish(AppEvent::VoiceSpeakingUpdate {
                scope: session.scope,
                channel_id: session.channel_id,
                user_id,
                speaking,
            })
            .await;
    }

    async fn publish_error(&self, message: String) {
        self.events
            .publish(AppEvent::GatewayError { message })
            .await;
    }

    async fn publish_audio_sources_apply_failed(
        &self,
        requested_sources: VoiceAudioSources,
        active_sources: VoiceAudioSources,
        message: String,
    ) {
        self.events
            .publish(AppEvent::VoiceAudioSourcesApplyFailed {
                requested_input_source: requested_sources.input,
                requested_output_source: requested_sources.output,
                active_input_source: active_sources.input,
                active_output_source: active_sources.output,
                message,
            })
            .await;
    }

    async fn publish_stream_playback_ready(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamPlaybackWindowReady {
                scope,
                channel_id,
                user_id,
            })
            .await;
    }

    async fn publish_stream_playback_ended(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        reconnecting: bool,
    ) {
        self.events
            .publish(AppEvent::StreamPlaybackEnded {
                scope,
                channel_id,
                user_id,
                reconnecting,
            })
            .await;
    }

    async fn publish_stream_broadcast_started(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamBroadcastStarted { scope, channel_id })
            .await;
    }

    async fn publish_stream_broadcast_audio_unavailable(&self, message: String) {
        self.events
            .publish(AppEvent::StreamBroadcastAudioUnavailable { message })
            .await;
    }

    async fn publish_stream_broadcast_ended(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        self.events
            .publish(AppEvent::StreamBroadcastEnded { scope, channel_id })
            .await;
    }
}

impl VoiceGatewaySession {
    fn connection_established_event(&self) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEstablished {
            connection_id: self.connection_id,
        }
    }

    fn matches_connection_end(
        &self,
        connection_id: u64,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: &str,
        endpoint: &str,
    ) -> bool {
        self.connection_id == connection_id
            && self.scope == scope
            && self.channel_id == channel_id
            && self.session_id == session_id
            && self.endpoint == endpoint
    }

    fn connection_ended_event(&self, outcome: VoiceConnectionEnd) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEnded {
            connection_id: self.connection_id,
            scope: self.scope,
            channel_id: self.channel_id,
            session_id: self.session_id.clone(),
            endpoint: self.endpoint.clone(),
            outcome,
        }
    }
}

impl PartialEq for VoiceGatewaySession {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.channel_id == other.channel_id
            && self.user_id == other.user_id
            && self.session_id == other.session_id
            && self.endpoint == other.endpoint
            && self.token == other.token
    }
}

impl Eq for VoiceGatewaySession {}

impl VoiceSpeakingTracker {
    fn new(local_user_id: Id<UserMarker>) -> Self {
        Self {
            local_user_id,
            remote_deadlines: HashMap::new(),
            local_speaking: false,
        }
    }

    fn record_remote(
        &mut self,
        user_id: Id<UserMarker>,
        speaking: bool,
        now: Instant,
    ) -> Option<bool> {
        if user_id == self.local_user_id {
            return None;
        }
        if speaking {
            let was_active = self.remote_deadlines.contains_key(&user_id);
            self.remote_deadlines
                .insert(user_id, now + VOICE_REMOTE_SPEAKING_TTL);
            return (!was_active).then_some(true);
        }
        if self.remote_deadlines.remove(&user_id).is_some() {
            Some(false)
        } else {
            None
        }
    }

    fn record_local(&mut self, speaking: bool) -> Option<bool> {
        if self.local_speaking == speaking {
            return None;
        }
        self.local_speaking = speaking;
        Some(speaking)
    }

    fn expire_remote(&mut self, now: Instant) -> Vec<Id<UserMarker>> {
        let expired = self
            .remote_deadlines
            .iter()
            .filter_map(|(user_id, deadline)| (*deadline <= now).then_some(*user_id))
            .collect::<Vec<_>>();
        for user_id in &expired {
            self.remote_deadlines.remove(user_id);
        }
        expired
    }

    fn clear_all(&mut self) -> Vec<Id<UserMarker>> {
        let mut cleared = self.remote_deadlines.keys().copied().collect::<Vec<_>>();
        self.remote_deadlines.clear();
        if self.local_speaking {
            self.local_speaking = false;
            if !cleared.contains(&self.local_user_id) {
                cleared.push(self.local_user_id);
            }
        }
        cleared
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VoiceTransportSession {
    ssrc: u32,
    ip: String,
    port: u16,
    modes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiscoveredVoiceAddress {
    address: String,
    port: u16,
}

#[derive(Clone, Eq, PartialEq)]
struct VoiceSessionDescription {
    audio_codec: String,
    mode: String,
    secret_key: Vec<u8>,
    dave_protocol_version: Option<u64>,
    video_codec: Option<String>,
    media_session_id: String,
    keyframe_interval: Option<u64>,
}

impl fmt::Debug for VoiceSessionDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceSessionDescription")
            .field("audio_codec", &self.audio_codec)
            .field("mode", &self.mode)
            .field("secret_key", &"<redacted>")
            .field("secret_key_len", &self.secret_key.len())
            .field("dave_protocol_version", &self.dave_protocol_version)
            .field("video_codec", &self.video_codec)
            .field("media_session_id", &self.media_session_id)
            .field("keyframe_interval", &self.keyframe_interval)
            .finish()
    }
}

impl VoiceSessionDescription {
    fn uses_same_transport(&self, other: &Self) -> bool {
        self.mode == other.mode && self.secret_key == other.secret_key
    }
}

struct VoiceSpeakingTracker {
    local_user_id: Id<UserMarker>,
    remote_deadlines: HashMap<Id<UserMarker>, Instant>,
    local_speaking: bool,
}

/// A child task slot that aborts the task it holds when replaced or torn
/// down, logging the transition under the slot's label.
struct ManagedTask {
    label: &'static str,
    task: Option<JoinHandle<()>>,
}

impl ManagedTask {
    const fn new(label: &'static str) -> Self {
        Self { label, task: None }
    }

    fn replace(&mut self, task: JoinHandle<()>) {
        if let Some(previous) = self.task.replace(task) {
            logging::debug("voice", format!("aborting previous {}", self.label));
            previous.abort();
        }
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            logging::debug("voice", format!("aborting {}", self.label));
            task.abort();
        }
    }
}

struct VoiceChildTasks {
    heartbeat: ManagedTask,
    udp_ping: ManagedTask,
    udp_receive: ManagedTask,
    #[cfg(feature = "voice-playback")]
    udp_transmit: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    transmit_gate: Option<watch::Sender<VoiceCaptureGate>>,
    #[cfg(feature = "voice-playback")]
    playback_enabled: Option<Arc<AtomicBool>>,
    #[cfg(feature = "voice-playback")]
    playback_volume: Option<Arc<AtomicU8>>,
    #[cfg(feature = "voice-playback")]
    microphone_pcm_tx: Option<mpsc::Sender<VoiceMicrophoneFrame>>,
    opus_decode: ManagedTask,
    #[cfg(feature = "voice-playback")]
    audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    decoded_audio_output: Option<VoiceDecodedAudioOutput>,
    #[cfg(feature = "voice-playback")]
    microphone_capture: Option<VoiceMicrophoneCapture>,
    #[cfg(feature = "voice-playback")]
    microphone_source: Option<String>,
    #[cfg(feature = "voice-playback")]
    output_source: Option<String>,
    // Declared last so it is dropped after the task handles above — aborting
    // them before the runtime they ran on tears down.
    audio_runtime: Option<VoiceAudioRuntime>,
}

#[derive(Default)]
struct VoiceHeartbeatAckState {
    awaiting_ack: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceHeartbeatTimeout {
    generation: u64,
}

impl VoiceHeartbeatAckState {
    fn mark_sent(&mut self) -> bool {
        if self.awaiting_ack {
            return false;
        }
        self.awaiting_ack = true;
        true
    }

    fn mark_acknowledged(&mut self) {
        self.awaiting_ack = false;
    }

    fn reset(&mut self) {
        self.awaiting_ack = false;
    }
}

impl Default for VoiceChildTasks {
    fn default() -> Self {
        Self {
            heartbeat: ManagedTask::new("voice heartbeat task"),
            udp_ping: ManagedTask::new("voice UDP ping task"),
            udp_receive: ManagedTask::new("voice UDP receive task"),
            #[cfg(feature = "voice-playback")]
            udp_transmit: None,
            #[cfg(feature = "voice-playback")]
            transmit_gate: None,
            #[cfg(feature = "voice-playback")]
            playback_enabled: None,
            #[cfg(feature = "voice-playback")]
            playback_volume: None,
            #[cfg(feature = "voice-playback")]
            microphone_pcm_tx: None,
            opus_decode: ManagedTask::new("voice Opus decode task"),
            #[cfg(feature = "voice-playback")]
            audio_output: None,
            #[cfg(feature = "voice-playback")]
            decoded_audio_output: None,
            #[cfg(feature = "voice-playback")]
            microphone_capture: None,
            #[cfg(feature = "voice-playback")]
            microphone_source: None,
            #[cfg(feature = "voice-playback")]
            output_source: None,
            audio_runtime: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceCaptureGate {
    capture_enabled: bool,
    transmit_enabled: bool,
    use_voice_activity: bool,
    noise_suppression: bool,
    microphone_sensitivity: MicrophoneSensitivityDb,
    microphone_volume: VoiceVolumePercent,
}

struct VoiceGatewayControls {
    audio_sources_rx: watch::Receiver<VoiceAudioSourceSelection>,
    initial_capture_gate: VoiceCaptureGate,
    capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    initial_playback_gate: VoicePlaybackGate,
    playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
    participant_playback_rx:
        watch::Receiver<HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>>,
}

#[cfg(feature = "voice-playback")]
struct VoiceUdpTransmitContext {
    udp_socket: Arc<UdpSocket>,
    writer: VoiceWriter,
    description: VoiceSessionDescription,
    ssrc: u32,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    local_speaking_tx: mpsc::UnboundedSender<bool>,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophoneCapture {
    _stream: cpal::Stream,
    stats: Arc<VoiceMicrophoneCaptureStats>,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophonePcmFrames {
    frames_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    source_sample_rate: u32,
    source_pending: Vec<i16>,
    output_pending: Vec<i16>,
    next_source_frame: f64,
}

#[cfg(feature = "voice-playback")]
#[derive(Debug)]
struct VoiceMicrophoneFrame {
    samples: Vec<i16>,
    captured_at: Instant,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophoneCaptureStats {
    chunks: AtomicU64,
    frames: AtomicU64,
    min_callback_frames: AtomicU64,
    max_callback_frames: AtomicU64,
    queued_frames: AtomicU64,
    dropped_frames: AtomicU64,
    peak_sample: AtomicU64,
    clipped_samples: AtomicU64,
}

#[cfg(feature = "voice-playback")]
#[derive(Default)]
struct VoiceUdpTransmitStats {
    sent_packets: u64,
    stale_microphone_frames_dropped: u64,
    noise_suppressed_frames: u64,
    max_noise_suppression_processing_us: u128,
    overload_smoothed_frames: u64,
    limited_samples: u64,
    max_microphone_queue_depth: usize,
    max_microphone_frame_age_ms: u128,
    max_frame_gap_ms: u128,
    last_frame_at: Option<Instant>,
}

#[cfg(any(test, feature = "voice-playback"))]
#[derive(Default)]
struct VoiceTrailingSilence {
    remaining_frames: usize,
}

#[cfg(any(test, feature = "voice-playback"))]
impl VoiceTrailingSilence {
    fn start(&mut self, speaking: bool) {
        if speaking && self.remaining_frames == 0 {
            self.remaining_frames = DISCORD_TRAILING_SILENCE_FRAMES;
        }
    }

    fn cancel(&mut self) {
        self.remaining_frames = 0;
    }

    fn take_frame(&mut self) -> Option<bool> {
        if self.remaining_frames == 0 {
            return None;
        }
        self.remaining_frames -= 1;
        Some(self.remaining_frames == 0)
    }
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VoiceMicrophoneOverloadKind {
    HandlingNoise,
    Transient,
    Attenuated,
    Recovery,
}

#[cfg(any(test, feature = "voice-playback"))]
#[derive(Clone, Copy, Debug)]
struct VoiceMicrophoneOverloadDecision {
    kind: VoiceMicrophoneOverloadKind,
    gain: f32,
}

#[cfg(feature = "voice-playback")]
#[derive(Default)]
struct VoiceMicrophoneGateState {
    hangover_frames: u8,
    overload_recovery_frames: u8,
    handling_noise_suppression_frames: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceBinaryFrame<'a> {
    sequence: i64,
    opcode: u8,
    payload: &'a [u8],
}

impl fmt::Debug for VoiceGatewaySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceGatewaySession")
            .field("connection_id", &self.connection_id)
            .field("scope", &self.scope)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field("session_id", &"<redacted>")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl VoiceChildTasks {
    fn replace_heartbeat(&mut self, task: JoinHandle<()>) {
        self.heartbeat.replace(task);
    }

    fn replace_udp_receive(&mut self, task: JoinHandle<()>) {
        self.udp_receive.replace(task);
    }

    fn replace_udp_ping(&mut self, task: JoinHandle<()>) {
        self.udp_ping.replace(task);
    }

    #[cfg(feature = "voice-playback")]
    fn install_udp_transmit(
        &mut self,
        task: JoinHandle<()>,
        gate: watch::Sender<VoiceCaptureGate>,
        microphone_pcm_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    ) {
        debug_assert!(self.udp_transmit.is_none());
        self.udp_transmit = Some(task);
        self.transmit_gate = Some(gate);
        self.microphone_pcm_tx = Some(microphone_pcm_tx);
    }

    #[cfg(feature = "voice-playback")]
    fn signal_udp_transmit_stop(&mut self) {
        if let Some(gate) = self.transmit_gate.as_ref() {
            let _ = gate.send(VoiceCaptureGate {
                capture_enabled: false,
                transmit_enabled: false,
                use_voice_activity: true,
                noise_suppression: false,
                microphone_sensitivity: MicrophoneSensitivityDb::default(),
                microphone_volume: VoiceVolumePercent::default(),
            });
        }
        self.microphone_capture = None;
        self.microphone_pcm_tx = None;
        self.transmit_gate = None;
    }

    #[cfg(feature = "voice-playback")]
    async fn stop_udp_transmit_gracefully(&mut self, label: &str) -> bool {
        let Some(mut task) = self.udp_transmit.take() else {
            self.signal_udp_transmit_stop();
            return false;
        };
        logging::debug("voice", label);
        self.signal_udp_transmit_stop();
        match timeout(VOICE_TRANSMIT_SHUTDOWN_TIMEOUT, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                logging::debug("voice", format!("voice UDP transmit task ended: {error}"));
            }
            Err(_) => {
                logging::debug("voice", "voice UDP transmit graceful stop timed out");
                task.abort();
                let _ = task.await;
            }
        }
        true
    }

    fn replace_opus_decode(&mut self, opus_decode: VoiceOpusDecode) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(decoded_audio_output) = self.decoded_audio_output.take() {
                decoded_audio_output.replace(None);
            }
            self.audio_output = opus_decode.audio_output;
            self.decoded_audio_output = Some(opus_decode.decoded_audio_output);
            self.playback_enabled = Some(opus_decode.playback_enabled);
            self.playback_volume = Some(opus_decode.playback_volume);
        }
        self.opus_decode.replace(opus_decode.task);
    }

    fn abort_all(&mut self) {
        self.heartbeat.abort();
        self.udp_ping.abort();
        self.udp_receive.abort();
        #[cfg(feature = "voice-playback")]
        if let Some(task) = self.udp_transmit.take() {
            logging::debug("voice", "stopping voice UDP transmit task");
            self.signal_udp_transmit_stop();
            task.abort();
        }
        self.opus_decode.abort();
        #[cfg(feature = "voice-playback")]
        {
            if let Some(decoded_audio_output) = self.decoded_audio_output.take() {
                decoded_audio_output.replace(None);
            }
            self.audio_output = None;
            self.playback_enabled = None;
            self.playback_volume = None;
            self.microphone_capture = None;
        }
    }

    async fn shutdown_all(&mut self) {
        #[cfg(feature = "voice-playback")]
        let _ = self
            .stop_udp_transmit_gracefully("stopping voice UDP transmit task")
            .await;
        self.abort_all();
    }

    #[allow(dead_code)]
    fn set_microphone_capture_enabled(&mut self, enabled: bool) {
        #[cfg(feature = "voice-playback")]
        {
            match (enabled, self.microphone_capture.is_some()) {
                (true, false) => {
                    match VoiceMicrophoneCapture::start(
                        self.microphone_pcm_tx.clone(),
                        self.microphone_source.as_deref(),
                    ) {
                        Ok(capture) => self.microphone_capture = Some(capture),
                        Err(error) => logging::error(
                            "voice",
                            format!("voice microphone capture unavailable: {error}"),
                        ),
                    }
                }
                (false, true) => {
                    logging::debug("voice", "stopping voice microphone capture");
                    self.microphone_capture = None;
                }
                _ => {}
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = enabled;
        }
    }

    fn set_voice_transmit_gate(&mut self, capture_gate: VoiceCaptureGate) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(gate) = self.transmit_gate.as_ref() {
                let _ = gate.send(capture_gate);
            }
            self.set_microphone_capture_enabled(
                capture_gate.capture_enabled && self.microphone_pcm_tx.is_some(),
            );
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = capture_gate;
        }
    }

    fn set_voice_playback_gate(&mut self, playback_gate: VoicePlaybackGate) {
        #[cfg(feature = "voice-playback")]
        {
            if let Some(playback_enabled) = self.playback_enabled.as_ref() {
                playback_enabled.store(playback_gate.enabled, Ordering::Relaxed);
            }
            if let Some(playback_volume) = self.playback_volume.as_ref() {
                playback_volume.store(playback_gate.volume.value(), Ordering::Relaxed);
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = playback_gate;
        }
    }

    fn set_voice_audio_sources(
        &mut self,
        sources: VoiceAudioSources,
        capture_gate: VoiceCaptureGate,
    ) -> VoiceAudioSourcesApplyOutcome {
        #[cfg(feature = "voice-playback")]
        {
            let mut errors = Vec::new();
            if self.microphone_source != sources.input {
                let capture_should_run =
                    capture_gate.capture_enabled && self.microphone_pcm_tx.is_some();
                if self.microphone_capture.is_some() || capture_should_run {
                    logging::debug(
                        "voice",
                        "starting replacement voice microphone capture for new source",
                    );
                    match VoiceMicrophoneCapture::start(
                        self.microphone_pcm_tx.clone(),
                        sources.input.as_deref(),
                    ) {
                        Ok(capture) => {
                            self.microphone_capture = Some(capture);
                            self.microphone_source = sources.input;
                            logging::debug(
                                "voice",
                                "replaced voice microphone capture for new source",
                            );
                        }
                        Err(error) => errors.push(format!(
                            "Could not switch voice input source. The previous source remains active: {error}"
                        )),
                    }
                } else {
                    self.microphone_source = sources.input;
                }
            }
            if self.output_source != sources.output {
                if let Err(error) = self.replace_voice_audio_output(sources.output.as_deref()) {
                    errors.push(format!(
                        "Could not switch voice output source. The previous source remains active: {error}"
                    ));
                } else {
                    self.output_source = sources.output;
                }
            }
            VoiceAudioSourcesApplyOutcome {
                active_sources: VoiceAudioSources {
                    input: self.microphone_source.clone(),
                    output: self.output_source.clone(),
                },
                error: (!errors.is_empty()).then(|| errors.join(" ")),
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = capture_gate;
            VoiceAudioSourcesApplyOutcome {
                active_sources: sources,
                error: None,
            }
        }
    }

    #[cfg(feature = "voice-playback")]
    fn replace_voice_audio_output(&mut self, output_source: Option<&str>) -> Result<(), String> {
        let Some(decoded_audio_output) = self.decoded_audio_output.clone() else {
            return Ok(());
        };
        let (Some(playback_enabled), Some(playback_volume)) =
            (self.playback_enabled.clone(), self.playback_volume.clone())
        else {
            return Ok(());
        };

        let audio_output =
            VoiceAudioOutput::start(playback_enabled, playback_volume, output_source)?;
        decoded_audio_output.replace(Some(&audio_output));
        self.audio_output = Some(audio_output);
        logging::debug("voice", "replaced voice audio output for new source");
        Ok(())
    }
}

impl Drop for VoiceChildTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

#[cfg(test)]
mod tests;
