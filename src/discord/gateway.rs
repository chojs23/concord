use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio::time::{Instant, sleep};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Message as WsMessage,
        client::IntoClientRequest,
        handshake::client::Request,
        protocol::{CloseFrame, WebSocketConfig},
    },
};

use super::{
    ActivityInfo, PresenceStatus,
    client::publish_app_event,
    events::{AppEvent, SequencedAppEvent},
    fingerprint::{
        CLIENT_BROWSER, CLIENT_BROWSER_VERSION, ClientFingerprint, DISCORD_REFERRER_CURRENT,
        DISCORD_REFERRING_DOMAIN_CURRENT, discord_gateway_headers,
    },
    request_lifecycle::RequestLifecycle,
    state::{DiscordState, SnapshotRevision},
    voice::{self, VoiceRuntimeEvent},
};
use crate::logging;

mod parser;

pub(in crate::discord) use parser::parse_activity;
use parser::parse_user_account_dispatch;
pub(crate) use parser::{parse_channel_info, parse_message_info};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayCommand {
    RequestGuildMembers {
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
        presences: bool,
        nonce: Option<String>,
    },
    RequestGuildMembersByIds {
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
    },
    SubscribeDirectMessage {
        channel_id: Id<ChannelMarker>,
    },
    SubscribeGuildChannel {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    },
    UpdateMemberListSubscription {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    },
    UpdateVoiceState {
        /// `None` for DM and group-DM calls, which Discord joins with a null
        /// `guild_id` and the DM `channel_id` as the voice target.
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
    },
    UpdatePresence {
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct GatewayRuntime {
    pub(crate) fingerprint: Arc<ClientFingerprint>,
    pub(crate) effects_tx: mpsc::Sender<SequencedAppEvent>,
    pub(crate) snapshots_tx: watch::Sender<SnapshotRevision>,
    pub(crate) state: Arc<RwLock<DiscordState>>,
    pub(crate) revision: Arc<RwLock<SnapshotRevision>>,
    pub(crate) gateway_session_id: Arc<RwLock<Option<String>>>,
    pub(crate) publish_lock: Arc<Mutex<()>>,
    pub(crate) request_lifecycle: Arc<std::sync::Mutex<RequestLifecycle>>,
    pub(crate) voice_events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
}

/// Discord user-account gateway endpoint. We pin to `v=9` because the v9
/// dispatch shapes line up with everything `parse_user_account_event` already
/// understands. `compress=false` keeps the wire human-readable. Switching to
/// `zlib-stream` is a follow-up.
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json";

/// Bitmask Discord checks before delivering user-account-only payloads such as
/// `READY_SUPPLEMENTAL.merged_presences.friends` and per-friend
/// `PRESENCE_UPDATE` dispatches. Without these bits set Discord assumes the
/// session is a bot and silently drops friend presence streaming.
///
/// Bits enabled (sum 253):
///   0  LAZY_USER_NOTIFICATIONS
///   2  VERSIONED_READ_STATES
///   3  VERSIONED_USER_GUILD_SETTINGS
///   4  DEDUPE_USER_OBJECTS
///   5  PRIORITIZED_READY_PAYLOAD
///   6  MULTIPLE_GUILD_EXPERIMENT_POPULATIONS
///   7  NON_CHANNEL_READ_STATES
const USER_ACCOUNT_CAPABILITIES: u64 = 253;

// Some user-account READY payloads exceed tungstenite's default 16 MiB frame
// cap before Discord has a chance to split the initial state across follow-up
// dispatches. Keep the limit bounded, but large enough for accounts with many
// guilds and channels until gateway compression is implemented.
const GATEWAY_WEBSOCKET_LIMIT: usize = 64 << 20;

const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
// Discord applies this budget to every JSON event on one Gateway connection.
// WebSocket control frames such as Pong and Close are not Gateway events.
const GATEWAY_SEND_LIMIT: usize = 120;
const GATEWAY_SEND_WINDOW: Duration = Duration::from_secs(60);
const MAX_GATEWAY_RETRY_DELAY: Duration = Duration::from_secs(30 * 60);

type GatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Shared, lockable WebSocket sink. Both the heartbeat task and the main
/// dispatch loop need to send over the same connection, so the sink lives
/// behind a `Mutex<Arc<…>>` instead of being moved into either side.
type WriterHandle = Arc<Mutex<futures::stream::SplitSink<GatewayStream, WsMessage>>>;

#[derive(Clone)]
struct GatewaySender {
    // Heartbeats and session setup use the urgent queue so a backlog of UI
    // commands cannot delay connection health traffic once a slot is free.
    urgent_tx: mpsc::UnboundedSender<GatewaySendRequest>,
    normal_tx: mpsc::UnboundedSender<GatewaySendRequest>,
}

struct GatewaySendRequest {
    payload: String,
    completion: Option<oneshot::Sender<Result<(), String>>>,
}

#[derive(Default)]
struct GatewaySendWindow {
    sent_at: VecDeque<Instant>,
}

#[derive(Default)]
struct SubscriptionDeduper {
    direct_messages: HashSet<Id<ChannelMarker>>,
    guild_channels: HashMap<GuildChannelSubscriptionKey, Vec<(u32, u32)>>,
}

impl SubscriptionDeduper {
    fn should_send(&mut self, command: &GatewayCommand) -> bool {
        match command {
            GatewayCommand::SubscribeDirectMessage { channel_id } => {
                self.direct_messages.insert(*channel_id)
            }
            GatewayCommand::SubscribeGuildChannel {
                guild_id,
                channel_id,
            } => self.should_send_guild_channel(*guild_id, *channel_id, &[(0, 99)]),
            GatewayCommand::UpdateMemberListSubscription {
                guild_id,
                channel_id,
                ranges,
            } => self.should_send_guild_channel(*guild_id, *channel_id, ranges),
            _ => true,
        }
    }

    fn should_send_guild_channel(
        &mut self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: &[(u32, u32)],
    ) -> bool {
        let key = GuildChannelSubscriptionKey {
            guild_id,
            channel_id,
        };
        if self
            .guild_channels
            .get(&key)
            .is_some_and(|last_ranges| last_ranges == ranges)
        {
            return false;
        }
        self.guild_channels.insert(key, ranges.to_vec());
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct GuildChannelSubscriptionKey {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
}

#[derive(Clone, Copy)]
struct GatewayPublishContext<'a> {
    effects_tx: &'a mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: &'a watch::Sender<SnapshotRevision>,
    state: &'a Arc<RwLock<DiscordState>>,
    revision: &'a Arc<RwLock<SnapshotRevision>>,
    gateway_session_id: &'a Arc<RwLock<Option<String>>>,
    publish_lock: &'a Arc<Mutex<()>>,
    request_lifecycle: &'a Arc<std::sync::Mutex<RequestLifecycle>>,
    voice_events_tx: &'a mpsc::UnboundedSender<VoiceRuntimeEvent>,
}

#[derive(Clone, Copy)]
struct FrameContext<'a> {
    sequence_cell: &'a Arc<Mutex<Option<u64>>>,
    heartbeat_ack: &'a Arc<Mutex<HeartbeatAckState>>,
    sender: &'a GatewaySender,
    fingerprint: &'a ClientFingerprint,
    publish: GatewayPublishContext<'a>,
}

#[derive(Default)]
struct HeartbeatAckState {
    awaiting_ack: bool,
}

impl HeartbeatAckState {
    fn mark_heartbeat_sent(&mut self) -> bool {
        if self.awaiting_ack {
            return false;
        }
        self.awaiting_ack = true;
        true
    }

    fn mark_ack_received(&mut self) {
        self.awaiting_ack = false;
    }
}

/// What to do after one connection lifecycle ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionOutcome {
    /// The websocket dropped or Discord asked us to reconnect. Try to RESUME
    /// using the saved session_id + sequence number.
    Resume,
    /// Authentication failed or Discord told us the session is dead. Throw
    /// the saved session away and start over with a fresh IDENTIFY.
    Reidentify,
    /// The downstream consumers went away, so stop the loop entirely.
    Stop,
    /// Discord rejected this gateway session in a way that retrying the same
    /// token or shard configuration cannot fix. Keep the UI alive so it can
    /// show the published gateway error.
    Fatal,
}

/// Mutable session bookkeeping that survives reconnects. We only persist what
/// op-6 RESUME needs (session_id + last seq) plus the resume URL Discord
/// hands us in READY.
#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    resume_url: Option<String>,
    last_sequence: Option<u64>,
    has_received_ready: bool,
    /// Whether the current connection reached READY or RESUMED. Read and
    /// cleared by the reconnect loop to reset the backoff after a healthy
    /// session.
    established: bool,
}

