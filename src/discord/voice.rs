use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    num::NonZeroU16,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use davey::{DaveSession, MediaType, ProposalsOperationType};
use futures::{SinkExt, StreamExt};
use opus::{Application as OpusApplication, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder};
use serde_json::{Value, json};
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "voice-playback")]
use std::sync::Mutex as StdMutex;
#[cfg(feature = "voice-playback")]
use std::sync::mpsc::{Receiver as StdReceiver, SyncSender, TryRecvError, sync_channel};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, Mutex as AsyncMutex, mpsc, watch},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::{
    CurrentVoiceConnectionState, DiscordState, SequencedAppEvent, SnapshotRevision,
    VoiceConnectionStatus, VoiceServerInfo, VoiceStateInfo,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, UserMarker},
    },
};
use crate::logging;

use super::{client::publish_app_event, events::AppEvent};

const VOICE_GATEWAY_VERSION: u8 = 9;
const VOICE_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_DISCOVERY_PACKET_LEN: usize = 74;
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const RTP_HEADER_MIN_LEN: usize = 12;
const RTP_VERSION: u8 = 2;
const DISCORD_VOICE_PAYLOAD_TYPE: u8 = 0x78;
const RTP_HEADER_EXTENSION_BYTES: usize = 4;
const RTP_EXTENSION_WORD_BYTES: usize = 4;
const RTP_AEAD_TAG_BYTES: usize = 16;
const RTP_AEAD_NONCE_SUFFIX_BYTES: usize = 4;
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
const VOICE_MIC_PCM_FRAME_QUEUE: usize = 32;
const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;
const VOICE_PLAYBACK_FRAME_QUEUE: usize = 256;
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
        guild_id: Id<GuildMarker>,
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
    guild_id: Id<GuildMarker>,
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
                guild_id: session.guild_id,
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
                guild_id: session.guild_id,
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
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        session_id: &str,
        endpoint: &str,
    ) -> bool {
        self.guild_id == guild_id
            && self.channel_id == channel_id
            && self.session_id == session_id
            && self.endpoint == endpoint
    }

    fn connection_ended_event(&self) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::ConnectionEnded {
            guild_id: self.guild_id,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RtpHeader {
    payload_type: u8,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    authenticated_header_len: usize,
    encrypted_extension_body_len: usize,
    payload_offset: usize,
}

enum VoiceRtpDecryptor {
    Aes256Gcm(Aes256Gcm),
    XChaCha20Poly1305(XChaCha20Poly1305),
}

#[allow(dead_code)]
enum VoiceRtpEncryptor {
    Aes256Gcm(Aes256Gcm),
    XChaCha20Poly1305(XChaCha20Poly1305),
}

struct DecryptedRtpPayload {
    media_payload: Vec<u8>,
    encrypted_extension_body_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
struct VoiceOutboundRtpState {
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
}

#[allow(dead_code)]
struct VoiceOpusEncode {
    encoder: OpusEncoder,
}

#[allow(dead_code)]
struct VoiceFakeOutboundSendState {
    rtp: VoiceOutboundRtpState,
    encryptor: VoiceRtpEncryptor,
    nonce_suffix: u32,
    allow_microphone_transmit: bool,
    self_mute: bool,
    dave_active: bool,
    speaking: bool,
    logged_block_reason: Option<VoiceFakeSendBlockReason>,
    events: Vec<VoiceFakeOutboundEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum VoiceFakeOutboundEvent {
    Speaking { speaking: bool, ssrc: u32 },
    Packet { bytes: Vec<u8> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum VoiceFakeSendOutcome {
    Noop,
    Sent,
    Blocked(VoiceFakeSendBlockReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum VoiceFakeSendBlockReason {
    DaveOutboundUnsupported,
    DaveOutboundMissingSession,
    DaveOutboundNotReady,
    DaveOutboundEncryptFailed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum VoiceDaveOutboundPayload {
    Plain(Vec<u8>),
    Encrypted(Vec<u8>),
    Blocked(VoiceFakeSendBlockReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum VoiceMediaPayload {
    Plain(Vec<u8>),
    DaveMissingUser { payload_len: usize },
    DaveNotReady { user_id: u64, payload_len: usize },
    DaveDecryptFailed { user_id: u64, message: String },
    DaveDecrypted { user_id: u64, opus: Vec<u8> },
}

impl VoiceMediaPayload {
    fn pending_reason(&self) -> &'static str {
        match self {
            Self::DaveMissingUser { .. } => "missing SSRC user mapping",
            Self::DaveNotReady { .. } => "DAVE session is not ready",
            _ => "not pending",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceSpeakingState {
    user_id: Option<u64>,
    ssrc: Option<u32>,
    speaking: Option<u64>,
}

#[derive(Default)]
struct VoiceSpeakingTracker {
    remote_deadlines: HashMap<Id<UserMarker>, Instant>,
    local_speaking: bool,
}

struct VoiceDaveState {
    user_id: u64,
    channel_id: u64,
    protocol_version: Option<NonZeroU16>,
    session: Option<DaveSession>,
    pending_transitions: HashMap<u16, u16>,
    known_user_ids: BTreeSet<u64>,
    ssrc_user_ids: HashMap<u32, u64>,
}

#[derive(Default)]
struct VoiceChildTasks {
    heartbeat: Option<JoinHandle<()>>,
    udp_receive: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    udp_transmit: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    transmit_gate: Option<watch::Sender<bool>>,
    #[cfg(feature = "voice-playback")]
    playback_enabled: Option<Arc<AtomicBool>>,
    #[cfg(feature = "voice-playback")]
    microphone_pcm_tx: Option<SyncSender<Vec<i16>>>,
    opus_decode: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    microphone_capture: Option<VoiceMicrophoneCapture>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VoicePlaybackFrame {
    ssrc: u32,
    user_id: Option<u64>,
    sequence: u16,
    timestamp: u32,
    opus: Vec<u8>,
}

struct VoiceOpusDecode {
    frames_tx: mpsc::Sender<VoicePlaybackFrame>,
    task: JoinHandle<()>,
    #[cfg(feature = "voice-playback")]
    audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    playback_enabled: Arc<AtomicBool>,
}

struct VoiceDecodedAudio {
    #[cfg(feature = "voice-playback")]
    samples_tx: Option<SyncSender<Vec<f32>>>,
}

#[cfg(feature = "voice-playback")]
struct VoiceAudioOutput {
    samples_tx: SyncSender<Vec<f32>>,
    _stream: cpal::Stream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceCaptureGate {
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoicePlaybackGate {
    enabled: bool,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophoneCapture {
    _stream: cpal::Stream,
    stats: Arc<VoiceMicrophoneCaptureStats>,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophonePcmFrames {
    frames_tx: SyncSender<Vec<i16>>,
    source_sample_rate: u32,
    source_pending: Vec<i16>,
    output_pending: Vec<i16>,
    next_source_frame: f64,
}

#[cfg(feature = "voice-playback")]
struct VoiceMicrophoneCaptureStats {
    chunks: AtomicU64,
    frames: AtomicU64,
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
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field("session_id", &"<redacted>")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl VoiceDaveState {
    fn new(session: &VoiceGatewaySession) -> Self {
        let user_id = session.user_id.get();
        let mut known_user_ids = BTreeSet::new();
        known_user_ids.insert(user_id);
        Self {
            user_id,
            channel_id: session.channel_id.get(),
            protocol_version: None,
            session: None,
            pending_transitions: HashMap::new(),
            known_user_ids,
            ssrc_user_ids: HashMap::new(),
        }
    }

    async fn handle_json_op(
        &mut self,
        writer: &VoiceWriter,
        opcode: u8,
        value: &Value,
    ) -> Result<(), String> {
        match opcode {
            VOICE_OP_SPEAKING => {
                self.handle_speaking_op(value);
            }
            VOICE_OP_CLIENTS_CONNECT => {
                for user_id in voice_user_ids(value) {
                    self.known_user_ids.insert(user_id);
                }
                logging::debug(
                    "voice",
                    format!(
                        "voice clients connected: known_users={}",
                        self.known_user_ids.len()
                    ),
                );
            }
            VOICE_OP_CLIENT_DISCONNECT => {
                if let Some(user_id) = voice_user_id(value) {
                    self.known_user_ids.remove(&user_id);
                    self.ssrc_user_ids.retain(|_, mapped_user_id| *mapped_user_id != user_id);
                    logging::debug(
                        "voice",
                        format!(
                            "voice client disconnected: user_id={} known_users={} known_ssrcs={}",
                            user_id,
                            self.known_user_ids.len(),
                            self.ssrc_user_ids.len()
                        ),
                    );
                }
            }
            VOICE_OP_MEDIA_SINK_WANTS => {
                logging::debug(
                    "voice",
                    format!(
                        "voice media sink wants received: field_count={}",
                        voice_data_field_count(value)
                    ),
                );
            }
            VOICE_OP_CLIENT_FLAGS => {
                logging::debug(
                    "voice",
                    format!(
                        "voice client flags received: user_id={:?} flags={:?}",
                        voice_user_id(value),
                        voice_data_u64(value, "flags")
                    ),
                );
            }
            VOICE_OP_CLIENT_PLATFORM => {
                logging::debug(
                    "voice",
                    format!(
                        "voice client platform received: user_id={:?} platform={:?}",
                        voice_user_id(value),
                        voice_data_string(value, "platform")
                    ),
                );
            }
            VOICE_OP_DAVE_PREPARE_TRANSITION => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE transition missing data".to_owned())?;
                let transition_id = json_u16(data, "transition_id")?;
                let protocol_version = json_u16(data, "protocol_version")
                    .or_else(|_| json_u16(data, "dave_protocol_version"))?;
                self.pending_transitions
                    .insert(transition_id, protocol_version);
                logging::debug(
                    "voice",
                    format!(
                        "DAVE prepare transition received: transition_id={} protocol_version={}",
                        transition_id, protocol_version
                    ),
                );
                if protocol_version == 0 {
                    if let Some(session) = self.session.as_mut() {
                        session.set_passthrough_mode(true, Some(120));
                    }
                }
                if transition_id == 0 {
                    self.execute_transition(transition_id)?;
                } else {
                    send_dave_transition_ready(writer, transition_id).await?;
                }
            }
            VOICE_OP_DAVE_EXECUTE_TRANSITION => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE execute transition missing data".to_owned())?;
                let transition_id = json_u16(data, "transition_id")?;
                self.execute_transition(transition_id)?;
            }
            VOICE_OP_DAVE_PREPARE_EPOCH => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE prepare epoch missing data".to_owned())?;
                let epoch = json_u64(data, "epoch")?;
                logging::debug("voice", format!("DAVE prepare epoch received: epoch={epoch}"));
                if epoch == 1 {
                    let protocol_version = json_u16(data, "protocol_version")
                        .or_else(|_| json_u16(data, "dave_protocol_version"))?;
                    self.reinit(protocol_version)?;
                    self.send_key_package(writer).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_speaking_op(&mut self, value: &Value) -> VoiceSpeakingState {
        let speaking = parse_voice_speaking(value);
        self.record_speaking_state(speaking);
        logging::debug(
            "voice",
            format!(
                "voice speaking received: user_id={:?} ssrc={:?} speaking={:?} known_ssrcs={}",
                speaking.user_id,
                speaking.ssrc,
                speaking.speaking,
                self.ssrc_user_ids.len()
            ),
        );
        speaking
    }

    async fn handle_binary_frame(
        &mut self,
        writer: &VoiceWriter,
        frame: VoiceBinaryFrame<'_>,
    ) -> Result<(), String> {
        match frame.opcode {
            VOICE_OP_DAVE_MLS_EXTERNAL_SENDER => {
                let session = self.session_mut()?;
                session
                    .set_external_sender(frame.payload)
                    .map_err(|error| format!("DAVE external sender failed: {error}"))?;
                logging::debug("voice", "DAVE external sender processed");
                self.send_key_package(writer).await?;
            }
            VOICE_OP_DAVE_MLS_PROPOSALS => {
                let Some((&operation, proposals)) = frame.payload.split_first() else {
                    return Err("DAVE proposals payload is empty".to_owned());
                };
                let operation_type = match operation {
                    0 => ProposalsOperationType::APPEND,
                    1 => ProposalsOperationType::REVOKE,
                    other => {
                        return Err(format!("DAVE proposals operation is unsupported: {other}"));
                    }
                };
                let known_user_ids = self.known_user_ids.iter().copied().collect::<Vec<_>>();
                let result = self
                    .session_mut()?
                    .process_proposals(operation_type, proposals, Some(&known_user_ids))
                    .map_err(|error| format!("DAVE proposals processing failed: {error}"))?;
                if let Some(commit_welcome) = result {
                    send_dave_commit_welcome(writer, commit_welcome).await?;
                }
                logging::debug("voice", "DAVE proposals processed");
            }
            VOICE_OP_DAVE_MLS_ANNOUNCE_COMMIT_TRANSITION => {
                let Some((transition_id, commit)) = split_transition_payload(frame.payload) else {
                    return Err("DAVE commit transition payload is too short".to_owned());
                };
                match self.session_mut()?.process_commit(commit) {
                    Ok(()) => {
                        logging::debug(
                            "voice",
                            format!("DAVE commit processed: transition_id={transition_id}"),
                        );
                        if transition_id != 0 {
                            self.pending_transitions.insert(
                                transition_id,
                                self.protocol_version.map(NonZeroU16::get).unwrap_or_default(),
                            );
                            send_dave_transition_ready(writer, transition_id).await?;
                        }
                    }
                    Err(error) => {
                        logging::error("voice", format!("DAVE commit failed: {error}"));
                        send_dave_invalid_commit_welcome(writer, transition_id).await?;
                        self.reinit_current()?;
                        self.send_key_package(writer).await?;
                    }
                }
            }
            VOICE_OP_DAVE_MLS_WELCOME => {
                let Some((transition_id, welcome)) = split_transition_payload(frame.payload) else {
                    return Err("DAVE welcome payload is too short".to_owned());
                };
                match self.session_mut()?.process_welcome(welcome) {
                    Ok(()) => {
                        logging::debug(
                            "voice",
                            format!("DAVE welcome processed: transition_id={transition_id}"),
                        );
                        if transition_id != 0 {
                            self.pending_transitions.insert(
                                transition_id,
                                self.protocol_version.map(NonZeroU16::get).unwrap_or_default(),
                            );
                            send_dave_transition_ready(writer, transition_id).await?;
                        }
                    }
                    Err(error) => {
                        logging::error("voice", format!("DAVE welcome failed: {error}"));
                        send_dave_invalid_commit_welcome(writer, transition_id).await?;
                        self.reinit_current()?;
                        self.send_key_package(writer).await?;
                    }
                }
            }
            other => logging::debug("voice", format!("unhandled voice binary op={other}")),
        }
        Ok(())
    }

    fn reinit(&mut self, protocol_version: u16) -> Result<(), String> {
        let Some(protocol_version) = NonZeroU16::new(protocol_version) else {
            self.protocol_version = None;
            if let Some(session) = self.session.as_mut() {
                session
                    .reset()
                    .map_err(|error| format!("DAVE reset failed: {error}"))?;
                session.set_passthrough_mode(true, Some(10));
            }
            logging::debug("voice", "DAVE disabled by protocol transition");
            return Ok(());
        };
        if let Some(session) = self.session.as_mut() {
            session
                .reinit(protocol_version, self.user_id, self.channel_id, None)
                .map_err(|error| format!("DAVE session reinit failed: {error}"))?;
        } else {
            self.session = Some(
                DaveSession::new(protocol_version, self.user_id, self.channel_id, None)
                    .map_err(|error| format!("DAVE session init failed: {error}"))?,
            );
        }
        self.protocol_version = Some(protocol_version);
        logging::debug(
            "voice",
            format!("DAVE session initialized: protocol_version={protocol_version}"),
        );
        Ok(())
    }

    fn reinit_current(&mut self) -> Result<(), String> {
        let protocol_version = self
            .protocol_version
            .map(NonZeroU16::get)
            .ok_or_else(|| "DAVE protocol version is not active".to_owned())?;
        self.reinit(protocol_version)
    }

    fn execute_transition(&mut self, transition_id: u16) -> Result<(), String> {
        let Some(protocol_version) = self.pending_transitions.remove(&transition_id) else {
            logging::debug(
                "voice",
                format!("DAVE execute transition ignored: transition_id={transition_id}"),
            );
            return Ok(());
        };
        if protocol_version == 0 {
            if let Some(session) = self.session.as_mut() {
                session.set_passthrough_mode(true, Some(10));
            }
            self.protocol_version = None;
        } else {
            self.protocol_version = NonZeroU16::new(protocol_version);
            if let Some(session) = self.session.as_mut() {
                session.set_passthrough_mode(true, Some(10));
            }
        }
        logging::debug(
            "voice",
            format!(
                "DAVE transition executed: transition_id={} protocol_version={}",
                transition_id, protocol_version
            ),
        );
        Ok(())
    }

    async fn send_key_package(&mut self, writer: &VoiceWriter) -> Result<(), String> {
        let key_package = self
            .session_mut()?
            .create_key_package()
            .map_err(|error| format!("DAVE key package creation failed: {error}"))?;
        send_voice_binary(writer, VOICE_OP_DAVE_MLS_KEY_PACKAGE, key_package).await?;
        logging::debug("voice", "DAVE key package sent");
        Ok(())
    }

    fn session_mut(&mut self) -> Result<&mut DaveSession, String> {
        self.session
            .as_mut()
            .ok_or_else(|| "DAVE session is not initialized".to_owned())
    }

    fn unwrap_media_payload_for_ssrc(&mut self, ssrc: u32, payload: &[u8]) -> VoiceMediaPayload {
        if !self.dave_media_active() || !looks_like_dave_media_frame(payload) {
            return VoiceMediaPayload::Plain(payload.to_vec());
        }
        let Some(user_id) = self.ssrc_user_ids.get(&ssrc).copied() else {
            return VoiceMediaPayload::DaveMissingUser {
                payload_len: payload.len(),
            };
        };
        let Some(session) = self.session.as_mut() else {
            return VoiceMediaPayload::DaveNotReady {
                user_id,
                payload_len: payload.len(),
            };
        };
        if !session.is_ready() {
            return VoiceMediaPayload::DaveNotReady {
                user_id,
                payload_len: payload.len(),
            };
        }
        match session.decrypt(user_id, MediaType::AUDIO, payload) {
            Ok(opus) => VoiceMediaPayload::DaveDecrypted { user_id, opus },
            Err(error) => VoiceMediaPayload::DaveDecryptFailed {
                user_id,
                message: error.to_string(),
            },
        }
    }

    fn user_id_for_ssrc(&self, ssrc: u32) -> Option<Id<UserMarker>> {
        self.ssrc_user_ids
            .get(&ssrc)
            .copied()
            .and_then(Id::<UserMarker>::new_checked)
    }

    #[allow(dead_code)]
    fn prepare_outbound_opus(&mut self, opus: &[u8]) -> VoiceDaveOutboundPayload {
        if self.protocol_version.is_none() {
            return VoiceDaveOutboundPayload::Plain(opus.to_vec());
        }
        let Some(session) = self.session.as_mut() else {
            return VoiceDaveOutboundPayload::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundMissingSession,
            );
        };
        if !session.is_ready() {
            return VoiceDaveOutboundPayload::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundNotReady,
            );
        }
        match session.encrypt_opus(opus) {
            Ok(encrypted) => VoiceDaveOutboundPayload::Encrypted(encrypted.into_owned()),
            Err(_) => VoiceDaveOutboundPayload::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundEncryptFailed,
            ),
        }
    }

    fn dave_media_active(&self) -> bool {
        self.protocol_version.is_some() && self.session.is_some()
    }

    fn record_speaking_state(&mut self, speaking: VoiceSpeakingState) {
        if let (Some(ssrc), Some(user_id)) = (speaking.ssrc, speaking.user_id) {
            self.ssrc_user_ids.insert(ssrc, user_id);
            self.known_user_ids.insert(user_id);
        }
    }
}

impl VoiceChildTasks {
    fn replace_heartbeat(&mut self, task: JoinHandle<()>) {
        if let Some(task) = self.heartbeat.take() {
            logging::debug("voice", "aborting previous voice heartbeat task");
            task.abort();
        }
        self.heartbeat = Some(task);
    }

    fn replace_udp_receive(&mut self, task: JoinHandle<()>) {
        if let Some(task) = self.udp_receive.take() {
            logging::debug("voice", "aborting previous voice UDP receive task");
            task.abort();
        }
        self.udp_receive = Some(task);
    }

    #[cfg(feature = "voice-playback")]
    fn replace_udp_transmit(
        &mut self,
        task: JoinHandle<()>,
        gate: watch::Sender<bool>,
        microphone_pcm_tx: SyncSender<Vec<i16>>,
    ) {
        if let Some(task) = self.udp_transmit.take() {
            logging::debug("voice", "stopping previous voice UDP transmit task");
            self.signal_udp_transmit_stop();
            drop(task);
        }
        self.udp_transmit = Some(task);
        self.transmit_gate = Some(gate);
        self.microphone_pcm_tx = Some(microphone_pcm_tx);
    }

    #[cfg(feature = "voice-playback")]
    fn signal_udp_transmit_stop(&mut self) {
        if let Some(gate) = self.transmit_gate.as_ref() {
            let _ = gate.send(false);
        }
        self.microphone_capture = None;
        self.microphone_pcm_tx = None;
        self.transmit_gate = None;
    }

    fn replace_opus_decode(&mut self, opus_decode: VoiceOpusDecode) {
        if let Some(task) = self.opus_decode.take() {
            logging::debug("voice", "aborting previous voice Opus decode task");
            task.abort();
        }
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = opus_decode.audio_output;
            self.playback_enabled = Some(opus_decode.playback_enabled);
        }
        self.opus_decode = Some(opus_decode.task);
    }

    fn abort_all(&mut self) {
        if let Some(task) = self.heartbeat.take() {
            logging::debug("voice", "aborting voice heartbeat task");
            task.abort();
        }
        if let Some(task) = self.udp_receive.take() {
            logging::debug("voice", "aborting voice UDP receive task");
            task.abort();
        }
        #[cfg(feature = "voice-playback")]
        if let Some(task) = self.udp_transmit.take() {
            logging::debug("voice", "stopping voice UDP transmit task");
            self.signal_udp_transmit_stop();
            drop(task);
        }
        if let Some(task) = self.opus_decode.take() {
            logging::debug("voice", "aborting voice Opus decode task");
            task.abort();
        }
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = None;
            self.playback_enabled = None;
            self.microphone_capture = None;
        }
    }

    #[allow(dead_code)]
    fn set_microphone_capture_enabled(&mut self, enabled: bool) {
        #[cfg(feature = "voice-playback")]
        {
            match (enabled, self.microphone_capture.is_some()) {
                (true, false) => match VoiceMicrophoneCapture::start(self.microphone_pcm_tx.clone()) {
                    Ok(capture) => self.microphone_capture = Some(capture),
                    Err(error) => logging::error(
                        "voice",
                        format!("voice microphone capture unavailable: {error}"),
                    ),
                },
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

    fn set_voice_transmit_gate(&mut self, enabled: bool) {
        #[cfg(feature = "voice-playback")]
        {
        if let Some(gate) = self.transmit_gate.as_ref() {
            let _ = gate.send(enabled);
        }
            self.set_microphone_capture_enabled(enabled && self.microphone_pcm_tx.is_some());
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = enabled;
        }
    }

    fn set_voice_playback_enabled(&mut self, enabled: bool) {
        #[cfg(feature = "voice-playback")]
        if let Some(playback_enabled) = self.playback_enabled.as_ref() {
            playback_enabled.store(enabled, Ordering::Relaxed);
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = enabled;
        }
    }
}

impl Drop for VoiceChildTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

impl VoiceOpusDecode {
    #[cfg(not(feature = "voice-playback"))]
    fn start(playback_enabled: bool) -> Self {
        let _ = playback_enabled;
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let task = tokio::spawn(run_voice_playback_decode(
            frames_rx,
            VoiceDecodedAudio::decode_only(),
        ));
        logging::debug(
            "voice",
            "voice Opus decode worker started without audio output device",
        );
        Self { frames_tx, task }
    }

    #[cfg(feature = "voice-playback")]
    fn start(playback_enabled: bool) -> Self {
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let playback_enabled = Arc::new(AtomicBool::new(playback_enabled));
        match VoiceAudioOutput::start(Arc::clone(&playback_enabled)) {
            Ok(audio_output) => {
                let decoded_audio = VoiceDecodedAudio::output(audio_output.samples_tx.clone());
                let task = tokio::spawn(run_voice_playback_decode(frames_rx, decoded_audio));
                logging::debug("voice", "voice Opus playback worker started with audio output");
                Self {
                    frames_tx,
                    task,
                    audio_output: Some(audio_output),
                    playback_enabled,
                }
            }
            Err(error) => {
                logging::error(
                    "voice",
                    format!("voice audio output unavailable, falling back to decode-only: {error}"),
                );
                let task = tokio::spawn(run_voice_playback_decode(
                    frames_rx,
                    VoiceDecodedAudio::decode_only(),
                ));
                Self {
                    frames_tx,
                    task,
                    audio_output: None,
                    playback_enabled,
                }
            }
        }
    }
}

impl VoiceDecodedAudio {
    fn decode_only() -> Self {
        Self {
            #[cfg(feature = "voice-playback")]
            samples_tx: None,
        }
    }

    #[cfg(feature = "voice-playback")]
    fn output(samples_tx: SyncSender<Vec<f32>>) -> Self {
        Self {
            samples_tx: Some(samples_tx),
        }
    }

    fn try_send(&self, samples: Vec<f32>) {
        #[cfg(feature = "voice-playback")]
        if let Some(samples_tx) = self.samples_tx.as_ref() {
            let _ = samples_tx.try_send(samples);
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = samples;
        }
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceAudioOutput {
    fn start(playback_enabled: Arc<AtomicBool>) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(playback_enabled);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    fn start_with_cpal(playback_enabled: Arc<AtomicBool>) -> Result<Self, String> {

        let (samples_tx, samples_rx) = sync_channel(VOICE_AUDIO_OUTPUT_QUEUE);
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device is available".to_owned())?;
        let supported_config = select_voice_output_config(&device)?;
        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();
        let stream = build_voice_output_stream(
            &device,
            &stream_config,
            sample_format,
            samples_rx,
            playback_enabled,
        )?;
        stream
            .play()
            .map_err(|error| format!("voice audio output stream start failed: {error}"))?;
        logging::debug(
            "voice",
            format!(
                "voice audio output stream started: sample_rate={} channels={} format={:?}",
                stream_config.sample_rate, stream_config.channels, sample_format
            ),
        );
        Ok(Self {
            samples_tx,
            _stream: stream,
        })
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneCapture {
    fn start(samples_tx: Option<SyncSender<Vec<i16>>>) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(samples_tx);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    fn start_with_cpal(samples_tx: Option<SyncSender<Vec<i16>>>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no default microphone input device is available".to_owned())?;
        let supported_config = device
            .default_input_config()
            .map_err(|error| format!("voice microphone default input config failed: {error}"))?;
        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();
        let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
        let stream = build_voice_input_stream(
            &device,
            &stream_config,
            sample_format,
            Arc::clone(&stats),
            samples_tx,
        )?;
        stream
            .play()
            .map_err(|error| format!("voice microphone input stream start failed: {error}"))?;
        logging::debug(
            "voice",
            format!(
                "voice microphone capture started: sample_rate={} channels={} format={:?}",
                stream_config.sample_rate, stream_config.channels, sample_format
            ),
        );
        Ok(Self {
            _stream: stream,
            stats,
        })
    }
}

#[cfg(feature = "voice-playback")]
impl Default for VoiceMicrophoneCaptureStats {
    fn default() -> Self {
        Self {
            chunks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophonePcmFrames {
    fn new(frames_tx: SyncSender<Vec<i16>>, source_sample_rate: u32) -> Self {
        Self {
            frames_tx,
            source_sample_rate,
            source_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            next_source_frame: 0.0,
        }
    }

    fn push_stereo_samples(&mut self, samples: &[i16]) {
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
            self.flush_output_frames();
            return;
        }

        self.source_pending.extend_from_slice(samples);
        self.resample_pending_source();
        self.flush_output_frames();
    }

    fn resample_pending_source(&mut self) {
        let source_frames = self.source_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        if source_frames < 2 {
            return;
        }

        let source_step = f64::from(self.source_sample_rate) / f64::from(DISCORD_VOICE_SAMPLE_RATE);
        while self.next_source_frame + 1.0 < source_frames as f64 {
            let frame_index = self.next_source_frame.floor() as usize;
            let fraction = self.next_source_frame - frame_index as f64;
            let left = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE],
                fraction,
            );
            let right = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE + 1],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE + 1],
                fraction,
            );
            self.output_pending.push(left);
            self.output_pending.push(right);
            self.next_source_frame += source_step;
        }

        let consumed_frames = self.next_source_frame.floor() as usize;
        if consumed_frames > 0 {
            self.source_pending
                .drain(..consumed_frames * DISCORD_VOICE_CHANNELS_USIZE);
            self.next_source_frame -= consumed_frames as f64;
        }
    }

    fn flush_output_frames(&mut self) {
        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let frame = self
                .output_pending
                .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                .collect::<Vec<_>>();
            let _ = self.frames_tx.try_send(frame);
        }
    }
}

#[cfg(feature = "voice-playback")]
fn interpolate_i16(current: i16, next: i16, fraction: f64) -> i16 {
    let value = f64::from(current) + (f64::from(next) - f64::from(current)) * fraction;
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(feature = "voice-playback")]
impl Drop for VoiceMicrophoneCapture {
    fn drop(&mut self) {
        logging::debug(
            "voice",
            format!(
                "voice microphone capture stopped: chunks={} frames={}",
                self.stats.chunks.load(Ordering::Relaxed),
                self.stats.frames.load(Ordering::Relaxed),
            ),
        );
    }
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<SyncSender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_voice_input_stream_f32(device, config, stats, samples_tx),
        cpal::SampleFormat::U8 => build_voice_input_stream_u8(device, config, stats, samples_tx),
        cpal::SampleFormat::I16 => build_voice_input_stream_i16(device, config, stats, samples_tx),
        cpal::SampleFormat::U16 => build_voice_input_stream_u16(device, config, stats, samples_tx),
        other => Err(format!(
            "unsupported voice microphone input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<SyncSender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[f32], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_f32_to_stereo_i16(input, channels);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<SyncSender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[i16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_i16_to_stereo_i16(input, channels);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<SyncSender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[u16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u16_to_stereo_i16(input, channels);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_u8(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<SyncSender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[u8], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u8_to_stereo_i16(input, channels);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn voice_input_f32_to_stereo_i16(input: &[f32], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
    })
}

#[cfg(feature = "voice-playback")]
fn voice_input_i16_to_stereo_i16(input: &[i16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| sample)
}

#[cfg(feature = "voice-playback")]
fn voice_input_u16_to_stereo_i16(input: &[u16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        let shifted = i32::from(sample) - 32768;
        shifted.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    })
}

#[cfg(feature = "voice-playback")]
fn voice_input_u8_to_stereo_i16(input: &[u8], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        (i16::from(sample) - 128) << 8
    })
}

#[cfg(feature = "voice-playback")]
fn voice_input_to_stereo_i16<T>(
    input: &[T],
    channels: usize,
    mut convert: impl FnMut(T) -> i16,
) -> Vec<i16>
where
    T: Copy,
{
    if channels == 0 {
        return Vec::new();
    }
    let frames = input.len() / channels;
    let mut stereo = Vec::with_capacity(frames * usize::from(DISCORD_VOICE_CHANNELS));
    for frame in input.chunks_exact(channels) {
        let left = convert(frame[0]);
        let right = if channels == 1 { left } else { convert(frame[1]) };
        stereo.push(left);
        stereo.push(right);
    }
    stereo
}

#[cfg(feature = "voice-playback")]
fn record_voice_input_chunk(
    sample_count: usize,
    channels: usize,
    stats: &VoiceMicrophoneCaptureStats,
) {
    let frames = sample_count / channels.max(1);
    stats.chunks.fetch_add(1, Ordering::Relaxed);
    stats
        .frames
        .fetch_add(u64::try_from(frames).unwrap_or(u64::MAX), Ordering::Relaxed);
}

#[cfg(feature = "voice-playback")]
fn log_voice_input_stream_error(error: cpal::StreamError) {
    logging::error("voice", format!("voice microphone input stream failed: {error}"));
}

#[cfg(all(feature = "voice-playback", target_os = "linux"))]
fn log_captured_alsa_errors(
    alsa_error_output: &Option<std::rc::Rc<std::cell::RefCell<alsa::Output>>>,
) {
    let Some(output) = alsa_error_output else {
        return;
    };
    let message = output.borrow().buffer_string(|bytes| {
        String::from_utf8_lossy(bytes).replace('\0', "")
    });
    let message = message.trim();
    if message.is_empty() {
        return;
    }
    logging::error("voice", format!("captured ALSA diagnostics: {message}"));
}

#[cfg(feature = "voice-playback")]
struct VoiceAudioBuffer {
    samples_rx: StdReceiver<Vec<f32>>,
    current: Vec<f32>,
    offset: usize,
}

#[cfg(feature = "voice-playback")]
impl VoiceAudioBuffer {
    fn new(samples_rx: StdReceiver<Vec<f32>>) -> Self {
        Self {
            samples_rx,
            current: Vec::new(),
            offset: 0,
        }
    }

    fn next_stereo_frame(&mut self) -> Option<[f32; 2]> {
        loop {
            if self.offset + 1 < self.current.len() {
                let frame = [self.current[self.offset], self.current[self.offset + 1]];
                self.offset += usize::from(DISCORD_VOICE_CHANNELS);
                return Some(frame);
            }
            match self.samples_rx.try_recv() {
                Ok(samples) => {
                    self.current = samples;
                    self.offset = 0;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }

    fn clear_pending(&mut self) {
        self.current.clear();
        self.offset = 0;
        while self.samples_rx.try_recv().is_ok() {}
    }
}

#[cfg(feature = "voice-playback")]
fn select_voice_output_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, String> {
    let sample_rate = DISCORD_VOICE_SAMPLE_RATE;
    let mut configs = device
        .supported_output_configs()
        .map_err(|error| format!("voice audio output config query failed: {error}"))?;
    if let Some(config) = configs.find(|config| {
        config.channels() == DISCORD_VOICE_CHANNELS
            && config.min_sample_rate() <= sample_rate
            && config.max_sample_rate() >= sample_rate
    }) {
        return Ok(config.with_sample_rate(sample_rate));
    }
    device
        .default_output_config()
        .map_err(|error| format!("voice default audio output config failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => {
            build_voice_output_stream_f32(device, config, samples_rx, playback_enabled)
        }
        cpal::SampleFormat::U8 => {
            build_voice_output_stream_u8(device, config, samples_rx, playback_enabled)
        }
        cpal::SampleFormat::I16 => {
            build_voice_output_stream_i16(device, config, samples_rx, playback_enabled)
        }
        cpal::SampleFormat::U16 => {
            build_voice_output_stream_u16(device, config, samples_rx, playback_enabled)
        }
        other => Err(format!("unsupported voice audio output sample format: {other:?}")),
    }
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [f32], _| {
                fill_voice_output_f32(output, channels, &mut buffer, &playback_enabled)
            },
            log_voice_output_stream_error,
            None,
        )
        .map_err(|error| format!("voice f32 audio output stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream_u8(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [u8], _| {
                fill_voice_output_u8(output, channels, &mut buffer, &playback_enabled)
            },
            log_voice_output_stream_error,
            None,
        )
        .map_err(|error| format!("voice u8 audio output stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [i16], _| {
                fill_voice_output_i16(output, channels, &mut buffer, &playback_enabled)
            },
            log_voice_output_stream_error,
            None,
        )
        .map_err(|error| format!("voice i16 audio output stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [u16], _| {
                fill_voice_output_u16(output, channels, &mut buffer, &playback_enabled)
            },
            log_voice_output_stream_error,
            None,
        )
        .map_err(|error| format!("voice u16 audio output stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_f32(
    output: &mut [f32],
    channels: usize,
    buffer: &mut VoiceAudioBuffer,
    playback_enabled: &AtomicBool,
) {
    if !playback_enabled.load(Ordering::Relaxed) {
        buffer.clear_pending();
        fill_voice_output_silence(output, channels, clamp_voice_sample);
        return;
    }
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, clamp_voice_sample);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_u8(
    output: &mut [u8],
    channels: usize,
    buffer: &mut VoiceAudioBuffer,
    playback_enabled: &AtomicBool,
) {
    if !playback_enabled.load(Ordering::Relaxed) {
        buffer.clear_pending();
        fill_voice_output_silence(output, channels, voice_sample_to_u8);
        return;
    }
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_u8);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_i16(
    output: &mut [i16],
    channels: usize,
    buffer: &mut VoiceAudioBuffer,
    playback_enabled: &AtomicBool,
) {
    if !playback_enabled.load(Ordering::Relaxed) {
        buffer.clear_pending();
        fill_voice_output_silence(output, channels, voice_sample_to_i16);
        return;
    }
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_i16);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_u16(
    output: &mut [u16],
    channels: usize,
    buffer: &mut VoiceAudioBuffer,
    playback_enabled: &AtomicBool,
) {
    if !playback_enabled.load(Ordering::Relaxed) {
        buffer.clear_pending();
        fill_voice_output_silence(output, channels, voice_sample_to_u16);
        return;
    }
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_u16);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_silence<T>(output: &mut [T], channels: usize, convert: fn(f32) -> T)
where
    T: Default + Copy,
{
    for frame in output.chunks_mut(channels) {
        write_voice_output_frame(frame, 0.0, 0.0, convert);
    }
}

#[cfg(feature = "voice-playback")]
fn write_voice_output_frame<T>(
    output: &mut [T],
    left: f32,
    right: f32,
    convert: fn(f32) -> T,
) where
    T: Default + Copy,
{
    match output {
        [] => {}
        [mono] => *mono = convert((left + right) * 0.5),
        [first, second, rest @ ..] => {
            *first = convert(left);
            *second = convert(right);
            for sample in rest {
                *sample = T::default();
            }
        }
    }
}

#[cfg(feature = "voice-playback")]
fn clamp_voice_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

#[cfg(feature = "voice-playback")]
fn voice_sample_to_u8(sample: f32) -> u8 {
    ((clamp_voice_sample(sample) + 1.0) * 0.5 * f32::from(u8::MAX)).round() as u8
}

#[cfg(feature = "voice-playback")]
fn voice_sample_to_i16(sample: f32) -> i16 {
    (clamp_voice_sample(sample) * f32::from(i16::MAX)).round() as i16
}

#[cfg(feature = "voice-playback")]
fn voice_sample_to_u16(sample: f32) -> u16 {
    ((clamp_voice_sample(sample) + 1.0) * 0.5 * f32::from(u16::MAX)).round() as u16
}

#[cfg(feature = "voice-playback")]
fn log_voice_output_stream_error(error: cpal::StreamError) {
    logging::error("voice", format!("voice audio output stream failed: {error}"));
}

#[derive(Debug, Eq, PartialEq)]
enum VoiceRuntimeAction {
    Connect(VoiceGatewaySession),
    Close,
}

#[derive(Default)]
struct VoiceRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    requested: Option<CurrentVoiceConnectionState>,
    current_voice: Option<ObservedSelfVoiceState>,
    server: Option<VoiceServerInfo>,
    active: Option<VoiceGatewaySession>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedSelfVoiceState {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

impl VoiceRuntimeState {
    fn apply(&mut self, event: VoiceRuntimeEvent) -> Option<VoiceRuntimeAction> {
        match event {
            VoiceRuntimeEvent::Requested(requested) => {
                if let Some(next) = requested
                    && self.requested.is_some_and(|current| {
                        current.guild_id != next.guild_id || current.channel_id != next.channel_id
                    })
                {
                    self.server = None;
                }
                self.requested = requested;
                if self.requested.is_none() {
                    self.current_voice = None;
                    self.server = None;
                    return self.close_active();
                }
            }
            VoiceRuntimeEvent::CurrentUserReady(user_id) => {
                self.current_user_id = user_id;
            }
            VoiceRuntimeEvent::VoiceState(state) => {
                if let Some(action) = self.record_voice_state(state) {
                    return Some(action);
                }
            }
            VoiceRuntimeEvent::VoiceServer(server) => {
                if server.endpoint.is_none() {
                    self.server = None;
                    return self.close_active();
                }
                self.server = Some(server);
            }
            VoiceRuntimeEvent::ConnectionEnded {
                guild_id,
                channel_id,
                session_id,
                endpoint,
            } => {
                if self.active.as_ref().is_some_and(|active| {
                    active.matches_connection_end(guild_id, channel_id, &session_id, &endpoint)
                }) {
                    self.active = None;
                    return self.connect_if_ready();
                }
                return None;
            }
            VoiceRuntimeEvent::Shutdown => return self.close_active(),
        }

        self.connect_if_ready()
    }

    fn record_voice_state(&mut self, state: VoiceStateInfo) -> Option<VoiceRuntimeAction> {
        if self.current_user_id != Some(state.user_id) {
            return None;
        }
        let requested = self.requested?;
        if state.guild_id != requested.guild_id {
            return None;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            self.server = None;
            return self.close_active();
        };
        let session_id = state
            .session_id
            .filter(|session_id| !session_id.is_empty())?;
        self.current_voice = Some(ObservedSelfVoiceState {
            guild_id: state.guild_id,
            channel_id,
            session_id,
        });
        None
    }

    fn connect_if_ready(&mut self) -> Option<VoiceRuntimeAction> {
        let requested = self.requested?;
        let voice = self.current_voice.as_ref()?;
        if requested.guild_id != voice.guild_id || requested.channel_id != voice.channel_id {
            return self.close_active();
        }
        let server = self.server.as_ref()?;
        if server.guild_id != requested.guild_id {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }
        let session = VoiceGatewaySession {
            guild_id: requested.guild_id,
            channel_id: requested.channel_id,
            user_id: self.current_user_id?,
            session_id: voice.session_id.clone(),
            endpoint,
            token: server.token.clone(),
        };
        if self.active.as_ref() == Some(&session) {
            return None;
        }
        self.active = Some(session.clone());
        Some(VoiceRuntimeAction::Connect(session))
    }

    fn close_active(&mut self) -> Option<VoiceRuntimeAction> {
        self.active.take().map(|_| VoiceRuntimeAction::Close)
    }

    fn capture_gate(&self) -> Option<VoiceCaptureGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.guild_id != requested.guild_id || active.channel_id != requested.channel_id {
            return None;
        }
        Some(VoiceCaptureGate {
            enabled: requested.allow_microphone_transmit && !requested.self_mute,
        })
    }

    fn playback_gate(&self) -> Option<VoicePlaybackGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.guild_id != requested.guild_id || active.channel_id != requested.channel_id {
            return None;
        }
        Some(VoicePlaybackGate {
            enabled: !requested.self_deaf,
        })
    }
}

pub(crate) fn forward_app_event(
    sender: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    event: &AppEvent,
) {
    let runtime_event = match event {
        AppEvent::Ready { user_id, .. } => VoiceRuntimeEvent::CurrentUserReady(*user_id),
        AppEvent::VoiceStateUpdate { state } => VoiceRuntimeEvent::VoiceState(state.clone()),
        AppEvent::VoiceServerUpdate { server } => VoiceRuntimeEvent::VoiceServer(server.clone()),
        _ => return,
    };
    let _ = sender.send(runtime_event);
}

pub(crate) async fn run_voice_runtime(
    mut events: mpsc::UnboundedReceiver<VoiceRuntimeEvent>,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
) {
    let mut state = VoiceRuntimeState::default();
    let mut connection_task: Option<JoinHandle<()>> = None;
    let mut capture_gate_tx: Option<mpsc::UnboundedSender<VoiceCaptureGate>> = None;
    let mut playback_gate_tx: Option<mpsc::UnboundedSender<VoicePlaybackGate>> = None;

    while let Some(event) = events.recv().await {
        let shutdown = matches!(event, VoiceRuntimeEvent::Shutdown);
        if let Some(action) = state.apply(event) {
            match action {
                VoiceRuntimeAction::Connect(session) => {
                    if let Some(task) = connection_task.take() {
                        logging::debug(
                            "voice",
                            "aborting previous voice connection task before reconnect",
                        );
                        task.abort();
                    }
                    let (next_capture_gate_tx, capture_gate_rx) = mpsc::unbounded_channel();
                    let (next_playback_gate_tx, playback_gate_rx) = mpsc::unbounded_channel();
                    capture_gate_tx = Some(next_capture_gate_tx);
                    playback_gate_tx = Some(next_playback_gate_tx);
                    let initial_capture_gate = state
                        .capture_gate()
                        .unwrap_or(VoiceCaptureGate { enabled: false });
                    let initial_playback_gate = state
                        .playback_gate()
                        .unwrap_or(VoicePlaybackGate { enabled: true });
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(
                        session,
                        events_tx.clone(),
                        status_publisher.clone(),
                        initial_capture_gate,
                        capture_gate_rx,
                        initial_playback_gate,
                        playback_gate_rx,
                    )));
                }
                VoiceRuntimeAction::Close => {
                    if let Some(task) = connection_task.take() {
                        logging::debug("voice", "aborting active voice connection task");
                        task.abort();
                    }
                    capture_gate_tx = None;
                    playback_gate_tx = None;
                }
            }
        }
        if state.active.is_none() {
            capture_gate_tx = None;
            playback_gate_tx = None;
        }
        if let (Some(capture_gate_tx), Some(capture_gate)) =
            (capture_gate_tx.as_ref(), state.capture_gate())
        {
            let _ = capture_gate_tx.send(capture_gate);
        }
        if let (Some(playback_gate_tx), Some(playback_gate)) =
            (playback_gate_tx.as_ref(), state.playback_gate())
        {
            let _ = playback_gate_tx.send(playback_gate);
        }
        if shutdown {
            break;
        }
    }

    if let Some(task) = connection_task {
        logging::debug(
            "voice",
            "aborting voice connection task during voice runtime shutdown",
        );
        task.abort();
    }
}

async fn run_voice_gateway_session(
    session: VoiceGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    initial_capture_gate: VoiceCaptureGate,
    capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    initial_playback_gate: VoicePlaybackGate,
    playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
) {
    match connect_voice_gateway(
        &session,
        &status_publisher,
        initial_capture_gate,
        capture_gate_rx,
        initial_playback_gate,
        playback_gate_rx,
    )
    .await
    {
        Ok(()) => {
            status_publisher
                .publish(
                    &session,
                    VoiceConnectionStatus::Disconnected,
                    "Voice gateway disconnected",
                )
                .await;
        }
        Err(error) => {
            logging::error("voice", &error);
            status_publisher
                .publish(&session, VoiceConnectionStatus::Failed, error)
                .await;
        }
    }
    let _ = events_tx.send(session.connection_ended_event());
}

async fn connect_voice_gateway(
    session: &VoiceGatewaySession,
    status_publisher: &VoiceStatusPublisher,
    initial_capture_gate: VoiceCaptureGate,
    mut capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    initial_playback_gate: VoicePlaybackGate,
    mut playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
) -> Result<(), String> {
    let url = voice_gateway_url(&session.endpoint)?;
    logging::debug("voice", format!("connecting voice websocket: {url}"));
    let connect_started = Instant::now();
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
        .await
        .map_err(|_| "voice websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("voice websocket connect failed: {error}"))?;
    logging::debug(
        "voice",
        format!(
            "voice websocket connected: status={} elapsed_ms={}",
            response.status(),
            connect_started.elapsed().as_millis()
        ),
    );
    status_publisher
        .publish(
            session,
            VoiceConnectionStatus::Connected,
            "Voice gateway connected",
        )
        .await;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut child_tasks = VoiceChildTasks::default();
    let mut speaking_tracker = VoiceSpeakingTracker::default();
    let mut speaking_sweep = tokio::time::interval(VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL);
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
    let (local_speaking_tx, mut local_speaking_rx) = mpsc::unbounded_channel();
    let (remote_speaking_tx, mut remote_speaking_rx) = mpsc::unbounded_channel();
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_mut, unused_variables, unused_assignments))]
    let mut current_capture_gate = initial_capture_gate;
    let mut current_playback_gate = initial_playback_gate;
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_mut, unused_variables, unused_assignments))]
    let mut voice_ready: Option<VoiceTransportSession> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let dave_state = Arc::new(Mutex::new(VoiceDaveState::new(session)));

    send_voice_text(&writer, voice_identify_payload(&session)).await?;
    logging::debug("voice", "voice identify sent");
    logging::debug("voice", "voice websocket read loop started");

    loop {
        let frame = tokio::select! {
            capture_gate = capture_gate_rx.recv() => {
                match capture_gate {
                    Some(capture_gate) => {
                        #[cfg(feature = "voice-playback")]
                        {
                            current_capture_gate = capture_gate;
                        }
                        child_tasks.set_voice_transmit_gate(capture_gate.enabled);
                        continue;
                    }
                    None => {
                        child_tasks.set_voice_transmit_gate(false);
                        break;
                    }
                }
            }
            playback_gate = playback_gate_rx.recv() => {
                match playback_gate {
                    Some(playback_gate) => {
                        current_playback_gate = playback_gate;
                        child_tasks.set_voice_playback_enabled(playback_gate.enabled);
                        continue;
                    }
                    None => {
                        child_tasks.set_voice_playback_enabled(false);
                        break;
                    }
                }
            }
            local_speaking = local_speaking_rx.recv() => {
                let Some(local_speaking) = local_speaking else {
                    break;
                };
                if let Some(speaking) = speaking_tracker.record_local(local_speaking) {
                    status_publisher
                        .publish_speaking(session, session.user_id, speaking)
                        .await;
                }
                continue;
            }
            remote_speaking = remote_speaking_rx.recv() => {
                let Some(user_id) = remote_speaking else {
                    break;
                };
                if let Some(speaking) = speaking_tracker.record_remote(user_id, true, Instant::now()) {
                    status_publisher.publish_speaking(session, user_id, speaking).await;
                }
                continue;
            }
            _ = speaking_sweep.tick() => {
                for user_id in speaking_tracker.expire_remote(Instant::now()) {
                    status_publisher.publish_speaking(session, user_id, false).await;
                }
                continue;
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break;
        };
        let frame = frame.map_err(|error| format!("voice websocket read failed: {error}"))?;
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("voice websocket JSON parse failed: {error}"))?;
                if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
                    *last_sequence.lock().await = Some(sequence);
                }
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
                match opcode {
                    VOICE_OP_READY => {
                        let ready = parse_voice_ready_payload(&value)?;
                        logging::debug(
                            "voice",
                            format!(
                                "voice ready received: ssrc={} udp={}:{} modes={}",
                                ready.ssrc,
                                ready.ip,
                                ready.port,
                                ready.modes.len()
                            ),
                        );
                        let mode = choose_encryption_mode(&ready.modes)?;
                        logging::debug("voice", format!("voice encryption mode selected: {mode}"));
                        let (socket, discovered) = discover_voice_udp_address(&ready).await?;
                        send_voice_text(&writer, voice_select_protocol_payload(&discovered, &mode))
                            .await?;
                        logging::debug(
                            "voice",
                            format!(
                                "voice select protocol sent: address={} port={} mode={}",
                                discovered.address, discovered.port, mode
                            ),
                        );
                        udp_socket = Some(socket);
                        #[cfg(feature = "voice-playback")]
                        {
                            voice_ready = Some(ready);
                        }
                        logging::debug("voice", "voice UDP discovery completed");
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = parse_voice_session_description(&value)?;
                        logging::debug(
                            "voice",
                            format!("voice session description received: {description:?}"),
                        );
                        if let Some(dave_protocol_version) = description.dave_protocol_version {
                            let dave_protocol_version = u16::try_from(dave_protocol_version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.lock().await.reinit(dave_protocol_version)?;
                        }
                        if let Some(socket) = udp_socket.as_ref() {
                            logging::debug("voice", "starting voice UDP receive task");
                            let opus_decode = VoiceOpusDecode::start(current_playback_gate.enabled);
                            let playback_tx = Some(opus_decode.frames_tx.clone());
                            child_tasks.replace_opus_decode(opus_decode);
                            child_tasks.set_voice_playback_enabled(current_playback_gate.enabled);
                            #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
                            let transmit_description = description.clone();
                            child_tasks.replace_udp_receive(tokio::spawn(run_voice_udp_receive(
                                Arc::clone(socket),
                                description,
                                Arc::clone(&dave_state),
                                playback_tx,
                                remote_speaking_tx.clone(),
                            )));
                            #[cfg(feature = "voice-playback")]
                            if let Some(ready) = voice_ready.as_ref() {
                                let (pcm_tx, pcm_rx) = sync_channel(VOICE_MIC_PCM_FRAME_QUEUE);
                                let (gate_tx, gate_rx) = watch::channel(current_capture_gate.enabled);
                                child_tasks.replace_udp_transmit(
                                    tokio::spawn(run_voice_udp_transmit(
                                        pcm_rx,
                                        Arc::clone(socket),
                                        Arc::clone(&writer),
                                        transmit_description,
                                        ready.ssrc,
                                        Arc::clone(&dave_state),
                                        gate_rx,
                                        local_speaking_tx.clone(),
                                    )),
                                    gate_tx,
                                    pcm_tx,
                                );
                                child_tasks.set_voice_transmit_gate(current_capture_gate.enabled);
                            }
                        }
                    }
                    VOICE_OP_HEARTBEAT_ACK => {}
                    VOICE_OP_HELLO => {
                        let interval = value
                            .get("d")
                            .and_then(|data| data.get("heartbeat_interval"))
                            .and_then(Value::as_u64)
                            .map(Duration::from_millis)
                            .ok_or_else(|| "voice hello missing heartbeat interval".to_owned())?;
                        logging::debug(
                            "voice",
                            format!(
                                "voice hello received: heartbeat_interval_ms={}",
                                interval.as_millis()
                            ),
                        );
                        child_tasks.replace_heartbeat(tokio::spawn(run_voice_heartbeat(
                            Arc::clone(&writer),
                            interval,
                            Arc::clone(&last_sequence),
                        )));
                        logging::debug("voice", "voice heartbeat task started");
                    }
                    VOICE_OP_CLIENTS_CONNECT
                    | VOICE_OP_CLIENT_DISCONNECT
                    | VOICE_OP_MEDIA_SINK_WANTS
                    | VOICE_OP_CLIENT_FLAGS
                    | VOICE_OP_CLIENT_PLATFORM
                    | VOICE_OP_DAVE_PREPARE_TRANSITION
                    | VOICE_OP_DAVE_EXECUTE_TRANSITION
                    | VOICE_OP_DAVE_PREPARE_EPOCH => {
                        dave_state
                            .lock()
                            .await
                            .handle_json_op(&writer, opcode, &value)
                            .await?;
                    }
                    VOICE_OP_SPEAKING => {
                        let speaking = dave_state.lock().await.handle_speaking_op(&value);
                        if let (Some(user_id), Some(speaking)) = (
                            speaking.user_id.and_then(Id::<UserMarker>::new_checked),
                            speaking.speaking,
                        ) {
                            if let Some(speaking) = speaking_tracker.record_remote(
                                user_id,
                                voice_speaking_microphone_active(speaking),
                                Instant::now(),
                            ) {
                                status_publisher
                                    .publish_speaking(session, user_id, speaking)
                                    .await;
                            }
                        }
                    }
                    other => logging::debug("voice", format!("unhandled voice gateway op={other}")),
                }
            }
            WsMessage::Ping(payload) => {
                let mut writer = writer.lock().await;
                writer
                    .send(WsMessage::Pong(payload))
                    .await
                    .map_err(|error| format!("voice websocket pong failed: {error}"))?;
            }
            WsMessage::Close(frame) => {
                if let Some(frame) = frame {
                    logging::debug(
                        "voice",
                        format!(
                            "voice websocket closed: code={} reason={}",
                            frame.code, frame.reason
                        ),
                    );
                } else {
                    logging::debug("voice", "voice websocket closed without close frame");
                }
                break;
            }
            WsMessage::Binary(payload) => {
                let frame = parse_voice_binary_frame(&payload)?;
                *last_sequence.lock().await = Some(frame.sequence);
                dave_state
                    .lock()
                    .await
                    .handle_binary_frame(&writer, frame)
                    .await?;
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }

    child_tasks.abort_all();
    for user_id in speaking_tracker.clear_all(session.user_id) {
        status_publisher.publish_speaking(session, user_id, false).await;
    }
    Ok(())
}

async fn discover_voice_udp_address(
    ready: &VoiceTransportSession,
) -> Result<(Arc<UdpSocket>, DiscoveredVoiceAddress), String> {
    logging::debug("voice", "binding voice UDP socket");
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|error| format!("voice UDP bind failed: {error}"))?;
    if let Ok(local_addr) = socket.local_addr() {
        logging::debug(
            "voice",
            format!("voice UDP socket bound: local={local_addr}"),
        );
    }
    logging::debug(
        "voice",
        format!(
            "connecting voice UDP socket: remote={}:{}",
            ready.ip, ready.port
        ),
    );
    socket
        .connect((ready.ip.as_str(), ready.port))
        .await
        .map_err(|error| format!("voice UDP connect failed: {error}"))?;
    logging::debug("voice", "voice UDP socket connected");
    logging::debug(
        "voice",
        format!("sending voice UDP discovery request: ssrc={}", ready.ssrc),
    );
    socket
        .send(&udp_discovery_request(ready.ssrc))
        .await
        .map_err(|error| format!("voice UDP discovery send failed: {error}"))?;

    let mut response = [0u8; UDP_DISCOVERY_PACKET_LEN];
    logging::debug("voice", "waiting for voice UDP discovery response");
    let len = timeout(UDP_DISCOVERY_TIMEOUT, socket.recv(&mut response))
        .await
        .map_err(|_| "voice UDP discovery timed out".to_owned())?
        .map_err(|error| format!("voice UDP discovery receive failed: {error}"))?;
    let discovered = parse_udp_discovery_response(&response[..len], ready.ssrc)?;
    logging::debug(
        "voice",
        format!(
            "voice UDP discovery response received: address={} port={}",
            discovered.address, discovered.port
        ),
    );
    Ok((Arc::new(socket), discovered))
}

async fn run_voice_udp_receive(
    socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    playback_tx: Option<mpsc::Sender<VoicePlaybackFrame>>,
    remote_speaking_tx: mpsc::UnboundedSender<Id<UserMarker>>,
) {
    let mode = description.mode.clone();
    let decryptor = match VoiceRtpDecryptor::new(&description.mode, &description.secret_key) {
        Ok(decryptor) => decryptor,
        Err(error) => {
            logging::error("voice", format!("voice RTP decrypt setup failed: {error}"));
            return;
        }
    };
    logging::debug(
        "voice",
        format!("voice UDP receive decrypt active: mode={mode}"),
    );
    let mut packet = vec![0u8; 2048];
    let mut rtp_packets = 0u64;
    let mut decrypted_packets = 0u64;
    let mut dave_decrypted_packets = 0u64;
    let mut dave_pending_packets = 0u64;
    let mut decrypt_failures = 0u64;
    let mut malformed_packets = 0u64;
    loop {
        match socket.recv(&mut packet).await {
            Ok(len) => match parse_rtp_header(&packet[..len]) {
                Ok(header) => {
                    rtp_packets = rtp_packets.saturating_add(1);
                    match decryptor.decrypt_packet(&packet[..len], &header) {
                        Ok(payload) => {
                            decrypted_packets = decrypted_packets.saturating_add(1);
                            let (remote_user_id, media) = {
                                let mut dave_state = dave_state.lock().await;
                                let remote_user_id = dave_state.user_id_for_ssrc(header.ssrc);
                                let media = dave_state
                                    .unwrap_media_payload_for_ssrc(header.ssrc, &payload.media_payload);
                                (remote_user_id, media)
                            };
                            let media_payload_len = match &media {
                                VoiceMediaPayload::Plain(payload) => payload.len(),
                                VoiceMediaPayload::DaveMissingUser { payload_len }
                                | VoiceMediaPayload::DaveNotReady { payload_len, .. } => {
                                    dave_pending_packets = dave_pending_packets.saturating_add(1);
                                    if dave_pending_packets == 1 || dave_pending_packets % 100 == 0 {
                                        logging::debug(
                                            "voice",
                                            format!(
                                                "DAVE media decrypt pending: count={} ssrc={} seq={} reason={}",
                                                dave_pending_packets,
                                                header.ssrc,
                                                header.sequence,
                                                media.pending_reason()
                                            ),
                                        );
                                    }
                                    *payload_len
                                }
                                VoiceMediaPayload::DaveDecryptFailed { message, .. } => {
                                    decrypt_failures = decrypt_failures.saturating_add(1);
                                    if decrypt_failures == 1 || decrypt_failures % 100 == 0 {
                                        logging::debug(
                                            "voice",
                                            format!(
                                                "DAVE media decrypt failed: count={} ssrc={} seq={} error={}",
                                                decrypt_failures, header.ssrc, header.sequence, message
                                            ),
                                        );
                                    }
                                    payload.media_payload.len()
                                }
                                VoiceMediaPayload::DaveDecrypted { opus, .. } => {
                                    dave_decrypted_packets = dave_decrypted_packets.saturating_add(1);
                                    opus.len()
                                }
                            };
                            if dave_decrypted_packets == 1 || dave_decrypted_packets % 500 == 0 {
                                if let VoiceMediaPayload::DaveDecrypted { user_id, .. } = &media {
                                    logging::debug(
                                        "voice",
                                        format!(
                                            "DAVE media decrypted: count={} user_id={} ssrc={} seq={} opus_len={}",
                                            dave_decrypted_packets,
                                            user_id,
                                            header.ssrc,
                                            header.sequence,
                                            media_payload_len
                                        ),
                                    );
                                }
                            }
                            if let Some(frame) = voice_playback_frame(&media, &header)
                                && let Some(tx) = playback_tx.as_ref()
                            {
                                let _ = tx.try_send(frame);
                            }
                            if let Some(user_id) = remote_user_id
                                && voice_media_payload_counts_as_remote_activity(&media)
                            {
                                let _ = remote_speaking_tx.send(user_id);
                            }
                            if decrypted_packets == 1 || decrypted_packets % 500 == 0 {
                                logging::debug(
                                    "voice",
                                    format!(
                                        "decrypted RTP packet: count={} ssrc={} seq={} timestamp={} payload_type={} payload_len={} extension_body_len={}",
                                        decrypted_packets,
                                        header.ssrc,
                                        header.sequence,
                                        header.timestamp,
                                        header.payload_type,
                                        media_payload_len,
                                        payload.encrypted_extension_body_len
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            decrypt_failures = decrypt_failures.saturating_add(1);
                            if decrypt_failures == 1 || decrypt_failures % 100 == 0 {
                                logging::debug(
                                    "voice",
                                    format!(
                                        "RTP decrypt failed: count={} ssrc={} seq={} timestamp={} error={}",
                                        decrypt_failures,
                                        header.ssrc,
                                        header.sequence,
                                        header.timestamp,
                                        error
                                    ),
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    malformed_packets = malformed_packets.saturating_add(1);
                    if malformed_packets == 1 || malformed_packets % 100 == 0 {
                        logging::debug(
                            "voice",
                            format!(
                                "ignoring non-RTP UDP packet: count={malformed_packets} error={error}"
                            ),
                        );
                    }
                }
            },
            Err(error) => {
                logging::error("voice", format!("voice UDP receive failed: {error}"));
                break;
            }
        }
    }
}

impl VoiceRtpDecryptor {
    fn new(mode: &str, secret_key: &[u8]) -> Result<Self, String> {
        match mode {
            AEAD_AES256_GCM_RTPSIZE => Aes256Gcm::new_from_slice(secret_key)
                .map(Self::Aes256Gcm)
                .map_err(|_| "voice AES-GCM key is invalid".to_owned()),
            AEAD_XCHACHA20_POLY1305_RTPSIZE => XChaCha20Poly1305::new_from_slice(secret_key)
                .map(Self::XChaCha20Poly1305)
                .map_err(|_| "voice XChaCha20-Poly1305 key is invalid".to_owned()),
            other => Err(format!("unsupported voice RTP decrypt mode: {other}")),
        }
    }

    fn decrypt_packet(
        &self,
        packet: &[u8],
        header: &RtpHeader,
    ) -> Result<DecryptedRtpPayload, String> {
        if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE {
            return Err(format!(
                "RTP packet has unsupported payload type: {}",
                header.payload_type
            ));
        }
        let sealed_end = packet
            .len()
            .checked_sub(RTP_AEAD_NONCE_SUFFIX_BYTES)
            .ok_or_else(|| "RTP packet is missing nonce suffix".to_owned())?;
        if sealed_end < header.authenticated_header_len + RTP_AEAD_TAG_BYTES {
            return Err("RTP packet is too short for encrypted payload".to_owned());
        }
        let nonce_suffix = &packet[sealed_end..];
        let sealed_payload = &packet[header.authenticated_header_len..sealed_end];
        let aad = &packet[..header.authenticated_header_len];
        let decrypted = match self {
            Self::Aes256Gcm(cipher) => {
                let mut nonce = [0u8; 12];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(nonce_suffix);
                cipher
                    .decrypt(
                        AesGcmNonce::from_slice(&nonce),
                        Payload {
                            msg: sealed_payload,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP AES-GCM decrypt failed".to_owned())?
            }
            Self::XChaCha20Poly1305(cipher) => {
                let mut nonce = [0u8; 24];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(nonce_suffix);
                cipher
                    .decrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: sealed_payload,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP XChaCha20-Poly1305 decrypt failed".to_owned())?
            }
        };
        if decrypted.len() < header.encrypted_extension_body_len {
            return Err("decrypted RTP payload is shorter than extension body".to_owned());
        }
        Ok(DecryptedRtpPayload {
            media_payload: decrypted[header.encrypted_extension_body_len..].to_vec(),
            encrypted_extension_body_len: header.encrypted_extension_body_len,
        })
    }
}

#[allow(dead_code)]
impl VoiceRtpEncryptor {
    fn new(mode: &str, secret_key: &[u8]) -> Result<Self, String> {
        match mode {
            AEAD_AES256_GCM_RTPSIZE => Aes256Gcm::new_from_slice(secret_key)
                .map(Self::Aes256Gcm)
                .map_err(|_| "voice AES-GCM key is invalid".to_owned()),
            AEAD_XCHACHA20_POLY1305_RTPSIZE => XChaCha20Poly1305::new_from_slice(secret_key)
                .map(Self::XChaCha20Poly1305)
                .map_err(|_| "voice XChaCha20-Poly1305 key is invalid".to_owned()),
            other => Err(format!("unsupported voice RTP encrypt mode: {other}")),
        }
    }

    fn encrypt_packet(
        &self,
        packet: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        let header = parse_rtp_header(packet)?;
        if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE {
            return Err(format!(
                "RTP packet has unsupported payload type: {}",
                header.payload_type
            ));
        }
        if packet.len() <= header.authenticated_header_len {
            return Err("RTP packet is missing media payload".to_owned());
        }

        let aad = &packet[..header.authenticated_header_len];
        let plaintext = &packet[header.authenticated_header_len..];
        let sealed_payload = match self {
            Self::Aes256Gcm(cipher) => {
                let mut nonce = [0u8; 12];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        AesGcmNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP AES-GCM encrypt failed".to_owned())?
            }
            Self::XChaCha20Poly1305(cipher) => {
                let mut nonce = [0u8; 24];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP XChaCha20-Poly1305 encrypt failed".to_owned())?
            }
        };

        let mut encrypted = Vec::with_capacity(
            header.authenticated_header_len + sealed_payload.len() + RTP_AEAD_NONCE_SUFFIX_BYTES,
        );
        encrypted.extend_from_slice(aad);
        encrypted.extend_from_slice(&sealed_payload);
        encrypted.extend_from_slice(&nonce_suffix);
        Ok(encrypted)
    }
}

#[allow(dead_code)]
impl VoiceOutboundRtpState {
    fn packetize(&mut self, opus_payload: &[u8]) -> Result<Vec<u8>, String> {
        let packet = build_voice_rtp_packet(
            self.sequence,
            self.timestamp,
            self.ssrc,
            opus_payload,
        )?;
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
        Ok(packet)
    }
}

#[allow(dead_code)]
impl VoiceOpusEncode {
    fn new() -> Result<Self, String> {
        OpusEncoder::new(
            DISCORD_VOICE_SAMPLE_RATE,
            Channels::Stereo,
            OpusApplication::Voip,
        )
        .map(|encoder| Self { encoder })
        .map_err(|error| format!("voice Opus encoder init failed: {error}"))
    }

    fn encode_20ms_i16(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
        if pcm.len() != DISCORD_OPUS_20MS_STEREO_SAMPLES {
            return Err(format!(
                "voice Opus encoder expected {} interleaved stereo samples, got {}",
                DISCORD_OPUS_20MS_STEREO_SAMPLES,
                pcm.len()
            ));
        }
        self.encoder
            .encode_vec(pcm, OPUS_MAX_ENCODED_FRAME_BYTES)
            .map_err(|error| format!("voice Opus encode failed: {error}"))
    }
}

#[allow(dead_code)]
impl VoiceFakeOutboundSendState {
    fn new(
        mode: &str,
        secret_key: &[u8],
        rtp: VoiceOutboundRtpState,
        nonce_suffix: u32,
    ) -> Result<Self, String> {
        Ok(Self {
            rtp,
            encryptor: VoiceRtpEncryptor::new(mode, secret_key)?,
            nonce_suffix,
            allow_microphone_transmit: false,
            self_mute: true,
            dave_active: false,
            speaking: false,
            logged_block_reason: None,
            events: Vec::new(),
        })
    }

    fn set_capture_gate(&mut self, allow_microphone_transmit: bool, self_mute: bool) {
        self.allow_microphone_transmit = allow_microphone_transmit;
        self.self_mute = self_mute;
    }

    fn set_dave_active(&mut self, active: bool) {
        self.dave_active = active;
    }

    fn events(&self) -> &[VoiceFakeOutboundEvent] {
        &self.events
    }

    fn take_events(&mut self) -> Vec<VoiceFakeOutboundEvent> {
        std::mem::take(&mut self.events)
    }

    fn record_blocked_transmit(&mut self, reason: VoiceFakeSendBlockReason) -> bool {
        if self.logged_block_reason == Some(reason) {
            return false;
        }
        self.logged_block_reason = Some(reason);
        true
    }

    fn take_logged_block_reason(&mut self) -> Option<VoiceFakeSendBlockReason> {
        self.logged_block_reason.take()
    }

    fn send_opus_frame(&mut self, opus_payload: &[u8]) -> Result<VoiceFakeSendOutcome, String> {
        self.send_opus_frame_with_dave_payload(VoiceDaveOutboundPayload::Plain(
            opus_payload.to_vec(),
        ))
    }

    fn send_opus_frame_with_dave(
        &mut self,
        opus_payload: &[u8],
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceFakeSendOutcome, String> {
        let dave_payload = dave.prepare_outbound_opus(opus_payload);
        self.send_opus_frame_with_dave_payload(dave_payload)
    }

    fn send_opus_frame_with_dave_payload(
        &mut self,
        dave_payload: VoiceDaveOutboundPayload,
    ) -> Result<VoiceFakeSendOutcome, String> {
        if !self.capture_gate_enabled() {
            return Ok(VoiceFakeSendOutcome::Noop);
        }
        if self.dave_active {
            return Ok(VoiceFakeSendOutcome::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundUnsupported,
            ));
        }
        let opus_payload = match dave_payload {
            VoiceDaveOutboundPayload::Plain(opus) | VoiceDaveOutboundPayload::Encrypted(opus) => opus,
            VoiceDaveOutboundPayload::Blocked(reason) => {
                return Ok(VoiceFakeSendOutcome::Blocked(reason));
            }
        };

        let encrypted = self.encrypt_current_packet(&opus_payload)?;
        if !self.speaking {
            self.events.push(VoiceFakeOutboundEvent::Speaking {
                speaking: true,
                ssrc: self.rtp.ssrc,
            });
            self.speaking = true;
        }
        self.events.push(VoiceFakeOutboundEvent::Packet { bytes: encrypted });
        self.advance_packet_state();
        Ok(VoiceFakeSendOutcome::Sent)
    }

    fn stop_speaking(&mut self) -> Result<VoiceFakeSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            VoiceDaveOutboundPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec())
        })
    }

    fn stop_speaking_with_dave(
        &mut self,
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceFakeSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            dave.prepare_outbound_opus(&DISCORD_OPUS_SILENCE_FRAME)
        })
    }

    fn stop_speaking_with_dave_payload(
        &mut self,
        mut next_silence: impl FnMut() -> VoiceDaveOutboundPayload,
    ) -> Result<VoiceFakeSendOutcome, String> {
        if !self.speaking {
            return Ok(VoiceFakeSendOutcome::Noop);
        }
        if !self.capture_gate_enabled() {
            return Ok(self.queue_speaking_off());
        }
        if self.dave_active {
            return Ok(self.queue_speaking_off());
        }
        if self.ensure_nonce_capacity(DISCORD_TRAILING_SILENCE_FRAMES).is_err() {
            return Ok(self.queue_speaking_off());
        }

        for _ in 0..DISCORD_TRAILING_SILENCE_FRAMES {
            let opus_payload = match next_silence() {
                VoiceDaveOutboundPayload::Plain(opus) | VoiceDaveOutboundPayload::Encrypted(opus) => opus,
                VoiceDaveOutboundPayload::Blocked(_) => {
                    return Ok(self.queue_speaking_off());
                }
            };
            let encrypted = self.encrypt_current_packet(&opus_payload)?;
            self.events.push(VoiceFakeOutboundEvent::Packet { bytes: encrypted });
            self.advance_packet_state();
        }
        Ok(self.queue_speaking_off())
    }

    fn queue_speaking_off(&mut self) -> VoiceFakeSendOutcome {
        self.events.push(VoiceFakeOutboundEvent::Speaking {
            speaking: false,
            ssrc: self.rtp.ssrc,
        });
        self.speaking = false;
        VoiceFakeSendOutcome::Sent
    }

    fn capture_gate_enabled(&self) -> bool {
        self.allow_microphone_transmit && !self.self_mute
    }

    fn encrypt_current_packet(&self, opus_payload: &[u8]) -> Result<Vec<u8>, String> {
        let nonce_suffix = self.current_nonce_suffix()?;
        let packet = build_voice_rtp_packet(
            self.rtp.sequence,
            self.rtp.timestamp,
            self.rtp.ssrc,
            opus_payload,
        )?;
        self.encryptor.encrypt_packet(&packet, nonce_suffix)
    }

    fn current_nonce_suffix(&self) -> Result<[u8; RTP_AEAD_NONCE_SUFFIX_BYTES], String> {
        if self.nonce_suffix == u32::MAX {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(self.nonce_suffix.to_be_bytes())
    }

    fn ensure_nonce_capacity(&self, packets: usize) -> Result<(), String> {
        let remaining = u32::MAX - self.nonce_suffix;
        if remaining < packets as u32 {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(())
    }

    fn advance_packet_state(&mut self) {
        self.rtp.sequence = self.rtp.sequence.wrapping_add(1);
        self.rtp.timestamp = self
            .rtp
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
        self.nonce_suffix = self.nonce_suffix.saturating_add(1);
    }
}

async fn run_voice_playback_decode(
    mut frames_rx: mpsc::Receiver<VoicePlaybackFrame>,
    decoded_audio: VoiceDecodedAudio,
) {
    let mut decoders = HashMap::new();
    let mut decoded_frames = 0u64;
    while let Some(frame) = frames_rx.recv().await {
        let decoder = match decoders.entry(frame.ssrc) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => match OpusDecoder::new(
                DISCORD_VOICE_SAMPLE_RATE,
                Channels::Stereo,
            ) {
                Ok(decoder) => entry.insert(decoder),
                Err(error) => {
                    logging::error("voice", format!("voice Opus decoder init failed: {error}"));
                    continue;
                }
            },
        };
        let mut decoded = vec![0.0f32; OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL * usize::from(DISCORD_VOICE_CHANNELS)];
        let samples_per_channel = match decoder.decode_float(&frame.opus, &mut decoded, false) {
            Ok(samples) => samples,
            Err(error) => {
                logging::debug(
                    "voice",
                    format!(
                        "voice Opus decode failed: ssrc={} seq={} error={}",
                        frame.ssrc, frame.sequence, error
                    ),
                );
                continue;
            }
        };
        let decoded_len = samples_per_channel * usize::from(DISCORD_VOICE_CHANNELS);
        decoded.truncate(decoded_len);
        decoded_audio.try_send(decoded.clone());
        decoded_frames = decoded_frames.saturating_add(1);
        if decoded_frames == 1 || decoded_frames % 500 == 0 {
            logging::debug(
                "voice",
                format!(
                    "voice Opus decoded: count={} ssrc={} user_id={:?} seq={} samples_per_channel={} pcm_samples={}",
                    decoded_frames,
                    frame.ssrc,
                    frame.user_id,
                    frame.sequence,
                    samples_per_channel,
                    decoded.len()
                ),
            );
        }
    }
}

#[cfg(feature = "voice-playback")]
async fn run_voice_udp_transmit(
    pcm_rx: StdReceiver<Vec<i16>>,
    udp_socket: Arc<UdpSocket>,
    writer: VoiceWriter,
    description: VoiceSessionDescription,
    ssrc: u32,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    mut gate_rx: watch::Receiver<bool>,
    local_speaking_tx: mpsc::UnboundedSender<bool>,
) {
    let rtp = VoiceOutboundRtpState {
        sequence: 0,
        timestamp: 0,
        ssrc,
    };
    let mut sender = match VoiceFakeOutboundSendState::new(
        &description.mode,
        &description.secret_key,
        rtp,
        0,
    ) {
        Ok(sender) => sender,
        Err(error) => {
            logging::error("voice", format!("voice UDP transmit init failed: {error}"));
            return;
        }
    };
    sender.set_capture_gate(*gate_rx.borrow(), false);
    let mut encoder = match VoiceOpusEncode::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            logging::error("voice", error);
            return;
        }
    };

    loop {
        tokio::select! {
            changed = gate_rx.changed() => {
                if changed.is_err() {
                    if let Err(error) = flush_voice_outbound_events(
                        &udp_socket,
                        &writer,
                        sender.stop_speaking_with_dave(&mut *dave_state.lock().await),
                        &mut sender,
                        &local_speaking_tx,
                    ).await {
                        logging::error("voice", error);
                    }
                    let _ = local_speaking_tx.send(false);
                    sender.set_capture_gate(false, false);
                    break;
                }
                let enabled = *gate_rx.borrow();
                if !enabled
                    && let Err(error) = flush_voice_outbound_events(
                        &udp_socket,
                        &writer,
                        sender.stop_speaking_with_dave(&mut *dave_state.lock().await),
                        &mut sender,
                        &local_speaking_tx,
                    ).await
                {
                    logging::error("voice", error);
                }
                if !enabled {
                    let _ = local_speaking_tx.send(false);
                }
                sender.set_capture_gate(enabled, false);
            }
            _ = sleep(Duration::from_millis(20)) => {
                if !*gate_rx.borrow() {
                    continue;
                }
                match pcm_rx.try_recv() {
                    Ok(frame) => {
                        let opus = match encoder.encode_20ms_i16(&frame) {
                            Ok(opus) => opus,
                            Err(error) => {
                                logging::debug("voice", error);
                                continue;
                            }
                        };
                        let outcome = sender.send_opus_frame_with_dave(&opus, &mut *dave_state.lock().await);
                        if let Err(error) = flush_voice_outbound_events(
                            &udp_socket,
                            &writer,
                            outcome,
                            &mut sender,
                            &local_speaking_tx,
                        ).await {
                            logging::error("voice", error);
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        if let Err(error) = flush_voice_outbound_events(
                            &udp_socket,
                            &writer,
                            sender.stop_speaking_with_dave(&mut *dave_state.lock().await),
                            &mut sender,
                            &local_speaking_tx,
                        ).await {
                            logging::error("voice", error);
                        }
                        let _ = local_speaking_tx.send(false);
                        sender.set_capture_gate(false, false);
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(feature = "voice-playback")]
async fn flush_voice_outbound_events(
    udp_socket: &UdpSocket,
    writer: &VoiceWriter,
    outcome: Result<VoiceFakeSendOutcome, String>,
    sender: &mut VoiceFakeOutboundSendState,
    local_speaking_tx: &mpsc::UnboundedSender<bool>,
) -> Result<(), String> {
    match outcome? {
        VoiceFakeSendOutcome::Sent => {
            for event in sender.take_events() {
                match event {
                    VoiceFakeOutboundEvent::Speaking { speaking, ssrc } => {
                        send_voice_text(writer, voice_speaking_payload(ssrc, speaking)).await?;
                        let _ = local_speaking_tx.send(speaking);
                    }
                    VoiceFakeOutboundEvent::Packet { bytes } => {
                        udp_socket
                            .send(&bytes)
                            .await
                            .map_err(|error| format!("voice UDP transmit failed: {error}"))?;
                    }
                }
            }
            if let Some(reason) = sender.take_logged_block_reason() {
                logging::debug(
                    "voice",
                    format!("voice UDP transmit resumed after block: {reason:?}"),
                );
            }
        }
        VoiceFakeSendOutcome::Noop => {
            let _ = sender.take_logged_block_reason();
        }
        VoiceFakeSendOutcome::Blocked(reason) => {
            if sender.record_blocked_transmit(reason) {
                logging::debug("voice", format!("voice UDP transmit blocked: {reason:?}"));
            }
        }
    }
    Ok(())
}

async fn run_voice_heartbeat(
    writer: VoiceWriter,
    interval: Duration,
    last_sequence: Arc<Mutex<Option<i64>>>,
) {
    loop {
        let sequence = last_sequence.lock().await.unwrap_or(-1);
        if let Err(error) = send_voice_text(&writer, voice_heartbeat_payload(sequence)).await {
            logging::error("voice", format!("voice heartbeat send failed: {error}"));
            break;
        }
        sleep(interval).await;
    }
}

async fn send_voice_text(writer: &VoiceWriter, payload: String) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Text(payload.into()))
        .await
        .map_err(|error| format!("voice websocket send failed: {error}"))
}

async fn send_voice_binary(
    writer: &VoiceWriter,
    opcode: u8,
    mut payload: Vec<u8>,
) -> Result<(), String> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(opcode);
    frame.append(&mut payload);
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Binary(frame.into()))
        .await
        .map_err(|error| format!("voice websocket binary send failed: {error}"))
}

async fn send_dave_transition_ready(
    writer: &VoiceWriter,
    transition_id: u16,
) -> Result<(), String> {
    send_voice_text(
        writer,
        json!({
            "op": VOICE_OP_DAVE_TRANSITION_READY,
            "d": {
                "transition_id": transition_id,
            },
        })
        .to_string(),
    )
    .await?;
    logging::debug(
        "voice",
        format!("DAVE transition ready sent: transition_id={transition_id}"),
    );
    Ok(())
}

async fn send_dave_commit_welcome(
    writer: &VoiceWriter,
    commit_welcome: davey::CommitWelcome,
) -> Result<(), String> {
    let mut payload = commit_welcome.commit;
    if let Some(mut welcome) = commit_welcome.welcome {
        payload.append(&mut welcome);
    }
    send_voice_binary(writer, VOICE_OP_DAVE_MLS_COMMIT_WELCOME, payload).await?;
    logging::debug("voice", "DAVE commit welcome sent");
    Ok(())
}

async fn send_dave_invalid_commit_welcome(
    writer: &VoiceWriter,
    transition_id: u16,
) -> Result<(), String> {
    send_voice_text(
        writer,
        json!({
            "op": VOICE_OP_DAVE_MLS_INVALID_COMMIT_WELCOME,
            "d": {
                "transition_id": transition_id,
            },
        })
        .to_string(),
    )
    .await?;
    logging::debug(
        "voice",
        format!("DAVE invalid commit welcome sent: transition_id={transition_id}"),
    );
    Ok(())
}

fn voice_gateway_url(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint
        .trim()
        .trim_start_matches("wss://")
        .trim_start_matches("https://")
        .trim_start_matches("ws://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if endpoint.is_empty() {
        return Err("voice endpoint is empty".to_owned());
    }
    Ok(format!("wss://{endpoint}/?v={VOICE_GATEWAY_VERSION}"))
}

fn voice_identify_payload(session: &VoiceGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.guild_id.to_string(),
            "user_id": session.user_id.to_string(),
            "channel_id": session.channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

fn voice_heartbeat_payload(sequence: i64) -> String {
    json!({
        "op": 3,
        "d": {
            "t": chrono::Utc::now().timestamp_millis(),
            "seq_ack": sequence,
        },
    })
    .to_string()
}

#[cfg(feature = "voice-playback")]
fn voice_speaking_payload(ssrc: u32, speaking: bool) -> String {
    json!({
        "op": VOICE_OP_SPEAKING,
        "d": {
            "speaking": if speaking { 1 } else { 0 },
            "delay": 0,
            "ssrc": ssrc,
        },
    })
    .to_string()
}

fn parse_voice_ready_payload(value: &Value) -> Result<VoiceTransportSession, String> {
    let data = value
        .get("d")
        .ok_or_else(|| "voice ready missing data".to_owned())?;
    let ssrc = data
        .get("ssrc")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "voice ready missing ssrc".to_owned())?;
    let ip = data
        .get("ip")
        .and_then(Value::as_str)
        .filter(|ip| !ip.is_empty())
        .ok_or_else(|| "voice ready missing UDP ip".to_owned())?
        .to_owned();
    let port = data
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "voice ready missing UDP port".to_owned())?;
    let modes = data
        .get("modes")
        .and_then(Value::as_array)
        .ok_or_else(|| "voice ready missing encryption modes".to_owned())?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    Ok(VoiceTransportSession {
        ssrc,
        ip,
        port,
        modes,
    })
}

fn choose_encryption_mode(modes: &[String]) -> Result<String, String> {
    for candidate in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        if modes.iter().any(|mode| mode == candidate) {
            return Ok(candidate.to_owned());
        }
    }
    Err("voice ready did not offer a supported encryption mode".to_owned())
}

fn udp_discovery_request(ssrc: u32) -> [u8; UDP_DISCOVERY_PACKET_LEN] {
    let mut packet = [0u8; UDP_DISCOVERY_PACKET_LEN];
    packet[0..2].copy_from_slice(&1u16.to_be_bytes());
    packet[2..4].copy_from_slice(&70u16.to_be_bytes());
    packet[4..8].copy_from_slice(&ssrc.to_be_bytes());
    packet
}

fn parse_udp_discovery_response(
    packet: &[u8],
    expected_ssrc: u32,
) -> Result<DiscoveredVoiceAddress, String> {
    if packet.len() < UDP_DISCOVERY_PACKET_LEN {
        return Err("voice UDP discovery response is too short".to_owned());
    }
    let packet_type = u16::from_be_bytes([packet[0], packet[1]]);
    if packet_type != 2 {
        return Err("voice UDP discovery response has invalid type".to_owned());
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]);
    if length != 70 {
        return Err("voice UDP discovery response has invalid length".to_owned());
    }
    let ssrc = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
    if ssrc != expected_ssrc {
        return Err("voice UDP discovery response has unexpected SSRC".to_owned());
    }
    let address_end = packet[8..72]
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| 8 + index)
        .unwrap_or(72);
    let address = std::str::from_utf8(&packet[8..address_end])
        .map_err(|error| format!("voice UDP discovery address is invalid UTF-8: {error}"))?
        .to_owned();
    if address.is_empty() {
        return Err("voice UDP discovery response has empty address".to_owned());
    }
    let port = u16::from_be_bytes([packet[72], packet[73]]);
    Ok(DiscoveredVoiceAddress { address, port })
}

fn voice_select_protocol_payload(discovered: &DiscoveredVoiceAddress, mode: &str) -> String {
    json!({
        "op": 1,
        "d": {
            "protocol": "udp",
            "data": {
                "address": discovered.address,
                "port": discovered.port,
                "mode": mode,
            },
        },
    })
    .to_string()
}

fn parse_voice_session_description(value: &Value) -> Result<VoiceSessionDescription, String> {
    let data = value
        .get("d")
        .ok_or_else(|| "voice session description missing data".to_owned())?;
    let mode = data
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| !mode.is_empty())
        .ok_or_else(|| "voice session description missing mode".to_owned())?
        .to_owned();
    let secret_key = data
        .get("secret_key")
        .and_then(Value::as_array)
        .ok_or_else(|| "voice session description missing secret key".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(|| "voice session description has invalid secret key byte".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if secret_key.len() != 32 {
        return Err("voice session description secret key is not 32 bytes".to_owned());
    }
    let dave_protocol_version = data.get("dave_protocol_version").and_then(Value::as_u64);
    Ok(VoiceSessionDescription {
        mode,
        secret_key,
        dave_protocol_version,
    })
}

fn parse_voice_binary_frame(payload: &[u8]) -> Result<VoiceBinaryFrame<'_>, String> {
    if payload.len() < 3 {
        return Err("voice binary frame is too short".to_owned());
    }
    let sequence = u16::from_be_bytes([payload[0], payload[1]]);
    Ok(VoiceBinaryFrame {
        sequence: i64::from(sequence),
        opcode: payload[2],
        payload: &payload[3..],
    })
}

fn split_transition_payload(payload: &[u8]) -> Option<(u16, &[u8])> {
    if payload.len() < 2 {
        return None;
    }
    Some((u16::from_be_bytes([payload[0], payload[1]]), &payload[2..]))
}

fn json_u64(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing numeric field: {key}"))
}

fn json_u16(value: &Value, key: &str) -> Result<u16, String> {
    json_u64(value, key).and_then(|value| {
        u16::try_from(value).map_err(|_| format!("numeric field does not fit u16: {key}"))
    })
}

fn voice_user_ids(value: &Value) -> Vec<u64> {
    voice_data(value)
        .and_then(|data| data.get("user_ids"))
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(voice_user_id_value).collect())
        .unwrap_or_default()
}

fn voice_user_id(value: &Value) -> Option<u64> {
    voice_data(value)
        .and_then(|data| data.get("user_id"))
        .and_then(voice_user_id_value)
}

fn parse_voice_speaking(value: &Value) -> VoiceSpeakingState {
    VoiceSpeakingState {
        user_id: voice_user_id(value),
        ssrc: voice_data_u32(value, "ssrc"),
        speaking: voice_data_u64(value, "speaking"),
    }
}

fn voice_speaking_microphone_active(speaking: u64) -> bool {
    speaking & 1 != 0
}

fn voice_data(value: &Value) -> Option<&Value> {
    value.get("d")
}

fn voice_data_u64(value: &Value, key: &str) -> Option<u64> {
    voice_data(value).and_then(|data| data.get(key)).and_then(Value::as_u64)
}

fn voice_data_u32(value: &Value, key: &str) -> Option<u32> {
    voice_data_u64(value, key).and_then(|value| u32::try_from(value).ok())
}

fn voice_data_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    voice_data(value).and_then(|data| data.get(key)).and_then(Value::as_str)
}

fn voice_data_field_count(value: &Value) -> usize {
    voice_data(value).and_then(Value::as_object).map_or(0, serde_json::Map::len)
}

fn voice_user_id_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn looks_like_dave_media_frame(payload: &[u8]) -> bool {
    payload.len() >= DAVE_MIN_SUPPLEMENTAL_BYTES
        && payload[payload.len() - DAVE_MAGIC_MARKER.len()..] == DAVE_MAGIC_MARKER
}

fn voice_playback_frame(media: &VoiceMediaPayload, header: &RtpHeader) -> Option<VoicePlaybackFrame> {
    let (user_id, opus) = match media {
        VoiceMediaPayload::Plain(opus) => (None, opus.clone()),
        VoiceMediaPayload::DaveDecrypted { user_id, opus } => (Some(*user_id), opus.clone()),
        VoiceMediaPayload::DaveMissingUser { .. }
        | VoiceMediaPayload::DaveNotReady { .. }
        | VoiceMediaPayload::DaveDecryptFailed { .. } => return None,
    };
    Some(VoicePlaybackFrame {
        ssrc: header.ssrc,
        user_id,
        sequence: header.sequence,
        timestamp: header.timestamp,
        opus,
    })
}

fn voice_media_payload_counts_as_remote_activity(media: &VoiceMediaPayload) -> bool {
    let opus = match media {
        VoiceMediaPayload::Plain(opus) | VoiceMediaPayload::DaveDecrypted { opus, .. } => opus,
        VoiceMediaPayload::DaveMissingUser { .. }
        | VoiceMediaPayload::DaveNotReady { .. }
        | VoiceMediaPayload::DaveDecryptFailed { .. } => return false,
    };
    opus.as_slice() != DISCORD_OPUS_SILENCE_FRAME
}

#[allow(dead_code)]
fn build_voice_rtp_packet(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    opus_payload: &[u8],
) -> Result<Vec<u8>, String> {
    if opus_payload.is_empty() {
        return Err("voice RTP packet requires a non-empty Opus payload".to_owned());
    }

    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + opus_payload.len());
    packet.push(RTP_VERSION << 6);
    packet.push(DISCORD_VOICE_PAYLOAD_TYPE);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(opus_payload);
    Ok(packet)
}

fn parse_rtp_header(packet: &[u8]) -> Result<RtpHeader, String> {
    if packet.len() < RTP_HEADER_MIN_LEN {
        return Err("RTP packet is too short".to_owned());
    }
    let version = packet[0] >> 6;
    if version != RTP_VERSION {
        return Err("RTP packet has unsupported version".to_owned());
    }
    let has_extension = packet[0] & 0x10 != 0;
    let csrc_count = usize::from(packet[0] & 0x0f);
    let mut authenticated_header_len = RTP_HEADER_MIN_LEN + csrc_count * 4;
    if packet.len() < authenticated_header_len {
        return Err("RTP packet is shorter than CSRC list".to_owned());
    }
    let mut encrypted_extension_body_len = 0;
    if has_extension {
        if packet.len() < authenticated_header_len + RTP_HEADER_EXTENSION_BYTES {
            return Err("RTP packet is shorter than extension header".to_owned());
        }
        let extension_words =
            u16::from_be_bytes([packet[authenticated_header_len + 2], packet[authenticated_header_len + 3]]);
        authenticated_header_len += RTP_HEADER_EXTENSION_BYTES;
        encrypted_extension_body_len = usize::from(extension_words) * RTP_EXTENSION_WORD_BYTES;
    }
    let payload_offset = authenticated_header_len + encrypted_extension_body_len;
        if packet.len() < payload_offset {
            return Err("RTP packet is shorter than extension body".to_owned());
        }

    Ok(RtpHeader {
        payload_type: packet[1] & 0x7f,
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
        authenticated_header_len,
        encrypted_extension_body_len,
        payload_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested_voice() -> CurrentVoiceConnectionState {
        CurrentVoiceConnectionState {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            self_mute: true,
            self_deaf: false,
            allow_microphone_transmit: false,
        }
    }

    fn voice_state(user_id: u64, channel_id: Option<Id<ChannelMarker>>) -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Id::new(1),
            channel_id,
            user_id: Id::new(user_id),
            session_id: Some("voice-session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }

    fn voice_server() -> VoiceServerInfo {
        VoiceServerInfo {
            guild_id: Id::new(1),
            endpoint: Some("voice.example.com".to_owned()),
            token: "secret-token".to_owned(),
        }
    }

    #[test]
    fn voice_runtime_assembles_local_voice_session() {
        let mut state = VoiceRuntimeState::default();

        assert_eq!(
            state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10)))),
            None
        );
        assert_eq!(
            state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice()))),
            None
        );
        assert_eq!(
            state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
                10,
                Some(Id::new(10))
            ))),
            None
        );
        let action = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        match action {
            Some(VoiceRuntimeAction::Connect(session)) => {
                assert_eq!(session.guild_id, Id::new(1));
                assert_eq!(session.channel_id, Id::new(10));
                assert_eq!(session.user_id, Id::new(10));
                assert_eq!(session.endpoint, "voice.example.com");
            }
            other => panic!("expected connect action, got {other:?}"),
        }
    }

    #[test]
    fn voice_runtime_capture_gate_requires_allowed_active_unmuted_voice() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));

        let mut requested = requested_voice();
        requested.allow_microphone_transmit = true;
        requested.self_mute = false;
        state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
        assert_eq!(state.capture_gate(), None);

        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10)),
        )));
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
        assert_eq!(state.capture_gate(), Some(VoiceCaptureGate { enabled: true }));
        assert_eq!(state.playback_gate(), Some(VoicePlaybackGate { enabled: true }));

        requested.self_mute = true;
        state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
        assert_eq!(state.capture_gate(), Some(VoiceCaptureGate { enabled: false }));
        assert_eq!(state.playback_gate(), Some(VoicePlaybackGate { enabled: true }));

        requested.self_deaf = true;
        state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
        assert_eq!(state.capture_gate(), Some(VoiceCaptureGate { enabled: false }));
        assert_eq!(state.playback_gate(), Some(VoicePlaybackGate { enabled: false }));

        requested.self_mute = false;
        requested.allow_microphone_transmit = false;
        requested.self_deaf = false;
        state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
        assert_eq!(state.capture_gate(), Some(VoiceCaptureGate { enabled: false }));
        assert_eq!(state.playback_gate(), Some(VoicePlaybackGate { enabled: true }));

        let mut other_channel = requested;
        other_channel.channel_id = Id::new(11);
        other_channel.allow_microphone_transmit = true;
        state.apply(VoiceRuntimeEvent::Requested(Some(other_channel)));
        assert_eq!(state.capture_gate(), None);
        assert_eq!(state.playback_gate(), None);
    }

    #[test]
    fn voice_runtime_ignores_other_user_voice_state() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        assert_eq!(
            state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
                99,
                Some(Id::new(10))
            ))),
            None
        );
    }

    #[test]
    fn voice_runtime_closes_on_leave() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10)),
        )));
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        assert_eq!(
            state.apply(VoiceRuntimeEvent::Requested(None)),
            Some(VoiceRuntimeAction::Close)
        );
    }

    #[test]
    fn voice_runtime_reconnects_after_matching_connection_end() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10)),
        )));
        let connected = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
        let Some(VoiceRuntimeAction::Connect(session)) = connected else {
            panic!("expected initial voice connect action, got {connected:?}");
        };

        assert_eq!(
            state.apply(session.connection_ended_event()),
            Some(VoiceRuntimeAction::Connect(session))
        );
    }

    #[test]
    fn voice_gateway_session_debug_redacts_secrets() {
        let session = VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "secret-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "secret-token".to_owned(),
        };

        let debug = format!("{session:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-session"));
        assert!(!debug.contains("secret-token"));
    }

    #[test]
    fn voice_state_debug_redacts_session_id() {
        let state = voice_state(10, Some(Id::new(10)));

        let debug = format!("{state:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("voice-session"));
    }

    #[test]
    fn voice_dave_state_tracks_speaking_ssrc_mapping() {
        let session = VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "voice-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "voice-token".to_owned(),
        };
        let mut state = VoiceDaveState::new(&session);

        state.record_speaking_state(VoiceSpeakingState {
            user_id: Some(30),
            ssrc: Some(1234),
            speaking: Some(1),
        });

        assert_eq!(state.ssrc_user_ids.get(&1234), Some(&30));
        assert_eq!(state.user_id_for_ssrc(1234), Some(Id::new(30)));
        assert_eq!(state.user_id_for_ssrc(9999), None);
        assert!(state.known_user_ids.contains(&30));
    }

    #[test]
    fn voice_speaking_uses_microphone_bit_only() {
        assert!(!voice_speaking_microphone_active(0));
        assert!(voice_speaking_microphone_active(1));
        assert!(!voice_speaking_microphone_active(2));
        assert!(voice_speaking_microphone_active(5));
    }

    #[test]
    fn voice_speaking_tracker_expires_remote_speakers_and_tracks_local_edges() {
        let mut tracker = VoiceSpeakingTracker::default();
        let remote_user = Id::new(30);
        let local_user = Id::new(20);
        let now = Instant::now();

        assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
        assert_eq!(
            tracker.record_remote(remote_user, true, now + VOICE_REMOTE_SPEAKING_TTL / 2),
            None
        );
        assert!(tracker.expire_remote(now + VOICE_REMOTE_SPEAKING_TTL).is_empty());
        assert_eq!(
            tracker.expire_remote(now + VOICE_REMOTE_SPEAKING_TTL + VOICE_REMOTE_SPEAKING_TTL / 2),
            vec![remote_user]
        );
        assert_eq!(tracker.record_remote(remote_user, false, now), None);
        assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
        assert_eq!(tracker.record_remote(remote_user, false, now), Some(false));

        assert_eq!(tracker.record_local(true), Some(true));
        assert_eq!(tracker.record_local(true), None);
        assert_eq!(tracker.clear_all(local_user), vec![local_user]);
    }

    #[test]
    fn voice_dave_outbound_opus_fails_closed_unless_ready() {
        let mut state = VoiceDaveState::new(&test_voice_gateway_session());

        assert_eq!(
            state.prepare_outbound_opus(b"opus-frame"),
            VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
        );

        state.protocol_version = NonZeroU16::new(1);
        assert_eq!(
            state.prepare_outbound_opus(b"opus-frame"),
            VoiceDaveOutboundPayload::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundMissingSession,
            )
        );

        state.reinit(1).expect("DAVE session should initialize");
        assert_eq!(
            state.prepare_outbound_opus(b"opus-frame"),
            VoiceDaveOutboundPayload::Blocked(VoiceFakeSendBlockReason::DaveOutboundNotReady)
        );

        state.reinit(0).expect("DAVE should disable cleanly");
        assert_eq!(
            state.prepare_outbound_opus(b"opus-frame"),
            VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
        );
    }

    #[test]
    fn dave_media_detection_requires_magic_marker() {
        assert!(!looks_like_dave_media_frame(b"opus-frame"));

        let mut payload = vec![0u8; DAVE_MIN_SUPPLEMENTAL_BYTES];
        let marker_start = payload.len() - DAVE_MAGIC_MARKER.len();
        payload[marker_start..].copy_from_slice(&DAVE_MAGIC_MARKER);

        assert!(looks_like_dave_media_frame(&payload));
    }

    #[test]
    fn voice_playback_frame_uses_only_playable_media_payloads() {
        let header = RtpHeader {
            payload_type: DISCORD_VOICE_PAYLOAD_TYPE,
            sequence: 7,
            timestamp: 8,
            ssrc: 9,
            authenticated_header_len: 12,
            encrypted_extension_body_len: 0,
            payload_offset: 12,
        };

        assert_eq!(
            voice_playback_frame(&VoiceMediaPayload::Plain(b"opus".to_vec()), &header),
            Some(VoicePlaybackFrame {
                ssrc: 9,
                user_id: None,
                sequence: 7,
                timestamp: 8,
                opus: b"opus".to_vec(),
            })
        );
        assert_eq!(
            voice_playback_frame(
                &VoiceMediaPayload::DaveDecrypted {
                    user_id: 42,
                    opus: b"dave-opus".to_vec(),
                },
                &header,
            ),
            Some(VoicePlaybackFrame {
                ssrc: 9,
                user_id: Some(42),
                sequence: 7,
                timestamp: 8,
                opus: b"dave-opus".to_vec(),
            })
        );
        assert_eq!(
            voice_playback_frame(
                &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
                &header,
            ),
            None
        );
    }

    #[test]
    fn remote_speaking_activity_ignores_silence_and_unplayable_media() {
        assert!(!voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec()),
        ));
        assert!(!voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::DaveDecrypted {
                user_id: 42,
                opus: DISCORD_OPUS_SILENCE_FRAME.to_vec(),
            },
        ));
        assert!(!voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
        ));
        assert!(!voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::DaveNotReady {
                user_id: 42,
                payload_len: 4,
            },
        ));
        assert!(!voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::DaveDecryptFailed {
                user_id: 42,
                message: "failed".to_owned(),
            },
        ));
        assert!(voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::Plain(b"opus".to_vec()),
        ));
        assert!(voice_media_payload_counts_as_remote_activity(
            &VoiceMediaPayload::DaveDecrypted {
                user_id: 42,
                opus: b"opus".to_vec(),
            },
        ));
    }

    #[test]
    fn voice_identify_payload_matches_expected_shape() {
        let session = VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "voice-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "voice-token".to_owned(),
        };
        let payload: Value = serde_json::from_str(&voice_identify_payload(&session))
            .expect("voice identify payload is valid JSON");

        assert_eq!(payload["op"].as_u64(), Some(0));
        assert_eq!(payload["d"]["server_id"].as_str(), Some("1"));
        assert_eq!(payload["d"]["user_id"].as_str(), Some("20"));
        assert_eq!(payload["d"]["channel_id"].as_str(), Some("10"));
        assert_eq!(payload["d"]["session_id"].as_str(), Some("voice-session"));
        assert_eq!(payload["d"]["token"].as_str(), Some("voice-token"));
        assert_eq!(
            payload["d"]["max_dave_protocol_version"].as_u64(),
            Some(u64::from(davey::DAVE_PROTOCOL_VERSION))
        );
    }

    #[test]
    fn voice_gateway_url_normalizes_endpoint() {
        assert_eq!(
            voice_gateway_url("voice.example.com:2048/").as_deref(),
            Ok("wss://voice.example.com:2048/?v=9")
        );
        assert_eq!(
            voice_gateway_url("wss://voice.example.com").as_deref(),
            Ok("wss://voice.example.com/?v=9")
        );
        assert_eq!(
            voice_gateway_url("https://voice.example.com").as_deref(),
            Ok("wss://voice.example.com/?v=9")
        );
        assert_eq!(
            voice_gateway_url("   /").expect_err("empty endpoint should be rejected"),
            "voice endpoint is empty"
        );
    }

    #[test]
    fn voice_ready_payload_parses_udp_transport_fields() {
        let payload = json!({
            "op": 2,
            "d": {
                "ssrc": 0x01020304u32,
                "ip": "203.0.113.10",
                "port": 50000u64,
                "modes": [
                    "aead_xchacha20_poly1305_rtpsize",
                    "aead_aes256_gcm_rtpsize"
                ],
            },
        });

        let ready = parse_voice_ready_payload(&payload).expect("ready payload should parse");

        assert_eq!(ready.ssrc, 0x01020304);
        assert_eq!(ready.ip, "203.0.113.10");
        assert_eq!(ready.port, 50000);
        assert_eq!(
            choose_encryption_mode(&ready.modes).as_deref(),
            Ok(AEAD_AES256_GCM_RTPSIZE)
        );
    }

    #[test]
    fn udp_discovery_and_select_protocol_match_expected_shapes() {
        let packet = udp_discovery_request(0x01020304);

        assert_eq!(packet.len(), UDP_DISCOVERY_PACKET_LEN);
        assert_eq!(
            &packet[..8],
            &[0x00, 0x01, 0x00, 0x46, 0x01, 0x02, 0x03, 0x04]
        );
        assert!(packet[8..].iter().all(|byte| *byte == 0));

        let mut response = [0u8; UDP_DISCOVERY_PACKET_LEN];
        response[0..2].copy_from_slice(&2u16.to_be_bytes());
        response[2..4].copy_from_slice(&70u16.to_be_bytes());
        response[4..8].copy_from_slice(&0x01020304u32.to_be_bytes());
        response[8..21].copy_from_slice(b"203.0.113.10\0");
        response[72..74].copy_from_slice(&50000u16.to_be_bytes());

        let discovered = parse_udp_discovery_response(&response, 0x01020304)
            .expect("discovery response should parse");

        assert_eq!(
            discovered,
            DiscoveredVoiceAddress {
                address: "203.0.113.10".to_owned(),
                port: 50000,
            }
        );
        let payload: Value = serde_json::from_str(&voice_select_protocol_payload(
            &discovered,
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("select protocol payload should be valid JSON");

        assert_eq!(payload["op"].as_u64(), Some(1));
        assert_eq!(payload["d"]["protocol"].as_str(), Some("udp"));
        assert_eq!(
            payload["d"]["data"]["address"].as_str(),
            Some("203.0.113.10")
        );
        assert_eq!(payload["d"]["data"]["port"].as_u64(), Some(50000));
        assert_eq!(
            payload["d"]["data"]["mode"].as_str(),
            Some(AEAD_XCHACHA20_POLY1305_RTPSIZE)
        );
    }

    #[test]
    fn voice_session_description_parses_mode_and_redacts_secret() {
        let payload = json!({
            "op": 4,
            "d": {
                "mode": AEAD_XCHACHA20_POLY1305_RTPSIZE,
                "secret_key": (0u8..32).collect::<Vec<_>>(),
                "dave_protocol_version": 1,
            },
        });

        let description =
            parse_voice_session_description(&payload).expect("session description should parse");
        let debug = format!("{description:?}");

        assert_eq!(description.mode, AEAD_XCHACHA20_POLY1305_RTPSIZE);
        assert_eq!(description.secret_key.len(), 32);
        assert_eq!(description.dave_protocol_version, Some(1));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("31"));
    }

    #[test]
    fn rtp_header_parses_minimal_and_extended_packets() {
        let packet = [
            0x80, 0x78, 0x12, 0x34, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        ];

        let header = parse_rtp_header(&packet).expect("RTP header should parse");

        assert_eq!(
            header,
            RtpHeader {
                payload_type: 0x78,
                sequence: 0x1234,
                timestamp: 0x01020304,
                ssrc: 0x05060708,
                authenticated_header_len: 12,
                encrypted_extension_body_len: 0,
                payload_offset: 12,
            }
        );

        let mut extended = vec![0x91, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
        extended.extend_from_slice(&0x11223344u32.to_be_bytes());
        extended.extend_from_slice(&0x1000u16.to_be_bytes());
        extended.extend_from_slice(&1u16.to_be_bytes());
        extended.extend_from_slice(&0x55667788u32.to_be_bytes());

        let header = parse_rtp_header(&extended).expect("extended RTP header should parse");

        assert_eq!(header.authenticated_header_len, 20);
        assert_eq!(header.encrypted_extension_body_len, 4);
        assert_eq!(header.payload_offset, 24);
    }

    #[test]
    fn rtp_decrypts_aead_rtpsize_modes_and_strips_extension_body() {
        let key = [7u8; 32];
        let nonce_suffix = [1, 2, 3, 4];
        let mut header = vec![0x90, 0x78, 0, 7, 0, 0, 0, 8, 0, 0, 0, 9];
        header.extend_from_slice(&0x1000u16.to_be_bytes());
        header.extend_from_slice(&1u16.to_be_bytes());
        let plaintext = [b"ext!".as_slice(), b"opus-frame".as_slice()].concat();

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let mut packet = header.clone();
            packet.extend(encrypt_test_rtp_payload(mode, &key, &header, &plaintext, nonce_suffix));
            packet.extend_from_slice(&nonce_suffix);
            let rtp_header = parse_rtp_header(&packet).expect("RTP header should parse");
            let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");

            let decrypted = decryptor
                .decrypt_packet(&packet, &rtp_header)
                .expect("RTP payload should decrypt");

            assert_eq!(decrypted.encrypted_extension_body_len, 4);
            assert_eq!(decrypted.media_payload, b"opus-frame");
        }
    }

    #[test]
    fn outbound_rtp_packet_builder_sets_header_and_advances_state() {
        let mut state = VoiceOutboundRtpState {
            sequence: u16::MAX,
            timestamp: u32::MAX - 100,
            ssrc: 0x01020304,
        };

        let packet = state
            .packetize(&DISCORD_OPUS_SILENCE_FRAME)
            .expect("RTP packet should build");
        let header = parse_rtp_header(&packet).expect("RTP header should parse");

        assert_eq!(packet[0], 0x80);
        assert_eq!(header.payload_type, DISCORD_VOICE_PAYLOAD_TYPE);
        assert_eq!(header.sequence, u16::MAX);
        assert_eq!(header.timestamp, u32::MAX - 100);
        assert_eq!(header.ssrc, 0x01020304);
        assert_eq!(header.payload_offset, RTP_HEADER_MIN_LEN);
        assert_eq!(&packet[header.payload_offset..], DISCORD_OPUS_SILENCE_FRAME);
        assert_eq!(state.sequence, 0);
        assert_eq!(state.timestamp, (u32::MAX - 100).wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT));

        assert_eq!(
            build_voice_rtp_packet(1, 2, 3, &[]).expect_err("empty payload should fail"),
            "voice RTP packet requires a non-empty Opus payload"
        );
    }

    #[test]
    fn outbound_rtp_encrypts_aead_rtpsize_modes_for_decrypt_round_trip() {
        let key = [9u8; 32];
        let nonce_suffix = [4, 3, 2, 1];
        let packet = build_voice_rtp_packet(7, 960, 42, b"opus-frame")
            .expect("RTP packet should build");

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let encryptor = VoiceRtpEncryptor::new(mode, &key).expect("encryptor should build");
            let encrypted = encryptor
                .encrypt_packet(&packet, nonce_suffix)
                .expect("RTP payload should encrypt");
            let header = parse_rtp_header(&encrypted).expect("encrypted RTP header should parse");
            let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");
            let decrypted = decryptor
                .decrypt_packet(&encrypted, &header)
                .expect("RTP payload should decrypt");

            assert_eq!(&encrypted[encrypted.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..], nonce_suffix);
            assert_eq!(header.sequence, 7);
            assert_eq!(header.timestamp, 960);
            assert_eq!(header.ssrc, 42);
            assert_eq!(decrypted.media_payload, b"opus-frame");
        }
    }

    #[test]
    fn opus_encoder_encodes_decodable_20ms_stereo_frame() {
        let mut encoder = VoiceOpusEncode::new().expect("Opus encoder should build");
        let pcm = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

        let opus = encoder
            .encode_20ms_i16(&pcm)
            .expect("20 ms stereo frame should encode");

        assert!(!opus.is_empty());

        let mut decoder = OpusDecoder::new(DISCORD_VOICE_SAMPLE_RATE, Channels::Stereo)
            .expect("Opus decoder should build");
        let mut decoded = vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES];
        let samples_per_channel = decoder
            .decode_float(&opus, &mut decoded, false)
            .expect("encoded Opus should decode");

        assert_eq!(samples_per_channel, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL);
        assert_eq!(
            encoder.encode_20ms_i16(&pcm[..pcm.len() - 1]).expect_err("short frame should fail"),
            format!(
                "voice Opus encoder expected {} interleaved stereo samples, got {}",
                DISCORD_OPUS_20MS_STEREO_SAMPLES,
                DISCORD_OPUS_20MS_STEREO_SAMPLES - 1
            )
        );
    }

    #[cfg(feature = "voice-playback")]
    #[test]
    fn microphone_input_conversion_produces_20ms_stereo_frames() {
        let mono = vec![0.5f32; DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL];
        let stereo = voice_input_f32_to_stereo_i16(&mono, 1);
        assert_eq!(stereo.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
        assert_eq!(stereo[0], stereo[1]);
        assert!(stereo[0] > 0);

        let interleaved = voice_input_i16_to_stereo_i16(&[1, 2, 3, 4, 5, 6], 3);
        assert_eq!(interleaved, vec![1, 2, 4, 5]);

        let unsigned = voice_input_u8_to_stereo_i16(&[0, 255], 2);
        assert_eq!(unsigned, vec![i16::MIN, 32512]);
    }

    #[cfg(feature = "voice-playback")]
    #[test]
    fn microphone_pcm_frames_resample_44100_to_48000() {
        let (tx, rx) = sync_channel(4);
        let mut frames = VoiceMicrophonePcmFrames::new(tx, 44_100);
        let input_frames = 883;
        let mut samples = Vec::with_capacity(input_frames * DISCORD_VOICE_CHANNELS_USIZE);
        for index in 0..input_frames {
            samples.push(index as i16);
            samples.push(-(index as i16));
        }

        frames.push_stereo_samples(&samples);
        let frame = rx.try_recv().expect("resampled 20 ms frame should be queued");

        assert_eq!(frame.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
        assert_eq!(frame[0], 0);
        assert_eq!(frame[1], 0);
        assert!(frame[frame.len() - 2] > 870);
        assert!(frame[frame.len() - 1] < -870);
        assert!(rx.try_recv().is_err());
    }

    #[cfg(feature = "voice-playback")]
    #[test]
    fn voice_speaking_payload_matches_expected_shape() {
        let on: Value = serde_json::from_str(&voice_speaking_payload(1234, true))
            .expect("speaking-on payload should be JSON");
        assert_eq!(on["op"].as_u64(), Some(u64::from(VOICE_OP_SPEAKING)));
        assert_eq!(on["d"]["speaking"].as_u64(), Some(1));
        assert_eq!(on["d"]["delay"].as_u64(), Some(0));
        assert_eq!(on["d"]["ssrc"].as_u64(), Some(1234));

        let off: Value = serde_json::from_str(&voice_speaking_payload(1234, false))
            .expect("speaking-off payload should be JSON");
        assert_eq!(off["d"]["speaking"].as_u64(), Some(0));
    }

    #[test]
    fn fake_outbound_noops_when_capture_gate_is_closed() {
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
        let rtp = state.rtp;

        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect("send should no-op"),
            VoiceFakeSendOutcome::Noop
        );
        assert!(state.events().is_empty());
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, 10);

        state.set_capture_gate(true, true);
        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect("muted send should no-op"),
            VoiceFakeSendOutcome::Noop
        );
        assert!(state.events().is_empty());
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, 10);
    }

    #[test]
    fn fake_outbound_blocks_dave_active_plaintext_fallback() {
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
        state.set_capture_gate(true, false);
        state.set_dave_active(true);
        let rtp = state.rtp;

        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect("DAVE block should be reported"),
            VoiceFakeSendOutcome::Blocked(VoiceFakeSendBlockReason::DaveOutboundUnsupported)
        );
        assert!(state.events().is_empty());
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, 10);
    }

    #[test]
    fn fake_outbound_uses_dave_outbound_policy_before_transport_encrypt() {
        let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
        state.set_capture_gate(true, false);

        assert_eq!(
            state
                .send_opus_frame_with_dave(b"opus-frame", &mut dave)
                .expect("DAVE inactive frame should send"),
            VoiceFakeSendOutcome::Sent
        );
        assert_fake_packet(
            AEAD_AES256_GCM_RTPSIZE,
            &state.events()[1],
            7,
            960,
            42,
            b"opus-frame",
            30u32.to_be_bytes(),
        );

        let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
        dave.reinit(1).expect("DAVE session should initialize");
        let mut blocked = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
        blocked.set_capture_gate(true, false);
        let rtp = blocked.rtp;

        assert_eq!(
            blocked
                .send_opus_frame_with_dave(b"opus-frame", &mut dave)
                .expect("DAVE not-ready frame should block"),
            VoiceFakeSendOutcome::Blocked(VoiceFakeSendBlockReason::DaveOutboundNotReady)
        );
        assert!(blocked.events().is_empty());
        assert_eq!(blocked.rtp, rtp);
        assert_eq!(blocked.nonce_suffix, 30);
    }

    #[test]
    fn fake_outbound_sends_encrypted_packets_without_live_io() {
        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let mut state = fake_outbound_state(mode, 0x01020304);
            state.set_capture_gate(true, false);

            assert_eq!(
                state.send_opus_frame(b"opus-frame").expect("first frame should send"),
                VoiceFakeSendOutcome::Sent
            );
            assert_eq!(state.events().len(), 2);
            assert_eq!(
                state.events()[0],
                VoiceFakeOutboundEvent::Speaking {
                    speaking: true,
                    ssrc: 42,
                }
            );
            assert_fake_packet(mode, &state.events()[1], 7, 960, 42, b"opus-frame", [1, 2, 3, 4]);
            assert_eq!(state.rtp.sequence, 8);
            assert_eq!(state.rtp.timestamp, 1920);
            assert_eq!(state.nonce_suffix, 0x01020305);

            assert_eq!(
                state.send_opus_frame(b"next-frame").expect("second frame should send"),
                VoiceFakeSendOutcome::Sent
            );
            assert_eq!(state.events().len(), 3);
            assert_fake_packet(mode, &state.events()[2], 8, 1920, 42, b"next-frame", [1, 2, 3, 5]);
            assert_eq!(state.rtp.sequence, 9);
            assert_eq!(state.rtp.timestamp, 2880);
            assert_eq!(state.nonce_suffix, 0x01020306);
        }
    }

    #[test]
    fn fake_outbound_stop_sends_finite_silence_then_speaking_off() {
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
        state.set_capture_gate(true, false);

        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect("frame should send"),
            VoiceFakeSendOutcome::Sent
        );
        assert_eq!(
            state.stop_speaking().expect("stop should send silence"),
            VoiceFakeSendOutcome::Sent
        );

        assert_eq!(state.events().len(), DISCORD_TRAILING_SILENCE_FRAMES + 3);
        for index in 0..DISCORD_TRAILING_SILENCE_FRAMES {
            assert_fake_packet(
                AEAD_AES256_GCM_RTPSIZE,
                &state.events()[index + 2],
                8 + index as u16,
                1920 + index as u32 * DISCORD_OPUS_TIMESTAMP_INCREMENT,
                42,
                &DISCORD_OPUS_SILENCE_FRAME,
                (21 + index as u32).to_be_bytes(),
            );
        }
        assert_eq!(
            state.events()[DISCORD_TRAILING_SILENCE_FRAMES + 2],
            VoiceFakeOutboundEvent::Speaking {
                speaking: false,
                ssrc: 42,
            }
        );
        assert_eq!(state.rtp.sequence, 13);
        assert_eq!(state.rtp.timestamp, 6720);
        assert_eq!(state.nonce_suffix, 26);
    }

    #[test]
    fn fake_outbound_stop_sends_speaking_off_when_capture_gate_closes() {
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
        state.set_capture_gate(true, false);
        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect("frame should send"),
            VoiceFakeSendOutcome::Sent
        );
        let event_count = state.events().len();
        let rtp = state.rtp;
        let nonce_suffix = state.nonce_suffix;

        state.set_capture_gate(true, true);
        assert_eq!(
            state.stop_speaking().expect("muted stop should send speaking off"),
            VoiceFakeSendOutcome::Sent
        );
        assert_eq!(state.events().len(), event_count + 1);
        assert_eq!(
            state.events()[event_count],
            VoiceFakeOutboundEvent::Speaking {
                speaking: false,
                ssrc: 42,
            }
        );
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, nonce_suffix);

        state.speaking = true;
        state.set_capture_gate(false, false);
        assert_eq!(
            state.stop_speaking().expect("disallowed stop should send speaking off"),
            VoiceFakeSendOutcome::Sent
        );
        assert_eq!(state.events().len(), event_count + 2);
        assert_eq!(
            state.events()[event_count + 1],
            VoiceFakeOutboundEvent::Speaking {
                speaking: false,
                ssrc: 42,
            }
        );
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, nonce_suffix);
    }

    #[test]
    fn fake_outbound_stop_uses_dave_policy_for_silence_frames() {
        let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
        dave.reinit(1).expect("DAVE session should initialize");
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
        state.set_capture_gate(true, false);
        state.speaking = true;
        let rtp = state.rtp;

        assert_eq!(
            state
                .stop_speaking_with_dave(&mut dave)
                .expect("DAVE not-ready silence should still send speaking off"),
            VoiceFakeSendOutcome::Sent
        );
        assert_eq!(
            state.events(),
            &[VoiceFakeOutboundEvent::Speaking {
                speaking: false,
                ssrc: 42,
            }]
        );
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, 20);
        assert!(!state.speaking);
    }

    #[test]
    fn fake_outbound_nonce_exhaustion_fails_without_state_change() {
        let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX);
        state.set_capture_gate(true, false);
        let rtp = state.rtp;

        assert_eq!(
            state.send_opus_frame(b"opus-frame").expect_err("exhausted nonce should fail"),
            "voice RTP nonce suffix exhausted"
        );
        assert!(state.events().is_empty());
        assert_eq!(state.rtp, rtp);
        assert_eq!(state.nonce_suffix, u32::MAX);

        let mut stopping = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX - 2);
        stopping.set_capture_gate(true, false);
        stopping.speaking = true;
        let rtp = stopping.rtp;
        assert_eq!(
            stopping.stop_speaking().expect("stop should still clear speaking"),
            VoiceFakeSendOutcome::Sent
        );
        assert_eq!(
            stopping.events(),
            &[VoiceFakeOutboundEvent::Speaking {
                speaking: false,
                ssrc: 42,
            }]
        );
        assert_eq!(stopping.rtp, rtp);
        assert_eq!(stopping.nonce_suffix, u32::MAX - 2);
        assert!(!stopping.speaking);
    }

    #[test]
    fn rtp_header_rejects_malformed_packets() {
        assert_eq!(
            parse_rtp_header(&[0; 11]).expect_err("short packet should fail"),
            "RTP packet is too short"
        );

        let packet = [0x40, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

        assert_eq!(
            parse_rtp_header(&packet).expect_err("wrong version should fail"),
            "RTP packet has unsupported version"
        );
    }

    fn fake_outbound_state(mode: &str, nonce_suffix: u32) -> VoiceFakeOutboundSendState {
        VoiceFakeOutboundSendState::new(
            mode,
            &[9u8; 32],
            VoiceOutboundRtpState {
                sequence: 7,
                timestamp: 960,
                ssrc: 42,
            },
            nonce_suffix,
        )
        .expect("fake outbound state should build")
    }

    fn test_voice_gateway_session() -> VoiceGatewaySession {
        VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "voice-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "voice-token".to_owned(),
        }
    }

    fn assert_fake_packet(
        mode: &str,
        event: &VoiceFakeOutboundEvent,
        sequence: u16,
        timestamp: u32,
        ssrc: u32,
        expected_payload: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) {
        let VoiceFakeOutboundEvent::Packet { bytes } = event else {
            panic!("expected fake packet event, got {event:?}");
        };
        let packet_bytes = bytes.as_slice();
        let header = parse_rtp_header(packet_bytes).expect("fake RTP header should parse");
        let decryptor = VoiceRtpDecryptor::new(mode, &[9u8; 32]).expect("decryptor should build");
        let decrypted = decryptor
            .decrypt_packet(packet_bytes, &header)
            .expect("fake RTP packet should decrypt");

        let actual_nonce_suffix = &packet_bytes[packet_bytes.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..];
        assert_eq!(actual_nonce_suffix, nonce_suffix.as_slice());
        assert_eq!(header.sequence, sequence);
        assert_eq!(header.timestamp, timestamp);
        assert_eq!(header.ssrc, ssrc);
        assert_eq!(decrypted.media_payload, expected_payload);
    }

    fn encrypt_test_rtp_payload(
        mode: &str,
        key: &[u8],
        aad: &[u8],
        plaintext: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Vec<u8> {
        match mode {
            AEAD_AES256_GCM_RTPSIZE => {
                let cipher = Aes256Gcm::new_from_slice(key).expect("test key is valid");
                let mut nonce = [0u8; 12];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        AesGcmNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .expect("test payload encrypts")
            }
            AEAD_XCHACHA20_POLY1305_RTPSIZE => {
                let cipher = XChaCha20Poly1305::new_from_slice(key).expect("test key is valid");
                let mut nonce = [0u8; 24];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .expect("test payload encrypts")
            }
            other => panic!("unsupported test mode: {other}"),
        }
    }
}
