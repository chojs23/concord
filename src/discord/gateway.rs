use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Message as WsMessage,
        protocol::{CloseFrame, WebSocketConfig},
    },
};

use super::{
    client::publish_app_event,
    events::{AppEvent, SequencedAppEvent},
    state::{DiscordState, SnapshotRevision},
    voice::{self, VoiceRuntimeEvent},
};
use crate::logging;

mod parser;

use parser::parse_user_account_event;
pub(crate) use parser::{parse_channel_info, parse_message_info};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayCommand {
    RequestGuildMembers {
        guild_id: Id<GuildMarker>,
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
        guild_id: Id<GuildMarker>,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
    },
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct GatewayRuntime {
    pub(crate) effects_tx: mpsc::Sender<SequencedAppEvent>,
    pub(crate) snapshots_tx: watch::Sender<SnapshotRevision>,
    pub(crate) state: Arc<RwLock<DiscordState>>,
    pub(crate) revision: Arc<RwLock<SnapshotRevision>>,
    pub(crate) publish_lock: Arc<Mutex<()>>,
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

const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const BROWSER_VERSION: &str = "120.0.0.0";
const CLIENT_BUILD_NUMBER: u64 = 250000;

// Some user-account READY payloads exceed tungstenite's default 16 MiB frame
// cap before Discord has a chance to split the initial state across follow-up
// dispatches. Keep the limit bounded, but large enough for accounts with many
// guilds and channels until gateway compression is implemented.
const GATEWAY_WEBSOCKET_LIMIT: usize = 64 << 20;

const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

type GatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Shared, lockable WebSocket sink. Both the heartbeat task and the main
/// dispatch loop need to send over the same connection, so the sink lives
/// behind a `Mutex<Arc<…>>` instead of being moved into either side.
type WriterHandle = Arc<Mutex<futures::stream::SplitSink<GatewayStream, WsMessage>>>;

#[derive(Clone, Copy)]
struct GatewayPublishContext<'a> {
    effects_tx: &'a mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: &'a watch::Sender<SnapshotRevision>,
    state: &'a Arc<RwLock<DiscordState>>,
    revision: &'a Arc<RwLock<SnapshotRevision>>,
    publish_lock: &'a Arc<Mutex<()>>,
    voice_events_tx: &'a mpsc::UnboundedSender<VoiceRuntimeEvent>,
}

#[derive(Clone, Copy)]
struct FrameContext<'a> {
    sequence_cell: &'a Arc<Mutex<Option<u64>>>,
    writer: &'a WriterHandle,
    publish: GatewayPublishContext<'a>,
}

/// What to do after one connection lifecycle ends.
enum ConnectionOutcome {
    /// The websocket dropped or Discord asked us to reconnect. Try to RESUME
    /// using the saved session_id + sequence number.
    Resume,
    /// Authentication failed or Discord told us the session is dead. Throw
    /// the saved session away and start over with a fresh IDENTIFY.
    Reidentify,
    /// The downstream consumers went away, so stop the loop entirely.
    Stop,
}

/// Mutable session bookkeeping that survives reconnects. We only persist what
/// op-6 RESUME needs (session_id + last seq) plus the resume URL Discord
/// hands us in READY.
#[derive(Default)]
struct SessionState {
    session_id: Option<String>,
    resume_url: Option<String>,
    last_sequence: Option<u64>,
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