impl SessionState {
    fn clear(&mut self) {
        self.session_id = None;
        self.resume_url = None;
        self.last_sequence = None;
    }

    fn can_resume(&self) -> bool {
        self.session_id.is_some()
    }

    fn next_url(&self) -> String {
        match self.resume_url.as_deref() {
            // Discord embeds `?v=...&encoding=...` already, but it costs
            // nothing to append our own and helps when the resume URL is bare.
            Some(url) if !url.is_empty() => format!("{url}/?v=9&encoding=json"),
            _ => GATEWAY_URL.to_owned(),
        }
    }
}

pub async fn run_gateway(
    token: String,
    mut commands: mpsc::UnboundedReceiver<GatewayCommand>,
    runtime: GatewayRuntime,
) {
    let mut session = SessionState::default();
    let mut backoff = RECONNECT_BASE_DELAY;
    let mut publish_gateway_closed = true;

    loop {
        let publish = GatewayPublishContext {
            effects_tx: &runtime.effects_tx,
            snapshots_tx: &runtime.snapshots_tx,
            state: &runtime.state,
            revision: &runtime.revision,
            gateway_session_id: &runtime.gateway_session_id,
            publish_lock: &runtime.publish_lock,
            request_lifecycle: &runtime.request_lifecycle,
            voice_events_tx: &runtime.voice_events_tx,
        };
        let outcome = match connect_and_run(
            &token,
            &mut commands,
            &mut session,
            &runtime.fingerprint,
            publish,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                logging::error("gateway", format!("connection error: {error}"));
                publish_gateway_event(
                    publish,
                    AppEvent::GatewayError {
                        message: format!("connection error: {error}"),
                    },
                )
                .await;
                ConnectionOutcome::Resume
            }
        };

        match outcome {
            ConnectionOutcome::Stop => break,
            ConnectionOutcome::Resume => {
                if !session.can_resume() {
                    // No saved session, fall through to a clean IDENTIFY.
                }
            }
            ConnectionOutcome::Reidentify => {
                session.clear();
                *runtime
                    .gateway_session_id
                    .write()
                    .expect("gateway session id lock is not poisoned") = None;
            }
            ConnectionOutcome::Fatal => {
                publish_gateway_closed = false;
                break;
            }
        }

        // Exponential backoff with full jitter so a flapping network doesn't
        // hammer Discord. A connection that reached READY/RESUMED was healthy,
        // so its disconnect restarts the backoff from the base delay.
        if std::mem::take(&mut session.established) {
            backoff = RECONNECT_BASE_DELAY;
        }
        let jitter = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64);
        let delay = Duration::from_millis(jitter);
        logging::debug(
            "gateway",
            format!("reconnecting in {}ms", delay.as_millis()),
        );
        sleep(delay).await;
        backoff = (backoff * 2).min(RECONNECT_MAX_DELAY);
    }

    if publish_gateway_closed {
        let publish = GatewayPublishContext {
            effects_tx: &runtime.effects_tx,
            snapshots_tx: &runtime.snapshots_tx,
            state: &runtime.state,
            revision: &runtime.revision,
            gateway_session_id: &runtime.gateway_session_id,
            publish_lock: &runtime.publish_lock,
            request_lifecycle: &runtime.request_lifecycle,
            voice_events_tx: &runtime.voice_events_tx,
        };
        publish_gateway_event(publish, AppEvent::GatewayClosed).await;
    }
}

