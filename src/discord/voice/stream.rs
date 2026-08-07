use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
};

use rand::random;
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};
use uuid::Uuid;

use crate::support::media_player::MediaPlayerIpcEndpoint;

use super::media::{
    GatewayChildTasks, annex_b_nals, build_rtcp_sender_report, current_unix_time,
    packetize_h264_payloads,
};
use super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS;
use super::*;

const STREAM_RTP_PACKET_BYTES: usize = 4096;
const LOCAL_H264_MAX_PAYLOAD_BYTES: usize = 1200;
const STREAM_STARTUP_BUFFER_MAX_FRAMES: usize = 180;
const STREAM_STARTUP_BUFFER_MAX_BYTES: usize = 32 * 1024 * 1024;
const STREAM_H264_ACCESS_UNIT_MAX_BYTES: usize = 8 * 1024 * 1024;
const STREAM_H264_ACCESS_UNIT_MAX_PACKETS: usize = 4096;
const STREAM_STARTUP_REPLAY_FRAME_TICKS: u32 = 90;
const STREAM_PLAYER_READY_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT: Duration = Duration::from_secs(1);
const STREAM_PLAYER_INPUT_CONFIG: &str = "SPACE ignore\np ignore\nPAUSE ignore\nPLAYPAUSE ignore\nPAUSEONLY ignore\nXF86_PAUSE ignore\n. ignore\n, ignore\n";
const OPUS_RTP_CLOCK_RATE: u32 = 48_000;
const VIDEO_RTP_CLOCK_RATE: u32 = 90_000;
const STREAM_PRESENTATION_CLOCK_MAX_CORRECTION: Duration = Duration::from_millis(250);
// Allow normal packet jitter and short Opus duration changes, but do not carry
// multi-second source clock resets into mpv's local playback timeline.
const STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS: u32 = DISCORD_OPUS_TIMESTAMP_INCREMENT * 6;
const STREAM_AUDIO_REORDER_DELAY: Duration = Duration::from_millis(100);
const STREAM_AUDIO_REORDER_INTERVAL: Duration = Duration::from_millis(20);
const STREAM_AUDIO_MAX_PENDING_PACKETS: usize = 64;
const STREAM_KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const STREAM_VIDEO_NACK_INTERVAL: Duration = Duration::from_millis(100);
const STREAM_VIDEO_GAP_TIMEOUT: Duration = Duration::from_millis(500);
const STREAM_VIDEO_MAX_PENDING_PACKETS: usize = 2_048;
const STREAM_VIDEO_MAX_PENDING_BYTES: usize = 4 * 1024 * 1024;
const STREAM_VIDEO_MAX_NACK_SEQUENCES: usize = 64;
const STREAM_RTCP_RECEIVER_REPORT_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_TRANSPORT_FEEDBACK_INTERVAL: Duration = Duration::from_millis(50);
const STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES: usize = 512;
const LOCAL_RTCP_REPORT_INTERVAL: Duration = Duration::from_secs(1);
const STREAM_CONNECTION_STABLE_INTERVAL: Duration = Duration::from_secs(10);
const STREAM_WATCH_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(250);
const STREAM_WATCH_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);
const RTCP_SENDER_REPORT: u8 = 200;
const RTCP_TRANSPORT_LAYER_FEEDBACK: u8 = 205;
const RTCP_GENERIC_NACK_FORMAT: u8 = 1;
const RTCP_TRANSPORT_WIDE_FEEDBACK_FORMAT: u8 = 15;
const RTCP_RECEIVER_REPORT: u8 = 201;
const RTCP_SOURCE_DESCRIPTION: u8 = 202;
const RTCP_SDES_CNAME: u8 = 1;
const RTCP_PAYLOAD_SPECIFIC_FEEDBACK: u8 = 206;
const RTCP_PLI_FORMAT: u8 = 1;
const RTCP_PLI_LENGTH_WORDS_MINUS_ONE: u16 = 2;
const RTP_ONE_BYTE_EXTENSION_PROFILE: u16 = 0xbede;
const DISCORD_TRANSPORT_SEQUENCE_EXTENSION_ID: u8 = 5;
const TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS: i64 = 64_000;
const TRANSPORT_FEEDBACK_DELTA_MICROS: i64 = 250;
const TRANSPORT_PACKET_NOT_RECEIVED: u8 = 0;
const TRANSPORT_PACKET_RECEIVED_SMALL_DELTA: u8 = 1;
const TRANSPORT_PACKET_RECEIVED_LARGE_DELTA: u8 = 2;

#[derive(Clone, Eq, PartialEq)]
pub(super) struct StreamGatewaySession {
    pub(super) connection_id: u64,
    pub(super) request: StreamWatchRequest,
    pub(super) current_user_id: Id<UserMarker>,
    pub(super) session_id: String,
    pub(super) rtc_server_id: String,
    pub(super) rtc_channel_id: Id<ChannelMarker>,
    pub(super) endpoint: String,
    pub(super) token: String,
    reconnect_delay: Duration,
}

impl std::fmt::Debug for StreamGatewaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamGatewaySession")
            .field("connection_id", &self.connection_id)
            .field("request", &self.request)
            .field("current_user_id", &self.current_user_id)
            .field("session_id", &self.session_id)
            .field("rtc_server_id", &self.rtc_server_id)
            .field("rtc_channel_id", &self.rtc_channel_id)
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .field("reconnect_delay", &self.reconnect_delay)
            .finish()
    }
}

struct StreamPlayerLogTasks {
    tasks: Vec<JoinHandle<()>>,
}

impl StreamPlayerLogTasks {
    fn new(tasks: [JoinHandle<()>; 2]) -> Self {
        Self {
            tasks: Vec::from(tasks),
        }
    }

    async fn finish(mut self) {
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
    }
}

impl Drop for StreamPlayerLogTasks {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedStreamVoiceState {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

#[derive(Clone)]
struct StreamPlayerReadySignal {
    player_ready: Arc<AtomicBool>,
    ready_tx: mpsc::UnboundedSender<u64>,
    media_generation: u64,
    display_name: String,
}

#[derive(Debug, Eq, PartialEq)]
struct StreamConnectionFailure {
    message: String,
    outcome: VoiceConnectionEnd,
}

impl StreamConnectionFailure {
    fn reconnect(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: VoiceConnectionEnd::Reconnect,
        }
    }

    fn stop(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: VoiceConnectionEnd::Stop,
        }
    }
}

impl From<String> for StreamConnectionFailure {
    fn from(message: String) -> Self {
        Self::reconnect(message)
    }
}

#[derive(Default)]
pub(super) struct StreamRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    current_voice: Option<ObservedStreamVoiceState>,
    requested: Option<StreamWatchRequest>,
    create: Option<StreamCreateInfo>,
    server: Option<StreamServerInfo>,
    active: Option<StreamGatewaySession>,
    reconnect_attempts: u8,
    next_connection_id: u64,
}

#[derive(Default)]
pub(super) struct StreamRuntimeUpdate {
    pub(super) close_stream_key: Option<String>,
    pub(super) send_delete: bool,
    pub(super) connect: Option<StreamGatewaySession>,
    pub(super) playback_ended: Option<StreamPlaybackEnded>,
    pub(super) error: Option<String>,
}

pub(super) struct StreamPlaybackEnded {
    pub(super) request: StreamWatchRequest,
    pub(super) reconnecting: bool,
}

impl StreamRuntimeState {
    pub(super) fn apply(&mut self, event: &VoiceRuntimeEvent) -> StreamRuntimeUpdate {
        let mut update = StreamRuntimeUpdate::default();
        match event {
            VoiceRuntimeEvent::CurrentUserReady(user_id) => self.current_user_id = *user_id,
            VoiceRuntimeEvent::VoiceState(state) => self.record_voice_state(state, &mut update),
            VoiceRuntimeEvent::WatchStreamRequested(request) => {
                if self
                    .requested
                    .as_ref()
                    .is_none_or(|current| current.stream_key != request.stream_key)
                {
                    update.playback_ended =
                        self.requested.take().map(|request| StreamPlaybackEnded {
                            request,
                            reconnecting: false,
                        });
                    update.close_stream_key =
                        self.active.take().map(|active| active.request.stream_key);
                    update.send_delete = update.close_stream_key.is_some();
                    self.create = None;
                    self.server = None;
                    self.reconnect_attempts = 0;
                }
                self.requested = Some(request.clone());
            }
            VoiceRuntimeEvent::WatchStreamCancelled { stream_key } => {
                self.clear_matching(stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::StreamCreate(stream) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == stream.stream_key)
                {
                    self.create = Some(stream.clone());
                }
            }
            VoiceRuntimeEvent::StreamServer(server) => {
                if self
                    .requested
                    .as_ref()
                    .is_some_and(|request| request.stream_key == server.stream_key)
                {
                    if self.active.as_ref().is_some_and(|active| {
                        !server.matches_connection(&active.endpoint, &active.token)
                    }) {
                        update.playback_ended =
                            self.requested
                                .as_ref()
                                .cloned()
                                .map(|request| StreamPlaybackEnded {
                                    request,
                                    reconnecting: true,
                                });
                        update.close_stream_key =
                            self.active.take().map(|active| active.request.stream_key);
                    }
                    self.server = Some(server.clone());
                }
            }
            VoiceRuntimeEvent::StreamDelete(stream) => {
                if let Some(request) = self
                    .requested
                    .as_ref()
                    .filter(|request| request.stream_key == stream.stream_key)
                    && (!stream.reason.is_empty() || stream.unavailable)
                {
                    let reason = if stream.reason.is_empty() {
                        "stream unavailable"
                    } else {
                        stream.reason.as_str()
                    };
                    update.error = Some(format!(
                        "Could not watch {}'s stream: {reason}",
                        request.display_name
                    ));
                }
                self.clear_matching(&stream.stream_key, &mut update, false);
            }
            VoiceRuntimeEvent::StreamConnectionEstablished {
                connection_id,
                stream_key,
            } => {
                if self.active.as_ref().is_some_and(|active| {
                    active.connection_id == *connection_id
                        && active.request.stream_key == *stream_key
                }) {
                    self.reconnect_attempts = 0;
                }
            }
            VoiceRuntimeEvent::StreamConnectionEnded {
                connection_id,
                stream_key,
                outcome,
            } => {
                if self.active.as_ref().is_some_and(|active| {
                    active.connection_id == *connection_id
                        && active.request.stream_key == *stream_key
                }) {
                    self.active = None;
                    if *outcome == VoiceConnectionEnd::Stop
                        || self.reconnect_attempts >= MAX_VOICE_RECONNECT_ATTEMPTS
                    {
                        update.playback_ended =
                            self.requested.take().map(|request| StreamPlaybackEnded {
                                request,
                                reconnecting: false,
                            });
                        self.create = None;
                        self.server = None;
                        self.reconnect_attempts = 0;
                        update.close_stream_key = Some(stream_key.clone());
                        update.send_delete = true;
                    } else {
                        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
                        update.playback_ended =
                            self.requested
                                .as_ref()
                                .cloned()
                                .map(|request| StreamPlaybackEnded {
                                    request,
                                    reconnecting: true,
                                });
                    }
                }
            }
            VoiceRuntimeEvent::Shutdown => {
                update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                    request,
                    reconnecting: false,
                });
                update.close_stream_key =
                    self.active.take().map(|active| active.request.stream_key);
                update.send_delete = update.close_stream_key.is_some();
                self.create = None;
                self.server = None;
            }
            _ => {}
        }

        if self.active.is_none() {
            update.connect = self.connect_if_ready();
        }
        update
    }

    fn record_voice_state(&mut self, state: &VoiceStateInfo, update: &mut StreamRuntimeUpdate) {
        if self.current_user_id != Some(state.user_id) {
            return;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
            self.create = None;
            self.server = None;
            return;
        };
        let Some(scope) = state.scope() else {
            return;
        };
        let Some(session_id) = state
            .session_id
            .as_ref()
            .filter(|session_id| !session_id.is_empty())
        else {
            return;
        };
        self.current_voice = Some(ObservedStreamVoiceState {
            scope,
            channel_id,
            session_id: session_id.clone(),
        });
        if self
            .requested
            .as_ref()
            .is_some_and(|request| request.scope != scope || request.channel_id != channel_id)
        {
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
            update.close_stream_key = self.active.take().map(|active| active.request.stream_key);
            update.send_delete = update.close_stream_key.is_some();
            self.create = None;
            self.server = None;
        }
    }

    fn clear_matching(
        &mut self,
        stream_key: &str,
        update: &mut StreamRuntimeUpdate,
        send_delete: bool,
    ) {
        if self
            .requested
            .as_ref()
            .is_some_and(|request| request.stream_key == stream_key)
        {
            update.playback_ended = self.requested.take().map(|request| StreamPlaybackEnded {
                request,
                reconnecting: false,
            });
            self.create = None;
            self.server = None;
            self.reconnect_attempts = 0;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.request.stream_key == stream_key)
        {
            self.active = None;
            update.close_stream_key = Some(stream_key.to_owned());
            update.send_delete = send_delete;
        }
    }

    fn connect_if_ready(&mut self) -> Option<StreamGatewaySession> {
        let request = self.requested.as_ref()?;
        let current_voice = self.current_voice.as_ref()?;
        if request.scope != current_voice.scope || request.channel_id != current_voice.channel_id {
            return None;
        }
        let create = self.create.as_ref()?;
        let server = self.server.as_ref()?;
        if create.stream_key != request.stream_key || server.stream_key != request.stream_key {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }

        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        let session = StreamGatewaySession {
            connection_id: self.next_connection_id,
            request: request.clone(),
            current_user_id: self.current_user_id?,
            session_id: current_voice.session_id.clone(),
            rtc_server_id: create.rtc_server_id.clone(),
            rtc_channel_id: create.rtc_channel_id,
            endpoint,
            token: server.token.clone(),
            reconnect_delay: stream_watch_reconnect_delay(self.reconnect_attempts),
        };
        self.active = Some(session.clone());
        Some(session)
    }
}

fn stream_watch_reconnect_delay(reconnect_attempts: u8) -> Duration {
    if reconnect_attempts == 0 {
        return Duration::ZERO;
    }
    let multiplier = 1u32 << u32::from(reconnect_attempts.saturating_sub(1).min(3));
    let base_delay = STREAM_WATCH_RECONNECT_BASE_DELAY
        .saturating_mul(multiplier)
        .min(STREAM_WATCH_RECONNECT_MAX_DELAY);
    let jitter_limit_millis =
        u64::try_from((base_delay / 4).as_millis()).expect("bounded retry jitter fits u64");
    let jitter = Duration::from_millis(random::<u64>() % (jitter_limit_millis + 1));
    base_delay
        .saturating_add(jitter)
        .min(STREAM_WATCH_RECONNECT_MAX_DELAY)
}

pub(super) async fn run_stream_gateway_session(
    session: StreamGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
) {
    if !session.reconnect_delay.is_zero() {
        logging::debug(
            "stream",
            format!(
                "waiting {:?} before reconnecting stream watch",
                session.reconnect_delay
            ),
        );
        sleep(session.reconnect_delay).await;
    }
    let outcome = match connect_stream_gateway(&session, &events_tx, &status_publisher).await {
        Ok(outcome) => outcome,
        Err(error) => {
            logging::error("stream", &error.message);
            status_publisher
                .publish_error(format!(
                    "Could not watch {}'s stream: {}",
                    session.request.display_name, error.message
                ))
                .await;
            error.outcome
        }
    };
    let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEnded {
        connection_id: session.connection_id,
        stream_key: session.request.stream_key.clone(),
        outcome,
    });
}

async fn connect_stream_gateway(
    session: &StreamGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: &VoiceStatusPublisher,
) -> Result<VoiceConnectionEnd, StreamConnectionFailure> {
    let url = gateway::voice_gateway_url(&session.endpoint)?;
    logging::debug("stream", format!("connecting stream websocket: {url}"));
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
        .await
        .map_err(|_| "stream websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("stream websocket connect failed: {error}"))?;
    logging::debug(
        "stream",
        format!("stream websocket connected: status={}", response.status()),
    );
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut gateway_control = gateway::StreamVoiceGatewayControl::new(
        Arc::clone(&writer),
        session.current_user_id,
        &session.rtc_server_id,
    )?;
    let dave_state = gateway_control.dave_state();
    let mut child_tasks = GatewayChildTasks::default();
    let (media_finished_tx, mut media_finished_rx) =
        mpsc::unbounded_channel::<Result<(), StreamConnectionFailure>>();
    let (player_ready_tx, mut player_ready_rx) = mpsc::unbounded_channel::<u64>();
    let (video_source_tx, video_source_rx) = watch::channel(StreamVideoSource::default());
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let mut local_ssrc: Option<u32> = None;
    let mut current_description: Option<VoiceSessionDescription> = None;
    let mut media_generation = 0u64;
    let mut connection_stable_deadline: Option<Instant> = None;

    gateway::send_voice_text(&writer, stream_identify_payload(session)).await?;
    logging::debug("stream", "stream identify sent");

    let result = loop {
        let frame = tokio::select! {
            _ = gateway_control.heartbeat_timed_out() => {
                break Ok(VoiceConnectionEnd::Reconnect);
            }
            media_result = media_finished_rx.recv(), if child_tasks.has_media() => {
                match media_result {
                    Some(Ok(())) => break Ok(VoiceConnectionEnd::Stop),
                    Some(Err(error)) => break Err(error),
                    None => break Ok(VoiceConnectionEnd::Reconnect),
                }
            }
            ready_generation = player_ready_rx.recv(), if child_tasks.has_media() => {
                if stream_player_ready_is_current(ready_generation, media_generation) {
                    status_publisher
                        .publish_stream_playback_ready(
                            session.request.scope,
                            session.request.channel_id,
                            session.request.owner_id,
                        )
                        .await;
                    connection_stable_deadline =
                        Some(Instant::now() + STREAM_CONNECTION_STABLE_INTERVAL);
                }
                continue;
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                connection_stable_deadline.unwrap_or_else(Instant::now)
            )), if connection_stable_deadline.is_some() => {
                connection_stable_deadline = None;
                let _ = events_tx.send(VoiceRuntimeEvent::StreamConnectionEstablished {
                    connection_id: session.connection_id,
                    stream_key: session.request.stream_key.clone(),
                });
                continue;
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break Ok(VoiceConnectionEnd::Reconnect);
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                break Err(StreamConnectionFailure::reconnect(format!(
                    "stream websocket read failed: {error}"
                )));
            }
        };
        match gateway_control.frame_action(&frame).await? {
            gateway::StreamVoiceGatewayFrameAction::Payload => {}
            gateway::StreamVoiceGatewayFrameAction::Continue => continue,
            gateway::StreamVoiceGatewayFrameAction::End(outcome) => break Ok(outcome),
        }
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("stream websocket JSON parse failed: {error}"))?;
                gateway_control.record_sequence(&value).await;
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
                match opcode {
                    VOICE_OP_READY => {
                        let ready = gateway::parse_voice_ready_payload(&value)?;
                        let mode = gateway::choose_encryption_mode(&ready.modes)?;
                        let (socket, discovered) =
                            gateway::discover_voice_udp_address(&ready).await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_select_protocol_payload(&discovered, &mode),
                        )
                        .await?;
                        gateway::send_voice_text(
                            &writer,
                            stream_receive_only_video_payload(ready.ssrc),
                        )
                        .await?;
                        local_ssrc = Some(ready.ssrc);
                        udp_socket = Some(socket);
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = gateway::parse_voice_session_description(&value)?;
                        if description
                            .video_codec
                            .as_deref()
                            .is_some_and(|codec| !codec.eq_ignore_ascii_case("H264"))
                        {
                            break Err(StreamConnectionFailure::reconnect(format!(
                                "stream selected unsupported video codec: {}",
                                description.video_codec.as_deref().unwrap_or("none")
                            )));
                        }
                        dave_state
                            .lock()
                            .await
                            .apply_protocol_version(description.dave_protocol_version)?;
                        let Some(socket) = udp_socket.as_ref() else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before UDP ready",
                            ));
                        };
                        let Some(local_ssrc) = local_ssrc else {
                            break Err(StreamConnectionFailure::reconnect(
                                "stream session description arrived before local SSRC",
                            ));
                        };
                        if current_description.as_ref() == Some(&description) {
                            continue;
                        }
                        let socket_for_media = Arc::clone(socket);
                        let description_for_media = description.clone();
                        let dave_for_media = Arc::clone(&dave_state);
                        let source_for_media = video_source_rx.clone();
                        let finished = media_finished_tx.clone();
                        let owner_id = session.request.owner_id;
                        media_generation = media_generation.wrapping_add(1);
                        connection_stable_deadline = None;
                        let stream_player_ready = StreamPlayerReadySignal {
                            player_ready: Arc::new(AtomicBool::new(false)),
                            ready_tx: player_ready_tx.clone(),
                            media_generation,
                            display_name: session.request.display_name.clone(),
                        };
                        child_tasks
                            .replace_media(tokio::spawn(async move {
                                let result = run_stream_media(
                                    socket_for_media,
                                    description_for_media,
                                    dave_for_media,
                                    source_for_media,
                                    owner_id,
                                    local_ssrc,
                                    stream_player_ready,
                                )
                                .await;
                                let _ = finished.send(result);
                            }))
                            .await;
                        child_tasks
                            .replace_keepalive(tokio::spawn(gateway::run_voice_udp_keepalive(
                                Arc::clone(socket),
                            )))
                            .await;
                        current_description = Some(description);
                    }
                    VOICE_OP_VIDEO => {
                        if let Some(source) =
                            parse_stream_video_source(&value, session.request.owner_id)
                        {
                            logging::debug(
                                "stream",
                                format!(
                                    "stream video source selected: audio_ssrc={} video_ssrc={} rtx_ssrc={:?} pixel_count={:?}",
                                    source.audio_ssrc,
                                    source.video_ssrc,
                                    source.rtx_ssrc,
                                    source.pixel_count,
                                ),
                            );
                            {
                                let mut dave = dave_state.lock().await;
                                dave.record_ssrc_user(source.audio_ssrc, session.request.owner_id);
                                dave.record_ssrc_user(source.video_ssrc, session.request.owner_id);
                            }
                            gateway::send_voice_text(
                                &writer,
                                stream_media_sink_wants_payload(
                                    source.audio_ssrc,
                                    source.video_ssrc,
                                    source.pixel_count,
                                ),
                            )
                            .await?;
                            video_source_tx.send_replace(source);
                        }
                    }
                    other => {
                        if !gateway_control
                            .handle_json_op(other, &value, &mut child_tasks)
                            .await?
                        {
                            logging::debug(
                                "stream",
                                format!("unhandled stream gateway op={other}"),
                            );
                        }
                    }
                }
            }
            WsMessage::Binary(payload) => {
                gateway_control.handle_binary(&payload).await?;
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Close(_) | WsMessage::Frame(_) => {
                unreachable!("gateway control frames are handled first")
            }
        }
    };

    child_tasks.shutdown().await;
    result
}