    loop {
        let publish = GatewayPublishContext {
            effects_tx: &runtime.effects_tx,
            snapshots_tx: &runtime.snapshots_tx,
            state: &runtime.state,
            revision: &runtime.revision,
            publish_lock: &runtime.publish_lock,
            voice_events_tx: &runtime.voice_events_tx,
        };
        let outcome = match connect_and_run(&token, &mut commands, &mut session, publish).await {
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
            ConnectionOutcome::Reidentify => session.clear(),
        }

        // Exponential backoff with full jitter so a flapping network doesn't
        // hammer Discord. Successful sessions reset the delay below.
        let jitter = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64);
        let delay = Duration::from_millis(jitter);
        logging::debug(
            "gateway",
            format!("reconnecting in {}ms", delay.as_millis()),
        );
        sleep(delay).await;
        backoff = (backoff * 2).min(RECONNECT_MAX_DELAY);
    }

    let publish = GatewayPublishContext {
        effects_tx: &runtime.effects_tx,
        snapshots_tx: &runtime.snapshots_tx,
        state: &runtime.state,
        revision: &runtime.revision,
        publish_lock: &runtime.publish_lock,
        voice_events_tx: &runtime.voice_events_tx,
    };
    publish_gateway_event(publish, AppEvent::GatewayClosed).await;
}

async fn connect_and_run(
    token: &str,
    commands: &mut mpsc::UnboundedReceiver<GatewayCommand>,
    session: &mut SessionState,
    publish: GatewayPublishContext<'_>,
) -> Result<ConnectionOutcome, String> {
    let url = session.next_url();
    logging::debug("gateway", format!("connecting to {url}"));

    let (ws, _response) = connect_async_with_config(&url, Some(gateway_websocket_config()), false)
        .await
        .map_err(|error| format!("websocket connect failed: {error}"))?;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));

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
        send_text(&writer, payload).await?;
        logging::debug("gateway", "RESUME sent");
    } else {
        let payload = build_identify_payload(token);
        send_text(&writer, payload).await?;
        logging::debug("gateway", "IDENTIFY sent");
    }

    // Background heartbeat task driven by Discord's interval. We jitter the
    // first beat per the API recommendation. The task reads the latest seq
    // from a shared atomic via the sequence cell.
    let writer_for_heartbeat = Arc::clone(&writer);
    let sequence_cell: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(session.last_sequence));
    let sequence_for_heartbeat = Arc::clone(&sequence_cell);
    let initial_jitter = {
        let jitter_ms =
            rand::thread_rng().gen_range(0..=heartbeat_interval.as_millis().min(2_000) as u64);
        Duration::from_millis(jitter_ms)
    };
    let heartbeat_task = tokio::spawn(async move {
        sleep(initial_jitter).await;
        loop {
            let seq = *sequence_for_heartbeat.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            if let Err(error) = send_text(&writer_for_heartbeat, payload).await {
                logging::error("gateway", format!("heartbeat send failed: {error}"));
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
                        } else if let Err(error) = dispatch_command(&writer, command).await {
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
                            writer: &writer,
                            publish,
                        };
                        match handle_frame(
                            value,
                            &text,
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
        }
    };

    heartbeat_task.abort();
    Ok(outcome)
}

fn gateway_websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(GATEWAY_WEBSOCKET_LIMIT))
        .max_frame_size(Some(GATEWAY_WEBSOCKET_LIMIT))
}

enum FrameOutcome {
    Continue,
    Resume,
    Reidentify,
}

