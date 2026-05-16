use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    num::NonZeroU16,
    sync::Arc,
    time::{Duration, Instant},
};

use davey::{DaveSession, ProposalsOperationType};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::{
    CurrentVoiceConnectionState, VoiceServerInfo, VoiceStateInfo,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, UserMarker},
    },
};
use crate::logging;

use super::events::AppEvent;

const VOICE_GATEWAY_VERSION: u8 = 9;
const VOICE_WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_DISCOVERY_PACKET_LEN: usize = 74;
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const RTP_HEADER_MIN_LEN: usize = 12;
const RTP_VERSION: u8 = 2;
const RTP_HEADER_EXTENSION_BYTES: usize = 4;
const RTP_EXTENSION_WORD_BYTES: usize = 4;
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
    Shutdown,
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
    payload_offset: usize,
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
                if let (Some(ssrc), Some(user_id)) = (speaking.ssrc, speaking.user_id) {
                    self.ssrc_user_ids.insert(ssrc, user_id);
                    self.known_user_ids.insert(user_id);
                }
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

pub(crate) async fn run_voice_runtime(mut events: mpsc::UnboundedReceiver<VoiceRuntimeEvent>) {
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
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(session)));
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

async fn run_voice_gateway_session(session: VoiceGatewaySession) {
    if let Err(error) = connect_voice_gateway(session).await {
        logging::error("voice", error);
    }
}

async fn connect_voice_gateway(session: VoiceGatewaySession) -> Result<(), String> {
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
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut heartbeat_task: Option<JoinHandle<()>> = None;
    let mut udp_receive_task: Option<JoinHandle<()>> = None;
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let mut dave_state = VoiceDaveState::new(&session);

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
                        if let Some(task) = udp_receive_task.take() {
                            task.abort();
                        }
                        if let Some(dave_protocol_version) = description.dave_protocol_version {
                            let dave_protocol_version = u16::try_from(dave_protocol_version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.reinit(dave_protocol_version)?;
                        }
                        if let Some(socket) = udp_socket.as_ref() {
                            logging::debug("voice", "starting voice UDP receive task");
                            udp_receive_task = Some(tokio::spawn(run_voice_udp_receive(
                                Arc::clone(socket),
                                description.mode,
                            )));
                        }
                    }
                    VOICE_OP_HEARTBEAT_ACK => {}
                    VOICE_OP_HELLO => {
                        if let Some(task) = heartbeat_task.take() {
                            logging::debug("voice", "replacing voice heartbeat task");
                            task.abort();
                        }
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
                        heartbeat_task = Some(tokio::spawn(run_voice_heartbeat(
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
                        dave_state.handle_json_op(&writer, opcode, &value).await?;
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
                dave_state.handle_binary_frame(&writer, frame).await?;
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }

    if let Some(task) = heartbeat_task {
        logging::debug("voice", "aborting voice heartbeat task");
        task.abort();
    }
    if let Some(task) = udp_receive_task {
        logging::debug("voice", "aborting voice UDP receive task");
        task.abort();
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

async fn run_voice_udp_receive(socket: Arc<UdpSocket>, mode: String) {
    logging::debug(
        "voice",
        format!("voice UDP receive skeleton active: mode={mode}"),
    );
    let mut packet = vec![0u8; 2048];
    let mut rtp_packets = 0u64;
    let mut malformed_packets = 0u64;
    loop {
        match socket.recv(&mut packet).await {
            Ok(len) => match parse_rtp_header(&packet[..len]) {
                Ok(header) => {
                    rtp_packets = rtp_packets.saturating_add(1);
                    if rtp_packets == 1 || rtp_packets % 500 == 0 {
                        logging::debug(
                            "voice",
                            format!(
                                "received RTP packet: count={} ssrc={} seq={} timestamp={} payload_type={} payload_offset={}",
                                rtp_packets,
                                header.ssrc,
                                header.sequence,
                                header.timestamp,
                                header.payload_type,
                                header.payload_offset
                            ),
                        );
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
    let mut payload_offset = RTP_HEADER_MIN_LEN + csrc_count * 4;
    if packet.len() < payload_offset {
        return Err("RTP packet is shorter than CSRC list".to_owned());
    }
    if has_extension {
        if packet.len() < payload_offset + RTP_HEADER_EXTENSION_BYTES {
            return Err("RTP packet is shorter than extension header".to_owned());
        }
        let extension_words =
            u16::from_be_bytes([packet[payload_offset + 2], packet[payload_offset + 3]]);
        payload_offset +=
            RTP_HEADER_EXTENSION_BYTES + usize::from(extension_words) * RTP_EXTENSION_WORD_BYTES;
        if packet.len() < payload_offset {
            return Err("RTP packet is shorter than extension body".to_owned());
        }
    }

    Ok(RtpHeader {
        payload_type: packet[1] & 0x7f,
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
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
                payload_offset: 12,
            }
        );

        let mut extended = vec![0x91, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
        extended.extend_from_slice(&0x11223344u32.to_be_bytes());
        extended.extend_from_slice(&0x1000u16.to_be_bytes());
        extended.extend_from_slice(&1u16.to_be_bytes());
        extended.extend_from_slice(&0x55667788u32.to_be_bytes());

        let header = parse_rtp_header(&extended).expect("extended RTP header should parse");

        assert_eq!(header.payload_offset, 24);
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
}