async fn connect_and_run(
    token: &str,
    commands: &mut mpsc::UnboundedReceiver<GatewayCommand>,
    session: &mut SessionState,
    fingerprint: &ClientFingerprint,
    publish: GatewayPublishContext<'_>,
) -> Result<ConnectionOutcome, String> {
    let url = session.next_url();
    logging::debug("gateway", format!("connecting to {url}"));

    let request = gateway_request(&url, fingerprint)?;
    let (ws, _response) =
        connect_async_with_config(request, Some(gateway_websocket_config()), false)
            .await
            .map_err(|error| format!("websocket connect failed: {error}"))?;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let (sender, mut gateway_send_error_rx, gateway_writer_task) =
        spawn_gateway_sender(Arc::clone(&writer));
    let mut subscription_deduper = SubscriptionDeduper::default();

    // Discord must speak first with op-10 HELLO carrying heartbeat_interval.
    // If the first frame is anything else, fail fast and try a clean
    // re-identify.
    let hello_frame = match reader.next().await {
        Some(Ok(WsMessage::Text(text))) => text,
        Some(Ok(WsMessage::Close(frame))) => {
            let message = websocket_close_message("websocket closed before HELLO", frame.as_ref());
            log_and_publish_gateway_error(publish, message).await;
            return Ok(ConnectionOutcome::Reidentify);
        }
        Some(Ok(_)) => return Err("unexpected non-text frame before HELLO".to_owned()),
        Some(Err(error)) => return Err(format!("read HELLO failed: {error}")),
        None => return Err("connection closed before HELLO".to_owned()),
    };
    let hello: Value =
        serde_json::from_str(&hello_frame).map_err(|error| format!("HELLO parse: {error}"))?;
    if hello.get("op").and_then(Value::as_u64) != Some(10) {
        return Err(format!(
            "first frame was not HELLO: {}",
            hello.get("op").and_then(Value::as_u64).unwrap_or_default()
        ));
    }
    let heartbeat_interval_ms = hello
        .get("d")
        .and_then(|d| d.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .unwrap_or(41250);
    let heartbeat_interval = Duration::from_millis(heartbeat_interval_ms);

    // Either resume with the saved session or send a fresh IDENTIFY. RESUME
    // tells Discord to replay missed dispatches. This is good for transient drops.
    // IDENTIFY rebuilds the world from scratch.
    if session.can_resume() {
        let payload = build_resume_payload(token, session);
        send_text(&sender, payload).await?;
        logging::debug("gateway", "RESUME sent");
    } else {
        let payload = build_identify_payload(token, fingerprint);
        send_text(&sender, payload).await?;
        logging::debug("gateway", "IDENTIFY sent");
    }

    // Background heartbeat task driven by Discord's interval. We jitter the
    // first beat per the API recommendation. The task reads the latest seq
    // from a shared atomic via the sequence cell.
    let sender_for_heartbeat = sender.clone();
    let sequence_cell: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(session.last_sequence));
    let sequence_for_heartbeat = Arc::clone(&sequence_cell);
    let heartbeat_ack: Arc<Mutex<HeartbeatAckState>> = Arc::default();
    let heartbeat_ack_for_task = Arc::clone(&heartbeat_ack);
    let (heartbeat_timeout_tx, mut heartbeat_timeout_rx) = mpsc::unbounded_channel();
    let initial_jitter = {
        let jitter_ms =
            rand::thread_rng().gen_range(0..=heartbeat_interval.as_millis().min(2_000) as u64);
        Duration::from_millis(jitter_ms)
    };
    let heartbeat_task = tokio::spawn(async move {
        sleep(initial_jitter).await;
        loop {
            {
                let mut state = heartbeat_ack_for_task.lock().await;
                if !state.mark_heartbeat_sent() {
                    logging::error("gateway", "heartbeat ACK timeout; reconnecting");
                    let _ = heartbeat_timeout_tx.send(());
                    break;
                }
            }
            let seq = *sequence_for_heartbeat.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            if let Err(error) = send_text(&sender_for_heartbeat, payload).await {
                logging::error("gateway", format!("heartbeat send failed: {error}"));
                let _ = heartbeat_timeout_tx.send(());
                break;
            }
            sleep(heartbeat_interval).await;
        }
    });

    // Main loop: race incoming frames against outgoing user commands. The
    // heartbeat task is already running on its own cadence in the background.
    let outcome = loop {
        tokio::select! {
            biased;

            maybe_command = commands.recv() => {
                match maybe_command {
                    Some(command) => {
                        if let GatewayCommand::Shutdown = command {
                            if let Err(error) = close_websocket(&writer).await {
                                let message = format!("gateway shutdown failed: {error}");
                                log_and_publish_gateway_error(publish, message).await;
                            }
                            break ConnectionOutcome::Stop;
                        } else if let Err(error) =
                            dispatch_command(&sender, command, &mut subscription_deduper)
                        {
                            let message = format!("command send failed: {error}");
                            log_and_publish_gateway_error(publish, message).await;
                            break ConnectionOutcome::Resume;
                        }
                    }
                    None => break ConnectionOutcome::Stop,
                }
            }
            frame = reader.next() => {
                match frame {
                    Some(Ok(WsMessage::Text(text))) => {
                        let value: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(error) => {
                                logging::debug(
                                    "gateway",
                                    format!("ignoring non-JSON frame: {error}"),
                                );
                                continue;
                            }
                        };
                        let frame_context = FrameContext {
                            sequence_cell: &sequence_cell,
                            heartbeat_ack: &heartbeat_ack,
                            sender: &sender,
                            fingerprint,
                            publish,
                        };
                        match handle_frame(
                            value,
                            session,
                            frame_context,
                        ).await {
                            FrameOutcome::Continue => {}
                            FrameOutcome::Resume => break ConnectionOutcome::Resume,
                            FrameOutcome::Reidentify => break ConnectionOutcome::Reidentify,
                        }
                    }
                    Some(Ok(WsMessage::Binary(_))) => {
                        // Compression isn't enabled in the IDENTIFY, so binary
                        // frames are unexpected. Log and ignore rather than
                        // panic on bad input.
                        logging::debug("gateway", "ignoring unexpected binary frame");
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        let mut writer = writer.lock().await;
                        if let Err(error) = writer.send(WsMessage::Pong(payload)).await {
                            let message = format!("websocket pong send failed: {error}");
                            log_and_publish_gateway_error(publish, message).await;
                            break ConnectionOutcome::Resume;
                        }
                    }
                    Some(Ok(WsMessage::Pong(_))) | Some(Ok(WsMessage::Frame(_))) => {}
                    Some(Ok(WsMessage::Close(frame))) => {
                        let outcome = close_outcome(frame.as_ref());
                        let message = websocket_close_message("websocket closed", frame.as_ref());
                        log_and_publish_gateway_error(publish, message).await;
                        break outcome;
                    }
                    Some(Err(error)) => {
                        let message = format!("websocket read error: {error}");
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                    None => {
                        let message = "websocket closed without frame".to_owned();
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                }
            }
            _ = heartbeat_timeout_rx.recv() => {
                break ConnectionOutcome::Resume;
            }
            Some(error) = gateway_send_error_rx.recv() => {
                log_and_publish_gateway_error(publish, error).await;
                break ConnectionOutcome::Resume;
            }
        }
    };

    heartbeat_task.abort();
    gateway_writer_task.abort();
    Ok(outcome)
}

fn gateway_websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(GATEWAY_WEBSOCKET_LIMIT))
        .max_frame_size(Some(GATEWAY_WEBSOCKET_LIMIT))
}