fn stream_identify_payload(session: &StreamGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.rtc_server_id,
            "user_id": session.current_user_id.to_string(),
            "channel_id": session.rtc_channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "video": true,
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

fn stream_select_protocol_payload(discovered: &DiscoveredVoiceAddress, mode: &str) -> String {
    json!({
        "op": 1,
        "d": {
            "protocol": "udp",
            "data": {
                "address": discovered.address,
                "port": discovered.port,
                "mode": mode,
            },
            "codecs": [
                {
                    "name": "opus",
                    "type": "audio",
                    "priority": 1000,
                    "payload_type": DISCORD_VOICE_PAYLOAD_TYPE,
                    "encode": false,
                    "decode": true,
                },
                {
                    "name": "H264",
                    "type": "video",
                    "priority": 1000,
                    "payload_type": DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
                    "rtx_payload_type": DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE,
                    "encode": false,
                    "decode": true,
                },
            ],
            "rtc_connection_id": Uuid::new_v4().to_string(),
        },
    })
    .to_string()
}

fn stream_receive_only_video_payload(audio_ssrc: u32) -> String {
    json!({
        "op": VOICE_OP_VIDEO,
        "d": {
            "audio_ssrc": audio_ssrc,
            "video_ssrc": 0,
            "rtx_ssrc": 0,
            "streams": [],
        },
    })
    .to_string()
}

fn stream_media_sink_wants_payload(
    audio_ssrc: u32,
    video_ssrc: u32,
    video_pixel_count: Option<u64>,
) -> String {
    let mut wants = serde_json::Map::new();
    if audio_ssrc != 0 {
        wants.insert(audio_ssrc.to_string(), Value::from(100));
    }
    if video_ssrc != 0 {
        wants.insert(video_ssrc.to_string(), Value::from(100));
        if let Some(pixel_count) = video_pixel_count {
            let mut pixel_counts = serde_json::Map::new();
            pixel_counts.insert(video_ssrc.to_string(), Value::from(pixel_count));
            wants.insert("pixelCounts".to_owned(), Value::Object(pixel_counts));
        }
    }
    wants.insert("any".to_owned(), Value::from(0));
    json!({
        "op": VOICE_OP_MEDIA_SINK_WANTS,
        "d": Value::Object(wants),
    })
    .to_string()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StreamVideoSource {
    audio_ssrc: u32,
    video_ssrc: u32,
    rtx_ssrc: Option<u32>,
    pixel_count: Option<u64>,
}

#[derive(Debug, Eq, PartialEq)]
struct RecoveredStreamAudioPacket {
    marker: bool,
    sequence: u16,
    timestamp: u32,
    opus: Vec<u8>,
}

#[derive(Debug)]
struct PendingStreamAudioPacket {
    packet: RecoveredStreamAudioPacket,
    arrived_at: Instant,
}

#[derive(Default)]
struct StreamAudioRecovery {
    next_sequence: Option<u16>,
    pending: HashMap<u16, PendingStreamAudioPacket>,
    first_buffered_at: Option<Instant>,
    started: bool,
}

#[derive(Default)]
struct StreamAudioRecoveryUpdate {
    ready: Vec<RecoveredStreamAudioPacket>,
    skipped_sequences: u16,
    dropped_stale_packets: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct RecoveredStreamVideoPacket {
    header: RtpHeader,
    payload: Vec<u8>,
}

#[derive(Default)]
struct StreamVideoRecovery {
    next_sequence: Option<u16>,
    pending: HashMap<u16, RecoveredStreamVideoPacket>,
    pending_bytes: usize,
    gap_started_at: Option<Instant>,
    last_nack_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamVideoRecoveryReset {
    distance: u16,
    pending_packets: usize,
    pending_bytes: usize,
    gap_age: Option<Duration>,
}

#[derive(Default)]
struct StreamVideoRecoveryUpdate {
    ready: Vec<RecoveredStreamVideoPacket>,
    reset: Option<StreamVideoRecoveryReset>,
}

#[derive(Default)]
struct StreamPliThrottle {
    media_ssrc: Option<u32>,
    last_sent_at: Option<Instant>,
}

impl StreamPliThrottle {
    fn permit(&mut self, media_ssrc: u32, now: Instant) -> bool {
        if self.media_ssrc != Some(media_ssrc) {
            self.media_ssrc = Some(media_ssrc);
            self.last_sent_at = Some(now);
            return true;
        }
        if self.last_sent_at.is_some_and(|last| {
            now.saturating_duration_since(last) < STREAM_KEYFRAME_REQUEST_INTERVAL
        }) {
            return false;
        }
        self.last_sent_at = Some(now);
        true
    }

    async fn send_if_due(
        &mut self,
        control: &mut StreamRtcpControl,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        media_ssrc: u32,
        elapsed: Duration,
    ) -> Result<bool, String> {
        if !self.permit(media_ssrc, Instant::now()) {
            return Ok(false);
        }
        let feedback = build_rtcp_pli(sender_ssrc, media_ssrc);
        control
            .send_feedback(socket, encryptor, sender_ssrc, &feedback, "PLI", elapsed)
            .await?;
        Ok(true)
    }
}

#[derive(Clone, Copy, Default)]
struct StreamMediaCounters {
    audio_stale_packets: u64,
    audio_skipped_packets: u64,
    primary_video_packets: u64,
    rtx_video_packets: u64,
    h264_frames: u64,
    h264_bytes: u64,
    decoder_resets: u64,
    transport_feedbacks: u64,
    nacks: u64,
    plis: u64,
    suppressed_plis: u64,
}

impl StreamMediaCounters {
    fn observe_pli_request(&mut self, sent: bool) {
        if sent {
            self.plis = self.plis.wrapping_add(1);
        } else {
            self.suppressed_plis = self.suppressed_plis.wrapping_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamRtcpReportBlock {
    source_ssrc: u32,
    fraction_lost: u8,
    cumulative_lost: i32,
    extended_highest_sequence: u32,
    interarrival_jitter: u32,
    last_sender_report: u32,
    delay_since_last_sender_report: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamRtcpSenderReport {
    sender_ssrc: u32,
    ntp_timestamp: u64,
    rtp_timestamp: u32,
    packet_count: u32,
    octet_count: u32,
}

#[derive(Clone, Copy)]
struct StreamNtpOrigin {
    ntp_timestamp: u64,
    local_elapsed: Duration,
}

#[derive(Default)]
struct StreamPresentationClock {
    origin: Option<StreamNtpOrigin>,
    reports: HashMap<u32, StreamRtcpSenderReport>,
}

impl StreamPresentationClock {
    fn observe_sender_report(&mut self, report: StreamRtcpSenderReport, elapsed: Duration) {
        self.origin.get_or_insert(StreamNtpOrigin {
            ntp_timestamp: report.ntp_timestamp,
            local_elapsed: elapsed,
        });
        self.reports.insert(report.sender_ssrc, report);
    }

    // RTCP Sender Reports connect every media RTP clock to one NTP clock. Map
    // that common sender time onto the local elapsed timeline so audio and
    // video no longer acquire separate offsets from their packet arrival time.
    fn map_timestamp(&self, ssrc: u32, source_timestamp: u32, clock_rate: u32) -> Option<u32> {
        let origin = self.origin?;
        let report = self.reports.get(&ssrc)?;
        let ntp_delta = report.ntp_timestamp.wrapping_sub(origin.ntp_timestamp) as i64;
        let ntp_delta_ticks = i128::from(ntp_delta) * i128::from(clock_rate) / (1i128 << 32);
        let source_delta_ticks =
            i128::from(source_timestamp.wrapping_sub(report.rtp_timestamp) as i32);
        let local_origin_ticks =
            i128::from(elapsed_rtp_timestamp(origin.local_elapsed, clock_rate));
        let local_timestamp = local_origin_ticks + ntp_delta_ticks + source_delta_ticks;
        if local_timestamp < 0 {
            return None;
        }
        Some(local_timestamp.rem_euclid(1i128 << 32) as u32)
    }
}

fn parse_stream_rtcp_sender_reports(
    compound: &[u8],
) -> Result<Vec<StreamRtcpSenderReport>, String> {
    let mut reports = Vec::new();
    let mut offset = 0usize;
    while offset < compound.len() {
        let remaining = compound.len() - offset;
        if remaining < 4 {
            return Err("RTCP compound packet has a truncated header".to_owned());
        }
        if compound[offset] >> 6 != RTP_VERSION {
            return Err("RTCP packet has an invalid version".to_owned());
        }
        let length_words_minus_one =
            u16::from_be_bytes([compound[offset + 2], compound[offset + 3]]);
        let packet_len = (usize::from(length_words_minus_one) + 1)
            .checked_mul(4)
            .ok_or_else(|| "RTCP packet length overflowed".to_owned())?;
        let packet_end = offset
            .checked_add(packet_len)
            .filter(|end| *end <= compound.len())
            .ok_or_else(|| "RTCP packet length exceeds the compound packet".to_owned())?;

        if compound[offset + 1] == RTCP_SENDER_REPORT {
            let report_count = usize::from(compound[offset] & 0x1f);
            let minimum_len = 28 + report_count * 24;
            if packet_len < minimum_len {
                return Err("RTCP sender report is truncated".to_owned());
            }
            let sender_ssrc = rtcp_u32(compound, offset + 4);
            let ntp_seconds = rtcp_u32(compound, offset + 8);
            let ntp_fraction = rtcp_u32(compound, offset + 12);
            reports.push(StreamRtcpSenderReport {
                sender_ssrc,
                ntp_timestamp: (u64::from(ntp_seconds) << 32) | u64::from(ntp_fraction),
                rtp_timestamp: rtcp_u32(compound, offset + 16),
                packet_count: rtcp_u32(compound, offset + 20),
                octet_count: rtcp_u32(compound, offset + 24),
            });
        }
        offset = packet_end;
    }
    Ok(reports)
}

fn rtcp_u32(packet: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        packet[offset],
        packet[offset + 1],
        packet[offset + 2],
        packet[offset + 3],
    ])
}

#[derive(Clone, Copy)]
struct StreamRtcpJitterOrigin {
    arrival_timestamp: u32,
    source_timestamp: u32,
}

#[derive(Clone, Copy)]
struct StreamRtcpLastSenderReport {
    middle_ntp_timestamp: u32,
    received_at: Duration,
}

#[derive(Default)]
struct StreamRtcpControl {
    nonce: u32,
    source_ssrc: u32,
    base_extended_sequence: Option<u32>,
    highest_extended_sequence: Option<u32>,
    received_packets: u32,
    expected_prior: u32,
    received_prior: u32,
    jitter_origin: Option<StreamRtcpJitterOrigin>,
    previous_transit: Option<i64>,
    jitter_q4: i64,
    last_sender_report: Option<StreamRtcpLastSenderReport>,
}

impl StreamRtcpControl {
    fn set_source(&mut self, source_ssrc: u32) {
        if self.source_ssrc == source_ssrc {
            return;
        }
        let nonce = self.nonce;
        *self = Self {
            nonce,
            source_ssrc,
            ..Self::default()
        };
    }

    fn observe_rtp(&mut self, sequence: u16, timestamp: u32, arrival: Duration) {
        if self.source_ssrc == 0 {
            return;
        }
        let extended = extend_transport_sequence(sequence, self.highest_extended_sequence);
        self.base_extended_sequence.get_or_insert(extended);
        if self
            .highest_extended_sequence
            .is_none_or(|highest| extended > highest)
        {
            self.highest_extended_sequence = Some(extended);
        }
        self.received_packets = self.received_packets.wrapping_add(1);

        let arrival_timestamp = elapsed_rtp_timestamp(arrival, VIDEO_RTP_CLOCK_RATE);
        let origin = *self.jitter_origin.get_or_insert(StreamRtcpJitterOrigin {
            arrival_timestamp,
            source_timestamp: timestamp,
        });
        let arrival_delta = arrival_timestamp.wrapping_sub(origin.arrival_timestamp) as i32;
        let source_delta = timestamp.wrapping_sub(origin.source_timestamp) as i32;
        let transit = i64::from(arrival_delta) - i64::from(source_delta);
        if let Some(previous_transit) = self.previous_transit {
            let delta = transit.abs_diff(previous_transit) as i64;
            self.jitter_q4 += delta - ((self.jitter_q4 + 8) >> 4);
        }
        self.previous_transit = Some(transit);
    }

    fn observe_sender_report(&mut self, report: StreamRtcpSenderReport, received_at: Duration) {
        if report.sender_ssrc != self.source_ssrc {
            return;
        }
        self.last_sender_report = Some(StreamRtcpLastSenderReport {
            middle_ntp_timestamp: (report.ntp_timestamp >> 16) as u32,
            received_at,
        });
    }

    fn report_block(&mut self, now: Duration) -> Option<StreamRtcpReportBlock> {
        let base = self.base_extended_sequence?;
        let highest = self.highest_extended_sequence?;
        let expected = highest.wrapping_sub(base).wrapping_add(1);
        let expected_interval = expected.wrapping_sub(self.expected_prior);
        let received_interval = self.received_packets.wrapping_sub(self.received_prior);
        let lost_interval = i64::from(expected_interval) - i64::from(received_interval);
        let fraction_lost = if expected_interval == 0 || lost_interval <= 0 {
            0
        } else {
            u8::try_from(((lost_interval << 8) / i64::from(expected_interval)).min(255))
                .expect("RTCP fraction loss is bounded to u8")
        };
        self.expected_prior = expected;
        self.received_prior = self.received_packets;

        let cumulative_lost = (i64::from(expected) - i64::from(self.received_packets))
            .clamp(-0x80_0000, 0x7f_ffff) as i32;
        let (last_sender_report, delay_since_last_sender_report) = self
            .last_sender_report
            .map(|report| {
                (
                    report.middle_ntp_timestamp,
                    duration_to_rtcp_delay(now.saturating_sub(report.received_at)),
                )
            })
            .unwrap_or_default();
        Some(StreamRtcpReportBlock {
            source_ssrc: self.source_ssrc,
            fraction_lost,
            cumulative_lost,
            extended_highest_sequence: highest,
            interarrival_jitter: u32::try_from((self.jitter_q4.max(0) + 8) >> 4)
                .unwrap_or(u32::MAX),
            last_sender_report,
            delay_since_last_sender_report,
        })
    }

    async fn send_feedback(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        feedback: &[u8],
        kind: &str,
        elapsed: Duration,
    ) -> Result<(), String> {
        let report = self.report_block(elapsed);
        let packet = build_stream_rtcp_compound(sender_ssrc, report, Some(feedback));
        self.send_packet(socket, encryptor, &packet, kind).await
    }

    async fn send_report(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        sender_ssrc: u32,
        elapsed: Duration,
    ) -> Result<(), String> {
        let report = self.report_block(elapsed);
        let packet = build_stream_rtcp_compound(sender_ssrc, report, None);
        self.send_packet(socket, encryptor, &packet, "receiver report")
            .await
    }

    async fn send_packet(
        &mut self,
        socket: &UdpSocket,
        encryptor: &VoiceRtpEncryptor,
        packet: &[u8],
        kind: &str,
    ) -> Result<(), String> {
        let encrypted = encryptor.encrypt_rtcp_feedback(packet, self.nonce.to_be_bytes())?;
        self.nonce = self
            .nonce
            .checked_add(1)
            .ok_or_else(|| "stream RTCP nonce exhausted".to_owned())?;
        socket
            .send(&encrypted)
            .await
            .map_err(|error| format!("send stream RTCP {kind} failed: {error}"))?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamTransportReceiveDelta {
    Small(u8),
    Large(i16),
}

impl StreamTransportReceiveDelta {
    fn status(self) -> u8 {
        match self {
            Self::Small(_) => TRANSPORT_PACKET_RECEIVED_SMALL_DELTA,
            Self::Large(_) => TRANSPORT_PACKET_RECEIVED_LARGE_DELTA,
        }
    }
}

#[derive(Default)]
struct StreamTransportFeedback {
    arrivals: BTreeMap<u32, Duration>,
    highest_extended_sequence: Option<u32>,
    next_unreported_sequence: Option<u32>,
    last_reported_sequence: Option<u32>,
    feedback_packet_count: u8,
}

impl StreamTransportFeedback {
    fn reset(&mut self) {
        self.arrivals.clear();
        self.highest_extended_sequence = None;
        self.next_unreported_sequence = None;
        self.last_reported_sequence = None;
    }

    fn observe(&mut self, sequence: u16, arrival: Duration) {
        let extended = extend_transport_sequence(sequence, self.highest_extended_sequence);
        if self
            .last_reported_sequence
            .is_some_and(|reported| extended <= reported)
        {
            return;
        }

        if self.next_unreported_sequence.is_some_and(|base| {
            extended.saturating_sub(base) >= STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES as u32
        }) {
            self.arrivals.clear();
            self.next_unreported_sequence = Some(extended);
            self.last_reported_sequence = extended.checked_sub(1);
        } else {
            self.next_unreported_sequence = Some(
                self.next_unreported_sequence
                    .map_or(extended, |base| base.min(extended)),
            );
        }

        self.highest_extended_sequence = Some(
            self.highest_extended_sequence
                .map_or(extended, |highest| highest.max(extended)),
        );
        self.arrivals.entry(extended).or_insert(arrival);
    }

    fn take_feedback(&mut self, sender_ssrc: u32, media_ssrc: u32) -> Option<Vec<u8>> {
        let base = self.next_unreported_sequence?;
        let highest = self.highest_extended_sequence?;
        if base > highest {
            return None;
        }
        let end =
            highest.min(base.saturating_add(STREAM_TRANSPORT_FEEDBACK_MAX_STATUSES as u32 - 1));
        let first_received = self.arrivals.range(base..=end).next()?.1;
        let first_arrival_micros = duration_micros(*first_received);
        let reference_time =
            first_arrival_micros.div_euclid(TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS);
        let reference_micros = reference_time * TRANSPORT_FEEDBACK_REFERENCE_TIME_MICROS;

        let mut statuses = Vec::with_capacity((end - base + 1) as usize);
        let mut deltas = Vec::new();
        let mut previous_received_micros = reference_micros;
        for extended in base..=end {
            let Some(arrival) = self.arrivals.get(&extended) else {
                statuses.push(TRANSPORT_PACKET_NOT_RECEIVED);
                continue;
            };
            let arrival_micros = duration_micros(*arrival);
            let delta = rounded_divide(
                arrival_micros - previous_received_micros,
                TRANSPORT_FEEDBACK_DELTA_MICROS,
            );
            let encoded = if let Ok(delta) = u8::try_from(delta) {
                StreamTransportReceiveDelta::Small(delta)
            } else if let Ok(delta) = i16::try_from(delta) {
                StreamTransportReceiveDelta::Large(delta)
            } else {
                break;
            };
            statuses.push(encoded.status());
            deltas.push(encoded);
            previous_received_micros = arrival_micros;
        }
        if statuses.is_empty() || !statuses.iter().any(|status| *status != 0) {
            return None;
        }

        let status_count =
            u16::try_from(statuses.len()).expect("transport status count is bounded");
        let actual_end = base + u32::from(status_count) - 1;
        let packet = build_transport_wide_feedback(
            sender_ssrc,
            media_ssrc,
            base as u16,
            reference_time,
            self.feedback_packet_count,
            &statuses,
            &deltas,
        );
        self.feedback_packet_count = self.feedback_packet_count.wrapping_add(1);
        self.last_reported_sequence = Some(actual_end);
        self.next_unreported_sequence = actual_end.checked_add(1);
        self.arrivals.retain(|sequence, _| *sequence > actual_end);
        Some(packet)
    }
}

fn extend_transport_sequence(sequence: u16, highest: Option<u32>) -> u32 {
    let Some(highest) = highest else {
        return u32::from(sequence);
    };
    let delta = i32::from(sequence.wrapping_sub(highest as u16) as i16);
    if delta >= 0 {
        highest.wrapping_add(delta as u32)
    } else {
        highest.saturating_sub(delta.unsigned_abs())
    }
}

fn duration_micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn rounded_divide(value: i64, divisor: i64) -> i64 {
    if value >= 0 {
        (value + divisor / 2) / divisor
    } else {
        (value - divisor / 2) / divisor
    }
}

fn duration_to_rtcp_delay(duration: Duration) -> u32 {
    let whole = duration.as_secs().saturating_mul(1 << 16);
    let fraction = u64::from(duration.subsec_nanos()).saturating_mul(1 << 16) / 1_000_000_000;
    u32::try_from(whole.saturating_add(fraction)).unwrap_or(u32::MAX)
}

impl StreamAudioRecovery {
    fn push(
        &mut self,
        packet: RecoveredStreamAudioPacket,
        now: Instant,
    ) -> StreamAudioRecoveryUpdate {
        let sequence = packet.sequence;
        if let Some(next_sequence) = self.next_sequence {
            let distance = sequence.wrapping_sub(next_sequence);
            if distance >= 0x8000 {
                if self.started {
                    return StreamAudioRecoveryUpdate {
                        dropped_stale_packets: 1,
                        ..StreamAudioRecoveryUpdate::default()
                    };
                }
                self.next_sequence = Some(sequence);
            }
        } else {
            self.next_sequence = Some(sequence);
            self.first_buffered_at = Some(now);
        }
        if self.pending.contains_key(&sequence) {
            return StreamAudioRecoveryUpdate {
                dropped_stale_packets: 1,
                ..StreamAudioRecoveryUpdate::default()
            };
        }
        self.pending.insert(
            sequence,
            PendingStreamAudioPacket {
                packet,
                arrived_at: now,
            },
        );
        self.poll(now)
    }

    fn poll(&mut self, now: Instant) -> StreamAudioRecoveryUpdate {
        if self.pending.is_empty() {
            return StreamAudioRecoveryUpdate::default();
        }
        if !self.started {
            let buffered_long_enough = self.first_buffered_at.is_some_and(|first| {
                now.saturating_duration_since(first) >= STREAM_AUDIO_REORDER_DELAY
            });
            if !buffered_long_enough && self.pending.len() < STREAM_AUDIO_MAX_PENDING_PACKETS {
                return StreamAudioRecoveryUpdate::default();
            }
            self.started = true;
            self.first_buffered_at = None;
        }

        let mut update = StreamAudioRecoveryUpdate::default();
        loop {
            while let Some(expected) = self.next_sequence {
                let Some(pending) = self.pending.remove(&expected) else {
                    break;
                };
                self.next_sequence = Some(expected.wrapping_add(1));
                update.ready.push(pending.packet);
            }
            if self.pending.is_empty() {
                break;
            }

            let expected = self
                .next_sequence
                .expect("started audio recovery has a next sequence");
            let Some((next_sequence, distance)) = self
                .pending
                .keys()
                .filter_map(|sequence| {
                    let distance = sequence.wrapping_sub(expected);
                    (distance < 0x8000).then_some((*sequence, distance))
                })
                .min_by_key(|(_, distance)| *distance)
            else {
                break;
            };
            let gap_started_at = self
                .pending
                .values()
                .map(|pending| pending.arrived_at)
                .min()
                .expect("non-empty audio recovery has a packet arrival time");
            let gap_expired =
                now.saturating_duration_since(gap_started_at) >= STREAM_AUDIO_REORDER_DELAY;
            if !gap_expired && self.pending.len() < STREAM_AUDIO_MAX_PENDING_PACKETS {
                break;
            }
            self.next_sequence = Some(next_sequence);
            update.skipped_sequences = update.skipped_sequences.wrapping_add(distance);
        }
        update
    }

    fn reset(&mut self) {
        self.next_sequence = None;
        self.pending.clear();
        self.first_buffered_at = None;
        self.started = false;
    }

    fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl StreamVideoRecovery {
    fn push(
        &mut self,
        packet: RecoveredStreamVideoPacket,
        now: Instant,
    ) -> StreamVideoRecoveryUpdate {
        let sequence = packet.header.sequence;
        let expected = *self.next_sequence.get_or_insert(sequence);
        let distance = sequence.wrapping_sub(expected);
        if distance >= 0x8000 {
            return StreamVideoRecoveryUpdate::default();
        }

        let is_new = !self.pending.contains_key(&sequence);
        if distance != 0 {
            self.gap_started_at.get_or_insert(now);
        }
        let pending_packets = self.pending.len() + usize::from(is_new);
        let pending_bytes =
            self.pending_bytes
                .saturating_add(if is_new { packet.payload.len() } else { 0 });
        let reset = if distance != 0
            && (pending_packets > STREAM_VIDEO_MAX_PENDING_PACKETS
                || pending_bytes > STREAM_VIDEO_MAX_PENDING_BYTES)
        {
            let context = StreamVideoRecoveryReset {
                distance,
                pending_packets,
                pending_bytes,
                gap_age: self
                    .gap_started_at
                    .map(|started| now.saturating_duration_since(started)),
            };
            self.reset();
            self.next_sequence = Some(sequence);
            Some(context)
        } else {
            None
        };
        if !self.pending.contains_key(&sequence) {
            self.pending_bytes = self.pending_bytes.saturating_add(packet.payload.len());
            self.pending.insert(sequence, packet);
        }

        let mut ready = Vec::new();
        while let Some(expected) = self.next_sequence {
            let Some(packet) = self.pending.remove(&expected) else {
                break;
            };
            self.pending_bytes = self.pending_bytes.saturating_sub(packet.payload.len());
            self.next_sequence = Some(expected.wrapping_add(1));
            ready.push(packet);
        }
        if self.pending.is_empty() {
            self.gap_started_at = None;
            self.last_nack_at = None;
        } else {
            self.gap_started_at.get_or_insert(now);
        }

        StreamVideoRecoveryUpdate { ready, reset }
    }

    fn take_nack_if_due(&mut self, now: Instant) -> Option<Vec<u16>> {
        if self
            .last_nack_at
            .is_some_and(|last| now.saturating_duration_since(last) < STREAM_VIDEO_NACK_INTERVAL)
        {
            return None;
        }
        let missing = self.missing_sequences();
        if missing.is_empty() {
            return None;
        }
        self.last_nack_at = Some(now);
        Some(missing)
    }

    fn take_expired_gap(&mut self, now: Instant) -> Option<StreamVideoRecoveryReset> {
        let started = self.gap_started_at?;
        let gap_age = now.saturating_duration_since(started);
        if gap_age < STREAM_VIDEO_GAP_TIMEOUT {
            return None;
        }
        let expected = self.next_sequence?;
        let distance = self
            .pending
            .keys()
            .map(|sequence| sequence.wrapping_sub(expected))
            .filter(|distance| *distance < 0x8000)
            .max()
            .unwrap_or_default();
        let context = StreamVideoRecoveryReset {
            distance,
            pending_packets: self.pending.len(),
            pending_bytes: self.pending_bytes,
            gap_age: Some(gap_age),
        };
        self.reset();
        Some(context)
    }

    fn reset(&mut self) {
        self.next_sequence = None;
        self.pending.clear();
        self.pending_bytes = 0;
        self.gap_started_at = None;
        self.last_nack_at = None;
    }

    fn missing_sequences(&self) -> Vec<u16> {
        let Some(expected) = self.next_sequence else {
            return Vec::new();
        };
        let Some(farthest) = self
            .pending
            .keys()
            .map(|sequence| sequence.wrapping_sub(expected))
            .filter(|distance| *distance < 0x8000)
            .max()
        else {
            return Vec::new();
        };
        (0..=farthest)
            .map(|distance| expected.wrapping_add(distance))
            .filter(|sequence| !self.pending.contains_key(sequence))
            .take(STREAM_VIDEO_MAX_NACK_SEQUENCES)
            .collect()
    }
}

fn recover_stream_video_packet(
    header: RtpHeader,
    mut payload: Vec<u8>,
    source: StreamVideoSource,
) -> Option<RecoveredStreamVideoPacket> {
    if header.payload_type == DISCORD_STREAM_VIDEO_PAYLOAD_TYPE && header.ssrc == source.video_ssrc
    {
        return Some(RecoveredStreamVideoPacket { header, payload });
    }
    if header.payload_type != DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
        || source.rtx_ssrc != Some(header.ssrc)
    {
        return None;
    }
    let original_sequence = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    if payload.len() <= 2 {
        return None;
    }
    payload.drain(..2);
    Some(RecoveredStreamVideoPacket {
        header: RtpHeader {
            payload_type: DISCORD_STREAM_VIDEO_PAYLOAD_TYPE,
            sequence: original_sequence,
            ssrc: source.video_ssrc,
            ..header
        },
        payload,
    })
}

fn parse_stream_transport_sequence(
    extension_profile: Option<u16>,
    extension_body: &[u8],
) -> Option<u16> {
    if extension_profile != Some(RTP_ONE_BYTE_EXTENSION_PROFILE) {
        return None;
    }

    let mut offset = 0usize;
    while offset < extension_body.len() {
        let descriptor = extension_body[offset];
        offset += 1;
        let extension_id = descriptor >> 4;
        if extension_id == 0 {
            continue;
        }
        if extension_id == 15 {
            break;
        }
        let extension_len = usize::from(descriptor & 0x0f) + 1;
        let end = offset.checked_add(extension_len)?;
        let value = extension_body.get(offset..end)?;
        if extension_id == DISCORD_TRANSPORT_SEQUENCE_EXTENSION_ID {
            return matches!(value.len(), 2 | 4).then(|| u16::from_be_bytes([value[0], value[1]]));
        }
        offset = end;
    }
    None
}

fn parse_stream_video_source(value: &Value, owner_id: Id<UserMarker>) -> Option<StreamVideoSource> {
    let data = value.get("d")?;
    if data.get("user_id").and_then(Value::as_str) != Some(owner_id.to_string().as_str()) {
        return None;
    }
    let audio_ssrc = data
        .get("audio_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())?;
    let fallback_video_ssrc = data
        .get("video_ssrc")
        .and_then(Value::as_u64)
        .and_then(|ssrc| u32::try_from(ssrc).ok())
        .filter(|ssrc| *ssrc != 0);
    let selected = data
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|stream| {
            stream
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .filter_map(|stream| {
            let ssrc = stream
                .get("ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok())?;
            let quality = stream
                .get("quality")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let rtx_ssrc = stream
                .get("rtx_ssrc")
                .and_then(Value::as_u64)
                .and_then(|ssrc| u32::try_from(ssrc).ok());
            let pixel_count = stream.get("max_resolution").and_then(|resolution| {
                let width = resolution.get("width")?.as_u64()?;
                let height = resolution.get("height")?.as_u64()?;
                width.checked_mul(height).filter(|pixels| *pixels != 0)
            });
            Some((quality, ssrc, rtx_ssrc, pixel_count))
        })
        .max_by_key(|(quality, _, _, _)| *quality);
    let (video_ssrc, rtx_ssrc, pixel_count) = selected
        .map(|(_, ssrc, rtx, pixels)| (ssrc, rtx, pixels))
        .or_else(|| fallback_video_ssrc.map(|ssrc| (ssrc, Some(ssrc.wrapping_add(1)), None)))?;
    Some(StreamVideoSource {
        audio_ssrc,
        video_ssrc,
        rtx_ssrc,
        pixel_count,
    })
}

async fn run_stream_media(
    discord_socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    video_source_rx: watch::Receiver<StreamVideoSource>,
    owner_id: Id<UserMarker>,
    local_ssrc: u32,
    stream_player_ready: StreamPlayerReadySignal,
) -> Result<(), StreamConnectionFailure> {
    let audio_ports = reserve_local_udp_port_pair()?;
    let video_ports = reserve_local_udp_port_pair()?;
    let audio_port = audio_ports.rtp_port;
    let audio_rtcp_port = audio_ports.rtcp_port;
    let video_port = video_ports.rtp_port;
    let video_rtcp_port = video_ports.rtcp_port;
    let mut sdp =
        NamedTempFile::new().map_err(|error| format!("create stream SDP failed: {error}"))?;
    sdp.write_all(stream_sdp(audio_port, audio_rtcp_port, video_port, video_rtcp_port).as_bytes())
        .map_err(|error| format!("write stream SDP failed: {error}"))?;
    sdp.flush()
        .map_err(|error| format!("flush stream SDP failed: {error}"))?;
    let mut player_input_config = NamedTempFile::new()
        .map_err(|error| format!("create stream mpv input config failed: {error}"))?;
    player_input_config
        .write_all(STREAM_PLAYER_INPUT_CONFIG.as_bytes())
        .map_err(|error| format!("write stream mpv input config failed: {error}"))?;
    player_input_config
        .flush()
        .map_err(|error| format!("flush stream mpv input config failed: {error}"))?;

    // Keep both RTP/RTCP pairs reserved until the SDP is complete, then
    // release them immediately before mpv binds its receive sockets.
    drop(audio_ports);
    drop(video_ports);
    let player_ipc = MediaPlayerIpcEndpoint::unique();
    player_ipc
        .prepare()
        .map_err(|error| format!("prepare stream mpv IPC failed: {error}"))?;
    let mut player = stream_player_command(
        sdp.path(),
        player_input_config.path(),
        &stream_player_ready.display_name,
        player_ipc.server_arg(),
    );
    let mut player = player.spawn().map_err(stream_player_spawn_failure)?;
    let player_id = player.id();
    let player_stdout = player
        .stdout
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stdout failed"))?;
    let player_stderr = player
        .stderr
        .take()
        .ok_or_else(|| StreamConnectionFailure::stop("capture stream mpv stderr failed"))?;
    let last_player_error = Arc::new(Mutex::new(None));
    let video_player_ready = Arc::clone(&stream_player_ready.player_ready);
    let player_log_tasks = StreamPlayerLogTasks::new([
        tokio::spawn(log_stream_player_output(
            "stream",
            "stdout",
            player_stdout,
            Arc::clone(&last_player_error),
            Some(stream_player_ready),
        )),
        tokio::spawn(log_stream_player_output(
            "stream",
            "stderr",
            player_stderr,
            Arc::clone(&last_player_error),
            None,
        )),
    ]);
    logging::debug(
        "stream",
        format!(
            "stream mpv started: pid={player_id:?} audio_port={audio_port} audio_rtcp_port={audio_rtcp_port} video_port={video_port} video_rtcp_port={video_rtcp_port}"
        ),
    );

    let local_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("bind local stream RTP socket failed: {error}"))?;
    let audio_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_port);
    let audio_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, audio_rtcp_port);
    let video_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_port);
    let video_rtcp_target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, video_rtcp_port);
    let decryptor = VoiceRtpDecryptor::new(&description.mode, &description.secret_key)?;
    let encryptor = VoiceRtpEncryptor::new(&description.mode, &description.secret_key)?;
    let mut packet = [0u8; STREAM_RTP_PACKET_BYTES];
    let mut h264 = H264Depacketizer::default();
    let mut h264_startup = H264StartupGate::default();
    let mut h264_startup_buffer = H264StartupBuffer::default();
    let mut audio_recovery = StreamAudioRecovery::default();
    let mut video_recovery = StreamVideoRecovery::default();
    let mut active_audio_source = 0u32;
    let mut active_video_source = (0u32, None);
    let mut local_audio = LocalStreamAudioForwarder::default();
    let mut local_video = LocalStreamVideoForwarder::default();
    let mut discord_rtcp = StreamRtcpControl::default();
    let mut pli_throttle = StreamPliThrottle::default();
    let mut transport_feedback = StreamTransportFeedback::default();
    let mut presentation_clock = StreamPresentationClock::default();
    let mut media_counters = StreamMediaCounters::default();
    let mut previous_media_counters = StreamMediaCounters::default();
    let mut previous_local_video_frames = 0u64;
    let mut previous_stats_elapsed = Duration::ZERO;
    let media_started_at = Instant::now();
    let mut player_audio = StreamPlayerAudioState::default();
    let mut logged_first_video_frame = false;
    let mut logged_video_before_player_ready = false;
    let mut logged_keyframe_request = false;
    let mut logged_local_sender_reports = false;
    let mut logged_discord_sender_report = false;
    let mut local_rtcp_report_ticks = 0u64;
    let mut keyframe_request_interval = tokio::time::interval(STREAM_KEYFRAME_REQUEST_INTERVAL);
    keyframe_request_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut video_recovery_interval = tokio::time::interval(STREAM_VIDEO_NACK_INTERVAL);
    video_recovery_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut audio_recovery_interval = tokio::time::interval(STREAM_AUDIO_REORDER_INTERVAL);
    audio_recovery_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut local_rtcp_report_interval = tokio::time::interval(LOCAL_RTCP_REPORT_INTERVAL);
    local_rtcp_report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut discord_rtcp_report_interval =
        tokio::time::interval(STREAM_RTCP_RECEIVER_REPORT_INTERVAL);
    discord_rtcp_report_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut transport_feedback_interval = tokio::time::interval(STREAM_TRANSPORT_FEEDBACK_INTERVAL);
    transport_feedback_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let player_ready_timeout = tokio::time::sleep(STREAM_PLAYER_READY_TIMEOUT);
    tokio::pin!(player_ready_timeout);

    loop {
        maybe_enable_stream_player_audio(
            &mut player_audio,
            video_player_ready.load(Ordering::Acquire),
            &player_ipc,
        )
        .await;
        tokio::select! {
            _ = &mut player_ready_timeout,
                if !video_player_ready.load(Ordering::Acquire) =>
            {
                let last_error = last_player_error.lock().await.clone();
                return Err(StreamConnectionFailure::stop(match last_error {
                    Some(error) => format!(
                        "stream mpv did not open its SDP input within {} seconds: {error}",
                        STREAM_PLAYER_READY_TIMEOUT.as_secs(),
                    ),
                    None => format!(
                        "stream mpv did not open its SDP input within {} seconds; local RTP ports may no longer be available",
                        STREAM_PLAYER_READY_TIMEOUT.as_secs(),
                    ),
                }));
            }
            _ = local_rtcp_report_interval.tick() => {
                let elapsed = media_started_at.elapsed();
                let source = *video_source_rx.borrow();
                let unix_time = current_unix_time();
                let mut sent_report = false;
                if source.audio_ssrc != 0
                    && local_audio.packets != 0
                    && let Some(audio_timestamp) = local_audio.timestamp_at(elapsed)
                {
                    let report = build_rtcp_sender_report(
                        source.audio_ssrc,
                        unix_time,
                        audio_timestamp,
                        local_audio.packets,
                        local_audio.octets,
                    );
                    let _ = local_socket.send_to(&report, audio_rtcp_target).await;
                    sent_report = true;
                }
                if source.video_ssrc != 0 && local_video.packets != 0 {
                    let report = build_rtcp_sender_report(
                        source.video_ssrc,
                        unix_time,
                        elapsed_rtp_timestamp(elapsed, VIDEO_RTP_CLOCK_RATE),
                        local_video.packets,
                        local_video.octets,
                    );
                    let _ = local_socket.send_to(&report, video_rtcp_target).await;
                    sent_report = true;
                }
                if sent_report {
                    local_rtcp_report_ticks = local_rtcp_report_ticks.wrapping_add(1);
                    if !logged_local_sender_reports {
                        logged_local_sender_reports = true;
                        logging::debug(
                            "stream",
                            format!(
                                "local RTCP sender reports started: elapsed_ms={}",
                                elapsed.as_millis()
                            ),
                        );
                    } else if local_rtcp_report_ticks.is_multiple_of(10) {
                        let interval_elapsed = elapsed.saturating_sub(previous_stats_elapsed);
                        let interval_local_video_frames = local_video
                            .frames
                            .wrapping_sub(previous_local_video_frames);
                        logging::debug(
                            "stream",
                            format!(
                                "stream media stats: elapsed_ms={} interval_ms={} interval_video_packets={} interval_rtx_packets={} interval_h264_frames={} interval_h264_bytes={} interval_output_video_frames={} audio_pending_packets={} audio_stale_packets={} audio_skipped_packets={} decoder_resets={} transport_feedbacks={} nacks={} plis={} suppressed_plis={}",
                                elapsed.as_millis(),
                                interval_elapsed.as_millis(),
                                media_counters.primary_video_packets.wrapping_sub(
                                    previous_media_counters.primary_video_packets,
                                ),
                                media_counters
                                    .rtx_video_packets
                                    .wrapping_sub(previous_media_counters.rtx_video_packets),
                                media_counters
                                    .h264_frames
                                    .wrapping_sub(previous_media_counters.h264_frames),
                                media_counters
                                    .h264_bytes
                                    .wrapping_sub(previous_media_counters.h264_bytes),
                                interval_local_video_frames,
                                audio_recovery.pending_len(),
                                media_counters.audio_stale_packets,
                                media_counters.audio_skipped_packets,
                                media_counters.decoder_resets,
                                media_counters.transport_feedbacks,
                                media_counters.nacks,
                                media_counters.plis,
                                media_counters.suppressed_plis,
                            ),
                        );
                        previous_media_counters = media_counters;
                        previous_local_video_frames = local_video.frames;
                        previous_stats_elapsed = elapsed;
                    }
                }
            }
            _ = discord_rtcp_report_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0 {
                    discord_rtcp.set_source(source.video_ssrc);
                    discord_rtcp
                        .send_report(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            media_started_at.elapsed(),
                        )
                        .await?;
                }
            }
            _ = transport_feedback_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0
                    && let Some(feedback) = transport_feedback
                        .take_feedback(local_ssrc, source.video_ssrc)
                {
                    discord_rtcp
                        .send_feedback(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            &feedback,
                            "transport-wide feedback",
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.transport_feedbacks =
                        media_counters.transport_feedbacks.wrapping_add(1);
                }
            }
            _ = audio_recovery_interval.tick() => {
                let source = *video_source_rx.borrow();
                if source.audio_ssrc != active_audio_source {
                    active_audio_source = source.audio_ssrc;
                    audio_recovery.reset();
                    local_audio.reset_source();
                }
                if source.audio_ssrc != 0 {
                    let update = audio_recovery.poll(Instant::now());
                    let destination = LocalStreamAudioDestination {
                        socket: &local_socket,
                        target: audio_target,
                        ssrc: source.audio_ssrc,
                        media_started_at,
                        presentation_clock: &presentation_clock,
                    };
                    forward_recovered_stream_audio(
                        update,
                        audio_recovery.pending_len(),
                        &mut local_audio,
                        &destination,
                        &mut player_audio,
                        &mut media_counters,
                    )
                    .await;
                }
            }
            _ = video_recovery_interval.tick() => {
                let source = *video_source_rx.borrow();
                let source_identity = (source.video_ssrc, source.rtx_ssrc);
                if source_identity != active_video_source {
                    active_video_source = source_identity;
                    discord_rtcp.set_source(source.video_ssrc);
                    transport_feedback.reset();
                    video_recovery.reset();
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                }
                let now = Instant::now();
                if let Some(reset) = video_recovery.take_expired_gap(now) {
                    media_counters.decoder_resets =
                        media_counters.decoder_resets.wrapping_add(1);
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                    let mut pli_sent = None;
                    if source.video_ssrc != 0 {
                        let sent = pli_throttle
                            .send_if_due(
                                &mut discord_rtcp,
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                source.video_ssrc,
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.observe_pli_request(sent);
                        pli_sent = Some(sent);
                    }
                    logging::debug(
                        "stream",
                        format!(
                            "stream video packet gap expired: distance={} pending_packets={} pending_bytes={} gap_age_ms={:?}; reset the H264 pipeline and {}",
                            reset.distance,
                            reset.pending_packets,
                            reset.pending_bytes,
                            reset.gap_age.map(|age| age.as_millis()),
                            match pli_sent {
                                Some(true) => "requested a new keyframe",
                                Some(false) => "kept the recent keyframe request",
                                None => "had no active video source for a keyframe request",
                            }
                        ),
                    );
                } else if let Some(missing) = video_recovery.take_nack_if_due(now)
                    && source.video_ssrc != 0
                {
                    let feedback = build_rtcp_nack(local_ssrc, source.video_ssrc, &missing);
                    discord_rtcp
                        .send_feedback(
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            &feedback,
                            "NACK",
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.nacks = media_counters.nacks.wrapping_add(1);
                }
            }
            _ = keyframe_request_interval.tick(), if !h264_startup.is_started() => {
                let source = *video_source_rx.borrow();
                if source.video_ssrc != 0 {
                    discord_rtcp.set_source(source.video_ssrc);
                    let sent = pli_throttle
                        .send_if_due(
                            &mut discord_rtcp,
                            &discord_socket,
                            &encryptor,
                            local_ssrc,
                            source.video_ssrc,
                            media_started_at.elapsed(),
                        )
                        .await?;
                    media_counters.observe_pli_request(sent);
                    if sent && !logged_keyframe_request {
                        logged_keyframe_request = true;
                        logging::debug(
                            "stream",
                            format!(
                                "stream compound RTCP keyframe request sent: sender_ssrc={local_ssrc} media_ssrc={}",
                                source.video_ssrc
                            ),
                        );
                    }
                }
            }
            status = player.wait() => {
                let status = status.map_err(|error| {
                    StreamConnectionFailure::stop(format!(
                        "wait for stream mpv failed: {error}"
                    ))
                })?;
                player_log_tasks.finish().await;
                let last_error = last_player_error.lock().await.clone();
                logging::debug("stream", format!("stream mpv exited: status={status}"));
                if status.success() {
                    return Ok(());
                }
                return Err(StreamConnectionFailure::stop(match last_error {
                    Some(error) => format!("stream mpv exited with {status}: {error}"),
                    None => format!("stream mpv exited with {status}"),
                }));
            }
            received = discord_socket.recv(&mut packet) => {
                let received =
                    received.map_err(|error| format!("stream UDP receive failed: {error}"))?;
                let packet_arrival = media_started_at.elapsed();
                let packet = &packet[..received];
                if looks_like_rtcp_packet(packet) {
                    let decrypted = match decryptor.decrypt_rtcp_feedback(packet) {
                        Ok(decrypted) => decrypted,
                        Err(error) => {
                            logging::debug(
                                "stream",
                                format!("stream RTCP decrypt failed: {error}"),
                            );
                            continue;
                        }
                    };
                    let reports = match parse_stream_rtcp_sender_reports(&decrypted) {
                        Ok(reports) => reports,
                        Err(error) => {
                            logging::debug(
                                "stream",
                                format!("stream RTCP parse failed: {error}"),
                            );
                            continue;
                        }
                    };
                    let source = *video_source_rx.borrow();
                    let elapsed = media_started_at.elapsed();
                    for report in reports {
                        if report.sender_ssrc != source.audio_ssrc
                            && report.sender_ssrc != source.video_ssrc
                        {
                            continue;
                        }
                        if report.sender_ssrc == source.video_ssrc {
                            discord_rtcp.set_source(source.video_ssrc);
                            discord_rtcp.observe_sender_report(report, elapsed);
                        }
                        presentation_clock.observe_sender_report(report, elapsed);
                        if !logged_discord_sender_report {
                            logged_discord_sender_report = true;
                            logging::debug(
                                "stream",
                                format!(
                                    "Discord RTCP sender clock started: elapsed_ms={} sender_ssrc={} rtp_timestamp={} packets={} octets={}",
                                    elapsed.as_millis(),
                                    report.sender_ssrc,
                                    report.rtp_timestamp,
                                    report.packet_count,
                                    report.octet_count,
                                ),
                            );
                        }
                    }
                    continue;
                }
                let header = match parse_rtp_header(packet) {
                    Ok(header) => header,
                    Err(_) => continue,
                };
                let source = *video_source_rx.borrow();
                if source.audio_ssrc != active_audio_source {
                    active_audio_source = source.audio_ssrc;
                    audio_recovery.reset();
                    local_audio.reset_source();
                }
                let source_identity = (source.video_ssrc, source.rtx_ssrc);
                if source_identity != active_video_source {
                    active_video_source = source_identity;
                    discord_rtcp.set_source(source.video_ssrc);
                    transport_feedback.reset();
                    video_recovery.reset();
                    reset_stream_h264_pipeline(
                        &mut h264,
                        &mut h264_startup,
                        &mut h264_startup_buffer,
                    );
                }
                if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE
                    && header.payload_type != DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                    && header.payload_type != DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE
                {
                    continue;
                }
                let decrypted = match decryptor.decrypt_packet_any(packet, &header) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        logging::debug("stream", format!("stream RTP decrypt failed: {error}"));
                        continue;
                    }
                };
                if let Some(sequence) = parse_stream_transport_sequence(
                    decrypted.extension_profile,
                    &decrypted.extension_body,
                ) {
                    transport_feedback.observe(sequence, packet_arrival);
                }
                if header.payload_type == DISCORD_VOICE_PAYLOAD_TYPE
                    && source.audio_ssrc != 0
                    && header.ssrc == source.audio_ssrc
                {
                    let media = dave_state
                        .lock()
                        .await
                        .unwrap_media_payload_for_ssrc(header.ssrc, &decrypted.media_payload);
                    let opus = match media {
                        VoiceMediaPayload::Plain(opus)
                        | VoiceMediaPayload::DaveDecrypted { opus, .. } => opus,
                        _ => continue,
                    };
                    let update = audio_recovery.push(
                        RecoveredStreamAudioPacket {
                            marker: header.marker,
                            sequence: header.sequence,
                            timestamp: header.timestamp,
                            opus,
                        },
                        Instant::now(),
                    );
                    let destination = LocalStreamAudioDestination {
                        socket: &local_socket,
                        target: audio_target,
                        ssrc: source.audio_ssrc,
                        media_started_at,
                        presentation_clock: &presentation_clock,
                    };
                    forward_recovered_stream_audio(
                        update,
                        audio_recovery.pending_len(),
                        &mut local_audio,
                        &destination,
                        &mut player_audio,
                        &mut media_counters,
                    )
                    .await;
                } else if source.video_ssrc != 0 {
                    let received_payload_type = header.payload_type;
                    if received_payload_type == DISCORD_STREAM_VIDEO_PAYLOAD_TYPE
                        && header.ssrc == source.video_ssrc
                    {
                        discord_rtcp.observe_rtp(
                            header.sequence,
                            header.timestamp,
                            packet_arrival,
                        );
                    }
                    let Some(video_packet) =
                        recover_stream_video_packet(header, decrypted.media_payload, source)
                    else {
                        continue;
                    };
                    if received_payload_type == DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE {
                        media_counters.rtx_video_packets =
                            media_counters.rtx_video_packets.wrapping_add(1);
                    } else {
                        media_counters.primary_video_packets =
                            media_counters.primary_video_packets.wrapping_add(1);
                    }
                    let now = Instant::now();
                    let recovery = video_recovery.push(video_packet, now);
                    if let Some(reset) = recovery.reset {
                        media_counters.decoder_resets =
                            media_counters.decoder_resets.wrapping_add(1);
                        reset_stream_h264_pipeline(
                            &mut h264,
                            &mut h264_startup,
                            &mut h264_startup_buffer,
                        );
                        let pli_sent = pli_throttle
                            .send_if_due(
                                &mut discord_rtcp,
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                source.video_ssrc,
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.observe_pli_request(pli_sent);
                        logging::debug(
                            "stream",
                            format!(
                                "stream video recovery budget exceeded: distance={} pending_packets={} packet_limit={} pending_bytes={} byte_limit={} gap_age_ms={:?}; reset the H264 pipeline and {}",
                                reset.distance,
                                reset.pending_packets,
                                STREAM_VIDEO_MAX_PENDING_PACKETS,
                                reset.pending_bytes,
                                STREAM_VIDEO_MAX_PENDING_BYTES,
                                reset.gap_age.map(|age| age.as_millis()),
                                if pli_sent {
                                    "requested a new keyframe"
                                } else {
                                    "kept the recent keyframe request"
                                }
                            ),
                        );
                    }
                    if let Some(missing) = video_recovery.take_nack_if_due(now) {
                        let feedback = build_rtcp_nack(local_ssrc, source.video_ssrc, &missing);
                        discord_rtcp
                            .send_feedback(
                                &discord_socket,
                                &encryptor,
                                local_ssrc,
                                &feedback,
                                "NACK",
                                media_started_at.elapsed(),
                            )
                            .await?;
                        media_counters.nacks = media_counters.nacks.wrapping_add(1);
                    }
                    for video_packet in recovery.ready {
                        let header = video_packet.header;
                        let frame = match h264.push(&header, &video_packet.payload) {
                            H264DepacketizerOutput::Pending => continue,
                            H264DepacketizerOutput::Frame(frame) => {
                                media_counters.h264_frames =
                                    media_counters.h264_frames.wrapping_add(1);
                                media_counters.h264_bytes = media_counters
                                    .h264_bytes
                                    .wrapping_add(
                                        u64::try_from(frame.len())
                                            .expect("bounded H264 frame length fits u64"),
                                    );
                                frame
                            }
                            H264DepacketizerOutput::BudgetExceeded => {
                                media_counters.decoder_resets =
                                    media_counters.decoder_resets.wrapping_add(1);
                                reset_stream_h264_pipeline(
                                    &mut h264,
                                    &mut h264_startup,
                                    &mut h264_startup_buffer,
                                );
                                let pli_sent = pli_throttle
                                    .send_if_due(
                                        &mut discord_rtcp,
                                        &discord_socket,
                                        &encryptor,
                                        local_ssrc,
                                        source.video_ssrc,
                                        media_started_at.elapsed(),
                                    )
                                    .await?;
                                media_counters.observe_pli_request(pli_sent);
                                logging::debug(
                                    "stream",
                                    format!(
                                        "stream H264 access unit exceeded its safety budget; {}",
                                        if pli_sent {
                                            "requested a new keyframe"
                                        } else {
                                            "kept the recent keyframe request"
                                        }
                                    ),
                                );
                                continue;
                            }
                        };
                        let frame = match dave_state
                            .lock()
                            .await
                            .decrypt_video_frame(owner_id, &frame)
                        {
                            Ok(Some(frame)) => frame,
                            Ok(None) => continue,
                            Err(error) => {
                                logging::debug("stream", error);
                                continue;
                            }
                        };
                        if !logged_first_video_frame {
                            logged_first_video_frame = true;
                            logging::debug(
                                "stream",
                                format!(
                                    "first stream video frame decrypted: elapsed_ms={} nal_types={:?}",
                                    media_started_at.elapsed().as_millis(),
                                    h264_nal_types(&frame),
                                ),
                            );
                        }
                        let player_ready = video_player_ready.load(Ordering::Acquire);
                        let local_video_destination = LocalStreamVideoDestination {
                            socket: &local_socket,
                            target: video_target,
                            ssrc: source.video_ssrc,
                            media_started_at,
                        };
                        if player_ready {
                            local_video
                                .replay_startup(
                                    &mut h264_startup_buffer,
                                    &local_video_destination,
                                )
                                .await;
                        }
                        if !player_ready && !logged_video_before_player_ready {
                            logged_video_before_player_ready = true;
                            logging::debug(
                                "stream",
                                "buffering stream video until mpv opens its SDP input",
                            );
                        }
                        let waiting_for_keyframe = !h264_startup.is_started();
                        let frame = accept_or_buffer_h264(
                            player_ready,
                            &mut h264_startup,
                            &mut h264_startup_buffer,
                            frame,
                            header.timestamp,
                        );
                        if waiting_for_keyframe && h264_startup.is_started() {
                            logging::debug(
                                "stream",
                                format!(
                                    "stream H264 keyframe {}: elapsed_ms={}",
                                    if player_ready { "accepted" } else { "buffered" },
                                    media_started_at.elapsed().as_millis()
                                ),
                            );
                        }
                        let Some(frame) = frame else {
                            continue;
                        };
                        local_video
                            .forward_live(
                                &local_video_destination,
                                &frame,
                                presentation_clock.map_timestamp(
                                    source.video_ssrc,
                                    frame.source_timestamp,
                                    VIDEO_RTP_CLOCK_RATE,
                                ),
                            )
                            .await;
                    }
                }
            }
        }
    }
}

fn stream_player_spawn_failure(error: std::io::Error) -> StreamConnectionFailure {
    let message = if error.kind() == std::io::ErrorKind::NotFound {
        "mpv is required to watch Discord streams; install mpv and make sure it is on PATH"
            .to_owned()
    } else {
        format!("start mpv for stream failed: {error}")
    };
    StreamConnectionFailure::stop(message)
}

fn stream_player_command(
    sdp_path: &Path,
    input_config_path: &Path,
    display_name: &str,
    ipc_server: &str,
) -> Command {
    let mut player = Command::new("mpv");
    player
        // Keep playback deterministic and prevent user cache settings from
        // turning this live input into a delayed stream.
        .arg("--no-config")
        // mpv disables terminal logs when its output is redirected. Force the
        // line-based log stream on so Concord can capture player lifecycle
        // events through stdout.
        .arg("--terminal=yes")
        // Built-in UI scripts delay SDP socket creation and are not needed for
        // the dedicated stream window.
        .arg("--load-scripts=no")
        // RTP has no seekable live edge. Remove pause controls that would leave
        // the viewer replaying buffered history after the broadcast resumes.
        .arg("--osc=no")
        // Keep normal output quiet, but include the lifecycle stages needed to
        // separate SDP, decoder, and display startup delay in a live log.
        .arg("--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v")
        // An SDP audio track can remain empty for a video-only broadcast. Do
        // not let that selected track block video startup. The first real Opus
        // packet selects it through JSON IPC after mpv has loaded the SDP.
        .arg("--aid=no")
        .arg(format!("--input-ipc-server={ipc_server}"))
        .arg(format!("--input-conf={}", input_config_path.display()))
        // Prefer mpv's safe hardware allowlist and let libavcodec use the
        // available CPU cores when it falls back.
        .arg("--hwdec=auto-safe")
        .arg("--vd-lavc-threads=0")
        // A high-resolution encoded frame can arrive as a short UDP burst.
        // Keep enough byte capacity for that burst and only 150ms of forward
        // media, which smooths arrival jitter without retaining stale history.
        .arg("--stream-buffer-size=1MiB")
        .arg("--audio-buffer=0.05")
        .arg("--cache=yes")
        .arg("--cache-pause=no")
        .arg("--cache-pause-initial=no")
        .arg("--cache-secs=0.15")
        .arg("--demuxer-readahead-secs=0.15")
        .arg("--demuxer-max-bytes=16MiB")
        .arg("--demuxer-max-back-bytes=0")
        .arg("--demuxer=lavf")
        .arg("--demuxer-lavf-format=sdp")
        .arg("--demuxer-lavf-probe-info=nostreams")
        .arg("--demuxer-lavf-analyzeduration=0.1")
        .arg("--demuxer-lavf-probesize=32")
        .arg("--demuxer-lavf-buffersize=262144")
        // mpv uses commas between lavf key/value options. Square brackets keep
        // the protocol list together as one value.
        .arg("--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],buffer_size=4194304,max_delay=50000,reorder_queue_size=512")
        .arg("--force-window=immediate")
        // Discord can change the encoded resolution when the broadcaster
        // resizes the captured window. Keep the native player window stable
        // and scale the video inside it instead of following every change.
        .arg("--auto-window-resize=no")
        .arg("--geometry=1280x720")
        .arg(format!("--title=Concord - {display_name}'s stream"))
        // Audio and video share one player so its volume and mute controls
        // apply to the complete broadcast.
        .arg("--video-latency-hacks=no")
        .arg("--video-sync=audio")
        // Skip late output frames instead of preserving live delay.
        // Decoder dropping can discard H264 reference frames.
        .arg("--framedrop=vo")
        .arg("--video-timing-offset=0")
        .arg("--")
        .arg(sdp_path)
        // Both output streams remain enabled so startup stages and failures
        // reach the Concord log. stdin is closed so mpv cannot consume TUI
        // input.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    player
}

#[derive(Default)]
struct StreamPlayerAudioState {
    real_packet_observed: bool,
    enable_requested: bool,
}

impl StreamPlayerAudioState {
    fn observe_real_packet(&mut self) {
        self.real_packet_observed = true;
    }

    fn take_enable_request(&mut self, player_ready: bool) -> bool {
        if !self.real_packet_observed || !player_ready || self.enable_requested {
            return false;
        }
        self.enable_requested = true;
        true
    }
}

async fn maybe_enable_stream_player_audio(
    state: &mut StreamPlayerAudioState,
    player_ready: bool,
    endpoint: &MediaPlayerIpcEndpoint,
) {
    if !state.take_enable_request(player_ready) {
        return;
    }

    match timeout(
        STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT,
        endpoint.set_property("aid", "auto"),
    )
    .await
    {
        Ok(Ok(())) => logging::debug("stream", "stream mpv audio enabled"),
        Ok(Err(error)) => {
            logging::error("stream", format!("enable stream mpv audio failed: {error}"))
        }
        Err(_) => logging::error(
            "stream",
            format!(
                "enable stream mpv audio timed out after {} second",
                STREAM_PLAYER_AUDIO_ENABLE_TIMEOUT.as_secs()
            ),
        ),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamAudioClockDiscontinuity {
    source_delta_ticks: i32,
    elapsed_delta_ticks: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamAudioTimestamp {
    local_timestamp: u32,
    discontinuity: Option<StreamAudioClockDiscontinuity>,
}

#[derive(Default)]
struct LocalStreamAudioClock {
    last_source_timestamp: Option<u32>,
    last_local_timestamp: Option<u32>,
    last_elapsed: Option<Duration>,
}

impl LocalStreamAudioClock {
    fn is_recent_replay(&self, source_timestamp: u32) -> bool {
        let Some(last_source_timestamp) = self.last_source_timestamp else {
            return false;
        };
        let source_delta_ticks = source_timestamp.wrapping_sub(last_source_timestamp) as i32;
        source_delta_ticks <= 0
            && source_delta_ticks.unsigned_abs() <= STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS
    }

    // Discord can replay an old packet or restart an audio clock when a stream
    // subscription settles. Valid source deltas retain their media timing. A
    // discontinuity starts a new source epoch on the existing local timeline.
    fn rebase(
        &mut self,
        source_timestamp: u32,
        elapsed: Duration,
        synchronized_timestamp: Option<u32>,
    ) -> StreamAudioTimestamp {
        let Some(last_source_timestamp) = self.last_source_timestamp else {
            let fallback_timestamp = elapsed_rtp_timestamp(elapsed, OPUS_RTP_CLOCK_RATE);
            let local_timestamp = select_synchronized_rtp_timestamp(
                fallback_timestamp,
                synchronized_timestamp,
                None,
                OPUS_RTP_CLOCK_RATE,
            );
            self.record(source_timestamp, local_timestamp, elapsed);
            return StreamAudioTimestamp {
                local_timestamp,
                discontinuity: None,
            };
        };
        let last_local_timestamp = self
            .last_local_timestamp
            .expect("an anchored audio clock has a local timestamp");
        let last_elapsed = self
            .last_elapsed
            .expect("an anchored audio clock has an elapsed timestamp");
        let elapsed_delta_ticks =
            elapsed_rtp_timestamp(elapsed.saturating_sub(last_elapsed), OPUS_RTP_CLOCK_RATE);
        let source_delta_ticks = source_timestamp.wrapping_sub(last_source_timestamp) as i32;
        let drift_ticks = i64::from(source_delta_ticks).abs_diff(i64::from(elapsed_delta_ticks));
        let source_is_continuous = source_delta_ticks > 0
            && drift_ticks <= u64::from(STREAM_AUDIO_CLOCK_DRIFT_TOLERANCE_TICKS);

        let (fallback_timestamp, discontinuity) = if source_is_continuous {
            (
                last_local_timestamp.wrapping_add(source_delta_ticks as u32),
                None,
            )
        } else {
            let elapsed_timestamp = elapsed_rtp_timestamp(elapsed, OPUS_RTP_CLOCK_RATE);
            let minimum_timestamp =
                last_local_timestamp.wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
            (
                later_rtp_timestamp(elapsed_timestamp, minimum_timestamp),
                Some(StreamAudioClockDiscontinuity {
                    source_delta_ticks,
                    elapsed_delta_ticks,
                }),
            )
        };
        let local_timestamp = select_synchronized_rtp_timestamp(
            fallback_timestamp,
            synchronized_timestamp,
            Some(last_local_timestamp),
            OPUS_RTP_CLOCK_RATE,
        );

        self.record(source_timestamp, local_timestamp, elapsed);
        StreamAudioTimestamp {
            local_timestamp,
            discontinuity,
        }
    }

    fn timestamp_at(&self, elapsed: Duration) -> Option<u32> {
        let last_local_timestamp = self.last_local_timestamp?;
        let last_elapsed = self.last_elapsed?;
        Some(last_local_timestamp.wrapping_add(elapsed_rtp_timestamp(
            elapsed.saturating_sub(last_elapsed),
            OPUS_RTP_CLOCK_RATE,
        )))
    }

    fn record(&mut self, source_timestamp: u32, local_timestamp: u32, elapsed: Duration) {
        self.last_source_timestamp = Some(source_timestamp);
        self.last_local_timestamp = Some(local_timestamp);
        self.last_elapsed = Some(elapsed);
    }
}

#[derive(Default)]
struct LocalStreamAudioForwarder {
    sequence: u16,
    packets: u32,
    octets: u32,
    clock: LocalStreamAudioClock,
    logged_first_packet: bool,
}

struct LocalStreamAudioDestination<'a> {
    socket: &'a UdpSocket,
    target: SocketAddrV4,
    ssrc: u32,
    media_started_at: Instant,
    presentation_clock: &'a StreamPresentationClock,
}

impl LocalStreamAudioForwarder {
    async fn forward(
        &mut self,
        packets: Vec<RecoveredStreamAudioPacket>,
        skipped_sequences: u16,
        destination: &LocalStreamAudioDestination<'_>,
        player_audio: &mut StreamPlayerAudioState,
    ) -> u64 {
        self.sequence = self.sequence.wrapping_add(skipped_sequences);
        let mut dropped_replays = 0u64;
        for packet in packets {
            // A short backward source timestamp is a delayed replay, not a new
            // clock epoch. Leave a local RTP sequence gap so mpv can conceal
            // it instead of decoding old Opus after newer audio.
            if self.clock.is_recent_replay(packet.timestamp) {
                self.sequence = self.sequence.wrapping_add(1);
                dropped_replays = dropped_replays.wrapping_add(1);
                continue;
            }
            player_audio.observe_real_packet();
            let elapsed = destination.media_started_at.elapsed();
            let synchronized_timestamp = destination.presentation_clock.map_timestamp(
                destination.ssrc,
                packet.timestamp,
                OPUS_RTP_CLOCK_RATE,
            );
            let audio_timestamp =
                self.clock
                    .rebase(packet.timestamp, elapsed, synchronized_timestamp);
            let local_timestamp = audio_timestamp.local_timestamp;
            if let Some(discontinuity) = audio_timestamp.discontinuity {
                logging::debug(
                    "stream",
                    format!(
                        "stream audio RTP clock re-anchored: source_timestamp={} source_delta_ticks={} elapsed_delta_ticks={} local_timestamp={local_timestamp}",
                        packet.timestamp,
                        discontinuity.source_delta_ticks,
                        discontinuity.elapsed_delta_ticks,
                    ),
                );
            }
            let local_packet = build_local_rtp_packet(
                LOCAL_STREAM_AUDIO_PAYLOAD_TYPE,
                packet.marker,
                self.sequence,
                local_timestamp,
                destination.ssrc,
                &packet.opus,
            );
            self.sequence = self.sequence.wrapping_add(1);
            let _ = destination
                .socket
                .send_to(&local_packet, destination.target)
                .await;
            self.packets = self.packets.wrapping_add(1);
            self.octets = self.octets.wrapping_add(packet.opus.len() as u32);
            if !self.logged_first_packet {
                self.logged_first_packet = true;
                logging::debug(
                    "stream",
                    format!(
                        "first stream audio forwarded: elapsed_ms={} source_timestamp={} local_timestamp={local_timestamp}",
                        elapsed.as_millis(),
                        packet.timestamp,
                    ),
                );
            }
        }
        dropped_replays
    }

    fn reset_source(&mut self) {
        self.clock = LocalStreamAudioClock::default();
    }

    fn timestamp_at(&self, elapsed: Duration) -> Option<u32> {
        self.clock.timestamp_at(elapsed)
    }
}

async fn forward_recovered_stream_audio(
    update: StreamAudioRecoveryUpdate,
    pending_packets: usize,
    forwarder: &mut LocalStreamAudioForwarder,
    destination: &LocalStreamAudioDestination<'_>,
    player_audio: &mut StreamPlayerAudioState,
    counters: &mut StreamMediaCounters,
) {
    counters.audio_stale_packets = counters
        .audio_stale_packets
        .wrapping_add(update.dropped_stale_packets);
    counters.audio_skipped_packets = counters
        .audio_skipped_packets
        .wrapping_add(u64::from(update.skipped_sequences));
    if update.skipped_sequences != 0 {
        logging::debug(
            "stream",
            format!(
                "stream audio packet gap expired: skipped_packets={} pending_packets={pending_packets}",
                update.skipped_sequences,
            ),
        );
    }
    let dropped_replays = forwarder
        .forward(
            update.ready,
            update.skipped_sequences,
            destination,
            player_audio,
        )
        .await;
    counters.audio_stale_packets = counters.audio_stale_packets.wrapping_add(dropped_replays);
}

#[derive(Default)]
struct LocalRtpClock {
    source_origin: Option<u32>,
    local_origin: u32,
}

impl LocalRtpClock {
    fn anchor(&mut self, source_timestamp: u32, local_timestamp: u32) {
        self.source_origin = Some(source_timestamp);
        self.local_origin = local_timestamp;
    }

    fn rebase(
        &mut self,
        source_timestamp: u32,
        elapsed: Duration,
        clock_rate: u32,
        synchronized_timestamp: Option<u32>,
    ) -> u32 {
        let previous_timestamp = self.source_origin.map(|_| self.local_origin);
        let source_origin = *self.source_origin.get_or_insert_with(|| {
            self.local_origin = elapsed_rtp_timestamp(elapsed, clock_rate);
            source_timestamp
        });
        let fallback_timestamp = self
            .local_origin
            .wrapping_add(source_timestamp.wrapping_sub(source_origin));
        let local_timestamp = select_synchronized_rtp_timestamp(
            fallback_timestamp,
            synchronized_timestamp,
            previous_timestamp,
            clock_rate,
        );
        self.anchor(source_timestamp, local_timestamp);
        local_timestamp
    }
}

fn select_synchronized_rtp_timestamp(
    fallback_timestamp: u32,
    synchronized_timestamp: Option<u32>,
    previous_timestamp: Option<u32>,
    clock_rate: u32,
) -> u32 {
    let Some(synchronized_timestamp) = synchronized_timestamp else {
        return fallback_timestamp;
    };
    let correction = synchronized_timestamp
        .wrapping_sub(fallback_timestamp)
        .cast_signed()
        .unsigned_abs();
    let maximum_correction =
        elapsed_rtp_timestamp(STREAM_PRESENTATION_CLOCK_MAX_CORRECTION, clock_rate);
    if correction > maximum_correction
        || previous_timestamp.is_some_and(|previous| {
            synchronized_timestamp.wrapping_sub(previous).cast_signed() <= 0
        })
    {
        fallback_timestamp
    } else {
        synchronized_timestamp
    }
}

#[derive(Default)]
struct LocalStreamVideoForwarder {
    sequence: u16,
    packets: u32,
    octets: u32,
    frames: u64,
    clock: LocalRtpClock,
    logged_first_frame: bool,
}

struct LocalStreamVideoDestination<'a> {
    socket: &'a UdpSocket,
    target: SocketAddrV4,
    ssrc: u32,
    media_started_at: Instant,
}

impl LocalStreamVideoForwarder {
    async fn replay_startup(
        &mut self,
        startup_buffer: &mut H264StartupBuffer,
        destination: &LocalStreamVideoDestination<'_>,
    ) {
        if startup_buffer.is_empty() {
            return;
        }

        let buffered_frames = startup_buffer.len();
        let replay_origin =
            elapsed_rtp_timestamp(destination.media_started_at.elapsed(), VIDEO_RTP_CLOCK_RATE);
        let mut replay_anchor = None;
        for (index, buffered) in startup_buffer.take().into_iter().enumerate() {
            let local_timestamp = replay_origin.wrapping_add(
                u32::try_from(index)
                    .expect("startup buffer length is bounded")
                    .wrapping_mul(STREAM_STARTUP_REPLAY_FRAME_TICKS),
            );
            self.forward_at_timestamp(destination, &buffered, local_timestamp, true)
                .await;
            replay_anchor = Some((buffered.source_timestamp, local_timestamp));
        }
        if let Some((source_timestamp, local_timestamp)) = replay_anchor {
            self.clock.anchor(source_timestamp, local_timestamp);
        }
        logging::debug(
            "stream",
            format!(
                "buffered H264 startup replayed: elapsed_ms={} frames={buffered_frames}",
                destination.media_started_at.elapsed().as_millis()
            ),
        );
    }

    async fn forward_live(
        &mut self,
        destination: &LocalStreamVideoDestination<'_>,
        frame: &BufferedH264Frame,
        synchronized_timestamp: Option<u32>,
    ) {
        let local_timestamp = self.clock.rebase(
            frame.source_timestamp,
            destination.media_started_at.elapsed(),
            VIDEO_RTP_CLOCK_RATE,
            synchronized_timestamp,
        );
        self.forward_at_timestamp(destination, frame, local_timestamp, false)
            .await;
    }

    async fn forward_at_timestamp(
        &mut self,
        destination: &LocalStreamVideoDestination<'_>,
        frame: &BufferedH264Frame,
        local_timestamp: u32,
        buffered: bool,
    ) {
        let (packet_count, octet_count) = send_local_h264_frame(
            destination.socket,
            destination.target,
            &frame.encoded,
            local_timestamp,
            destination.ssrc,
            &mut self.sequence,
        )
        .await;
        self.packets = self.packets.wrapping_add(packet_count);
        self.octets = self.octets.wrapping_add(octet_count);
        self.frames = self.frames.wrapping_add(1);
        if !self.logged_first_frame {
            self.logged_first_frame = true;
            logging::debug(
                "stream",
                format!(
                    "first {}stream video forwarded: elapsed_ms={} source_timestamp={} local_timestamp={local_timestamp}",
                    if buffered { "buffered " } else { "" },
                    destination.media_started_at.elapsed().as_millis(),
                    frame.source_timestamp,
                ),
            );
        }
    }
}

fn elapsed_rtp_timestamp(elapsed: Duration, clock_rate: u32) -> u32 {
    let whole = elapsed.as_secs().wrapping_mul(u64::from(clock_rate));
    let fractional =
        u64::from(elapsed.subsec_nanos()).wrapping_mul(u64::from(clock_rate)) / 1_000_000_000;
    whole.wrapping_add(fractional) as u32
}

fn later_rtp_timestamp(left: u32, right: u32) -> u32 {
    if left.wrapping_sub(right) as i32 >= 0 {
        left
    } else {
        right
    }
}

fn build_rtcp_receiver_report(sender_ssrc: u32, block: Option<StreamRtcpReportBlock>) -> Vec<u8> {
    let report_count = u8::from(block.is_some());
    let mut packet = Vec::with_capacity(if block.is_some() { 32 } else { 8 });
    packet.extend_from_slice(&[
        (RTP_VERSION << 6) | report_count,
        RTCP_RECEIVER_REPORT,
        0,
        0,
    ]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    if let Some(block) = block {
        packet.extend_from_slice(&block.source_ssrc.to_be_bytes());
        packet.push(block.fraction_lost);
        let cumulative_lost = block
            .cumulative_lost
            .clamp(-0x80_0000, 0x7f_ffff)
            .to_be_bytes();
        packet.extend_from_slice(&cumulative_lost[1..]);
        packet.extend_from_slice(&block.extended_highest_sequence.to_be_bytes());
        packet.extend_from_slice(&block.interarrival_jitter.to_be_bytes());
        packet.extend_from_slice(&block.last_sender_report.to_be_bytes());
        packet.extend_from_slice(&block.delay_since_last_sender_report.to_be_bytes());
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("RTCP receiver report length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

fn build_rtcp_sdes_cname(sender_ssrc: u32) -> Vec<u8> {
    let cname = format!("concord-{sender_ssrc}");
    let cname_len = u8::try_from(cname.len()).expect("stream RTCP CNAME fits u8");
    let mut packet = Vec::with_capacity(12 + cname.len());
    packet.extend_from_slice(&[(RTP_VERSION << 6) | 1, RTCP_SOURCE_DESCRIPTION, 0, 0]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&[RTCP_SDES_CNAME, cname_len]);
    packet.extend_from_slice(cname.as_bytes());
    packet.push(0);
    while !packet.len().is_multiple_of(4) {
        packet.push(0);
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("RTCP SDES length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

fn build_stream_rtcp_compound(
    sender_ssrc: u32,
    block: Option<StreamRtcpReportBlock>,
    feedback: Option<&[u8]>,
) -> Vec<u8> {
    let mut packet = build_rtcp_receiver_report(sender_ssrc, block);
    packet.extend_from_slice(&build_rtcp_sdes_cname(sender_ssrc));
    if let Some(feedback) = feedback {
        packet.extend_from_slice(feedback);
    }
    packet
}

fn build_rtcp_pli(sender_ssrc: u32, media_ssrc: u32) -> [u8; 12] {
    let mut packet = [0u8; 12];
    packet[0] = (RTP_VERSION << 6) | RTCP_PLI_FORMAT;
    packet[1] = RTCP_PAYLOAD_SPECIFIC_FEEDBACK;
    packet[2..4].copy_from_slice(&RTCP_PLI_LENGTH_WORDS_MINUS_ONE.to_be_bytes());
    packet[4..8].copy_from_slice(&sender_ssrc.to_be_bytes());
    packet[8..12].copy_from_slice(&media_ssrc.to_be_bytes());
    packet
}

fn build_rtcp_nack(sender_ssrc: u32, media_ssrc: u32, missing: &[u16]) -> Vec<u8> {
    let mut feedback_control = Vec::new();
    let mut index = 0usize;
    while let Some(&packet_id) = missing.get(index) {
        index += 1;
        let mut bitmask = 0u16;
        while let Some(&sequence) = missing.get(index) {
            let distance = sequence.wrapping_sub(packet_id);
            if !(1..=16).contains(&distance) {
                break;
            }
            bitmask |= 1 << (distance - 1);
            index += 1;
        }
        feedback_control.extend_from_slice(&packet_id.to_be_bytes());
        feedback_control.extend_from_slice(&bitmask.to_be_bytes());
    }

    let mut packet = Vec::with_capacity(12 + feedback_control.len());
    packet.push((RTP_VERSION << 6) | RTCP_GENERIC_NACK_FORMAT);
    packet.push(RTCP_TRANSPORT_LAYER_FEEDBACK);
    let length_words_minus_one =
        u16::try_from((12 + feedback_control.len()) / 4 - 1).expect("RTCP NACK length fits u16");
    packet.extend_from_slice(&length_words_minus_one.to_be_bytes());
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&media_ssrc.to_be_bytes());
    packet.extend_from_slice(&feedback_control);
    packet
}

fn build_transport_wide_feedback(
    sender_ssrc: u32,
    media_ssrc: u32,
    base_sequence: u16,
    reference_time: i64,
    feedback_packet_count: u8,
    statuses: &[u8],
    deltas: &[StreamTransportReceiveDelta],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(20 + statuses.len() / 3 + deltas.len() * 2);
    packet.extend_from_slice(&[
        (RTP_VERSION << 6) | RTCP_TRANSPORT_WIDE_FEEDBACK_FORMAT,
        RTCP_TRANSPORT_LAYER_FEEDBACK,
        0,
        0,
    ]);
    packet.extend_from_slice(&sender_ssrc.to_be_bytes());
    packet.extend_from_slice(&media_ssrc.to_be_bytes());
    packet.extend_from_slice(&base_sequence.to_be_bytes());
    packet.extend_from_slice(
        &u16::try_from(statuses.len())
            .expect("transport feedback status count fits u16")
            .to_be_bytes(),
    );
    let reference_time = (reference_time as u32) & 0x00ff_ffff;
    packet.extend_from_slice(&reference_time.to_be_bytes()[1..]);
    packet.push(feedback_packet_count);

    // Two-bit status vectors are slightly larger than mixed RLE chunks, but
    // their fixed seven-packet shape keeps loss and reordered arrivals clear.
    for statuses in statuses.chunks(7) {
        let mut chunk = 0xc000u16;
        for (index, status) in statuses.iter().enumerate() {
            chunk |= u16::from(*status & 0x03) << (12 - index * 2);
        }
        packet.extend_from_slice(&chunk.to_be_bytes());
    }
    for delta in deltas {
        match delta {
            StreamTransportReceiveDelta::Small(delta) => packet.push(*delta),
            StreamTransportReceiveDelta::Large(delta) => {
                packet.extend_from_slice(&delta.to_be_bytes());
            }
        }
    }
    while !packet.len().is_multiple_of(4) {
        packet.push(0);
    }
    let length_words_minus_one =
        u16::try_from(packet.len() / 4 - 1).expect("transport feedback length fits u16");
    packet[2..4].copy_from_slice(&length_words_minus_one.to_be_bytes());
    packet
}

fn reset_stream_h264_pipeline(
    h264: &mut H264Depacketizer,
    startup: &mut H264StartupGate,
    startup_buffer: &mut H264StartupBuffer,
) {
    *h264 = H264Depacketizer::default();
    *startup = H264StartupGate::default();
    startup_buffer.clear();
}

async fn log_stream_player_output(
    kind: &'static str,
    output: &'static str,
    stream: impl AsyncRead + Unpin,
    last_error: Arc<Mutex<Option<String>>>,
    player_ready: Option<StreamPlayerReadySignal>,
) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if !line.trim().is_empty() => {
                let input_ready = stream_player_input_is_ready(&line);
                logging::debug("stream", format!("mpv {kind} {output}: {line}"));
                *last_error.lock().await = Some(line);
                if let Some(player_ready) = player_ready.as_ref()
                    && input_ready
                    && !player_ready.player_ready.swap(true, Ordering::AcqRel)
                {
                    logging::debug("stream", format!("stream {kind} mpv input ready"));
                    let _ = player_ready.ready_tx.send(player_ready.media_generation);
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => {
                logging::debug(
                    "stream",
                    format!("read mpv {kind} {output} failed: {error}"),
                );
                break;
            }
        }
    }
}

fn stream_player_ready_is_current(ready_generation: Option<u64>, media_generation: u64) -> bool {
    ready_generation == Some(media_generation)
}

fn stream_player_input_is_ready(line: &str) -> bool {
    line.contains("[cplayer] Opening done:")
}

struct ReservedLocalUdpPortPair {
    _rtp_socket: std::net::UdpSocket,
    _rtcp_socket: std::net::UdpSocket,
    rtp_port: u16,
    rtcp_port: u16,
}

fn reserve_local_udp_port_pair() -> Result<ReservedLocalUdpPortPair, String> {
    for _ in 0..64 {
        let rtp_socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("reserve local stream RTP port failed: {error}"))?;
        let rtp_port = rtp_socket
            .local_addr()
            .map_err(|error| format!("read local stream RTP port failed: {error}"))?
            .port();
        let Some(rtcp_port) = rtp_port
            .checked_add(1)
            .filter(|_| rtp_port.is_multiple_of(2))
        else {
            continue;
        };
        let Ok(rtcp_socket) = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, rtcp_port)) else {
            continue;
        };
        return Ok(ReservedLocalUdpPortPair {
            _rtp_socket: rtp_socket,
            _rtcp_socket: rtcp_socket,
            rtp_port,
            rtcp_port,
        });
    }
    Err("reserve adjacent local stream RTP and RTCP ports failed".to_owned())
}

fn stream_sdp(
    audio_port: u16,
    audio_rtcp_port: u16,
    video_port: u16,
    video_rtcp_port: u16,
) -> String {
    format!(
        "v=0\r\n\
         o=- 0 0 IN IP4 127.0.0.1\r\n\
         s=-\r\n\
         c=IN IP4 127.0.0.1\r\n\
         t=0 0\r\n\
         m=audio {audio_port} RTP/AVP {LOCAL_STREAM_AUDIO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{audio_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_AUDIO_PAYLOAD_TYPE} opus/48000/2\r\n\
         a=recvonly\r\n\
         m=video {video_port} RTP/AVP {LOCAL_STREAM_VIDEO_PAYLOAD_TYPE}\r\n\
         a=rtcp:{video_rtcp_port} IN IP4 127.0.0.1\r\n\
         a=rtpmap:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} H264/90000\r\n\
         a=fmtp:{LOCAL_STREAM_VIDEO_PAYLOAD_TYPE} packetization-mode=1\r\n\
         a=recvonly\r\n"
    )
}

fn build_local_rtp_packet(
    payload_type: u8,
    marker: bool,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + payload.len());
    packet.push(RTP_VERSION << 6);
    packet.push((u8::from(marker) << 7) | payload_type);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

#[derive(Default)]
struct H264Depacketizer {
    timestamp: Option<u32>,
    expected_sequence: Option<u16>,
    frame: Vec<u8>,
    packet_count: usize,
    fragment_open: bool,
}

enum H264DepacketizerOutput {
    Pending,
    Frame(Vec<u8>),
    BudgetExceeded,
}

enum H264AppendError {
    Invalid,
    BudgetExceeded,
}

#[derive(Default)]
struct H264StartupGate {
    started: bool,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

struct BufferedH264Frame {
    encoded: Vec<u8>,
    source_timestamp: u32,
}

#[derive(Default)]
struct H264StartupBuffer {
    frames: VecDeque<BufferedH264Frame>,
    bytes: usize,
}

impl H264StartupGate {
    fn is_started(&self) -> bool {
        self.started
    }

    fn accept(&mut self, frame: Vec<u8>) -> Option<Vec<u8>> {
        let has_idr = {
            let mut has_idr = false;
            for nal in annex_b_nals(&frame) {
                match nal.first().map(|byte| byte & 0x1f) {
                    Some(5) => has_idr = true,
                    Some(7) => {
                        self.sps = Some(nal.to_vec());
                    }
                    Some(8) => {
                        self.pps = Some(nal.to_vec());
                    }
                    _ => {}
                }
            }
            has_idr
        };

        if self.started {
            return Some(frame);
        }
        if !has_idr || self.sps.is_none() || self.pps.is_none() {
            return None;
        }

        self.started = true;
        let mut startup_frame = Vec::new();
        append_annex_b_nal(
            &mut startup_frame,
            self.sps.as_deref().expect("startup requires an SPS"),
        );
        append_annex_b_nal(
            &mut startup_frame,
            self.pps.as_deref().expect("startup requires a PPS"),
        );
        for nal in annex_b_nals(&frame) {
            if !matches!(nal.first().map(|byte| byte & 0x1f), Some(7 | 8)) {
                append_annex_b_nal(&mut startup_frame, nal);
            }
        }
        Some(startup_frame)
    }
}

impl H264StartupBuffer {
    fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    fn len(&self) -> usize {
        self.frames.len()
    }

    fn push(&mut self, frame: BufferedH264Frame) -> bool {
        let Some(next_bytes) = self.bytes.checked_add(frame.encoded.len()) else {
            self.clear();
            return false;
        };
        if self.frames.len() >= STREAM_STARTUP_BUFFER_MAX_FRAMES
            || next_bytes > STREAM_STARTUP_BUFFER_MAX_BYTES
        {
            self.clear();
            return false;
        }
        self.bytes = next_bytes;
        self.frames.push_back(frame);
        true
    }

    fn take(&mut self) -> VecDeque<BufferedH264Frame> {
        self.bytes = 0;
        std::mem::take(&mut self.frames)
    }

    fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
    }
}

fn accept_or_buffer_h264(
    player_ready: bool,
    startup: &mut H264StartupGate,
    startup_buffer: &mut H264StartupBuffer,
    frame: Vec<u8>,
    source_timestamp: u32,
) -> Option<BufferedH264Frame> {
    let frame = BufferedH264Frame {
        encoded: startup.accept(frame)?,
        source_timestamp,
    };
    if player_ready {
        return Some(frame);
    }
    if !startup_buffer.push(frame) {
        *startup = H264StartupGate::default();
    }
    None
}

fn append_annex_b_nal(frame: &mut Vec<u8>, nal: &[u8]) {
    frame.extend_from_slice(&[0, 0, 0, 1]);
    frame.extend_from_slice(nal);
}

fn h264_nal_types(frame: &[u8]) -> Vec<u8> {
    annex_b_nals(frame)
        .into_iter()
        .filter_map(|nal| nal.first().map(|byte| byte & 0x1f))
        .collect()
}

impl H264Depacketizer {
    fn push(&mut self, header: &RtpHeader, payload: &[u8]) -> H264DepacketizerOutput {
        if self.timestamp != Some(header.timestamp)
            || self
                .expected_sequence
                .is_some_and(|expected| expected != header.sequence)
        {
            self.reset(header.timestamp);
        }
        if self.packet_count >= STREAM_H264_ACCESS_UNIT_MAX_PACKETS {
            self.reset(header.timestamp);
            return H264DepacketizerOutput::BudgetExceeded;
        }
        self.packet_count += 1;
        self.expected_sequence = Some(header.sequence.wrapping_add(1));
        let Some(nal_type) = payload.first().map(|byte| byte & 0x1f) else {
            self.reset(header.timestamp);
            return H264DepacketizerOutput::Pending;
        };
        let accepted = match nal_type {
            1..=23 => self.append_nal(payload),
            24 => self.append_stap_a(payload),
            28 => self.append_fu_a(payload),
            _ => Err(H264AppendError::Invalid),
        };
        match accepted {
            Ok(()) => {}
            Err(H264AppendError::Invalid) => {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::Pending;
            }
            Err(H264AppendError::BudgetExceeded) => {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::BudgetExceeded;
            }
        }
        if header.marker {
            if self.fragment_open || self.frame.is_empty() {
                self.reset(header.timestamp);
                return H264DepacketizerOutput::Pending;
            }
            self.timestamp = None;
            self.expected_sequence = None;
            self.packet_count = 0;
            return H264DepacketizerOutput::Frame(std::mem::take(&mut self.frame));
        }
        H264DepacketizerOutput::Pending
    }

    fn reset(&mut self, timestamp: u32) {
        self.timestamp = Some(timestamp);
        self.expected_sequence = None;
        self.frame.clear();
        self.packet_count = 0;
        self.fragment_open = false;
    }

    fn append_nal(&mut self, nal: &[u8]) -> Result<(), H264AppendError> {
        self.ensure_capacity(4usize.saturating_add(nal.len()))?;
        self.frame.extend_from_slice(&[0, 0, 0, 1]);
        self.frame.extend_from_slice(nal);
        self.fragment_open = false;
        Ok(())
    }

    fn append_stap_a(&mut self, payload: &[u8]) -> Result<(), H264AppendError> {
        let mut cursor = 1usize;
        let mut required_bytes = 0usize;
        let mut nal_count = 0usize;
        while cursor + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[cursor], payload[cursor + 1]]));
            cursor += 2;
            let Some(nal) = payload.get(cursor..cursor.saturating_add(size)) else {
                return Err(H264AppendError::Invalid);
            };
            if nal.is_empty() {
                return Err(H264AppendError::Invalid);
            }
            required_bytes = required_bytes
                .checked_add(4)
                .and_then(|bytes| bytes.checked_add(nal.len()))
                .ok_or(H264AppendError::BudgetExceeded)?;
            cursor += size;
            nal_count += 1;
        }
        if nal_count == 0 || cursor != payload.len() {
            return Err(H264AppendError::Invalid);
        }
        self.ensure_capacity(required_bytes)?;

        cursor = 1;
        while cursor + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[cursor], payload[cursor + 1]]));
            cursor += 2;
            let nal = &payload[cursor..cursor + size];
            self.frame.extend_from_slice(&[0, 0, 0, 1]);
            self.frame.extend_from_slice(nal);
            cursor += size;
        }
        self.fragment_open = false;
        Ok(())
    }

    fn append_fu_a(&mut self, payload: &[u8]) -> Result<(), H264AppendError> {
        if payload.len() < 3 {
            return Err(H264AppendError::Invalid);
        }
        let indicator = payload[0];
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let header_bytes = if start { 5 } else { 0 };
        self.ensure_capacity(header_bytes + payload.len() - 2)?;
        if start {
            self.frame.extend_from_slice(&[0, 0, 0, 1]);
            self.frame.push((indicator & 0xe0) | (fu_header & 0x1f));
            self.fragment_open = !end;
        } else if !self.fragment_open {
            return Err(H264AppendError::Invalid);
        } else if end {
            self.fragment_open = false;
        }
        self.frame.extend_from_slice(&payload[2..]);
        Ok(())
    }

    fn ensure_capacity(&self, additional_bytes: usize) -> Result<(), H264AppendError> {
        if self
            .frame
            .len()
            .checked_add(additional_bytes)
            .is_some_and(|bytes| bytes <= STREAM_H264_ACCESS_UNIT_MAX_BYTES)
        {
            Ok(())
        } else {
            Err(H264AppendError::BudgetExceeded)
        }
    }
}

fn packetize_h264_frame(
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> Vec<Vec<u8>> {
    let payloads = packetize_h264_payloads(frame, LOCAL_H264_MAX_PAYLOAD_BYTES);
    let payload_count = payloads.len();
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let packet = build_local_rtp_packet(
                LOCAL_STREAM_VIDEO_PAYLOAD_TYPE,
                index + 1 == payload_count,
                *sequence,
                timestamp,
                ssrc,
                &payload,
            );
            *sequence = sequence.wrapping_add(1);
            packet
        })
        .collect()
}

async fn send_local_h264_frame(
    socket: &UdpSocket,
    target: SocketAddrV4,
    frame: &[u8],
    timestamp: u32,
    ssrc: u32,
    sequence: &mut u16,
) -> (u32, u32) {
    let packets = packetize_h264_frame(frame, timestamp, ssrc, sequence);
    let packet_count = u32::try_from(packets.len()).expect("H264 packet count fits u32");
    let octet_count = packets.iter().fold(0u32, |total, packet| {
        total.wrapping_add(packet.len().saturating_sub(RTP_HEADER_MIN_LEN) as u32)
    });
    for packet in packets {
        let _ = socket.send_to(&packet, target).await;
    }
    (packet_count, octet_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    struct TaskDropSignal(Option<oneshot::Sender<()>>);

    impl Drop for TaskDropSignal {
        fn drop(&mut self) {
            if let Some(dropped) = self.0.take() {
                let _ = dropped.send(());
            }
        }
    }

    fn cancellable_test_task() -> (JoinHandle<()>, oneshot::Receiver<()>, oneshot::Receiver<()>) {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _drop_signal = TaskDropSignal(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        (task, started_rx, dropped_rx)
    }

    #[tokio::test]
    async fn dropping_stream_child_tasks_aborts_every_child() {
        let (heartbeat, heartbeat_started, heartbeat_dropped) = cancellable_test_task();
        let (keepalive, keepalive_started, keepalive_dropped) = cancellable_test_task();
        let (media, media_started, media_dropped) = cancellable_test_task();
        let mut child_tasks = GatewayChildTasks::default();
        child_tasks.replace_heartbeat(heartbeat).await;
        child_tasks.replace_keepalive(keepalive).await;
        child_tasks.replace_media(media).await;

        heartbeat_started.await.expect("heartbeat test task starts");
        keepalive_started.await.expect("keepalive test task starts");
        media_started.await.expect("media test task starts");
        drop(child_tasks);

        for dropped in [heartbeat_dropped, keepalive_dropped, media_dropped] {
            timeout(Duration::from_secs(1), dropped)
                .await
                .expect("stream child task is aborted promptly")
                .expect("stream child task drop signal is sent");
        }
    }

    #[tokio::test]
    async fn dropping_stream_player_log_tasks_aborts_both_readers() {
        let (stdout, stdout_started, stdout_dropped) = cancellable_test_task();
        let (stderr, stderr_started, stderr_dropped) = cancellable_test_task();
        let log_tasks = StreamPlayerLogTasks::new([stdout, stderr]);

        stdout_started.await.expect("stdout test task starts");
        stderr_started.await.expect("stderr test task starts");
        drop(log_tasks);

        for dropped in [stdout_dropped, stderr_dropped] {
            timeout(Duration::from_secs(1), dropped)
                .await
                .expect("stream player log task is aborted promptly")
                .expect("stream player log task drop signal is sent");
        }
    }

    #[test]
    fn stream_player_readiness_accepts_only_the_current_media_generation() {
        assert!(stream_player_ready_is_current(Some(8), 8));
        assert!(!stream_player_ready_is_current(Some(7), 8));
        assert!(!stream_player_ready_is_current(None, 8));
    }

    fn stream_request() -> StreamWatchRequest {
        StreamWatchRequest {
            stream_key: "guild:10:20:99".to_owned(),
            scope: VoiceScope::Guild(Id::new(10)),
            channel_id: Id::new(20),
            owner_id: Id::new(99),
            display_name: "Streamer".to_owned(),
        }
    }

    fn current_voice_state() -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Some(Id::new(10)),
            channel_id: Some(Id::new(20)),
            user_id: Id::new(5),
            session_id: Some("parent-session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }

    fn connected_stream_runtime() -> (StreamRuntimeState, StreamGatewaySession) {
        let mut state = StreamRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()));
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(5))));
        state.apply(&VoiceRuntimeEvent::VoiceState(current_voice_state()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: "guild:10:20:99".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            viewer_ids: Vec::new(),
            paused: false,
        }));
        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: "guild:10:20:99".to_owned(),
            endpoint: Some("stream.example.com".to_owned()),
            token: "stream-token".to_owned(),
        }));
        let session = update.connect.expect("stream session should be ready");
        (state, session)
    }

    #[test]
    fn stream_gateway_session_debug_redacts_token() {
        let (_, session) = connected_stream_runtime();
        let debug = format!("{session:?}");

        assert!(!debug.contains("stream-token"));
        assert!(debug.contains("<redacted>"));
    }

    fn stream_connection_ended(
        session: &StreamGatewaySession,
        outcome: VoiceConnectionEnd,
    ) -> VoiceRuntimeEvent {
        VoiceRuntimeEvent::StreamConnectionEnded {
            connection_id: session.connection_id,
            stream_key: session.request.stream_key.clone(),
            outcome,
        }
    }

    #[test]
    fn h264_fu_a_round_trips_through_local_packetizer() {
        let frame = [0, 0, 0, 1, 0x65]
            .into_iter()
            .chain(std::iter::repeat_n(0xaa, 3000))
            .collect::<Vec<_>>();
        let mut sequence = 7;
        let packets = packetize_h264_frame(&frame, 90_000, 42, &mut sequence);
        assert!(packets.len() > 1);

        let mut depacketizer = H264Depacketizer::default();
        let mut decoded = None;
        for packet in packets {
            let header = parse_rtp_header(&packet).expect("local RTP packet is valid");
            if let H264DepacketizerOutput::Frame(frame) =
                depacketizer.push(&header, &packet[header.payload_offset..])
            {
                decoded = Some(frame);
            }
        }
        assert_eq!(decoded, Some(frame));
    }

    #[test]
    fn h264_assembly_enforces_packet_and_memory_budgets() {
        let mut oversized = H264Depacketizer::default();
        let mut oversized_header =
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 1, 90_000, 42);
        oversized_header.marker = false;
        let mut oversized_payload = vec![0xaa; STREAM_H264_ACCESS_UNIT_MAX_BYTES + 1];
        oversized_payload[0] = 0x7c;
        oversized_payload[1] = 0x85;
        assert!(matches!(
            oversized.push(&oversized_header, &oversized_payload),
            H264DepacketizerOutput::BudgetExceeded
        ));

        let mut fragmented = H264Depacketizer::default();
        let mut fragment_header =
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 1, 93_000, 42);
        fragment_header.marker = false;
        assert!(matches!(
            fragmented.push(&fragment_header, &[0x7c, 0x85, 0xaa]),
            H264DepacketizerOutput::Pending
        ));
        for sequence in 2..=STREAM_H264_ACCESS_UNIT_MAX_PACKETS {
            fragment_header.sequence = u16::try_from(sequence).expect("packet budget fits u16");
            assert!(matches!(
                fragmented.push(&fragment_header, &[0x7c, 0x05, 0xaa]),
                H264DepacketizerOutput::Pending
            ));
        }
        fragment_header.sequence =
            u16::try_from(STREAM_H264_ACCESS_UNIT_MAX_PACKETS + 1).expect("packet budget fits u16");
        assert!(matches!(
            fragmented.push(&fragment_header, &[0x7c, 0x45, 0xaa]),
            H264DepacketizerOutput::BudgetExceeded
        ));

        let mut startup = H264StartupBuffer::default();
        for timestamp in 0..32 {
            assert!(startup.push(BufferedH264Frame {
                encoded: vec![0; 1024 * 1024],
                source_timestamp: timestamp,
            }));
        }
        assert!(!startup.push(BufferedH264Frame {
            encoded: vec![0],
            source_timestamp: 32,
        }));
        assert!(startup.is_empty());
        assert_eq!(startup.bytes, 0);
    }

    fn stream_video_header(
        payload_type: u8,
        sequence: u16,
        timestamp: u32,
        ssrc: u32,
    ) -> RtpHeader {
        RtpHeader {
            has_padding: false,
            marker: true,
            payload_type,
            sequence,
            timestamp,
            ssrc,
            authenticated_header_len: RTP_HEADER_MIN_LEN,
            encrypted_extension_body_len: 0,
            payload_offset: RTP_HEADER_MIN_LEN,
        }
    }

    #[test]
    fn stream_video_reorders_primary_and_rtx_packets_before_depacketization() {
        let source = StreamVideoSource {
            audio_ssrc: 7,
            video_ssrc: 42,
            rtx_ssrc: Some(43),
            pixel_count: None,
        };
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();

        let first_payload = b"first".to_vec();
        let first_payload_ptr = first_payload.as_ptr();
        let first = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 10, 90_000, 42),
            first_payload,
            source,
        )
        .expect("primary video packet should be accepted");
        assert_eq!(first.payload.as_ptr(), first_payload_ptr);
        assert_eq!(
            recovery
                .push(first, now)
                .ready
                .into_iter()
                .map(|packet| packet.header.sequence)
                .collect::<Vec<_>>(),
            vec![10]
        );

        let third = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 12, 90_000, 42),
            b"third".to_vec(),
            source,
        )
        .expect("later primary video packet should be accepted");
        assert!(recovery.push(third, now).ready.is_empty());
        assert_eq!(recovery.take_nack_if_due(now), Some(vec![11]));

        let repaired = recover_stream_video_packet(
            stream_video_header(DISCORD_STREAM_VIDEO_RTX_PAYLOAD_TYPE, 800, 90_000, 43),
            vec![0, 11, b's', b'e', b'c', b'o', b'n', b'd'],
            source,
        )
        .expect("RTX video packet should recover its original packet");
        assert_eq!(repaired.header.sequence, 11);
        assert_eq!(repaired.header.ssrc, source.video_ssrc);
        assert_eq!(repaired.payload, b"second");
        assert_eq!(
            recovery
                .push(repaired, now)
                .ready
                .into_iter()
                .map(|packet| packet.header.sequence)
                .collect::<Vec<_>>(),
            vec![11, 12]
        );
        assert_eq!(recovery.take_nack_if_due(now), None);
    }

    #[test]
    fn stream_video_recovery_keeps_waiting_beyond_the_old_packet_window() {
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();
        let packet = |sequence, payload| RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, sequence, 90_000, 42),
            payload,
        };

        assert_eq!(recovery.push(packet(100, vec![1]), now).ready.len(), 1);
        assert!(recovery.push(packet(102, vec![2, 3]), now).ready.is_empty());
        let update = recovery.push(packet(230, vec![4]), now + Duration::from_millis(25));

        assert!(update.reset.is_none());
        assert!(update.ready.is_empty());
        assert_eq!(recovery.pending.len(), 2);
        assert_eq!(recovery.pending_bytes, 3);
    }

    #[test]
    fn stream_video_recovery_expires_an_unrepaired_gap() {
        let now = Instant::now();
        let mut recovery = StreamVideoRecovery::default();
        let first = RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 20, 90_000, 42),
            payload: vec![1],
        };
        let third = RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, 22, 90_000, 42),
            payload: vec![3],
        };

        assert_eq!(recovery.push(first, now).ready.len(), 1);
        assert!(recovery.push(third, now).ready.is_empty());
        assert_eq!(
            recovery.take_expired_gap(now + STREAM_VIDEO_GAP_TIMEOUT / 2),
            None
        );
        assert_eq!(
            recovery.take_expired_gap(now + STREAM_VIDEO_GAP_TIMEOUT),
            Some(StreamVideoRecoveryReset {
                distance: 1,
                pending_packets: 1,
                pending_bytes: 1,
                gap_age: Some(STREAM_VIDEO_GAP_TIMEOUT),
            })
        );
        assert!(recovery.pending.is_empty());
        assert_eq!(recovery.pending_bytes, 0);
    }

    #[test]
    fn stream_video_recovery_resets_only_when_a_pending_budget_is_exceeded() {
        let now = Instant::now();
        let packet = |sequence, payload| RecoveredStreamVideoPacket {
            header: stream_video_header(DISCORD_STREAM_VIDEO_PAYLOAD_TYPE, sequence, 90_000, 42),
            payload,
        };

        let mut packet_limited = StreamVideoRecovery::default();
        assert_eq!(packet_limited.push(packet(0, vec![0]), now).ready.len(), 1);
        for sequence in 2..=u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 1)
            .expect("video packet budget fits u16")
        {
            assert!(
                packet_limited
                    .push(packet(sequence, vec![0]), now)
                    .reset
                    .is_none()
            );
        }
        let packet_reset = packet_limited.push(
            packet(
                u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 2)
                    .expect("video packet budget fits u16"),
                vec![0],
            ),
            now,
        );
        assert_eq!(
            packet_reset.reset,
            Some(StreamVideoRecoveryReset {
                distance: u16::try_from(STREAM_VIDEO_MAX_PENDING_PACKETS + 1)
                    .expect("video packet budget fits u16"),
                pending_packets: STREAM_VIDEO_MAX_PENDING_PACKETS + 1,
                pending_bytes: STREAM_VIDEO_MAX_PENDING_PACKETS + 1,
                gap_age: Some(Duration::ZERO),
            })
        );

        let mut byte_limited = StreamVideoRecovery::default();
        assert_eq!(byte_limited.push(packet(0, vec![0]), now).ready.len(), 1);
        assert!(
            byte_limited
                .push(packet(2, vec![0; STREAM_VIDEO_MAX_PENDING_BYTES]), now)
                .reset
                .is_none()
        );
        let byte_reset = byte_limited.push(packet(3, vec![0]), now);
        assert_eq!(
            byte_reset.reset,
            Some(StreamVideoRecoveryReset {
                distance: 2,
                pending_packets: 2,
                pending_bytes: STREAM_VIDEO_MAX_PENDING_BYTES + 1,
                gap_age: Some(Duration::ZERO),
            })
        );
    }

    #[test]
    fn stream_generic_nack_groups_missing_sequences_into_pid_and_bitmask() {
        let nack = build_rtcp_nack(7, 42, &[1_000, 1_001, 1_003, 1_020]);

        assert_eq!(&nack[..4], &[0x81, 205, 0, 4]);
        assert_eq!(&nack[4..8], &7u32.to_be_bytes());
        assert_eq!(&nack[8..12], &42u32.to_be_bytes());
        assert_eq!(&nack[12..16], &[0x03, 0xe8, 0, 5]);
        assert_eq!(&nack[16..20], &[0x03, 0xfc, 0, 0]);
    }

    #[test]
    fn stream_pli_throttle_limits_the_active_video_source_to_one_request_per_second() {
        let now = Instant::now();
        let mut throttle = StreamPliThrottle::default();

        assert!(throttle.permit(42, now));
        assert!(!throttle.permit(
            42,
            now + STREAM_KEYFRAME_REQUEST_INTERVAL - Duration::from_millis(1)
        ));
        assert!(throttle.permit(42, now + STREAM_KEYFRAME_REQUEST_INTERVAL));
        assert!(throttle.permit(
            43,
            now + STREAM_KEYFRAME_REQUEST_INTERVAL + Duration::from_millis(1)
        ));
    }

    #[test]
    fn stream_transport_sequence_parses_native_one_byte_extensions() {
        let extensions = [0x30, 0xaa, 0, 0x51, 0x12, 0x34, 0];

        assert_eq!(
            parse_stream_transport_sequence(Some(RTP_ONE_BYTE_EXTENSION_PROFILE), &extensions),
            Some(0x1234)
        );
        assert_eq!(
            parse_stream_transport_sequence(
                Some(RTP_ONE_BYTE_EXTENSION_PROFILE),
                &[0x53, 0x12, 0x34, 0x80, 0x10],
            ),
            Some(0x1234)
        );
        assert_eq!(
            parse_stream_transport_sequence(Some(0x1000), &extensions),
            None
        );
        assert_eq!(
            parse_stream_transport_sequence(Some(RTP_ONE_BYTE_EXTENSION_PROFILE), &[0x50, 0x12],),
            None
        );
    }

    #[test]
    fn stream_transport_feedback_reports_loss_arrival_deltas_and_wrap() {
        assert_eq!(
            extend_transport_sequence(0, Some(u32::from(u16::MAX))),
            1 << 16
        );
        assert_eq!(
            extend_transport_sequence(u16::MAX, Some(1 << 16)),
            u32::from(u16::MAX)
        );

        let mut feedback = StreamTransportFeedback::default();
        feedback.observe(u16::MAX - 1, Duration::from_millis(64));
        feedback.observe(u16::MAX, Duration::from_micros(64_250));
        feedback.observe(1, Duration::from_millis(65));

        let packet = feedback
            .take_feedback(7, 42)
            .expect("received transport packets should produce feedback");
        assert_eq!(&packet[..4], &[0x8f, RTCP_TRANSPORT_LAYER_FEEDBACK, 0, 6]);
        assert_eq!(&packet[4..8], &7u32.to_be_bytes());
        assert_eq!(&packet[8..12], &42u32.to_be_bytes());
        assert_eq!(&packet[12..14], &(u16::MAX - 1).to_be_bytes());
        assert_eq!(&packet[14..16], &4u16.to_be_bytes());
        assert_eq!(&packet[16..19], &[0, 0, 1]);
        assert_eq!(packet[19], 0);
        assert_eq!(&packet[20..22], &0xd440u16.to_be_bytes());
        assert_eq!(&packet[22..25], &[0, 1, 3]);
        assert_eq!(&packet[25..], &[0, 0, 0]);
        assert!(feedback.take_feedback(7, 42).is_none());

        let compound = build_stream_rtcp_compound(7, None, Some(&packet));
        let key = [0x42; 32];
        let encryptor = VoiceRtpEncryptor::new(AEAD_AES256_GCM_RTPSIZE, &key)
            .expect("transport feedback encryptor should initialize");
        let encrypted = encryptor
            .encrypt_rtcp_feedback(&compound, 11u32.to_be_bytes())
            .expect("transport feedback should encrypt as compound RTCP");
        let decryptor = VoiceRtpDecryptor::new(AEAD_AES256_GCM_RTPSIZE, &key)
            .expect("transport feedback decryptor should initialize");
        assert_eq!(
            decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("transport feedback should decrypt"),
            compound
        );

        let mut reordered = StreamTransportFeedback::default();
        reordered.observe(11, Duration::from_millis(90));
        reordered.observe(10, Duration::from_millis(100));
        let packet = reordered
            .take_feedback(7, 42)
            .expect("reordered transport packets should produce feedback");
        assert_eq!(&packet[12..16], &[0, 10, 0, 2]);
        assert_eq!(&packet[20..22], &0xd800u16.to_be_bytes());
        assert_eq!(packet[22], 144);
        assert_eq!(&packet[23..25], &(-40i16).to_be_bytes());
    }

    #[test]
    fn stream_video_starts_at_idr_with_cached_parameter_sets() {
        let parameter_sets = vec![0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x33];
        let idr = vec![0, 0, 0, 1, 0x65, 0x44];
        let mut gate = H264StartupGate::default();

        assert_eq!(gate.accept(parameter_sets), None);
        assert_eq!(gate.accept(predicted.clone()), None);

        let startup = gate
            .accept(idr)
            .expect("IDR should start local video playback");
        assert_eq!(h264_nal_types(&startup), vec![7, 8, 5]);
        assert!(gate.is_started());
        assert_eq!(gate.accept(predicted.clone()), Some(predicted));
    }

    #[test]
    fn stream_video_waits_for_parameter_sets_before_accepting_idr() {
        let idr = vec![0, 0, 0, 1, 0x65, 0x44];
        let parameter_sets = vec![0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22];
        let mut gate = H264StartupGate::default();

        assert_eq!(gate.accept(idr.clone()), None);
        assert!(!gate.is_started());
        assert_eq!(gate.accept(parameter_sets), None);

        let startup = gate
            .accept(idr)
            .expect("IDR should start after parameter sets arrive");
        assert_eq!(h264_nal_types(&startup), vec![7, 8, 5]);
        assert!(gate.is_started());
    }

    #[test]
    fn stream_video_replays_the_initial_gop_after_player_readiness() {
        let startup_frame = vec![
            0, 0, 0, 1, 0x67, 0x11, 0, 0, 0, 1, 0x68, 0x22, 0, 0, 0, 1, 0x65, 0x33,
        ];
        let predicted = vec![0, 0, 0, 1, 0x41, 0x44];
        let mut gate = H264StartupGate::default();
        let mut buffer = H264StartupBuffer::default();

        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, startup_frame.clone(), 90_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert!(gate.is_started());
        assert_eq!(buffer.len(), 1);
        assert_eq!(
            accept_or_buffer_h264(false, &mut gate, &mut buffer, predicted.clone(), 93_000,)
                .map(|frame| frame.encoded),
            None
        );
        assert_eq!(buffer.len(), 2);
        assert_eq!(
            buffer
                .frames
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((startup_frame, 90_000))
        );
        assert_eq!(
            buffer
                .frames
                .pop_front()
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted.clone(), 93_000))
        );
        assert_eq!(
            accept_or_buffer_h264(true, &mut gate, &mut buffer, predicted.clone(), 96_000)
                .map(|frame| (frame.encoded, frame.source_timestamp)),
            Some((predicted, 96_000))
        );

        let mut clock = LocalRtpClock::default();
        clock.anchor(93_000, 10_000);
        assert_eq!(
            clock.rebase(96_000, Duration::from_secs(10), VIDEO_RTP_CLOCK_RATE, None,),
            13_000
        );
        assert!(stream_player_input_is_ready(
            "[cplayer] Opening done: /tmp/concord-video.sdp"
        ));
        assert!(!stream_player_input_is_ready(
            "[cplayer] Starting playback..."
        ));
    }

    #[tokio::test]
    async fn local_video_forwarder_replays_then_continues_the_source_clock() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("test sender should bind");
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("test receiver should bind");
        let target = match receiver
            .local_addr()
            .expect("test receiver should have an address")
        {
            std::net::SocketAddr::V4(target) => target,
            std::net::SocketAddr::V6(_) => panic!("test receiver should use IPv4"),
        };
        let media_started_at = Instant::now();
        let destination = LocalStreamVideoDestination {
            socket: &socket,
            target,
            ssrc: 42,
            media_started_at,
        };
        let mut startup = H264StartupBuffer::default();
        assert!(startup.push(BufferedH264Frame {
            encoded: vec![0, 0, 0, 1, 0x65, 0x11],
            source_timestamp: 90_000,
        }));
        assert!(startup.push(BufferedH264Frame {
            encoded: vec![0, 0, 0, 1, 0x41, 0x22],
            source_timestamp: 93_000,
        }));
        let mut forwarder = LocalStreamVideoForwarder::default();

        forwarder.replay_startup(&mut startup, &destination).await;
        forwarder
            .forward_live(
                &destination,
                &BufferedH264Frame {
                    encoded: vec![0, 0, 0, 1, 0x41, 0x33],
                    source_timestamp: 96_000,
                },
                None,
            )
            .await;
        let mut timestamps = Vec::new();
        let mut packet = [0u8; 1500];
        for _ in 0..3 {
            let (received, _) = timeout(Duration::from_secs(1), receiver.recv_from(&mut packet))
                .await
                .expect("local video packet should arrive")
                .expect("local video packet should be readable");
            timestamps.push(
                parse_rtp_header(&packet[..received])
                    .expect("local video packet should contain a valid RTP header")
                    .timestamp,
            );
        }

        assert!(startup.is_empty());
        assert_eq!(forwarder.frames, 3);
        assert_eq!(timestamps[2], timestamps[1].wrapping_add(3_000));
    }

    #[test]
    fn stream_keyframe_request_uses_encrypted_compound_rtcp() {
        let sender_ssrc = 0x0102_0304;
        let media_ssrc = 0x0506_0708;
        let pli = build_rtcp_pli(sender_ssrc, media_ssrc);
        let feedback = build_stream_rtcp_compound(sender_ssrc, None, Some(&pli));
        assert_eq!(&feedback[..4], &[0x80, 201, 0, 1]);
        assert_eq!(&feedback[4..8], &sender_ssrc.to_be_bytes());

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 9u32.to_be_bytes())
                .expect("RTCP feedback should encrypt");
            assert_eq!(&encrypted[..8], &feedback[..8]);
            assert_eq!(
                encrypted.len(),
                feedback.len() + RTP_AEAD_TAG_BYTES + RTP_AEAD_NONCE_SUFFIX_BYTES
            );

            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("RTCP feedback body should decrypt");
            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    fn stream_compound_rtcp_reports_source_and_round_trips() {
        let sender_ssrc = 7;
        let media_ssrc = 42;
        let mut control = StreamRtcpControl::default();
        control.set_source(media_ssrc);
        for (sequence, timestamp, arrival) in [
            (u16::MAX - 1, 0, Duration::ZERO),
            (u16::MAX, 900, Duration::from_millis(10)),
            (0, 1_800, Duration::from_millis(20)),
        ] {
            control.observe_rtp(sequence, timestamp, arrival);
        }
        control.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: media_ssrc,
                ntp_timestamp: 0x0102_0304_0506_0708,
                rtp_timestamp: 1_800,
                packet_count: 3,
                octet_count: 3_000,
            },
            Duration::from_secs(1),
        );
        let block = control
            .report_block(Duration::from_millis(1_500))
            .expect("received video should produce a report block");
        assert_eq!(block.source_ssrc, media_ssrc);
        assert_eq!(block.fraction_lost, 0);
        assert_eq!(block.cumulative_lost, 0);
        assert_eq!(block.extended_highest_sequence, 1 << 16);
        assert_eq!(block.interarrival_jitter, 0);
        assert_eq!(block.last_sender_report, 0x0304_0506);
        assert_eq!(block.delay_since_last_sender_report, 1 << 15);

        let pli = build_rtcp_pli(sender_ssrc, media_ssrc);
        let feedback = build_stream_rtcp_compound(sender_ssrc, Some(block), Some(&pli));
        assert_eq!(&feedback[..4], &[0x81, 201, 0, 7]);
        assert_eq!(&feedback[4..8], &sender_ssrc.to_be_bytes());
        assert_eq!(&feedback[8..12], &media_ssrc.to_be_bytes());
        assert_eq!(&feedback[13..16], &[0, 0, 0]);
        assert_eq!(&feedback[16..20], &(1u32 << 16).to_be_bytes());
        assert_eq!(&feedback[20..24], &0u32.to_be_bytes());
        assert_eq!(&feedback[24..28], &0x0304_0506u32.to_be_bytes());
        assert_eq!(&feedback[28..32], &(1u32 << 15).to_be_bytes());
        assert_eq!(&feedback[32..36], &[0x81, RTCP_SOURCE_DESCRIPTION, 0, 4]);
        assert_eq!(&feedback[36..40], &sender_ssrc.to_be_bytes());
        assert_eq!(&feedback[40..42], &[RTCP_SDES_CNAME, 9]);
        assert_eq!(&feedback[42..51], b"concord-7");
        assert_eq!(feedback[51], 0);
        assert_eq!(&feedback[52..], &pli);

        let mut packet_types = Vec::new();
        let mut offset = 0;
        while offset < feedback.len() {
            packet_types.push(feedback[offset + 1]);
            let length_words_minus_one =
                u16::from_be_bytes([feedback[offset + 2], feedback[offset + 3]]);
            offset += (usize::from(length_words_minus_one) + 1) * 4;
        }
        assert_eq!(offset, feedback.len());
        assert_eq!(
            packet_types,
            vec![
                RTCP_RECEIVER_REPORT,
                RTCP_SOURCE_DESCRIPTION,
                RTCP_PAYLOAD_SPECIFIC_FEEDBACK,
            ]
        );

        for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
            let key = [0x42; 32];
            let encryptor =
                VoiceRtpEncryptor::new(mode, &key).expect("feedback encryptor should initialize");
            let encrypted = encryptor
                .encrypt_rtcp_feedback(&feedback, 10u32.to_be_bytes())
                .expect("compound RTCP feedback should encrypt");
            let decryptor =
                VoiceRtpDecryptor::new(mode, &key).expect("feedback decryptor should initialize");
            let decrypted = decryptor
                .decrypt_rtcp_feedback(&encrypted)
                .expect("compound RTCP feedback should decrypt");

            assert_eq!(decrypted, feedback);
        }
    }

    #[test]
    fn stream_receiver_report_measures_interval_loss_and_jitter() {
        let mut control = StreamRtcpControl::default();
        control.set_source(42);
        control.observe_rtp(10, 0, Duration::ZERO);
        control.observe_rtp(12, 1_800, Duration::from_millis(30));

        let first = control
            .report_block(Duration::from_secs(1))
            .expect("received video should produce a report block");
        assert_eq!(first.fraction_lost, 85);
        assert_eq!(first.cumulative_lost, 1);
        assert_eq!(first.interarrival_jitter, 56);

        control.observe_rtp(11, 900, Duration::from_millis(35));
        let repaired = control
            .report_block(Duration::from_secs(2))
            .expect("late video should update the next report interval");
        assert_eq!(repaired.fraction_lost, 0);
        assert_eq!(repaired.cumulative_lost, 0);
    }

    #[test]
    fn local_sender_report_maps_rtp_to_a_shared_ntp_clock() {
        let report = build_rtcp_sender_report(
            0x0102_0304,
            Duration::new(1, 500_000_000),
            90_000,
            30,
            45_000,
        );

        assert_eq!(&report[..4], &[0x80, 200, 0, 6]);
        assert_eq!(&report[4..8], &0x0102_0304u32.to_be_bytes());
        assert_eq!(&report[8..12], &2_208_988_801u32.to_be_bytes());
        assert_eq!(&report[12..16], &0x8000_0000u32.to_be_bytes());
        assert_eq!(&report[16..20], &90_000u32.to_be_bytes());
        assert_eq!(&report[20..24], &30u32.to_be_bytes());
        assert_eq!(&report[24..28], &45_000u32.to_be_bytes());
    }

    #[test]
    fn stream_parses_sender_reports_from_compound_rtcp() {
        let sender_ssrc = 0x0102_0304;
        let mut compound = build_rtcp_receiver_report(7, None);
        compound.extend_from_slice(&build_rtcp_sender_report(
            sender_ssrc,
            Duration::new(1, 500_000_000),
            90_000,
            30,
            45_000,
        ));

        let reports = parse_stream_rtcp_sender_reports(&compound)
            .expect("compound RTCP should contain a valid sender report");

        assert_eq!(
            reports,
            vec![StreamRtcpSenderReport {
                sender_ssrc,
                ntp_timestamp: (u64::from(2_208_988_801u32) << 32) | 0x8000_0000,
                rtp_timestamp: 90_000,
                packet_count: 30,
                octet_count: 45_000,
            }]
        );
    }

    #[test]
    fn stream_rejects_a_truncated_rtcp_sender_report() {
        let mut report = build_rtcp_sender_report(42, Duration::from_secs(1), 90_000, 30, 45_000);
        report[2..4].copy_from_slice(&100u16.to_be_bytes());

        assert_eq!(
            parse_stream_rtcp_sender_reports(&report),
            Err("RTCP packet length exceeds the compound packet".to_owned())
        );
    }

    #[test]
    fn stream_presentation_clock_aligns_audio_and_video_sender_time() {
        let ntp_timestamp = (u64::from(2_208_988_801u32) << 32) | 0x8000_0000;
        let mut clock = StreamPresentationClock::default();
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 7,
                ntp_timestamp,
                rtp_timestamp: 3_000_000,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(100),
        );
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 42,
                ntp_timestamp,
                rtp_timestamp: 90_000_000,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(105),
        );

        let synchronized_audio = clock.map_timestamp(7, 3_004_800, OPUS_RTP_CLOCK_RATE);
        let synchronized_video = clock.map_timestamp(42, 90_009_000, VIDEO_RTP_CLOCK_RATE);
        assert_eq!(synchronized_audio, Some(9_600));
        assert_eq!(synchronized_video, Some(18_000));

        let mut local_audio = LocalStreamAudioClock::default();
        let mut local_video = LocalRtpClock::default();
        assert_eq!(
            local_audio
                .rebase(3_004_800, Duration::from_millis(205), synchronized_audio,)
                .local_timestamp,
            9_600
        );
        assert_eq!(
            local_video.rebase(
                90_009_000,
                Duration::from_millis(205),
                VIDEO_RTP_CLOCK_RATE,
                synchronized_video,
            ),
            18_000
        );
    }

    #[test]
    fn stream_presentation_clock_preserves_rtp_timestamp_wrap() {
        let mut clock = StreamPresentationClock::default();
        clock.observe_sender_report(
            StreamRtcpSenderReport {
                sender_ssrc: 42,
                ntp_timestamp: u64::from(2_208_988_801u32) << 32,
                rtp_timestamp: u32::MAX - 8_999,
                packet_count: 0,
                octet_count: 0,
            },
            Duration::from_millis(100),
        );

        assert_eq!(
            clock.map_timestamp(42, 0, VIDEO_RTP_CLOCK_RATE),
            Some(18_000)
        );
    }

    #[test]
    fn stream_sdp_keeps_audio_and_video_on_one_input() {
        let sdp = stream_sdp(50_000, 50_001, 50_002, 50_003);

        assert!(sdp.contains("m=audio 50000 RTP/AVP 111\r\n"));
        assert!(sdp.contains("a=rtcp:50001 IN IP4 127.0.0.1\r\n"));
        assert!(sdp.contains("m=video 50002 RTP/AVP 96\r\n"));
        assert!(sdp.contains("a=rtcp:50003 IN IP4 127.0.0.1\r\n"));
    }

    #[test]
    fn stream_video_source_prefers_highest_active_quality() {
        let value = json!({
            "op": 12,
            "d": {
                "user_id": "99",
                "audio_ssrc": 10,
                "video_ssrc": 20,
                "streams": [
                    {"ssrc": 20, "rtx_ssrc": 21, "quality": 50, "active": true},
                    {
                        "ssrc": 30,
                        "rtx_ssrc": 31,
                        "quality": 100,
                        "active": true,
                        "max_resolution": {"type": "fixed", "width": 2560, "height": 1440}
                    }
                ]
            }
        });
        assert_eq!(
            parse_stream_video_source(&value, Id::new(99)),
            Some(StreamVideoSource {
                audio_ssrc: 10,
                video_ssrc: 30,
                rtx_ssrc: Some(31),
                pixel_count: Some(2560 * 1440),
            })
        );
    }

    #[test]
    fn stream_runtime_waits_for_parent_voice_and_both_stream_events() {
        let mut state = StreamRuntimeState::default();
        assert!(
            state
                .apply(&VoiceRuntimeEvent::WatchStreamRequested(stream_request()))
                .connect
                .is_none()
        );
        state.apply(&VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(5))));
        state.apply(&VoiceRuntimeEvent::VoiceState(current_voice_state()));
        state.apply(&VoiceRuntimeEvent::StreamCreate(StreamCreateInfo {
            stream_key: "guild:10:20:99".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            viewer_ids: Vec::new(),
            paused: false,
        }));

        let update = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: "guild:10:20:99".to_owned(),
            endpoint: Some("stream.example.com".to_owned()),
            token: "stream-token".to_owned(),
        }));
        let session = update.connect.expect("stream session should now be ready");
        assert_eq!(session.session_id, "parent-session");
        assert_eq!(session.rtc_server_id, "400");
        assert_eq!(session.rtc_channel_id, Id::new(401));
        assert_eq!(session.request.owner_id, Id::new(99));
    }

    #[test]
    fn stream_runtime_pending_cancel_ends_playback_once() {
        let request = stream_request();
        let mut state = StreamRuntimeState::default();
        state.apply(&VoiceRuntimeEvent::WatchStreamRequested(request.clone()));

        let cancelled = state.apply(&VoiceRuntimeEvent::WatchStreamCancelled {
            stream_key: request.stream_key.clone(),
        });
        let ended = cancelled
            .playback_ended
            .expect("pending stream cancellation ends preparing playback");
        assert_eq!(ended.request, request);
        assert!(!ended.reconnecting);

        let repeated = state.apply(&VoiceRuntimeEvent::WatchStreamCancelled {
            stream_key: ended.request.stream_key,
        });
        assert!(repeated.playback_ended.is_none());
    }

    #[test]
    fn stream_runtime_rotates_active_stream_servers() {
        let (mut state, initial) = connected_stream_runtime();

        let rotated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("replacement.example.com".to_owned()),
            token: "replacement-token".to_owned(),
        }));
        assert_eq!(
            rotated.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!rotated.send_delete);
        assert!(
            rotated
                .playback_ended
                .as_ref()
                .is_some_and(|ended| ended.reconnecting)
        );
        let replacement = rotated
            .connect
            .expect("new stream server starts a replacement connection");
        assert_eq!(replacement.endpoint, "replacement.example.com");
        assert_eq!(replacement.token, "replacement-token");

        let unavailable = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: None,
            token: "pending-token".to_owned(),
        }));
        assert_eq!(
            unavailable.close_stream_key.as_deref(),
            Some(initial.request.stream_key.as_str())
        );
        assert!(!unavailable.send_delete);
        assert!(unavailable.connect.is_none());

        let reallocated = state.apply(&VoiceRuntimeEvent::StreamServer(StreamServerInfo {
            stream_key: initial.request.stream_key.clone(),
            endpoint: Some("reallocated.example.com".to_owned()),
            token: "reallocated-token".to_owned(),
        }));
        let active = reallocated
            .connect
            .expect("reallocated stream server reconnects");
        assert_ne!(active.connection_id, replacement.connection_id);

        let stale_end = state.apply(&stream_connection_ended(
            &replacement,
            VoiceConnectionEnd::Stop,
        ));
        assert!(stale_end.close_stream_key.is_none());
        assert!(stale_end.connect.is_none());
        assert_eq!(
            state
                .active
                .as_ref()
                .expect("reallocated connection remains active")
                .connection_id,
            active.connection_id
        );
    }

    #[test]
    fn stream_runtime_stops_after_consecutive_pre_playback_failures() {
        let (mut state, mut active) = connected_stream_runtime();

        for attempt in 1..=MAX_VOICE_RECONNECT_ATTEMPTS {
            let update = state.apply(&stream_connection_ended(
                &active,
                VoiceConnectionEnd::Reconnect,
            ));
            assert!(
                update.close_stream_key.is_none(),
                "retry {attempt} should keep the watch request active"
            );
            active = update
                .connect
                .expect("retry within the limit should reconnect");
        }

        let stopped = state.apply(&stream_connection_ended(
            &active,
            VoiceConnectionEnd::Reconnect,
        ));
        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    fn stream_runtime_stops_immediately_after_terminal_failure() {
        let (mut state, active) = connected_stream_runtime();

        let stopped = state.apply(&stream_connection_ended(&active, VoiceConnectionEnd::Stop));

        assert!(stopped.connect.is_none());
        assert_eq!(
            stopped.close_stream_key.as_deref(),
            Some(active.request.stream_key.as_str())
        );
        assert!(stopped.send_delete);
    }

    #[test]
    fn stream_runtime_resets_retries_only_after_stable_playback() {
        let (mut state, initial) = connected_stream_runtime();
        let first_retry = state.apply(&stream_connection_ended(
            &initial,
            VoiceConnectionEnd::Reconnect,
        ));
        let mut active = first_retry
            .connect
            .expect("the first transport failure should reconnect");

        state.apply(&VoiceRuntimeEvent::StreamConnectionEstablished {
            connection_id: active.connection_id,
            stream_key: active.request.stream_key.clone(),
        });

        for _ in 0..MAX_VOICE_RECONNECT_ATTEMPTS {
            active = state
                .apply(&stream_connection_ended(
                    &active,
                    VoiceConnectionEnd::Reconnect,
                ))
                .connect
                .expect("stable playback should restore the full retry budget");
        }
    }

    #[test]
    fn stream_failures_distinguish_player_and_transport_errors() {
        for error in [
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not executable"),
        ] {
            let failure = stream_player_spawn_failure(error);
            assert_eq!(failure.outcome, VoiceConnectionEnd::Stop);
        }
        assert_eq!(
            StreamConnectionFailure::from("stream UDP receive failed".to_owned()).outcome,
            VoiceConnectionEnd::Reconnect
        );
    }

    #[test]
    fn stream_gateway_payloads_request_stream_audio_and_h264() {
        let session = StreamGatewaySession {
            connection_id: 1,
            request: stream_request(),
            current_user_id: Id::new(5),
            session_id: "parent-session".to_owned(),
            rtc_server_id: "400".to_owned(),
            rtc_channel_id: Id::new(401),
            endpoint: "stream.example.com".to_owned(),
            token: "stream-token".to_owned(),
            reconnect_delay: Duration::ZERO,
        };
        let identify: Value =
            serde_json::from_str(&stream_identify_payload(&session)).expect("valid identify json");
        assert_eq!(identify["d"]["server_id"], "400");
        assert_eq!(identify["d"]["channel_id"], "401");
        assert_eq!(identify["d"]["video"], true);
        assert!(identify["d"].get("streams").is_none());

        let selected: Value = serde_json::from_str(&stream_select_protocol_payload(
            &DiscoveredVoiceAddress {
                address: "127.0.0.1".to_owned(),
                port: 5000,
            },
            AEAD_XCHACHA20_POLY1305_RTPSIZE,
        ))
        .expect("valid select protocol json");
        assert_eq!(selected["d"]["codecs"][0]["name"], "opus");
        assert_eq!(selected["d"]["codecs"][0]["payload_type"], 120);
        assert_eq!(selected["d"]["codecs"][0]["encode"], false);
        assert_eq!(selected["d"]["codecs"][0]["decode"], true);
        assert_eq!(selected["d"]["codecs"][1]["name"], "H264");
        assert_eq!(selected["d"]["codecs"][1]["payload_type"], 103);
        assert_eq!(selected["d"]["codecs"][1]["decode"], true);

        let wants: Value = serde_json::from_str(&stream_media_sink_wants_payload(
            800,
            900,
            Some(2560 * 1440),
        ))
        .expect("valid media sink wants json");
        assert_eq!(
            wants,
            json!({
                "op": 15,
                "d": {
                    "800": 100,
                    "900": 100,
                    "any": 0,
                    "pixelCounts": {"900": 2560 * 1440}
                }
            })
        );
    }

    #[test]
    fn stream_player_controls_the_complete_broadcast() {
        let player = stream_player_command(
            Path::new("/tmp/concord-stream.sdp"),
            Path::new("/tmp/concord-stream-input.conf"),
            "neo",
            "/tmp/concord-stream-mpv.sock",
        )
        .as_std()
        .get_args()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert_eq!(
            player,
            vec![
                "--no-config",
                "--terminal=yes",
                "--load-scripts=no",
                "--osc=no",
                "--msg-level=all=warn,cplayer=v,lavf=v,vd=v,ad=v",
                "--aid=no",
                "--input-ipc-server=/tmp/concord-stream-mpv.sock",
                "--input-conf=/tmp/concord-stream-input.conf",
                "--hwdec=auto-safe",
                "--vd-lavc-threads=0",
                "--stream-buffer-size=1MiB",
                "--audio-buffer=0.05",
                "--cache=yes",
                "--cache-pause=no",
                "--cache-pause-initial=no",
                "--cache-secs=0.15",
                "--demuxer-readahead-secs=0.15",
                "--demuxer-max-bytes=16MiB",
                "--demuxer-max-back-bytes=0",
                "--demuxer=lavf",
                "--demuxer-lavf-format=sdp",
                "--demuxer-lavf-probe-info=nostreams",
                "--demuxer-lavf-analyzeduration=0.1",
                "--demuxer-lavf-probesize=32",
                "--demuxer-lavf-buffersize=262144",
                "--demuxer-lavf-o=protocol_whitelist=[file,udp,rtp],buffer_size=4194304,max_delay=50000,reorder_queue_size=512",
                "--force-window=immediate",
                "--auto-window-resize=no",
                "--geometry=1280x720",
                "--title=Concord - neo's stream",
                "--video-latency-hacks=no",
                "--video-sync=audio",
                "--framedrop=vo",
                "--video-timing-offset=0",
                "--",
                "/tmp/concord-stream.sdp",
            ]
        );
        assert_eq!(
            STREAM_PLAYER_INPUT_CONFIG,
            "SPACE ignore\np ignore\nPAUSE ignore\nPLAYPAUSE ignore\nPAUSEONLY ignore\nXF86_PAUSE ignore\n. ignore\n, ignore\n"
        );
    }

    #[test]
    fn stream_player_audio_waits_for_real_media_and_player_readiness() {
        let mut audio = StreamPlayerAudioState::default();

        assert!(!audio.take_enable_request(false));
        assert!(!audio.take_enable_request(true));

        audio.observe_real_packet();
        assert!(!audio.take_enable_request(false));
        assert!(audio.take_enable_request(true));
        assert!(
            !audio.take_enable_request(true),
            "audio should be enabled only once"
        );
    }

    fn recovered_stream_audio_packet(sequence: u16, timestamp: u32) -> RecoveredStreamAudioPacket {
        RecoveredStreamAudioPacket {
            marker: false,
            sequence,
            timestamp,
            opus: vec![sequence as u8],
        }
    }

    #[test]
    fn stream_audio_recovery_orders_delayed_packets_and_drops_duplicates() {
        let now = Instant::now();
        let mut recovery = StreamAudioRecovery::default();

        assert!(
            recovery
                .push(recovered_stream_audio_packet(10, 10_000), now)
                .ready
                .is_empty()
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(12, 11_920),
                    now + Duration::from_millis(20),
                )
                .ready
                .is_empty()
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(11, 10_960),
                    now + Duration::from_millis(40),
                )
                .ready
                .is_empty()
        );

        let update = recovery.poll(now + STREAM_AUDIO_REORDER_DELAY);
        assert_eq!(
            update
                .ready
                .iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![10, 11, 12]
        );
        assert_eq!(update.skipped_sequences, 0);
        assert_eq!(update.dropped_stale_packets, 0);

        let duplicate = recovery.push(
            recovered_stream_audio_packet(12, 11_920),
            now + STREAM_AUDIO_REORDER_DELAY,
        );
        assert!(duplicate.ready.is_empty());
        assert_eq!(duplicate.dropped_stale_packets, 1);
    }

    #[test]
    fn stream_audio_recovery_exposes_expired_gaps_across_sequence_wrap() {
        let now = Instant::now();
        let mut recovery = StreamAudioRecovery::default();

        assert!(
            recovery
                .push(recovered_stream_audio_packet(u16::MAX - 1, 10_000), now,)
                .ready
                .is_empty()
        );
        assert_eq!(
            recovery
                .poll(now + STREAM_AUDIO_REORDER_DELAY)
                .ready
                .into_iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![u16::MAX - 1]
        );
        assert!(
            recovery
                .push(
                    recovered_stream_audio_packet(0, 11_920),
                    now + STREAM_AUDIO_REORDER_DELAY + Duration::from_millis(20),
                )
                .ready
                .is_empty()
        );

        let update =
            recovery.poll(now + STREAM_AUDIO_REORDER_DELAY * 2 + Duration::from_millis(20));
        assert_eq!(update.skipped_sequences, 1);
        assert_eq!(
            update
                .ready
                .iter()
                .map(|packet| packet.sequence)
                .collect::<Vec<_>>(),
            vec![0]
        );

        let late = recovery.push(
            recovered_stream_audio_packet(u16::MAX, 10_960),
            now + STREAM_AUDIO_REORDER_DELAY * 2 + Duration::from_millis(20),
        );
        assert_eq!(late.dropped_stale_packets, 1);
        assert!(late.ready.is_empty());
    }

    #[test]
    fn local_rtp_clocks_share_live_time_without_source_clock_offsets() {
        let mut audio = LocalStreamAudioClock::default();
        let mut video = LocalRtpClock::default();

        assert_eq!(
            audio
                .rebase(3_000_000, Duration::from_millis(100), None)
                .local_timestamp,
            4_800
        );
        assert_eq!(
            video.rebase(
                90_000_000,
                Duration::from_millis(125),
                VIDEO_RTP_CLOCK_RATE,
                None,
            ),
            11_250
        );
        assert_eq!(
            audio
                .rebase(3_000_960, Duration::from_millis(120), None)
                .local_timestamp,
            5_760
        );
        assert_eq!(
            video.rebase(
                90_003_000,
                Duration::from_millis(158),
                VIDEO_RTP_CLOCK_RATE,
                None,
            ),
            14_250
        );
    }

    #[test]
    fn stream_audio_clock_reanchors_a_backward_source_timestamp() {
        let mut clock = LocalStreamAudioClock::default();

        let first = clock.rebase(3_181_287_385, Duration::ZERO, None);
        assert_eq!(first.local_timestamp, 0);
        assert!(first.discontinuity.is_none());

        let before_reset = clock.rebase(3_181_475_545, Duration::from_millis(3_920), None);
        assert_eq!(before_reset.local_timestamp, 188_160);
        assert!(before_reset.discontinuity.is_none());

        let reset = clock.rebase(3_181_114_584, Duration::from_millis(3_940), None);
        assert_eq!(reset.local_timestamp, 189_120);
        assert_eq!(
            reset.discontinuity,
            Some(StreamAudioClockDiscontinuity {
                source_delta_ticks: -360_961,
                elapsed_delta_ticks: 960,
            })
        );

        let after_reset = clock.rebase(3_181_115_544, Duration::from_millis(3_960), None);
        assert_eq!(after_reset.local_timestamp, 190_080);
        assert!(after_reset.discontinuity.is_none());
        assert_eq!(
            clock.timestamp_at(Duration::from_millis(3_980)),
            Some(191_040)
        );
    }

    #[test]
    fn stream_audio_clock_recognizes_recent_replayed_timestamps() {
        let mut clock = LocalStreamAudioClock::default();
        let _ = clock.rebase(10_000, Duration::ZERO, None);

        assert!(clock.is_recent_replay(10_000));
        assert!(clock.is_recent_replay(6_160));
        assert!(!clock.is_recent_replay(4_239));
        assert!(!clock.is_recent_replay(10_960));
    }

    #[test]
    fn stream_audio_clock_preserves_a_source_timestamp_wrap() {
        let mut clock = LocalStreamAudioClock::default();

        let before_wrap = clock.rebase(u32::MAX - 479, Duration::from_millis(100), None);
        let after_wrap = clock.rebase(480, Duration::from_millis(120), None);

        assert_eq!(before_wrap.local_timestamp, 4_800);
        assert_eq!(after_wrap.local_timestamp, 5_760);
        assert!(after_wrap.discontinuity.is_none());
    }
}