async fn handle_frame(
    value: Value,
    raw: &str,
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
            // Capture the session_id and resume_url from READY so a later
            // disconnect can RESUME instead of redoing the heavy initial sync.
            if dispatch_type == "READY"
                && let Some(d) = value.get("d")
            {
                session.session_id = d
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                session.resume_url = d
                    .get("resume_gateway_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            let events = parse_user_account_event(raw);
            for app_event in events {
                publish_gateway_event(context.publish, app_event).await;
            }
            FrameOutcome::Continue
        }
        // Answer Discord heartbeat requests immediately. The background task
        // only paces our own heartbeat sends.
        1 => {
            let seq = *context.sequence_cell.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            if let Err(error) = send_text(context.writer, payload).await {
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
        // Heartbeat ack. No action needed.
        11 => FrameOutcome::Continue,
        other => {
            logging::debug("gateway", format!("unhandled gateway op={other}"));
            FrameOutcome::Continue
        }
    }
}

async fn publish_gateway_event(context: GatewayPublishContext<'_>, event: AppEvent) {
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

async fn log_and_publish_gateway_error(context: GatewayPublishContext<'_>, message: String) {
    logging::error("gateway", &message);
    publish_gateway_event(context, AppEvent::GatewayError { message }).await;
}

fn close_outcome(frame: Option<&CloseFrame>) -> ConnectionOutcome {
    let Some(frame) = frame else {
        return ConnectionOutcome::Resume;
    };
    // Per Discord's documented close codes: anything outside 4000-4009 is a
    // hard fail (auth, intent mismatch, sharding) where RESUME would just be
    // rejected, so we re-IDENTIFY from scratch.
    let code = u16::from(frame.code);
    match code {
        4000..=4009 => ConnectionOutcome::Resume,
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

async fn dispatch_command(writer: &WriterHandle, command: GatewayCommand) -> Result<(), String> {
    let payload = match command {
        GatewayCommand::RequestGuildMembers { guild_id } => {
            logging::debug(
                "gateway",
                format!("requesting guild members: guild={}", guild_id.get()),
            );
            json!({
                "op": 8,
                "d": {
                    "guild_id": guild_id.to_string(),
                    "query": "",
                    "limit": 0,
                    "presences": true,
                },
            })
            .to_string()
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
                    guild_id.get(),
                    channel_id.map(|id| id.get()).unwrap_or_default(),
                    self_mute,
                    self_deaf,
                ),
            );
            voice_state_update_payload(guild_id, channel_id, self_mute, self_deaf)
        }
        GatewayCommand::Shutdown => return Ok(()),
    };
    send_text(writer, payload).await
}

async fn close_websocket(writer: &WriterHandle) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .close()
        .await
        .map_err(|error| format!("websocket close failed: {error}"))
}

async fn send_text(writer: &WriterHandle, payload: String) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Text(payload.into()))
        .await
        .map_err(|error| format!("websocket send failed: {error}"))
}