fn gateway_request(url: &str, fingerprint: &ClientFingerprint) -> Result<Request, String> {
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("websocket request failed: {error}"))?;
    request
        .headers_mut()
        .extend(discord_gateway_headers(fingerprint));
    Ok(request)
}

enum FrameOutcome {
    Continue,
    Resume,
    Reidentify,
}

async fn handle_frame(
    value: Value,
    session: &mut SessionState,
    context: FrameContext<'_>,
) -> FrameOutcome {
    let op = value.get("op").and_then(Value::as_u64).unwrap_or_default();
    match op {
        // Dispatch
        0 => {
            if let Some(seq) = value.get("s").and_then(Value::as_u64) {
                session.last_sequence = Some(seq);
                *context.sequence_cell.lock().await = Some(seq);
            }
            let dispatch_type = value.get("t").and_then(Value::as_str).unwrap_or("");
            let mut publish_reidentified = false;
            if dispatch_type == "RATE_LIMITED"
                && let Some((guild_id, retry_after)) = gateway_guild_member_rate_limit(&value)
            {
                logging::debug(
                    "gateway",
                    format!(
                        "guild member requests rate limited: guild={} retry_after_ms={}",
                        guild_id.get(),
                        retry_after.as_millis()
                    ),
                );
            }
            // Capture the session_id and resume_url from READY so a later
            // disconnect can RESUME instead of redoing the heavy initial sync.
            if dispatch_type == "READY"
                && let Some(d) = value.get("d")
            {
                let was_reidentify = session.has_received_ready;
                if let Some(installation_id) = ready_installation_id(d) {
                    match context.fingerprint.update_installation_id(installation_id) {
                        Ok(true) => {
                            logging::debug("fingerprint", "updated installation id from READY");
                        }
                        Ok(false) => {}
                        Err(error) => logging::debug(
                            "fingerprint",
                            format!("could not persist READY installation id: {error}"),
                        ),
                    }
                }
                session.session_id = d
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                session.resume_url = d
                    .get("resume_gateway_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                *context
                    .publish
                    .gateway_session_id
                    .write()
                    .expect("gateway session id lock is not poisoned") = session.session_id.clone();
                if was_reidentify {
                    publish_reidentified = true;
                }
                session.has_received_ready = true;
                session.established = true;
            } else if dispatch_type == "RESUMED" {
                session.established = true;
                publish_gateway_event(context.publish, AppEvent::GatewayResumed).await;
            }
            if let Some(parsed) = parse_user_account_dispatch(value) {
                publish_gateway_event(
                    context.publish,
                    AppEvent::GatewayDispatchReceived {
                        dispatch: parsed.dispatch,
                    },
                )
                .await;
                for app_event in parsed.events {
                    publish_gateway_event(context.publish, app_event).await;
                }
            }
            if publish_reidentified {
                publish_gateway_event(context.publish, AppEvent::GatewayReidentified).await;
            }
            FrameOutcome::Continue
        }
        // Answer Discord heartbeat requests immediately. The background task
        // only paces our own heartbeat sends.
        1 => {
            let seq = *context.sequence_cell.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            context.heartbeat_ack.lock().await.mark_heartbeat_sent();
            if let Err(error) = send_text(context.sender, payload).await {
                let message = format!("heartbeat response send failed: {error}");
                log_and_publish_gateway_error(context.publish, message).await;
            }
            FrameOutcome::Continue
        }
        // Discord wants us to drop and resume. Saved session_id and seq make
        // the resume cheap.
        7 => {
            logging::debug("gateway", "RECONNECT requested");
            FrameOutcome::Resume
        }
        // `d` tells us whether an invalid session is resumable. Anything else
        // means we have to throw it away.
        9 => {
            let resumable = value.get("d").and_then(Value::as_bool).unwrap_or(false);
            logging::debug("gateway", format!("INVALID_SESSION resumable={resumable}"));
            if resumable {
                FrameOutcome::Resume
            } else {
                FrameOutcome::Reidentify
            }
        }
        11 => {
            context.heartbeat_ack.lock().await.mark_ack_received();
            FrameOutcome::Continue
        }
        other => {
            logging::debug("gateway", format!("unhandled gateway op={other}"));
            FrameOutcome::Continue
        }
    }
}

async fn publish_gateway_event(context: GatewayPublishContext<'_>, event: AppEvent) {
    context
        .request_lifecycle
        .lock()
        .expect("request lifecycle lock is not poisoned")
        .record_event(&event);
    publish_app_event(
        context.effects_tx,
        context.snapshots_tx,
        context.state,
        context.revision,
        context.publish_lock,
        &event,
    )
    .await;
    voice::forward_app_event(context.voice_events_tx, &event);
}

fn ready_installation_id(ready: &Value) -> Option<&str> {
    ready
        .get("apex_experiments")
        .and_then(|experiments| experiments.get("installation"))
        .and_then(Value::as_str)
}

async fn log_and_publish_gateway_error(context: GatewayPublishContext<'_>, message: String) {
    logging::error("gateway", &message);
    publish_gateway_event(context, AppEvent::GatewayError { message }).await;
}

fn close_outcome(frame: Option<&CloseFrame>) -> ConnectionOutcome {
    let Some(frame) = frame else {
        return ConnectionOutcome::Resume;
    };
    close_code_outcome(u16::from(frame.code))
}

fn close_code_outcome(code: u16) -> ConnectionOutcome {
    // Authentication and gateway configuration failures are not transient.
    // Retrying the same IDENTIFY would hide the real problem behind Loading...
    // and can loop forever for codes such as 4004.
    match code {
        4004 | 4010..=4014 => ConnectionOutcome::Fatal,
        4007 | 4009 => ConnectionOutcome::Reidentify,
        4000..=4003 | 4005 | 4008 => ConnectionOutcome::Resume,
        _ => ConnectionOutcome::Reidentify,
    }
}

fn websocket_close_message(context: &str, frame: Option<&CloseFrame>) -> String {
    if let Some(frame) = frame {
        format!(
            "{context}: code={} reason={:?}",
            u16::from(frame.code),
            frame.reason.as_str()
        )
    } else {
        context.to_owned()
    }
}

fn dispatch_command(
    sender: &GatewaySender,
    command: GatewayCommand,
    subscription_deduper: &mut SubscriptionDeduper,
) -> Result<(), String> {
    if !subscription_deduper.should_send(&command) {
        logging::debug("gateway", "skipping duplicate channel subscription");
        return Ok(());
    }

    let payload = match command {
        GatewayCommand::RequestGuildMembers {
            guild_id,
            query,
            limit,
            presences,
            nonce,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "requesting guild members: guild={} query_len={} limit={} presences={}",
                    guild_id.get(),
                    query.len(),
                    limit,
                    presences
                ),
            );
            request_guild_members_payload(guild_id, &query, limit, presences, nonce.as_deref())
        }
        GatewayCommand::RequestGuildMembersByIds {
            guild_id,
            user_ids,
            presences,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "requesting guild members by id: guild={} users={} presences={}",
                    guild_id.get(),
                    user_ids.len(),
                    presences
                ),
            );
            request_guild_members_by_ids_payload(guild_id, &user_ids, presences)
        }
        GatewayCommand::SubscribeDirectMessage { channel_id } => {
            logging::debug(
                "gateway",
                format!("subscribing to DM: channel={}", channel_id.get()),
            );
            direct_message_subscribe_payload(channel_id)
        }
        GatewayCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "subscribing to guild channel: guild={} channel={}",
                    guild_id.get(),
                    channel_id.get()
                ),
            );
            guild_channel_subscribe_payload(guild_id, channel_id, &[(0, 99)])
        }
        GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            ranges,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "updating member list ranges: guild={} channel={} ranges={:?}",
                    guild_id.get(),
                    channel_id.get(),
                    ranges
                ),
            );
            guild_channel_subscribe_payload(guild_id, channel_id, &ranges)
        }
        GatewayCommand::UpdateVoiceState {
            guild_id,
            channel_id,
            self_mute,
            self_deaf,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "updating voice state: guild={} channel={} self_mute={} self_deaf={}",
                    guild_id.map(|id| id.get()).unwrap_or_default(),
                    channel_id.map(|id| id.get()).unwrap_or_default(),
                    self_mute,
                    self_deaf,
                ),
            );
            voice_state_update_payload(guild_id, channel_id, self_mute, self_deaf)
        }
        GatewayCommand::UpdatePresence { status, activities } => {
            logging::debug(
                "gateway",
                format!(
                    "updating presence status: {} activities={}",
                    status.label(),
                    activities.len()
                ),
            );
            presence_update_payload(status, &activities)
        }
        GatewayCommand::Shutdown => return Ok(()),
    };
    sender.enqueue_text(payload)
}

