#[cfg(test)]
use std::num::NonZeroU16;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[cfg(feature = "voice-playback")]
mod audio_buffer;
mod audio_runtime;
mod dave;
mod gateway;
mod info;
mod levels;
#[cfg(any(test, feature = "voice-playback"))]
mod microphone;
mod opus;
mod outbound;
mod playback;
mod rtp;
mod runtime;
mod state;

#[cfg(all(feature = "voice-playback", not(test)))]
use gateway::voice_speaking_payload;
#[cfg(test)]
use gateway::*;
#[cfg(not(test))]
use gateway::{run_voice_gateway_session, send_voice_binary, send_voice_text};
pub use info::{
    VoiceConnectionStatus, VoiceScope, VoiceServerInfo, VoiceSoundKind, VoiceStateInfo,
};
#[cfg(all(feature = "voice-playback", target_os = "linux", not(test)))]
use microphone::log_captured_alsa_errors;
#[cfg(all(feature = "voice-playback", not(test)))]
use microphone::run_voice_udp_transmit;
#[cfg(test)]
use microphone::*;
#[cfg(test)]
use runtime::{VoiceRuntimeAction, VoiceRuntimeState};
pub(crate) use runtime::{forward_app_event, run_voice_runtime};
pub(in crate::discord) use state::VoiceState;
pub use state::{CurrentVoiceConnectionState, VoiceParticipantState};

use self::opus::VoiceOpusDecode;
#[cfg(any(test, feature = "voice-playback"))]
use self::opus::VoiceOpusEncode;
#[cfg(test)]
use self::opus::mix_voice_decoded_samples;
use self::outbound::VoiceOutboundSendBlockReason;
#[cfg(any(test, feature = "voice-playback"))]
use self::outbound::{VoiceOutboundSendEvent, VoiceOutboundSendOutcome, VoiceOutboundSendState};
#[cfg(test)]
use ::opus::{Channels, Decoder as OpusDecoder};
#[cfg(all(test, feature = "voice-playback"))]
use audio_buffer::VoiceAudioBuffer;
use audio_runtime::VoiceAudioRuntime;
use dave::{VoiceDaveState, VoiceMediaPayload, voice_speaking_microphone_active};
#[cfg(test)]
use dave::{VoiceSpeakingState, looks_like_dave_media_frame};
#[cfg(feature = "voice-playback")]
use playback::VoiceAudioOutput;
#[cfg(test)]
use playback::VoicePlaybackPlayoutBuffer;
#[cfg(all(test, feature = "voice-playback"))]
use playback::write_voice_output_frame;
use playback::{VoicePlaybackFrame, VoicePlaybackGate};
#[cfg(test)]
use playback::{VoicePlaybackPostProcess, VoicePlayoutFrame};
#[cfg(any(test, feature = "voice-playback"))]
use rtp::VoiceOutboundRtpState;
#[cfg(test)]
use rtp::VoiceRtpEncryptor;
use rtp::{
    RtpHeader, VoiceRtpDecryptor, looks_like_rtcp_packet, parse_rtp_header, rtcp_sender_ssrc,
};

#[cfg(test)]
use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
#[cfg(test)]
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
#[cfg(feature = "voice-playback")]
use std::sync::Mutex as StdMutex;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, Mutex as AsyncMutex, mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::{
    DiscordState, SequencedAppEvent, SnapshotRevision,
    ids::{
        Id,
        marker::{ChannelMarker, UserMarker},
    },
};
use crate::logging;
pub use levels::{MicrophoneSensitivityDb, VoiceVolumePercent};

use super::{client::publish_app_event, events::AppEvent};