fn build_identify_payload(token: &str) -> String {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": USER_ACCOUNT_CAPABILITIES,
            "properties": {
                "os": "Linux",
                "browser": "Chrome",
                "device": "",
                "system_locale": "en-US",
                "browser_user_agent": BROWSER_USER_AGENT,
                "browser_version": BROWSER_VERSION,
                "os_version": "",
                "referrer": "",
                "referring_domain": "",
                "referrer_current": "",
                "referring_domain_current": "",
                "release_channel": "stable",
                "client_build_number": CLIENT_BUILD_NUMBER,
                "client_event_source": Value::Null,
            },
            "presence": {
                "status": "unknown",
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
    guild_id: Id<GuildMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    self_mute: bool,
    self_deaf: bool,
) -> String {
    json!({
        "op": 4,
        "d": {
            "guild_id": guild_id.to_string(),
            "channel_id": channel_id.map(|channel_id| channel_id.to_string()),
            "self_mute": self_mute,
            "self_deaf": self_deaf,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker},
    };
    use serde_json::json;

    use super::{
        GATEWAY_WEBSOCKET_LIMIT, SessionState, USER_ACCOUNT_CAPABILITIES, build_identify_payload,
        build_resume_payload, direct_message_subscribe_payload, gateway_websocket_config,
        guild_channel_subscribe_payload, voice_state_update_payload,
    };

    #[test]
    fn gateway_websocket_config_allows_large_ready_payloads() {
        let config = gateway_websocket_config();

        assert_eq!(config.max_message_size, Some(GATEWAY_WEBSOCKET_LIMIT));
        assert_eq!(config.max_frame_size, Some(GATEWAY_WEBSOCKET_LIMIT));
    }

    #[test]
    fn identify_payload_carries_user_account_capabilities() {
        let payload: serde_json::Value =
            serde_json::from_str(&build_identify_payload("dummy-token"))
                .expect("identify payload should be valid json");
        assert_eq!(payload["op"].as_u64(), Some(2));
        assert_eq!(
            payload["d"]["capabilities"].as_u64(),
            Some(USER_ACCOUNT_CAPABILITIES)
        );
        // Browser-style fingerprint is what unlocks friend presence streaming
        // for user accounts.
        assert!(
            payload["d"]["properties"]["browser_user_agent"]
                .as_str()
                .unwrap_or_default()
                .contains("Chrome")
        );
        assert_eq!(payload["d"]["compress"].as_bool(), Some(false));
    }

    #[test]
    fn resume_payload_uses_saved_session_id_and_seq() {
        let session = SessionState {
            session_id: Some("sess-123".to_owned()),
            last_sequence: Some(42),
            ..SessionState::default()
        };
        let payload: serde_json::Value =
            serde_json::from_str(&build_resume_payload("dummy-token", &session))
                .expect("resume payload should be valid json");
        assert_eq!(payload["op"].as_u64(), Some(6));
        assert_eq!(payload["d"]["session_id"].as_str(), Some("sess-123"));
        assert_eq!(payload["d"]["seq"].as_u64(), Some(42));
    }

    #[test]
    fn direct_message_subscribe_payload_matches_expected_shape() {
        let payload: serde_json::Value = serde_json::from_str(&direct_message_subscribe_payload(
            Id::<ChannelMarker>::new(20),
        ))
        .expect("payload should be valid json");

        assert_eq!(
            payload,
            json!({
                "op": 13,
                "d": {
                    "channel_id": "20"
                }
            })
        );
    }

    #[test]
    fn guild_channel_subscribe_payload_matches_shape_and_member_ranges() {
        for (ranges, expected_ranges) in [
            (&[(0, 99)][..], json!([[0, 99]])),
            (
                &[(0, 99), (100, 199), (200, 299)][..],
                json!([[0, 99], [100, 199], [200, 299]]),
            ),
        ] {
            let payload: serde_json::Value =
                serde_json::from_str(&guild_channel_subscribe_payload(
                    Id::<GuildMarker>::new(10),
                    Id::<ChannelMarker>::new(20),
                    ranges,
                ))
                .expect("payload should be valid json");

            assert_eq!(payload["op"].as_u64(), Some(37));
            assert_eq!(payload["d"]["subscriptions"]["10"]["typing"], json!(true));
            assert_eq!(
                payload["d"]["subscriptions"]["10"]["activities"],
                json!(true)
            );
            assert_eq!(payload["d"]["subscriptions"]["10"]["threads"], json!(true));
            assert_eq!(
                payload["d"]["subscriptions"]["10"]["channels"]["20"],
                expected_ranges
            );
            if ranges == &[(0, 99)][..] {
                assert_eq!(
                    payload,
                    json!({
                        "op": 37,
                        "d": {
                            "subscriptions": {
                                "10": {
                                    "typing": true,
                                    "activities": true,
                                    "threads": true,
                                    "channels": {
                                        "20": [[0, 99]]
                                    }
                                }
                            }
                        }
                    })
                );
            }
        }
    }

    #[test]
    fn voice_state_update_payload_joins_and_leaves_voice_channel() {
        let join_payload: serde_json::Value = serde_json::from_str(&voice_state_update_payload(
            Id::<GuildMarker>::new(10),
            Some(Id::<ChannelMarker>::new(20)),
            true,
            false,
        ))
        .expect("voice join payload should be valid json");
        assert_eq!(join_payload["op"].as_u64(), Some(4));
        assert_eq!(join_payload["d"]["guild_id"].as_str(), Some("10"));
        assert_eq!(join_payload["d"]["channel_id"].as_str(), Some("20"));
        assert_eq!(join_payload["d"]["self_mute"].as_bool(), Some(true));
        assert_eq!(join_payload["d"]["self_deaf"].as_bool(), Some(false));

        let leave_payload: serde_json::Value = serde_json::from_str(&voice_state_update_payload(
            Id::<GuildMarker>::new(10),
            None,
            true,
            false,
        ))
        .expect("voice leave payload should be valid json");
        assert!(leave_payload["d"]["channel_id"].is_null());
    }
}