async fn close_websocket(writer: &WriterHandle) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .close()
        .await
        .map_err(|error| format!("websocket close failed: {error}"))
}

async fn send_text(sender: &GatewaySender, payload: String) -> Result<(), String> {
    sender.send_urgent(payload).await
}

impl GatewaySender {
    async fn send_urgent(&self, payload: String) -> Result<(), String> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.urgent_tx
            .send(GatewaySendRequest {
                payload,
                completion: Some(completion_tx),
            })
            .map_err(|_| "gateway writer task stopped".to_owned())?;
        completion_rx
            .await
            .map_err(|_| "gateway writer task stopped before send completed".to_owned())?
    }

    fn enqueue_text(&self, payload: String) -> Result<(), String> {
        self.normal_tx
            .send(GatewaySendRequest {
                payload,
                completion: None,
            })
            .map_err(|_| "gateway writer task stopped".to_owned())
    }
}

impl GatewaySendWindow {
    fn delay_at(&mut self, now: Instant) -> Option<Duration> {
        while self
            .sent_at
            .front()
            .is_some_and(|sent_at| now.duration_since(*sent_at) >= GATEWAY_SEND_WINDOW)
        {
            self.sent_at.pop_front();
        }
        if self.sent_at.len() < GATEWAY_SEND_LIMIT {
            return None;
        }
        self.sent_at
            .front()
            .map(|sent_at| (*sent_at + GATEWAY_SEND_WINDOW).duration_since(now))
    }