const VOICE_GATEWAY_VERSION: u8 = 9;
const VOICE_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const VOICE_CONNECTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const UDP_DISCOVERY_PACKET_LEN: usize = 74;
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const RTP_HEADER_MIN_LEN: usize = 12;
const RTP_VERSION: u8 = 2;
const DISCORD_VOICE_PAYLOAD_TYPE: u8 = 0x78;
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
const VOICE_MIC_TRANSMIT_BOOST_GAIN: f32 = 1.15;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_SOFT_LIMIT_THRESHOLD: f32 = 0.85;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_SOFT_LIMIT_CEILING: f32 = 0.95;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_MIC_SOFT_LIMIT_CURVE: f32 = 4.0;
const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;
const VOICE_PLAYBACK_FRAME_QUEUE: usize = 256;
#[cfg(any(test, feature = "voice-playback"))]
const VOICE_PLAYBACK_FRAME_DURATION: Duration = Duration::from_millis(20);
const VOICE_PLAYBACK_POLL_DURATION: Duration = Duration::from_millis(10);
const VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL: usize = 480;
#[cfg(feature = "voice-playback")]
const VOICE_TRANSMIT_STATS_LOG_INTERVAL: Duration = Duration::from_secs(5);
const VOICE_PLAYBACK_JITTER_BUFFER_DELAY: Duration = Duration::from_millis(60);
const VOICE_PLAYBACK_MAX_BUFFERED_FRAMES_PER_SSRC: usize = 32;
const VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES: usize = 5;
#[cfg(feature = "voice-playback")]
const VOICE_OUTPUT_UNDERRUN_FADE_MILLIS: u32 = 5;
const VOICE_OUTPUT_LOW_PASS_CUTOFF_HZ: f32 = 8_000.0;
#[cfg(feature = "voice-playback")]
const VOICE_AUDIO_OUTPUT_QUEUE: usize = 64;
const AEAD_AES256_GCM_RTPSIZE: &str = "aead_aes256_gcm_rtpsize";
const AEAD_XCHACHA20_POLY1305_RTPSIZE: &str = "aead_xchacha20_poly1305_rtpsize";
const VOICE_REMOTE_SPEAKING_TTL: Duration = Duration::from_millis(500);
const VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL: Duration = Duration::from_millis(250);

const VOICE_OP_READY: u8 = 2;
const VOICE_OP_SESSION_DESCRIPTION: u8 = 4;
const VOICE_OP_SPEAKING: u8 = 5;
const VOICE_OP_HEARTBEAT_ACK: u8 = 6;
const VOICE_OP_HELLO: u8 = 8;
const VOICE_OP_CLIENTS_CONNECT: u8 = 11;
const VOICE_OP_CLIENT_DISCONNECT: u8 = 13;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VoiceRuntimeEvent {
    Requested(Option<CurrentVoiceConnectionState>),
    CurrentUserReady(Option<Id<UserMarker>>),
    VoiceState(VoiceStateInfo),
    VoiceServer(VoiceServerInfo),
    ConnectionEnded {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: String,
        endpoint: String,
    },
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct VoiceStatusPublisher {
    effects_tx: mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    revision: Arc<RwLock<SnapshotRevision>>,
    publish_lock: Arc<AsyncMutex<()>>,
}

#[derive(Clone, Eq, PartialEq)]
struct VoiceGatewaySession {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    session_id: String,
    endpoint: String,
    token: String,
}

impl VoiceStatusPublisher {
    pub(crate) fn new(
        effects_tx: mpsc::Sender<SequencedAppEvent>,
        snapshots_tx: watch::Sender<SnapshotRevision>,
        state: Arc<RwLock<DiscordState>>,
        revision: Arc<RwLock<SnapshotRevision>>,
        publish_lock: Arc<AsyncMutex<()>>,
    ) -> Self {
        Self {
            effects_tx,
            snapshots_tx,
            state,
            revision,
            publish_lock,
        }
    }

    async fn publish(
        &self,
        session: &VoiceGatewaySession,
        status: VoiceConnectionStatus,
        message: impl Into<String>,
    ) {
        publish_app_event(
            &self.effects_tx,
            &self.snapshots_tx,
            &self.state,
            &self.revision,
            &self.publish_lock,
            &AppEvent::VoiceConnectionStatusChanged {
                scope: session.scope,
                channel_id: Some(session.channel_id),
                status,
                message: Some(message.into()),
            },
        )
        .await;
    }

    async fn publish_speaking(
        &self,
        session: &VoiceGatewaySession,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) {
        publish_app_event(
            &self.effects_tx,
            &self.snapshots_tx,
            &self.state,
            &self.revision,
            &self.publish_lock,
            &AppEvent::VoiceSpeakingUpdate {
                scope: session.scope,
                channel_id: session.channel_id,
                user_id,
                speaking,
            },
        )
        .await;
    }
}

impl VoiceGatewaySession {
    fn matches_connection_end(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        session_id: &str,
        endpoint: &str,
    ) -> bool {
        self.scope == scope
            && self.channel_id == channel_id
            && self.session_id == session_id
            && self.endpoint == endpoint
    }

    fn connection_ended_event(&self) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEnded {
            scope: self.scope,
            channel_id: self.channel_id,
            session_id: self.session_id.clone(),
            endpoint: self.endpoint.clone(),
        }
    }
}

