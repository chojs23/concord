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
use opus::{Channels, Decoder as OpusDecoder};
use serde_json::{Value, json};
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
const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;
const VOICE_PLAYBACK_FRAME_QUEUE: usize = 256;
#[cfg(feature = "voice-playback")]
const VOICE_AUDIO_OUTPUT_QUEUE: usize = 64;
const AEAD_AES256_GCM_RTPSIZE: &str = "aead_aes256_gcm_rtpsize";
const AEAD_XCHACHA20_POLY1305_RTPSIZE: &str = "aead_xchacha20_poly1305_rtpsize";

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

struct DecryptedRtpPayload {
    media_payload: Vec<u8>,
    encrypted_extension_body_len: usize,
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
    opus_decode: Option<JoinHandle<()>>,
    #[cfg(feature = "voice-playback")]
    audio_output: Option<VoiceAudioOutput>,
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

    fn replace_opus_decode(&mut self, opus_decode: VoiceOpusDecode) {
        if let Some(task) = self.opus_decode.take() {
            logging::debug("voice", "aborting previous voice Opus decode task");
            task.abort();
        }
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = opus_decode.audio_output;
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
        if let Some(task) = self.opus_decode.take() {
            logging::debug("voice", "aborting voice Opus decode task");
            task.abort();
        }
        #[cfg(feature = "voice-playback")]
        {
            self.audio_output = None;
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
    fn start() -> Self {
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
    fn start() -> Self {
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        match VoiceAudioOutput::start() {
            Ok(audio_output) => {
                let decoded_audio = VoiceDecodedAudio::output(audio_output.samples_tx.clone());
                let task = tokio::spawn(run_voice_playback_decode(frames_rx, decoded_audio));
                logging::debug("voice", "voice Opus playback worker started with audio output");
                Self {
                    frames_tx,
                    task,
                    audio_output: Some(audio_output),
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
    fn start() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal();

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    fn start_with_cpal() -> Result<Self, String> {

        let (samples_tx, samples_rx) = sync_channel(VOICE_AUDIO_OUTPUT_QUEUE);
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device is available".to_owned())?;
        let supported_config = select_voice_output_config(&device)?;
        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();
        let stream = build_voice_output_stream(&device, &stream_config, sample_format, samples_rx)?;
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
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_voice_output_stream_f32(device, config, samples_rx),
        cpal::SampleFormat::U8 => build_voice_output_stream_u8(device, config, samples_rx),
        cpal::SampleFormat::I16 => build_voice_output_stream_i16(device, config, samples_rx),
        cpal::SampleFormat::U16 => build_voice_output_stream_u16(device, config, samples_rx),
        other => Err(format!("unsupported voice audio output sample format: {other:?}")),
    }
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_rx: StdReceiver<Vec<f32>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [f32], _| fill_voice_output_f32(output, channels, &mut buffer),
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
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [u8], _| fill_voice_output_u8(output, channels, &mut buffer),
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
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [i16], _| fill_voice_output_i16(output, channels, &mut buffer),
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
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let mut buffer = VoiceAudioBuffer::new(samples_rx);
    device
        .build_output_stream(
            config,
            move |output: &mut [u16], _| fill_voice_output_u16(output, channels, &mut buffer),
            log_voice_output_stream_error,
            None,
        )
        .map_err(|error| format!("voice u16 audio output stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_f32(output: &mut [f32], channels: usize, buffer: &mut VoiceAudioBuffer) {
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, clamp_voice_sample);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_u8(output: &mut [u8], channels: usize, buffer: &mut VoiceAudioBuffer) {
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_u8);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_i16(output: &mut [i16], channels: usize, buffer: &mut VoiceAudioBuffer) {
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_i16);
    }
}

#[cfg(feature = "voice-playback")]
fn fill_voice_output_u16(output: &mut [u16], channels: usize, buffer: &mut VoiceAudioBuffer) {
    for frame in output.chunks_mut(channels) {
        let [left, right] = buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
        write_voice_output_frame(frame, left, right, voice_sample_to_u16);
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
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(
                        session,
                        events_tx.clone(),
                        status_publisher.clone(),
                    )));
                }
                VoiceRuntimeAction::Close => {
                    if let Some(task) = connection_task.take() {
                        logging::debug("voice", "aborting active voice connection task");
                        task.abort();
                    }
                }
            }
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
) {
    match connect_voice_gateway(&session, &status_publisher).await {
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
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let dave_state = Arc::new(Mutex::new(VoiceDaveState::new(session)));

    send_voice_text(&writer, voice_identify_payload(&session)).await?;
    logging::debug("voice", "voice identify sent");
    logging::debug("voice", "voice websocket read loop started");

    while let Some(frame) = reader.next().await {
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
                            let opus_decode = VoiceOpusDecode::start();
                            let playback_tx = Some(opus_decode.frames_tx.clone());
                            child_tasks.replace_opus_decode(opus_decode);
                            child_tasks.replace_udp_receive(tokio::spawn(run_voice_udp_receive(
                                Arc::clone(socket),
                                description,
                                Arc::clone(&dave_state),
                                playback_tx,
                            )));
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
                    | VOICE_OP_SPEAKING
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
                            let media = dave_state
                                .lock()
                                .await
                                .unwrap_media_payload_for_ssrc(header.ssrc, &payload.media_payload);
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
        assert!(state.known_user_ids.contains(&30));
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