    fn record(&mut self, now: Instant) {
        self.sent_at.push_back(now);
    }
}

fn spawn_gateway_sender(
    writer: WriterHandle,
) -> (
    GatewaySender,
    mpsc::UnboundedReceiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let (urgent_tx, urgent_rx) = mpsc::unbounded_channel();
    let (normal_tx, normal_rx) = mpsc::unbounded_channel();
    let (error_tx, error_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run_gateway_sender(writer, urgent_rx, normal_rx, error_tx));
    (
        GatewaySender {
            urgent_tx,
            normal_tx,
        },
        error_rx,
        task,
    )
}

fn gateway_guild_member_rate_limit(value: &Value) -> Option<(Id<GuildMarker>, Duration)> {
    let data = value.get("d")?;
    if data.get("opcode").and_then(Value::as_u64) != Some(8) {
        return None;
    }
    let guild_id = data
        .get("meta")?
        .get("guild_id")?
        .as_str()?
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let retry_after = data.get("retry_after")?.as_f64()?;
    if !retry_after.is_finite() || retry_after < 0.0 {
        return None;
    }
    Some((
        guild_id,
        Duration::from_secs_f64(retry_after.min(MAX_GATEWAY_RETRY_DELAY.as_secs_f64())),
    ))
}

async fn run_gateway_sender(
    writer: WriterHandle,
    mut urgent_rx: mpsc::UnboundedReceiver<GatewaySendRequest>,
    mut normal_rx: mpsc::UnboundedReceiver<GatewaySendRequest>,
    error_tx: mpsc::UnboundedSender<String>,
) {
    let mut urgent = VecDeque::new();
    let mut normal = VecDeque::new();
    let mut urgent_open = true;
    let mut normal_open = true;
    let mut window = GatewaySendWindow::default();

    loop {
        drain_gateway_requests(&mut urgent_rx, &mut urgent, &mut urgent_open);
        drain_gateway_requests(&mut normal_rx, &mut normal, &mut normal_open);

        if urgent.is_empty() && normal.is_empty() {
            if !urgent_open && !normal_open {
                return;
            }
            tokio::select! {
                biased;
                request = urgent_rx.recv(), if urgent_open => {
                    match request {
                        Some(request) => urgent.push_back(request),
                        None => urgent_open = false,
                    }
                }
                request = normal_rx.recv(), if normal_open => {
                    match request {
                        Some(request) => normal.push_back(request),
                        None => normal_open = false,
                    }
                }
            }
            continue;
        }

        if let Some(delay) = window.delay_at(Instant::now()) {
            tokio::select! {
                biased;
                request = urgent_rx.recv(), if urgent_open => {
                    match request {
                        Some(request) => urgent.push_back(request),
                        None => urgent_open = false,
                    }
                }
                _ = sleep(delay) => {}
            }
            continue;
        }

        let request = urgent
            .pop_front()
            .or_else(|| normal.pop_front())
            .expect("gateway send queue is not empty");
        window.record(Instant::now());
        let result = {
            let mut writer = writer.lock().await;
            writer
                .send(WsMessage::Text(request.payload.into()))
                .await
                .map_err(|error| format!("websocket send failed: {error}"))
        };
        if let Some(completion) = request.completion {
            let _ = completion.send(result.clone());
        }
        if let Err(error) = result {
            let _ = error_tx.send(error);
            return;
        }
    }
}