impl VoiceSpeakingTracker {
    fn record_remote(
        &mut self,
        user_id: Id<UserMarker>,
        speaking: bool,
        now: Instant,
    ) -> Option<bool> {
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

    fn clear_all(&mut self, local_user_id: Id<UserMarker>) -> Vec<Id<UserMarker>> {
        let mut cleared = self.remote_deadlines.keys().copied().collect::<Vec<_>>();
        self.remote_deadlines.clear();
        if self.local_speaking {
            self.local_speaking = false;
            if !cleared.contains(&local_user_id) {
                cleared.push(local_user_id);
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
    mode: String,
    secret_key: Vec<u8>,
    dave_protocol_version: Option<u64>,
}

impl fmt::Debug for VoiceSessionDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceSessionDescription")
            .field("mode", &self.mode)
            .field("secret_key", &"<redacted>")
            .field("secret_key_len", &self.secret_key.len())
            .field("dave_protocol_version", &self.dave_protocol_version)
            .finish()
    }
}

#[derive(Default)]
struct VoiceSpeakingTracker {
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
    microphone_pcm_tx: Option<mpsc::Sender<Vec<i16>>>,
    opus_decode: ManagedTask,
    #[cfg(feature = "voice-playback")]
    audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    microphone_capture: Option<VoiceMicrophoneCapture>,
    // Declared last so it is dropped after the task handles above — aborting
    // them before the runtime they ran on tears down.
    audio_runtime: Option<VoiceAudioRuntime>,
}

impl Default for VoiceChildTasks {
    fn default() -> Self {
        Self {
            heartbeat: ManagedTask::new("voice heartbeat task"),
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
            microphone_capture: None,
            audio_runtime: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceCaptureGate {
    enabled: bool,
    microphone_sensitivity: MicrophoneSensitivityDb,
    microphone_volume: VoiceVolumePercent,
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
    frames_tx: mpsc::Sender<Vec<i16>>,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    source_sample_rate: u32,
    source_pending: Vec<i16>,
    output_pending: Vec<i16>,
    next_source_frame: f64,
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
    overload_smoothed_frames: u64,
    limited_samples: u64,
    max_frame_gap_ms: u128,
    last_frame_at: Option<Instant>,
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

    #[cfg(feature = "voice-playback")]
    async fn replace_udp_transmit(
        &mut self,
        task: JoinHandle<()>,
        gate: watch::Sender<VoiceCaptureGate>,
        microphone_pcm_tx: mpsc::Sender<Vec<i16>>,
    ) {
        if self.udp_transmit.is_some() {
            self.stop_udp_transmit_gracefully("stopping previous voice UDP transmit task")
                .await;
        }
        self.udp_transmit = Some(task);
        self.transmit_gate = Some(gate);
        self.microphone_pcm_tx = Some(microphone_pcm_tx);
    }

    #[cfg(feature = "voice-playback")]
    fn signal_udp_transmit_stop(&mut self) {
        if let Some(gate) = self.transmit_gate.as_ref() {
            let _ = gate.send(VoiceCaptureGate {
                enabled: false,
                microphone_sensitivity: MicrophoneSensitivityDb::default(),
                microphone_volume: VoiceVolumePercent::default(),
            });
        }
        self.microphone_capture = None;
        self.microphone_pcm_tx = None;
        self.transmit_gate = None;
    }

    #[cfg(feature = "voice-playback")]
    async fn stop_udp_transmit_gracefully(&mut self, label: &str) {
        let Some(mut task) = self.udp_transmit.take() else {
            self.signal_udp_transmit_stop();
            return;
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
            }
        }
    }

    fn replace_opus_decode(&mut self, opus_decode: VoiceOpusDecode) {
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = opus_decode.audio_output;
            self.playback_enabled = Some(opus_decode.playback_enabled);
            self.playback_volume = Some(opus_decode.playback_volume);
        }
        self.opus_decode.replace(opus_decode.task);
    }

    fn abort_all(&mut self) {
        self.heartbeat.abort();
        self.udp_receive.abort();
        #[cfg(feature = "voice-playback")]
        if let Some(task) = self.udp_transmit.take() {
            logging::debug("voice", "stopping voice UDP transmit task");
            self.signal_udp_transmit_stop();
            drop(task);
        }
        self.opus_decode.abort();
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = None;
            self.playback_enabled = None;
            self.microphone_capture = None;
        }
    }

    async fn shutdown_all(&mut self) {
        #[cfg(feature = "voice-playback")]
        self.stop_udp_transmit_gracefully("stopping voice UDP transmit task")
            .await;
        self.abort_all();
    }

    #[allow(dead_code)]
    fn set_microphone_capture_enabled(&mut self, enabled: bool) {
        #[cfg(feature = "voice-playback")]
        {
            match (enabled, self.microphone_capture.is_some()) {
                (true, false) => {
                    match VoiceMicrophoneCapture::start(self.microphone_pcm_tx.clone()) {
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
                capture_gate.enabled && self.microphone_pcm_tx.is_some(),
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
}

impl Drop for VoiceChildTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

#[cfg(test)]
mod tests;