fn drain_gateway_requests(
    receiver: &mut mpsc::UnboundedReceiver<GatewaySendRequest>,
    queue: &mut VecDeque<GatewaySendRequest>,
    open: &mut bool,
) {
    while *open {
        match receiver.try_recv() {
            Ok(request) => queue.push_back(request),
            Err(mpsc::error::TryRecvError::Empty) => return,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                *open = false;
                return;
            }
        }
    }
}

fn build_identify_payload(token: &str, fingerprint: &ClientFingerprint) -> String {
    let mut properties = json!({
        "os": fingerprint.os,
        "browser": CLIENT_BROWSER,
        "device": "",
        "system_locale": fingerprint.system_locale,
        "browser_user_agent": fingerprint.user_agent,
        "browser_version": CLIENT_BROWSER_VERSION,
        "os_version": fingerprint.os_version,
        "referrer": "",
        "referring_domain": "",
        "referrer_current": DISCORD_REFERRER_CURRENT,
        "referring_domain_current": DISCORD_REFERRING_DOMAIN_CURRENT,
        "release_channel": "stable",
        "client_build_number": fingerprint.client_build_number,
        "client_event_source": Value::Null,
    });
    if let Some(installation_id) = fingerprint.installation_id() {
        properties["installation_id"] = Value::String(installation_id);
    }

    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": USER_ACCOUNT_CAPABILITIES,
            "properties": properties,
            "presence": {
                "status": PresenceStatus::Online.gateway_status(),
                "since": 0,
                "activities": [],
                "afk": false,
            },
            "compress": false,
            "client_state": {
                "guild_versions": {},
                "highest_last_message_id": "0",
                "read_state_version": 0,
                "user_guild_settings_version": -1,
                "user_settings_version": -1,
                "private_channels_version": "0",
                "api_code_version": 0,
            },
        },
    })
    .to_string()
}

fn build_resume_payload(token: &str, session: &SessionState) -> String {
    json!({
        "op": 6,
        "d": {
            "token": token,
            "session_id": session.session_id.as_deref().unwrap_or_default(),
            "seq": session.last_sequence.unwrap_or_default(),
        },
    })
    .to_string()
}

fn request_guild_members_payload(
    guild_id: Id<GuildMarker>,
    query: &str,
    limit: u16,
    presences: bool,
    nonce: Option<&str>,
) -> String {
    let mut data = json!({
        "guild_id": guild_id.to_string(),
        "query": query,
        "limit": limit,
        "presences": presences,
    });
    if let Some(nonce) = nonce {
        data["nonce"] = json!(nonce);
    }
    json!({
        "op": 8,
        "d": data,
    })
    .to_string()
}

fn request_guild_members_by_ids_payload(
    guild_id: Id<GuildMarker>,
    user_ids: &[Id<UserMarker>],
    presences: bool,
) -> String {
    let user_ids = user_ids
        .iter()
        .take(100)
        .map(|user_id| user_id.to_string())
        .collect::<Vec<_>>();
    json!({
        "op": 8,
        "d": {
            "guild_id": guild_id.to_string(),
            "user_ids": user_ids,
            "presences": presences,
        },
    })
    .to_string()
}

fn direct_message_subscribe_payload(channel_id: Id<ChannelMarker>) -> String {
    json!({
        "op": 13,
        "d": {
            "channel_id": channel_id.to_string(),
        },
    })
    .to_string()
}

fn guild_channel_subscribe_payload(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    ranges: &[(u32, u32)],
) -> String {
    let ranges_json: Vec<[u32; 2]> = ranges.iter().map(|(start, end)| [*start, *end]).collect();
    json!({
        "op": 37,
        "d": {
            "subscriptions": {
                guild_id.to_string(): {
                    "typing": true,
                    "activities": true,
                    "threads": true,
                    "channels": {
                        channel_id.to_string(): ranges_json,
                    },
                },
            },
        },
    })
    .to_string()
}

fn voice_state_update_payload(
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Option<Id<ChannelMarker>>,
    self_mute: bool,
    self_deaf: bool,
) -> String {
    // A null `guild_id` tells Discord this is a DM or group-DM call. The
    // `channel_id` then points at the DM channel rather than a guild voice channel.
    json!({
        "op": 4,
        "d": {
            "guild_id": guild_id.map(|guild_id| guild_id.to_string()),
            "channel_id": channel_id.map(|channel_id| channel_id.to_string()),
            "self_mute": self_mute,
            "self_deaf": self_deaf,
        },
    })
    .to_string()
}

fn presence_update_payload(status: PresenceStatus, activities: &[ActivityInfo]) -> String {
    json!({
        "op": 3,
        "d": {
            "since": 0,
            "activities": activities.iter().map(activity_gateway_payload).collect::<Vec<_>>(),
            "status": status.gateway_status(),
            "afk": false,
        },
    })
    .to_string()
}

fn activity_gateway_payload(activity: &ActivityInfo) -> Value {
    let mut value = json!({
        "name": activity.name.as_str(),
        "type": activity.kind.gateway_code(),
    });
    if let Some(details) = activity.details.as_deref() {
        value["details"] = json!(details);
    }
    if let Some(state) = activity.state.as_deref() {
        value["state"] = json!(state);
    }
    if let Some(url) = activity.url.as_deref() {
        value["url"] = json!(url);
    }
    // A Custom status carries its emoji here. Without it a status change would
    // re-broadcast the activity and drop the emoji.
    if let Some(emoji) = activity.emoji.as_ref() {
        let mut node = json!({ "name": emoji.name.as_str() });
        if let Some(id) = emoji.id {
            node["id"] = json!(id.get().to_string());
        }
        if emoji.animated {
            node["animated"] = json!(true);
        }
        value["emoji"] = node;
    }
    if let Some(application_id) = activity.application_id.as_deref() {
        value["application_id"] = json!(application_id);
    }
    if let Some(timestamps) = activity.timestamps.as_ref() {
        let mut node = json!({});
        if let Some(start) = timestamps.start {
            node["start"] = json!(start);
        }
        if let Some(end) = timestamps.end {
            node["end"] = json!(end);
        }
        value["timestamps"] = node;
    }
    if let Some(assets) = activity.assets.as_ref() {
        let mut node = json!({});
        if let Some(large_image) = assets.large_image.as_deref() {
            node["large_image"] = json!(large_image);
        }
        if let Some(large_text) = assets.large_text.as_deref() {
            node["large_text"] = json!(large_text);
        }
        if let Some(small_image) = assets.small_image.as_deref() {
            node["small_image"] = json!(small_image);
        }
        if let Some(small_text) = assets.small_text.as_deref() {
            node["small_text"] = json!(small_text);
        }
        value["assets"] = node;
    }
    if let Some(party) = activity.party.as_ref() {
        let mut node = json!({});
        if let Some(id) = party.id.as_deref() {
            node["id"] = json!(id);
        }
        if let Some((current, max)) = party.size {
            node["size"] = json!([current, max]);
        }
        value["party"] = node;
    }
    // User-account presence encodes buttons as a parallel pair: an array of
    // labels under `buttons` and their URLs under `metadata.button_urls`. This
    // differs from the bot `[{label, url}]` shape.
    if !activity.buttons.is_empty() {
        let labels: Vec<&str> = activity
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        let urls: Vec<&str> = activity
            .buttons
            .iter()
            .map(|button| button.url.as_str())
            .collect();
        value["buttons"] = json!(labels);
        value["metadata"] = json!({ "button_urls": urls });
    }
    value
}

#[cfg(test)]
mod tests;
