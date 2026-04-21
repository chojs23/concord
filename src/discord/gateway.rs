use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::discord::ids::{
    Id,
    marker::{
        AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker,
        UserMarker,
    },
};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::time::sleep;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message as WsMessage, protocol::CloseFrame},
};

use super::{
    AttachmentInfo, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, EmbedFieldInfo, EmbedInfo,
    FriendStatus, GuildFolder, MemberInfo, MentionInfo, MessageInfo, MessageKind,
    MessageReferenceInfo, MessageSnapshotInfo, PollAnswerInfo, PollInfo, PresenceStatus,
    ReactionInfo, ReplyInfo, RoleInfo,
    commands::ReactionEmoji,
    events::default_avatar_url,
    events::{AppEvent, AttachmentUpdate, PermissionOverwriteInfo, PermissionOverwriteKind},
};
use crate::logging;

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
}

/// Discord user-account gateway endpoint. We pin to `v=9` because the v9
/// dispatch shapes line up with everything `parse_user_account_event` already
/// understands. `compress=false` keeps the wire human-readable; switching to
/// `zlib-stream` is a follow-up.
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json";

/// Bitmask Discord checks before delivering user-account-only payloads such as
/// `READY_SUPPLEMENTAL.merged_presences.friends` and per-friend
/// `PRESENCE_UPDATE` dispatches. Without these bits set Discord assumes the
/// session is a bot and silently drops friend presence streaming.
///
/// We deliberately copy arikawa/ningen's set rather than reaching for the
/// modern client's full bitmask. The extra modern bits (USER_SETTINGS_PROTO,
/// CLIENT_STATE_V2, PASSIVE_GUILD_UPDATE, …) tell Discord to send things in
/// formats we don't decode yet — most painfully `user_settings_proto` instead
/// of the legacy JSON `user_settings.guild_folders`, which would leave the
/// sidebar with no folder grouping and unstable ordering.
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

const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

type GatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Shared, lockable WebSocket sink. Both the heartbeat task and the main
/// dispatch loop need to send over the same connection, so the sink lives
/// behind a `Mutex<Arc<…>>` instead of being moved into either side.
type WriterHandle = Arc<Mutex<futures::stream::SplitSink<GatewayStream, WsMessage>>>;

/// What to do after one connection lifecycle ends.
enum ConnectionOutcome {
    /// The websocket dropped or Discord asked us to reconnect; try to RESUME
    /// using the saved session_id + sequence number.
    Resume,
    /// Authentication failed or Discord told us the session is dead; throw
    /// the saved session away and start over with a fresh IDENTIFY.
    Reidentify,
    /// The downstream consumers went away — stop the loop entirely.
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
    tx: broadcast::Sender<AppEvent>,
    mut commands: mpsc::UnboundedReceiver<GatewayCommand>,
) {
    let mut session = SessionState::default();
    let mut backoff = RECONNECT_BASE_DELAY;

    loop {
        let outcome = match connect_and_run(&token, &tx, &mut commands, &mut session).await {
            Ok(outcome) => outcome,
            Err(error) => {
                logging::error("gateway", format!("connection error: {error}"));
                let _ = tx.send(AppEvent::GatewayError {
                    message: format!("connection error: {error}"),
                });
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

    let _ = tx.send(AppEvent::GatewayClosed);
}

async fn connect_and_run(
    token: &str,
    tx: &broadcast::Sender<AppEvent>,
    commands: &mut mpsc::UnboundedReceiver<GatewayCommand>,
    session: &mut SessionState,
) -> Result<ConnectionOutcome, String> {
    let url = session.next_url();
    logging::debug("gateway", format!("connecting to {url}"));

    let (ws, _response) = connect_async(&url)
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
            logging::debug(
                "gateway",
                format!(
                    "closed before HELLO: code={:?} reason={:?}",
                    frame.as_ref().map(|f| u16::from(f.code)),
                    frame.as_ref().map(|f| f.reason.as_str())
                ),
            );
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
    // tells Discord to replay missed dispatches (good for transient drops);
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
            if send_text(&writer_for_heartbeat, payload).await.is_err() {
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
                        if let Err(error) = dispatch_command(&writer, command).await {
                            logging::debug(
                                "gateway",
                                format!("command send failed: {error}"),
                            );
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
                        match handle_frame(value, &text, session, &sequence_cell, tx, &writer).await {
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
                        if writer.send(WsMessage::Pong(payload)).await.is_err() {
                            break ConnectionOutcome::Resume;
                        }
                    }
                    Some(Ok(WsMessage::Pong(_))) | Some(Ok(WsMessage::Frame(_))) => {}
                    Some(Ok(WsMessage::Close(frame))) => {
                        let outcome = close_outcome(frame.as_ref());
                        log_close(frame.as_ref());
                        break outcome;
                    }
                    Some(Err(error)) => {
                        logging::debug(
                            "gateway",
                            format!("websocket read error: {error}"),
                        );
                        break ConnectionOutcome::Resume;
                    }
                    None => break ConnectionOutcome::Resume,
                }
            }
        }
    };

    heartbeat_task.abort();
    Ok(outcome)
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
    sequence_cell: &Arc<Mutex<Option<u64>>>,
    tx: &broadcast::Sender<AppEvent>,
    writer: &WriterHandle,
) -> FrameOutcome {
    let op = value.get("op").and_then(Value::as_u64).unwrap_or_default();
    match op {
        // Dispatch
        0 => {
            if let Some(seq) = value.get("s").and_then(Value::as_u64) {
                session.last_sequence = Some(seq);
                *sequence_cell.lock().await = Some(seq);
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
            let started = Instant::now();
            let events = parse_user_account_event(raw);
            logging::timing("gateway", "dispatch parse", started.elapsed());
            for app_event in events {
                let _ = tx.send(app_event);
            }
            FrameOutcome::Continue
        }
        // Heartbeat request from Discord — answer immediately even though our
        // background task is pacing things.
        1 => {
            let seq = *sequence_cell.lock().await;
            let payload = json!({"op": 1, "d": seq}).to_string();
            let _ = send_text(writer, payload).await;
            FrameOutcome::Continue
        }
        // Reconnect — Discord wants us to drop and resume. Saved
        // session_id + seq makes the resume cheap.
        7 => {
            logging::debug("gateway", "RECONNECT requested");
            FrameOutcome::Resume
        }
        // Invalid Session — `d` is a bool that says whether the session is
        // resumable. Anything else means we have to throw it away.
        9 => {
            let resumable = value.get("d").and_then(Value::as_bool).unwrap_or(false);
            logging::debug("gateway", format!("INVALID_SESSION resumable={resumable}"));
            if resumable {
                FrameOutcome::Resume
            } else {
                FrameOutcome::Reidentify
            }
        }
        // Heartbeat ack — just drop, no action needed.
        11 => FrameOutcome::Continue,
        other => {
            logging::debug("gateway", format!("unhandled gateway op={other}"));
            FrameOutcome::Continue
        }
    }
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

fn log_close(frame: Option<&CloseFrame>) {
    if let Some(frame) = frame {
        logging::debug(
            "gateway",
            format!(
                "websocket closed: code={} reason={:?}",
                u16::from(frame.code),
                frame.reason.as_str()
            ),
        );
    } else {
        logging::debug("gateway", "websocket closed without frame");
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
    };
    send_text(writer, payload).await
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

/// Best-effort fallback that rebuilds the dashboard's domain events directly
/// from the raw gateway payload. We only extract the fields the UI consumes,
/// and skip anything we can't model. Returns an iterable so a single payload
/// (e.g. `GUILD_CREATE`) can produce multiple downstream events.
fn parse_user_account_event(raw: &str) -> Vec<AppEvent> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(event_type) = value.get("t").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(data) = value.get("d") else {
        return Vec::new();
    };

    match event_type {
        "READY" => parse_ready(data),
        "READY_SUPPLEMENTAL" => parse_ready_supplemental(data),
        "GUILD_CREATE" => {
            let started = Instant::now();
            let result = parse_guild_create(data).into_iter().collect();
            logging::timing("gateway", "fallback guild_create parse", started.elapsed());
            result
        }
        "GUILD_UPDATE" => parse_guild_update(data).into_iter().collect(),
        "GUILD_EMOJIS_UPDATE" => parse_guild_emojis_update(data).into_iter().collect(),
        "GUILD_DELETE" => parse_guild_delete(data).into_iter().collect(),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            parse_channel_upsert(data).into_iter().collect()
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => parse_channel_delete(data).into_iter().collect(),
        "THREAD_LIST_SYNC" => parse_thread_list_sync(data),
        "MESSAGE_CREATE" => parse_message_create(data).into_iter().collect(),
        "MESSAGE_UPDATE" => parse_message_update(data).into_iter().collect(),
        "MESSAGE_DELETE" => parse_message_delete(data).into_iter().collect(),
        "GUILD_MEMBER_ADD" => parse_member_add(data).into_iter().collect(),
        "GUILD_MEMBER_UPDATE" => parse_member_upsert(data).into_iter().collect(),
        "GUILD_MEMBER_LIST_UPDATE" => parse_member_list_update(data),
        "GUILD_MEMBERS_CHUNK" => parse_member_chunk(data),
        "RELATIONSHIP_ADD" => parse_relationship_add(data).into_iter().collect(),
        "RELATIONSHIP_REMOVE" => parse_relationship_remove(data).into_iter().collect(),
        "GUILD_MEMBER_REMOVE" => parse_member_remove(data).into_iter().collect(),
        "PRESENCE_UPDATE" => parse_presence_update(data),
        "TYPING_START" => parse_typing_start(data).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// User-account READY embeds the full guild list under `d.guilds`. Bots get a
/// stub list of unavailable guilds and a separate `GUILD_CREATE` per guild,
/// but user accounts never send standalone GUILD_CREATEs, so we emit a
/// synthetic GuildCreate for each entry inline.
fn parse_ready(data: &Value) -> Vec<AppEvent> {
    let total_started = Instant::now();
    let mut events = Vec::new();
    let mut stats = ReadyTimingStats::default();
    let mut current_user = None;
    let mut current_user_id = None;
    let mut current_user_status = None;

    let user_started = Instant::now();
    if let Some(user) = data.get("user") {
        let user_id = user.get("id").and_then(parse_id::<UserMarker>);
        let name = user
            .get("global_name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| user.get("username").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_owned();
        events.push(AppEvent::Ready {
            user: name,
            user_id,
        });
        current_user_id = user_id;
        current_user = parse_channel_recipient_info(user);
        current_user_status = parse_current_user_session_status(data);
        if let (Some(user), Some(status)) = (current_user.as_mut(), current_user_status) {
            user.status = Some(status);
        }
    }
    stats.user = user_started.elapsed();

    let guilds_started = Instant::now();
    if let Some(guilds) = data.get("guilds").and_then(Value::as_array) {
        stats.guilds = guilds.len();
        for guild in guilds {
            stats.guild_channels += channel_array_len(guild, "channels");
            stats.guild_channels += channel_array_len(guild, "threads");
            stats.guild_members += guild
                .get("members")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            stats.guild_presences += guild
                .get("presences")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if let Some(event) = parse_guild_create(guild) {
                events.push(event);
            }
        }
    }
    events.extend(parse_merged_member_events(data));
    stats.guilds_duration = guilds_started.elapsed();

    let mut merged_presences = parse_merged_presences(data);
    if let Some(presences) = data.get("presences").and_then(Value::as_array) {
        merged_presences.extend(presences.iter().filter_map(parse_presence_entry));
    }

    // With DEDUPE_USER_OBJECTS in capabilities (bit 4), Discord ships every
    // referenced user once at the top of READY's `users` array and replaces
    // each private channel's full `recipients` array with `recipient_ids`.
    // Index those users by id once so DM hydration below is O(1) per
    // recipient.
    let users_by_id: BTreeMap<Id<UserMarker>, &Value> = data
        .get("users")
        .and_then(Value::as_array)
        .map(|users| {
            users
                .iter()
                .filter_map(|user| {
                    let id = parse_id::<UserMarker>(user.get("id")?)?;
                    Some((id, user))
                })
                .collect()
        })
        .unwrap_or_default();

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    let private_channels_started = Instant::now();
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        stats.private_channels = privates.len();
        for channel in privates {
            if let Some(mut info) = parse_channel_info(channel, None) {
                hydrate_dm_recipients_from_ids(&mut info, channel, &users_by_id);
                apply_recipient_presences(&mut info, &merged_presences);
                add_current_user_to_group_dm(&mut info, current_user.as_ref());
                events.push(AppEvent::ChannelUpsert(info));
            }
        }
    }
    stats.private_channels_duration = private_channels_started.elapsed();

    if let (Some(user_id), Some(status)) = (current_user_id, current_user_status) {
        events.push(AppEvent::UserPresenceUpdate { user_id, status });
    }

    // User-account READY ships the friend list as `relationships`. Capture
    // it as a single event so the profile popup can show friend / pending /
    // blocked badges without an extra REST round trip.
    if let Some(relationships) = data.get("relationships").and_then(Value::as_array) {
        let parsed: Vec<(Id<UserMarker>, FriendStatus)> = relationships
            .iter()
            .filter_map(parse_relationship_entry)
            .collect();
        if !parsed.is_empty() {
            events.push(AppEvent::RelationshipsLoaded {
                relationships: parsed,
            });
        }
    }

    // Guild folder ordering and grouping live in the legacy `user_settings`
    // payload (the modern `user_settings_proto` blob is base64+protobuf and is
    // skipped for now). When present, every guild appears in some folder —
    // either an explicit one or a single-guild "container" with `id == null`.
    let folders_started = Instant::now();
    if let Some(folders) = data
        .get("user_settings")
        .and_then(|settings| settings.get("guild_folders"))
        .and_then(Value::as_array)
    {
        let folders: Vec<GuildFolder> = folders.iter().filter_map(parse_guild_folder).collect();
        stats.folders = folders.len();
        if !folders.is_empty() {
            events.push(AppEvent::GuildFoldersUpdate { folders });
        }
    }
    stats.folders_duration = folders_started.elapsed();
    stats.total = total_started.elapsed();

    log_ready_stats(&stats);

    events
}

fn parse_ready_supplemental(data: &Value) -> Vec<AppEvent> {
    let mut events = parse_supplemental_guild_events(data);
    events.extend(parse_merged_member_events(data));
    events.extend(
        parse_merged_presences(data)
            .into_iter()
            .map(|(user_id, status)| AppEvent::UserPresenceUpdate { user_id, status }),
    );
    events
}

fn parse_merged_member_events(data: &Value) -> Vec<AppEvent> {
    let Some(guilds) = data.get("guilds").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(merged_members) = data.get("merged_members").and_then(Value::as_array) else {
        return Vec::new();
    };

    guilds
        .iter()
        .zip(merged_members)
        .flat_map(|(guild, members)| {
            let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) else {
                return Vec::new();
            };
            members
                .as_array()
                .map(|members| guild_member_upsert_events(guild_id, members))
                .unwrap_or_default()
        })
        .collect()
}

fn parse_supplemental_guild_events(data: &Value) -> Vec<AppEvent> {
    let Some(guilds) = data.get("guilds").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut events = Vec::new();
    for guild in guilds {
        let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) else {
            continue;
        };
        if let Some(roles) = guild.get("roles").and_then(Value::as_array) {
            let roles: Vec<RoleInfo> = roles.iter().filter_map(parse_role_info).collect();
            if !roles.is_empty() {
                events.push(AppEvent::GuildRolesUpdate { guild_id, roles });
            }
        }
        if let Some(channels) = guild.get("channels").and_then(Value::as_array) {
            events.extend(
                channels
                    .iter()
                    .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                    .map(AppEvent::ChannelUpsert),
            );
        }
        if let Some(threads) = guild.get("threads").and_then(Value::as_array) {
            events.extend(
                threads
                    .iter()
                    .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                    .map(AppEvent::ChannelUpsert),
            );
        }
        if let Some(members) = guild.get("members").and_then(Value::as_array) {
            events.extend(guild_member_upsert_events(guild_id, members));
        }
        if let Some(member) = guild.get("member").and_then(parse_member_info) {
            events.push(AppEvent::GuildMemberUpsert { guild_id, member });
        }
        if let Some(presences) = guild.get("presences").and_then(Value::as_array) {
            events.extend(presences.iter().filter_map(parse_presence_entry).map(
                |(user_id, status)| AppEvent::PresenceUpdate {
                    guild_id,
                    user_id,
                    status,
                },
            ));
        }
    }
    events
}

fn guild_member_upsert_events(guild_id: Id<GuildMarker>, members: &[Value]) -> Vec<AppEvent> {
    members
        .iter()
        .filter_map(parse_member_info)
        .map(|member| AppEvent::GuildMemberUpsert { guild_id, member })
        .collect()
}

#[derive(Default)]
struct ReadyTimingStats {
    guilds: usize,
    guild_channels: usize,
    guild_members: usize,
    guild_presences: usize,
    private_channels: usize,
    folders: usize,
    user: Duration,
    guilds_duration: Duration,
    private_channels_duration: Duration,
    folders_duration: Duration,
    total: Duration,
}

fn log_ready_stats(stats: &ReadyTimingStats) {
    logging::timing(
        "gateway",
        format!(
            "ready user={:.2}ms guilds={:.2}ms private_channels={:.2}ms folders={:.2}ms counts guilds={} channels={} members={} presences={} private_channels={} folders={}",
            ms(stats.user),
            ms(stats.guilds_duration),
            ms(stats.private_channels_duration),
            ms(stats.folders_duration),
            stats.guilds,
            stats.guild_channels,
            stats.guild_members,
            stats.guild_presences,
            stats.private_channels,
            stats.folders,
        ),
        stats.total,
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn parse_guild_folder(value: &Value) -> Option<GuildFolder> {
    let guild_ids: Vec<Id<GuildMarker>> = value
        .get("guild_ids")?
        .as_array()?
        .iter()
        .filter_map(parse_id::<GuildMarker>)
        .collect();
    if guild_ids.is_empty() {
        return None;
    }

    let id = value.get("id").and_then(Value::as_u64);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let color = value.get("color").and_then(Value::as_u64).map(|c| c as u32);

    Some(GuildFolder {
        id,
        name,
        color,
        guild_ids,
    })
}

fn parse_merged_presences(data: &Value) -> BTreeMap<Id<UserMarker>, PresenceStatus> {
    let mut presences = BTreeMap::new();
    if let Some(merged) = data.get("merged_presences") {
        collect_presence_entries(merged, &mut presences);
    }
    presences
}

fn parse_current_user_session_status(data: &Value) -> Option<PresenceStatus> {
    data.get("sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| {
            sessions.iter().find_map(|session| {
                let status = session
                    .get("status")
                    .and_then(Value::as_str)
                    .map(parse_status)?;
                (status != PresenceStatus::Unknown).then_some(status)
            })
        })
}

fn collect_presence_entries(
    value: &Value,
    presences: &mut BTreeMap<Id<UserMarker>, PresenceStatus>,
) {
    if let Some((user_id, status)) = parse_presence_entry(value) {
        presences.insert(user_id, status);
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_presence_entries(item, presences);
        }
    } else if let Some(object) = value.as_object() {
        for item in object.values() {
            collect_presence_entries(item, presences);
        }
    }
}

fn apply_recipient_presences(
    channel: &mut ChannelInfo,
    presences: &BTreeMap<Id<UserMarker>, PresenceStatus>,
) {
    let Some(recipients) = channel.recipients.as_mut() else {
        return;
    };
    for recipient in recipients {
        if let Some(status) = presences.get(&recipient.user_id) {
            recipient.status = Some(*status);
        }
    }
}

/// Resolves a private channel's `recipient_ids` against READY's deduplicated
/// `users` array. With `DEDUPE_USER_OBJECTS` enabled Discord no longer
/// inlines the full recipient objects in private channels, so without this
/// step DM rows render as `dm-{channel_id}` and the recipient sidebar is
/// empty.
fn hydrate_dm_recipients_from_ids(
    channel: &mut ChannelInfo,
    raw: &Value,
    users_by_id: &BTreeMap<Id<UserMarker>, &Value>,
) {
    if !matches!(channel.kind.as_str(), "dm" | "group-dm") {
        return;
    }
    if channel
        .recipients
        .as_ref()
        .is_some_and(|recipients| !recipients.is_empty())
    {
        return;
    }
    let Some(ids) = raw.get("recipient_ids").and_then(Value::as_array) else {
        return;
    };
    let resolved: Vec<ChannelRecipientInfo> = ids
        .iter()
        .filter_map(parse_id::<UserMarker>)
        .filter_map(|user_id| {
            let user = users_by_id.get(&user_id)?;
            parse_channel_recipient_info(user)
        })
        .collect();
    if resolved.is_empty() {
        return;
    }
    // The previous `parse_channel_info` couldn't see the recipients, so its
    // name was a synthetic `dm-{channel_id}`. Rebuild the human-readable
    // label now using the same global_name → username preference the rest of
    // the parser uses.
    let synthetic_label = format!("dm-{}", channel.channel_id.get());
    if channel.name == synthetic_label {
        channel.name = resolved
            .iter()
            .map(|recipient| recipient.display_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    channel.recipients = Some(resolved);
}

fn add_current_user_to_group_dm(
    channel: &mut ChannelInfo,
    current_user: Option<&ChannelRecipientInfo>,
) {
    if channel.kind != "group-dm" {
        return;
    }
    let Some(current_user) = current_user else {
        return;
    };
    let Some(recipients) = channel.recipients.as_mut() else {
        return;
    };
    if recipients
        .iter()
        .any(|recipient| recipient.user_id == current_user.user_id)
    {
        return;
    }
    recipients.push(current_user.clone());
}

fn parse_guild_create(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    // With user-account `capabilities` containing LAZY_USER_NOTIFICATIONS
    // (bit 0), Discord nests the guild's name / icon / owner_id under a
    // `properties` sub-object instead of placing them at the root. Fall back
    // to that location so guilds don't all render as "unknown".
    let properties = data.get("properties");
    let lookup = |key: &str| -> Option<&Value> {
        data.get(key)
            .or_else(|| properties.and_then(|p| p.get(key)))
    };
    let name = lookup("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let mut channels: Vec<ChannelInfo> = data
        .get("channels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                .collect()
        })
        .unwrap_or_default();
    if let Some(threads) = data.get("threads").and_then(Value::as_array) {
        channels.extend(
            threads
                .iter()
                .filter_map(|channel| parse_channel_info(channel, Some(guild_id))),
        );
    }

    let members = data
        .get("members")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_member_info).collect())
        .unwrap_or_default();
    let member_count = data.get("member_count").and_then(Value::as_u64);

    let presences = data
        .get("presences")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_presence_entry).collect())
        .unwrap_or_default();

    let roles = data
        .get("roles")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_role_info).collect())
        .unwrap_or_default();

    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect())
        .unwrap_or_default();

    let owner_id = lookup("owner_id").and_then(parse_id::<UserMarker>);

    Some(AppEvent::GuildCreate {
        guild_id,
        name,
        member_count,
        owner_id,
        channels,
        members,
        presences,
        roles,
        emojis,
    })
}

fn parse_role_info(value: &Value) -> Option<RoleInfo> {
    let id = parse_id::<RoleMarker>(value.get("id")?)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_owned();
    let color = value
        .get("colors")
        .and_then(|colors| colors.get("primary_color"))
        .and_then(Value::as_u64)
        .or_else(|| value.get("color").and_then(Value::as_u64))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value != 0);
    let position = value.get("position").and_then(Value::as_i64).unwrap_or(0);
    let hoist = value.get("hoist").and_then(Value::as_bool).unwrap_or(false);
    // Discord serializes `permissions` as a string-encoded 64-bit bitfield.
    // Numeric form is also accepted as a fallback for older payloads / tests.
    let permissions = value
        .get("permissions")
        .and_then(|value| {
            value
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| value.as_u64())
        })
        .unwrap_or(0);

    Some(RoleInfo {
        id,
        name,
        color,
        position,
        hoist,
        permissions,
    })
}

fn parse_custom_emoji(value: &Value) -> Option<CustomEmojiInfo> {
    let id = parse_id::<EmojiMarker>(value.get("id")?)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_owned();
    let animated = value
        .get("animated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let available = value
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Some(CustomEmojiInfo {
        id,
        name,
        animated,
        available,
    })
}

fn parse_guild_emojis_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect())
        .unwrap_or_default();

    Some(AppEvent::GuildEmojisUpdate { guild_id, emojis })
}

fn parse_guild_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    // Same lazy-mode caveat as `parse_guild_create`: with capabilities such
    // as LAZY_USER_NOTIFICATIONS enabled, name/owner_id can ride inside a
    // `properties` sub-object instead of at the root.
    let properties = data.get("properties");
    let lookup = |key: &str| -> Option<&Value> {
        data.get(key)
            .or_else(|| properties.and_then(|p| p.get(key)))
    };
    let name = lookup("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect());
    let roles = data
        .get("roles")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_role_info).collect());
    let owner_id = lookup("owner_id").and_then(parse_id::<UserMarker>);
    Some(AppEvent::GuildUpdate {
        guild_id,
        name,
        owner_id,
        roles,
        emojis,
    })
}

fn parse_guild_delete(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    Some(AppEvent::GuildDelete { guild_id })
}

fn parse_channel_upsert(data: &Value) -> Option<AppEvent> {
    let info = parse_channel_info(data, None)?;
    Some(AppEvent::ChannelUpsert(info))
}

fn parse_channel_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::ChannelDelete {
        guild_id,
        channel_id,
    })
}

fn parse_thread_list_sync(data: &Value) -> Vec<AppEvent> {
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    data.get("threads")
        .and_then(Value::as_array)
        .map(|threads| {
            threads
                .iter()
                .filter_map(|thread| parse_channel_info(thread, guild_id))
                .map(AppEvent::ChannelUpsert)
                .collect()
        })
        .unwrap_or_default()
}

fn channel_array_len(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

fn parse_message_create(data: &Value) -> Option<AppEvent> {
    let message = parse_message_info(data)?;
    Some(AppEvent::MessageCreate {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.message_id,
        author_id: message.author_id,
        author: message.author,
        author_avatar_url: message.author_avatar_url,
        author_role_ids: message.author_role_ids,
        message_kind: message.message_kind,
        reference: message.reference,
        reply: message.reply,
        poll: message.poll,
        content: message.content,
        mentions: message.mentions,
        attachments: message.attachments,
        embeds: message.embeds,
        forwarded_snapshots: message.forwarded_snapshots,
    })
}

pub(crate) fn parse_message_info(data: &Value) -> Option<MessageInfo> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let author = data.get("author")?;
    let author_id = parse_id::<UserMarker>(author.get("id")?)?;
    let author_name = message_author_display_name(data, author);
    let author_avatar_url = raw_user_avatar_url(author_id, author);
    let author_role_ids = parse_message_author_role_ids(data);
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let message_kind = data
        .get("type")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .map(MessageKind::new)
        .unwrap_or_default();
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mentions = parse_mentions(data.get("mentions"));
    let attachments = parse_attachments(data.get("attachments"));
    let embeds = parse_embeds(data.get("embeds"));
    let reply = data.get("referenced_message").and_then(parse_reply_info);
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let reference = data
        .get("message_reference")
        .map(parse_message_reference_info);
    let source_channel_id = reference
        .as_ref()
        .and_then(|reference| reference.channel_id);
    let forwarded_snapshots =
        parse_message_snapshots(data.get("message_snapshots"), source_channel_id);
    Some(MessageInfo {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: author_name,
        author_avatar_url,
        author_role_ids,
        message_kind,
        reference,
        reply,
        poll,
        pinned: data.get("pinned").and_then(Value::as_bool).unwrap_or(false),
        reactions: parse_reactions(data.get("reactions")),
        content,
        mentions,
        attachments,
        embeds,
        forwarded_snapshots,
    })
}

fn parse_message_author_role_ids(data: &Value) -> Vec<Id<RoleMarker>> {
    data.get("member")
        .and_then(|member| member.get("roles"))
        .and_then(Value::as_array)
        .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
        .unwrap_or_default()
}

fn parse_message_reference_info(value: &Value) -> MessageReferenceInfo {
    MessageReferenceInfo {
        guild_id: value.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: value.get("channel_id").and_then(parse_id::<ChannelMarker>),
        message_id: value.get("message_id").and_then(parse_id::<MessageMarker>),
    }
}

fn parse_message_update(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let attachments = if data.get("attachments").is_some() {
        AttachmentUpdate::Replace(parse_attachments(data.get("attachments")))
    } else {
        AttachmentUpdate::Unchanged
    };
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let embeds = data.get("embeds").map(|value| parse_embeds(Some(value)));
    let mentions = data
        .get("mentions")
        .map(|value| parse_mentions(Some(value)));
    Some(AppEvent::MessageUpdate {
        guild_id,
        channel_id,
        message_id,
        poll,
        content,
        mentions,
        attachments,
        embeds,
    })
}

fn parse_message_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::MessageDelete {
        guild_id,
        channel_id,
        message_id,
    })
}

fn parse_member_upsert(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data)?;
    Some(AppEvent::GuildMemberUpsert { guild_id, member })
}

fn parse_member_add(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data)?;
    Some(AppEvent::GuildMemberAdd { guild_id, member })
}

fn parse_member_chunk(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };

    let mut events: Vec<AppEvent> = data
        .get("members")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(parse_member_info)
                .map(|member| AppEvent::GuildMemberUpsert { guild_id, member })
                .collect()
        })
        .unwrap_or_default();

    if let Some(presences) = data.get("presences").and_then(Value::as_array) {
        events.extend(presences.iter().filter_map(parse_presence_entry).map(
            |(user_id, status)| AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
            },
        ));
    }

    events
}

fn parse_member_list_update(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };
    let Some(ops) = data.get("ops").and_then(Value::as_array) else {
        return Vec::new();
    };

    // A single GUILD_MEMBER_LIST_UPDATE event can carry SYNC ops for several
    // ranges (e.g. `[0,99]` plus `[100,199]`). We previously dropped every
    // SYNC whose range did not start at zero, which left members past the
    // first chunk invisible in larger guilds.
    let mut events = Vec::new();
    for op in ops {
        match op.get("op").and_then(Value::as_str) {
            Some("SYNC") => {
                if let Some(items) = op.get("items").and_then(Value::as_array) {
                    for item in items {
                        events.extend(parse_member_list_item(guild_id, item));
                    }
                }
            }
            Some("INSERT" | "UPDATE") => {
                if let Some(item) = op.get("item") {
                    events.extend(parse_member_list_item(guild_id, item));
                }
            }
            _ => {}
        }
    }

    events
}

fn parse_member_list_item(guild_id: Id<GuildMarker>, item: &Value) -> Vec<AppEvent> {
    let Some(member) = item
        .get("member")
        .or_else(|| item.get("user").map(|_| item))
    else {
        return Vec::new();
    };
    let Some(member_info) = parse_member_info(member) else {
        return Vec::new();
    };
    let user_id = member_info.user_id;
    let status = member
        .get("presence")
        .and_then(|presence| presence.get("status"))
        .and_then(Value::as_str)
        .map(parse_status);

    let mut events = vec![AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_info,
    }];
    if let Some(status) = status {
        events.push(AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status,
        });
    }
    events
}

fn parse_attachments(value: Option<&Value>) -> Vec<AttachmentInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_attachment).collect())
        .unwrap_or_default()
}

fn parse_embeds(value: Option<&Value>) -> Vec<EmbedInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_embed).collect())
        .unwrap_or_default()
}

fn parse_embed(value: &Value) -> Option<EmbedInfo> {
    if value.get("type").and_then(Value::as_str) == Some("poll_result") {
        return None;
    }

    let fields = value
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields| fields.iter().filter_map(parse_embed_field).collect())
        .unwrap_or_default();
    let embed = EmbedInfo {
        color: value
            .get("color")
            .and_then(Value::as_u64)
            .and_then(|color| u32::try_from(color).ok()),
        provider_name: value
            .get("provider")
            .and_then(|provider| provider.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        author_name: value
            .get("author")
            .and_then(|author| author.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        fields,
        footer_text: value
            .get("footer")
            .and_then(|footer| footer.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        url: value.get("url").and_then(Value::as_str).map(str::to_owned),
        thumbnail_url: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        thumbnail_width: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("width"))
            .and_then(Value::as_u64),
        thumbnail_height: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("height"))
            .and_then(Value::as_u64),
        image_url: value
            .get("image")
            .and_then(|image| image.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        image_width: value
            .get("image")
            .and_then(|image| image.get("width"))
            .and_then(Value::as_u64),
        image_height: value
            .get("image")
            .and_then(|image| image.get("height"))
            .and_then(Value::as_u64),
        video_url: value
            .get("video")
            .and_then(|video| video.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    };

    embed_has_renderable_content(&embed).then_some(embed)
}

fn parse_embed_field(value: &Value) -> Option<EmbedFieldInfo> {
    Some(EmbedFieldInfo {
        name: value.get("name")?.as_str()?.to_owned(),
        value: value.get("value")?.as_str()?.to_owned(),
    })
}

fn embed_has_renderable_content(embed: &EmbedInfo) -> bool {
    embed.provider_name.is_some()
        || embed.author_name.is_some()
        || embed.title.is_some()
        || embed.description.is_some()
        || !embed.fields.is_empty()
        || embed.footer_text.is_some()
        || embed.url.is_some()
        || embed.thumbnail_url.is_some()
        || embed.image_url.is_some()
        || embed.video_url.is_some()
}

fn parse_message_snapshots(
    value: Option<&Value>,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Vec<MessageSnapshotInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_message_snapshot(item, source_channel_id))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_reply_info(value: &Value) -> Option<ReplyInfo> {
    if value.is_null() {
        return None;
    }

    let author = value.get("author")?;
    let author_name = message_author_display_name(value, author);
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mentions = parse_mentions(value.get("mentions"));

    Some(ReplyInfo {
        author: author_name,
        content,
        mentions,
    })
}

fn parse_mentions(value: Option<&Value>) -> Vec<MentionInfo> {
    value
        .and_then(Value::as_array)
        .map(|mentions| mentions.iter().filter_map(parse_mention_info).collect())
        .unwrap_or_default()
}

fn parse_reactions(value: Option<&Value>) -> Vec<ReactionInfo> {
    value
        .and_then(Value::as_array)
        .map(|reactions| reactions.iter().filter_map(parse_reaction_info).collect())
        .unwrap_or_default()
}

fn parse_reaction_info(value: &Value) -> Option<ReactionInfo> {
    Some(ReactionInfo {
        emoji: parse_reaction_emoji(value.get("emoji")?)?,
        count: value.get("count").and_then(Value::as_u64).unwrap_or(0),
        me: value.get("me").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn parse_reaction_emoji(value: &Value) -> Option<ReactionEmoji> {
    if let Some(id) = value.get("id").and_then(parse_id::<EmojiMarker>) {
        return Some(ReactionEmoji::Custom {
            id,
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            animated: value
                .get("animated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(|name| ReactionEmoji::Unicode(name.to_owned()))
}

fn parse_mention_info(value: &Value) -> Option<MentionInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let member = value.get("member");
    let nick = value
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = value
        .get("username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let display_name = nick.or(global_name).or(username)?;
    log_mention_raw_fields(user_id, member, nick, global_name, username, display_name);

    Some(MentionInfo {
        user_id,
        guild_nick: nick.map(str::to_owned),
        display_name: display_name.to_owned(),
    })
}

fn log_mention_raw_fields(
    user_id: Id<UserMarker>,
    member: Option<&Value>,
    nick: Option<&str>,
    global_name: Option<&str>,
    username: Option<&str>,
    display_name: &str,
) {
    logging::debug(
        "gateway",
        format!(
            "mention raw fields user_id={} has_member={} nick={} global_name={} username={} display_name={}",
            user_id.get(),
            member.is_some(),
            log_optional_name(nick),
            log_optional_name(global_name),
            log_optional_name(username),
            display_name,
        ),
    );
}

fn log_optional_name(value: Option<&str>) -> &str {
    value.unwrap_or("<missing>")
}

fn message_author_display_name(message: &Value, author: &Value) -> String {
    let nick = message
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = author
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = author.get("username").and_then(Value::as_str);
    nick.or(global_name)
        .or(username)
        .unwrap_or("unknown")
        .to_owned()
}

fn parse_poll_info(value: &Value) -> Option<PollInfo> {
    if value.is_null() {
        return None;
    }

    let question = value
        .get("question")
        .and_then(|question| question.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no question text>")
        .to_owned();
    let answers: Vec<PollAnswerInfo> = value
        .get("answers")
        .and_then(Value::as_array)
        .map(|answers| answers.iter().filter_map(parse_poll_answer_info).collect())
        .unwrap_or_default();
    let allow_multiselect = value
        .get("allow_multiselect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let results = value.get("results");
    let results_finalized = results
        .and_then(|results| results.get("is_finalized"))
        .and_then(Value::as_bool);
    let total_votes = results
        .and_then(|results| results.get("answer_counts"))
        .and_then(Value::as_array)
        .map(|counts| {
            counts
                .iter()
                .filter_map(|count| count.get("count").and_then(Value::as_u64))
                .sum()
        });

    Some(PollInfo {
        question,
        answers: answers
            .into_iter()
            .map(|mut answer| {
                if let Some(count) = poll_answer_count(results, answer.answer_id) {
                    answer.vote_count = Some(count.0);
                    answer.me_voted = count.1;
                }
                answer
            })
            .collect(),
        allow_multiselect,
        results_finalized,
        total_votes,
    })
}

fn parse_poll_result_embed(value: Option<&Value>) -> Option<PollInfo> {
    let embed = value?
        .as_array()?
        .iter()
        .find(|embed| embed.get("type").and_then(Value::as_str) == Some("poll_result"))?;
    let fields = embed.get("fields")?.as_array()?;
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;

    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(value) = field.get("value").and_then(Value::as_str) else {
            continue;
        };
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();
    Some(PollInfo {
        question: question.unwrap_or_else(|| "Poll results".to_owned()),
        answers,
        allow_multiselect: false,
        results_finalized: Some(true),
        total_votes,
    })
}

fn parse_poll_answer_info(value: &Value) -> Option<PollAnswerInfo> {
    let answer_id = value
        .get("answer_id")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())?;
    let text = value
        .get("poll_media")
        .and_then(|media| media.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no answer text>")
        .to_owned();

    Some(PollAnswerInfo {
        answer_id,
        text,
        vote_count: None,
        me_voted: false,
    })
}

fn poll_answer_count(results: Option<&Value>, answer_id: u8) -> Option<(u64, bool)> {
    results?
        .get("answer_counts")?
        .as_array()?
        .iter()
        .find(|count| {
            count
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|id| id == u64::from(answer_id))
        })
        .map(|count| {
            (
                count.get("count").and_then(Value::as_u64).unwrap_or(0),
                count
                    .get("me_voted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
}

fn parse_message_snapshot(
    value: &Value,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Option<MessageSnapshotInfo> {
    let message = value.get("message")?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let attachments = parse_attachments(message.get("attachments"));
    let embeds = parse_embeds(message.get("embeds"));
    let mentions = parse_mentions(message.get("mentions"));
    let timestamp = message
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);

    if content.as_deref().is_some_and(|value| !value.is_empty())
        || !attachments.is_empty()
        || !embeds.is_empty()
        || source_channel_id.is_some()
        || timestamp.is_some()
    {
        Some(MessageSnapshotInfo {
            content,
            mentions,
            attachments,
            embeds,
            source_channel_id,
            timestamp,
        })
    } else {
        None
    }
}

fn parse_attachment(value: &Value) -> Option<AttachmentInfo> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| value.get("proxy_url").and_then(Value::as_str))?
        .to_owned();
    let proxy_url = value
        .get("proxy_url")
        .and_then(Value::as_str)
        .unwrap_or(url.as_str())
        .to_owned();

    Some(AttachmentInfo {
        id: parse_id::<AttachmentMarker>(value.get("id")?)?,
        filename: value.get("filename")?.as_str()?.to_owned(),
        url,
        proxy_url,
        content_type: value
            .get("content_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        size: value
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        width: value.get("width").and_then(Value::as_u64),
        height: value.get("height").and_then(Value::as_u64),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn parse_member_remove(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let user = data.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    Some(AppEvent::GuildMemberRemove { guild_id, user_id })
}

fn parse_presence_update(data: &Value) -> Vec<AppEvent> {
    let Some((user_id, status)) = parse_presence_entry(data) else {
        return Vec::new();
    };
    if let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) {
        vec![AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status,
        }]
    } else {
        vec![AppEvent::UserPresenceUpdate { user_id, status }]
    }
}

/// Discord's TYPING_START shape: `{ channel_id, guild_id?, user_id,
/// timestamp, member? }`. Guild channels carry the typer's user_id directly,
/// while DMs sometimes only embed it under `member.user.id`. We accept both
/// and ignore the timestamp (state stamps its own Instant on receive).
fn parse_typing_start(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let user_id = data
        .get("user_id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("member")
                .and_then(|member| member.get("user"))
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    Some(AppEvent::TypingStart {
        channel_id,
        user_id,
    })
}

fn parse_channel_info(
    value: &Value,
    default_guild: Option<Id<GuildMarker>>,
) -> Option<ChannelInfo> {
    let channel_id = parse_id::<ChannelMarker>(value.get("id")?)?;
    let guild_id = value
        .get("guild_id")
        .and_then(parse_id::<GuildMarker>)
        .or(default_guild);
    let parent_id = value.get("parent_id").and_then(parse_id::<ChannelMarker>);
    let position = value
        .get("position")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());
    let last_message_id = value
        .get("last_message_id")
        .and_then(parse_id::<MessageMarker>);

    // Map Discord channel type integers to friendlier strings. DMs and
    // group-DMs are special-cased so the dashboard can render them with
    // a dedicated prefix.
    let kind = match value.get("type").and_then(Value::as_u64) {
        Some(0) => "text".to_owned(),
        Some(1) => "dm".to_owned(),
        Some(2) => "voice".to_owned(),
        Some(3) => "group-dm".to_owned(),
        Some(4) => "category".to_owned(),
        Some(5) => "announcement".to_owned(),
        Some(10) => "GuildNewsThread".to_owned(),
        Some(11) => "GuildPublicThread".to_owned(),
        Some(12) => "GuildPrivateThread".to_owned(),
        Some(13) => "stage".to_owned(),
        Some(15) => "forum".to_owned(),
        Some(other) => format!("type-{other}"),
        None => "channel".to_owned(),
    };

    let explicit_name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let name = explicit_name.unwrap_or_else(|| {
        if matches!(kind.as_str(), "dm" | "group-dm") {
            recipient_label(value).unwrap_or_else(|| format!("dm-{}", channel_id.get()))
        } else {
            format!("channel-{}", channel_id.get())
        }
    });
    let recipients = if matches!(kind.as_str(), "dm" | "group-dm") {
        value.get("recipients").and_then(|recipients| {
            Some(
                recipients
                    .as_array()?
                    .iter()
                    .filter_map(parse_channel_recipient_info)
                    .collect(),
            )
        })
    } else {
        None
    };

    let permission_overwrites = value
        .get("permission_overwrites")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_permission_overwrite)
                .collect()
        })
        .unwrap_or_default();

    Some(ChannelInfo {
        guild_id,
        channel_id,
        parent_id,
        position,
        last_message_id,
        name,
        kind,
        message_count: value.get("message_count").and_then(Value::as_u64),
        total_message_sent: value.get("total_message_sent").and_then(Value::as_u64),
        thread_archived: value
            .get("thread_metadata")
            .and_then(|metadata| metadata.get("archived"))
            .and_then(Value::as_bool),
        thread_locked: value
            .get("thread_metadata")
            .and_then(|metadata| metadata.get("locked"))
            .and_then(Value::as_bool),
        recipients,
        permission_overwrites,
    })
}

/// Parse one entry from a channel's `permission_overwrites` array. Discord
/// serializes the bitfields as decimal strings; the numeric fallback keeps
/// the parser tolerant of synthetic payloads (used in tests).
fn parse_permission_overwrite(value: &Value) -> Option<PermissionOverwriteInfo> {
    let id = value.get("id").and_then(|value| {
        value
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| value.as_u64())
    })?;
    let kind = match value.get("type").and_then(Value::as_u64)? {
        0 => PermissionOverwriteKind::Role,
        1 => PermissionOverwriteKind::Member,
        // Forward-compat: ignore unknown overwrite kinds so we neither grant
        // nor deny VIEW_CHANNEL based on a discriminant we can't interpret.
        _ => return None,
    };
    let parse_bits = |key: &str| -> u64 {
        value
            .get(key)
            .and_then(|value| {
                value
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| value.as_u64())
            })
            .unwrap_or(0)
    };
    Some(PermissionOverwriteInfo {
        id,
        kind,
        allow: parse_bits("allow"),
        deny: parse_bits("deny"),
    })
}

/// For DM channels, derive a display label from the recipients' names.
/// Skips the local user when present so 1-on-1 DMs read as just the peer.
fn recipient_label(value: &Value) -> Option<String> {
    let recipients = value.get("recipients")?.as_array()?;
    let names: Vec<String> = recipients
        .iter()
        .filter_map(|recipient| {
            let global = recipient
                .get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            let username = recipient.get("username").and_then(Value::as_str);
            global.or(username).map(str::to_owned)
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(names.join(", "))
}

fn parse_channel_recipient_info(value: &Value) -> Option<ChannelRecipientInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let global_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = value.get("username").and_then(Value::as_str);
    let display_name = global_name.or(username).unwrap_or("unknown").to_owned();
    let is_bot = value.get("bot").and_then(Value::as_bool).unwrap_or(false);
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status);

    Some(ChannelRecipientInfo {
        user_id,
        display_name,
        username: username.map(str::to_owned),
        is_bot,
        avatar_url: raw_user_avatar_url(user_id, value),
        status,
    })
}

fn parse_member_info(value: &Value) -> Option<MemberInfo> {
    let user = value.get("user");
    let user_id = user
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)?;
    let nick = value
        .get("nick")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = user
        .and_then(|user| user.get("global_name"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = user
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str);
    let display_name = nick
        .or(global_name)
        .or(username)
        .unwrap_or("unknown")
        .to_owned();
    let is_bot = user
        .and_then(|user| user.get("bot"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(MemberInfo {
        user_id,
        display_name,
        username: username.map(str::to_owned),
        is_bot,
        avatar_url: user.and_then(|user| raw_user_avatar_url(user_id, user)),
        role_ids: value
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
            .unwrap_or_default(),
    })
}

fn raw_user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
    let avatar = user
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    Some(match avatar {
        Some(hash) => {
            let extension = if hash.starts_with("a_") { "gif" } else { "png" };
            format!("https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}")
        }
        None => default_avatar_url(user_id, raw_discriminator(user).unwrap_or(0)),
    })
}

fn raw_discriminator(user: &Value) -> Option<u16> {
    user.get("discriminator").and_then(|value| {
        value
            .as_str()
            .and_then(|value| value.parse::<u16>().ok())
            .or_else(|| value.as_u64().and_then(|value| u16::try_from(value).ok()))
    })
}

fn parse_presence_entry(value: &Value) -> Option<(Id<UserMarker>, PresenceStatus)> {
    let user_id = presence_user_id(value)?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)?;
    Some((user_id, status))
}

fn presence_user_id(value: &Value) -> Option<Id<UserMarker>> {
    value
        .get("user")
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)
}

fn parse_relationship_add(data: &Value) -> Option<AppEvent> {
    let (user_id, status) = parse_relationship_entry(data)?;
    Some(AppEvent::RelationshipUpsert { user_id, status })
}

fn parse_relationship_remove(data: &Value) -> Option<AppEvent> {
    let user_id = data
        .get("id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("user")
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    Some(AppEvent::RelationshipRemove { user_id })
}

fn parse_relationship_entry(value: &Value) -> Option<(Id<UserMarker>, FriendStatus)> {
    // READY's `relationships` array uses ids on the entry itself for the
    // target user. Older shards may nest it under `user.id`; check both.
    let user_id = value
        .get("id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            value
                .get("user")
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    let kind = value.get("type").and_then(Value::as_u64)?;
    let status = match kind {
        1 => FriendStatus::Friend,
        2 => FriendStatus::Blocked,
        3 => FriendStatus::IncomingRequest,
        4 => FriendStatus::OutgoingRequest,
        _ => return None,
    };
    Some((user_id, status))
}

fn parse_status(value: &str) -> PresenceStatus {
    match value {
        "online" => PresenceStatus::Online,
        "idle" => PresenceStatus::Idle,
        "dnd" => PresenceStatus::DoNotDisturb,
        "offline" | "invisible" => PresenceStatus::Offline,
        _ => PresenceStatus::Unknown,
    }
}

fn parse_id<M>(value: &Value) -> Option<Id<M>> {
    value
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .map(Id::new)
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker},
    };
    use serde_json::json;

    use super::{
        SessionState, USER_ACCOUNT_CAPABILITIES, build_identify_payload, build_resume_payload,
        direct_message_subscribe_payload, guild_channel_subscribe_payload, parse_channel_info,
        parse_guild_create, parse_guild_emojis_update, parse_guild_update, parse_message_create,
        parse_message_info, parse_message_update, parse_user_account_event,
    };
    use crate::discord::{
        AppEvent, AttachmentUpdate, ChannelVisibilityStats, DiscordState, FriendStatus,
        MentionInfo, MessageKind, PollAnswerInfo, PollInfo, PresenceStatus, ReplyInfo,
    };

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
    fn guild_channel_subscribe_payload_matches_expected_shape() {
        let payload: serde_json::Value = serde_json::from_str(&guild_channel_subscribe_payload(
            Id::<GuildMarker>::new(10),
            Id::<ChannelMarker>::new(20),
            &[(0, 99)],
        ))
        .expect("payload should be valid json");

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

    #[test]
    fn guild_channel_subscribe_payload_emits_extended_member_ranges() {
        let payload: serde_json::Value = serde_json::from_str(&guild_channel_subscribe_payload(
            Id::<GuildMarker>::new(10),
            Id::<ChannelMarker>::new(20),
            &[(0, 99), (100, 199), (200, 299)],
        ))
        .expect("payload should be valid json");

        assert_eq!(
            payload["d"]["subscriptions"]["10"]["channels"]["20"],
            json!([[0, 99], [100, 199], [200, 299]])
        );
    }

    #[test]
    fn raw_member_list_update_populates_members_and_presence() {
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBER_LIST_UPDATE",
                "d": {
                    "guild_id": "10",
                    "ops": [{
                        "op": "SYNC",
                        "range": [0, 99],
                        "items": [{
                            "member": {
                                "user": {
                                    "id": "20",
                                    "username": "alice",
                                    "global_name": "Alice"
                                },
                                "nick": "Alice Nick",
                                "roles": ["30"],
                                "presence": { "status": "idle" }
                            }
                        }]
                    }]
                }
            })
            .to_string(),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10)
                    && member.user_id == Id::new(20)
                    && member.display_name == "Alice Nick"
                    && member.role_ids == vec![Id::new(30)]
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Idle
        )));
    }

    #[test]
    fn raw_member_list_update_processes_all_sync_ranges() {
        // Discord can ship more than one SYNC chunk in a single
        // GUILD_MEMBER_LIST_UPDATE — e.g. range [0,99] plus [100,199] — and
        // we need members from every chunk, not just the first.
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBER_LIST_UPDATE",
                "d": {
                    "guild_id": "10",
                    "ops": [
                        {
                            "op": "SYNC",
                            "range": [0, 99],
                            "items": [{
                                "member": {
                                    "user": { "id": "20", "username": "alice" },
                                    "roles": [],
                                    "presence": { "status": "online" }
                                }
                            }]
                        },
                        {
                            "op": "SYNC",
                            "range": [100, 199],
                            "items": [{
                                "member": {
                                    "user": { "id": "21", "username": "bob" },
                                    "roles": [],
                                    "presence": { "status": "idle" }
                                }
                            }]
                        }
                    ]
                }
            })
            .to_string(),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10) && member.user_id == Id::new(20)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10) && member.user_id == Id::new(21)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { user_id, status, .. }
                if *user_id == Id::new(21) && *status == PresenceStatus::Idle
        )));
    }

    #[test]
    fn raw_member_list_update_handles_insert_and_update_items() {
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBER_LIST_UPDATE",
                "d": {
                    "guild_id": "10",
                    "ops": [
                        {
                            "op": "INSERT",
                            "item": {
                                "member": {
                                    "user": {
                                        "id": "20",
                                        "username": "alice"
                                    },
                                    "roles": [],
                                    "presence": { "status": "online" }
                                }
                            }
                        },
                        {
                            "op": "UPDATE",
                            "item": {
                                "member": {
                                    "user": {
                                        "id": "30",
                                        "username": "bob"
                                    },
                                    "roles": [],
                                    "presence": { "status": "dnd" }
                                }
                            }
                        }
                    ]
                }
            })
            .to_string(),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Online
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(30)
                    && *status == PresenceStatus::DoNotDisturb
        )));
    }

    #[test]
    fn relationship_add_emits_friend_upsert() {
        let events = parse_user_account_event(
            &json!({
                "t": "RELATIONSHIP_ADD",
                "d": {
                    "id": "20",
                    "type": 1,
                    "user": {"id": "20", "username": "alice"}
                }
            })
            .to_string(),
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::RelationshipUpsert { user_id, status }
                if *user_id == Id::new(20) && *status == FriendStatus::Friend
        ));
    }

    #[test]
    fn relationship_remove_emits_event() {
        let events = parse_user_account_event(
            &json!({
                "t": "RELATIONSHIP_REMOVE",
                "d": {"id": "20", "type": 3}
            })
            .to_string(),
        );
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::RelationshipRemove { user_id } if *user_id == Id::new(20)
        ));
    }

    #[test]
    fn channel_parser_keeps_last_message_id() {
        let channel = parse_channel_info(
            &json!({
                "id": "10",
                "type": 1,
                "last_message_id": "99",
                "recipients": [{ "username": "neo" }]
            }),
            None,
        )
        .expect("dm channel should parse");

        assert_eq!(channel.last_message_id.map(|id| id.get()), Some(99));
    }

    #[test]
    fn raw_ready_parser_adds_current_user_to_group_dm_recipients() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "sessions": [{ "status": "idle" }],
                    "guilds": [],
                    "merged_presences": {
                        "friends": [
                            { "user": { "id": "20" }, "status": "online" },
                            { "user": { "id": "30" }, "status": "idle" }
                        ]
                    },
                    "private_channels": [{
                        "id": "10",
                        "type": 3,
                        "name": "project chat",
                        "recipients": [
                            {
                                "id": "20",
                                "username": "alice",
                                "global_name": "Alice",
                                "bot": false
                            },
                            {
                                "id": "30",
                                "username": "helper-bot",
                                "bot": true
                            }
                        ]
                    }]
                }
            })
            .to_string(),
        );

        let channel = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(channel) => Some(channel),
                _ => None,
            })
            .expect("ready should emit a private channel upsert");
        let recipients = channel
            .recipients
            .as_ref()
            .expect("group dm should carry recipients");

        assert_eq!(channel.kind, "group-dm");
        assert_eq!(recipients.len(), 3);
        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].display_name, "Alice");
        assert!(!recipients[0].is_bot);
        assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
        assert_eq!(recipients[1].display_name, "helper-bot");
        assert!(recipients[1].is_bot);
        assert_eq!(recipients[1].status, Some(PresenceStatus::Idle));
        assert_eq!(recipients[2].user_id, Id::new(99));
        assert_eq!(recipients[2].display_name, "neo");
        assert_eq!(recipients[2].status, Some(PresenceStatus::Idle));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status }
                if *user_id == Id::new(99) && *status == PresenceStatus::Idle
        )));
    }

    #[test]
    fn raw_ready_parser_applies_guild_merged_presence_to_dm_recipient() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [],
                    "merged_presences": {
                        "friends": [],
                        "guilds": [[
                            { "user_id": "20", "status": "idle" }
                        ]]
                    },
                    "private_channels": [{
                        "id": "10",
                        "type": 1,
                        "recipients": [{
                            "id": "20",
                            "username": "alice"
                        }]
                    }]
                }
            })
            .to_string(),
        );

        let channel = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(channel) => Some(channel),
                _ => None,
            })
            .expect("ready should emit a private channel upsert");
        let recipients = channel
            .recipients
            .as_ref()
            .expect("dm should carry recipients");

        assert_eq!(channel.kind, "dm");
        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].status, Some(PresenceStatus::Idle));
    }

    #[test]
    fn raw_ready_parser_applies_top_level_presence_to_dm_recipient() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [],
                    "presences": [{
                        "user": { "id": "20" },
                        "status": "online"
                    }],
                    "private_channels": [{
                        "id": "10",
                        "type": 1,
                        "recipients": [{
                            "id": "20",
                            "username": "alice"
                        }]
                    }]
                }
            })
            .to_string(),
        );

        let channel = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(channel) => Some(channel),
                _ => None,
            })
            .expect("ready should emit a private channel upsert");
        let recipients = channel
            .recipients
            .as_ref()
            .expect("dm should carry recipients");

        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
    }

    #[test]
    fn raw_ready_supplemental_updates_user_presences() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [
                            { "user_id": "20", "status": "online" }
                        ],
                        "guilds": [[
                            { "user_id": "30", "status": "idle" }
                        ]]
                    }
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status }
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status }
                if *user_id == Id::new(30) && *status == PresenceStatus::Idle
        )));
    }

    #[test]
    fn raw_ready_supplemental_updates_merged_member_roles() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "guilds": [{ "id": "1" }],
                    "merged_members": [[{
                        "user_id": "10",
                        "roles": ["20"]
                    }]]
                }
            })
            .to_string(),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.role_ids == vec![Id::new(20)]
        )));
    }

    #[test]
    fn raw_ready_supplemental_aligns_merged_members_by_guild_index() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "guilds": [{ "id": "1" }, { "id": "2" }],
                    "merged_members": [[{
                        "user_id": "10",
                        "roles": ["20"]
                    }], [{
                        "user_id": "10",
                        "roles": ["30"]
                    }]]
                }
            })
            .to_string(),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.role_ids == vec![Id::new(20)]
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(2)
                    && member.user_id == Id::new(10)
                    && member.role_ids == vec![Id::new(30)]
        )));
    }

    #[test]
    fn raw_ready_supplemental_member_roles_hide_role_denied_channel() {
        let ready_events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": { "id": "10", "username": "me" },
                    "guilds": [{
                        "id": "1",
                        "name": "guild",
                        "owner_id": "11",
                        "channels": [{
                            "id": "2",
                            "type": 0,
                            "name": "staff-hidden",
                            "permission_overwrites": [{
                                "id": "20",
                                "type": 0,
                                "allow": "0",
                                "deny": "1024"
                            }]
                        }],
                        "members": [],
                        "presences": [],
                        "roles": [],
                        "emojis": []
                    }],
                    "private_channels": []
                }
            })
            .to_string(),
        );
        let supplemental_events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "guilds": [{
                        "id": "1",
                        "roles": [{
                            "id": "1",
                            "name": "@everyone",
                            "permissions": "1024",
                            "position": 0,
                            "hoist": false
                        }, {
                            "id": "20",
                            "name": "Staff",
                            "permissions": "0",
                            "position": 1,
                            "hoist": false
                        }]
                    }],
                    "merged_members": [[{
                        "user_id": "10",
                        "roles": ["20"]
                    }]]
                }
            })
            .to_string(),
        );
        let mut state = DiscordState::default();
        for event in ready_events.iter().chain(supplemental_events.iter()) {
            state.apply_event(event);
        }

        assert_eq!(
            state.channel_visibility_stats(Some(Id::new(1))),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        assert!(
            state
                .viewable_channels_for_guild(Some(Id::new(1)))
                .is_empty()
        );
    }

    #[test]
    fn raw_ready_supplemental_accepts_bare_id_presence_entries() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [
                            { "id": "20", "status": "online" }
                        ]
                    }
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        ));
    }

    #[test]
    fn raw_ready_supplemental_ignores_non_presence_ids() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [],
                        "metadata": { "id": "20" }
                    }
                }
            })
            .to_string(),
        );

        assert!(events.is_empty());
    }

    #[test]
    fn raw_presence_update_without_guild_updates_user_presence() {
        let events = parse_user_account_event(
            &json!({
                "t": "PRESENCE_UPDATE",
                "d": {
                    "user": { "id": "20" },
                    "status": "dnd"
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::DoNotDisturb
        ));
    }

    #[test]
    fn raw_presence_update_accepts_user_id_field() {
        let events = parse_user_account_event(
            &json!({
                "t": "PRESENCE_UPDATE",
                "d": {
                    "user_id": "20",
                    "status": "online"
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        ));
    }

    #[test]
    fn thread_channel_parser_keeps_counts_and_status() {
        let channel = parse_channel_info(
            &json!({
                "id": "10",
                "guild_id": "1",
                "parent_id": "2",
                "type": 11,
                "name": "release notes",
                "message_count": 12,
                "total_message_sent": 14,
                "thread_metadata": { "archived": true, "locked": false }
            }),
            None,
        )
        .expect("thread channel should parse");

        assert_eq!(channel.kind, "GuildPublicThread");
        assert_eq!(channel.message_count, Some(12));
        assert_eq!(channel.total_message_sent, Some(14));
        assert_eq!(channel.thread_archived, Some(true));
        assert_eq!(channel.thread_locked, Some(false));
    }

    #[test]
    fn raw_thread_create_upserts_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_CREATE",
                "d": thread_payload(10, "release notes")
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelUpsert(channel)]
                if channel.channel_id == Id::new(10)
                    && channel.guild_id == Some(Id::new(1))
                    && channel.parent_id == Some(Id::new(2))
                    && channel.name == "release notes"
                    && channel.kind == "GuildPublicThread"
                    && channel.message_count == Some(12)
                    && channel.total_message_sent == Some(14)
                    && channel.thread_archived == Some(false)
                    && channel.thread_locked == Some(false)
        ));
    }

    #[test]
    fn raw_thread_update_upserts_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_UPDATE",
                "d": thread_payload(10, "renamed thread")
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelUpsert(channel)]
                if channel.channel_id == Id::new(10)
                    && channel.name == "renamed thread"
                    && channel.kind == "GuildPublicThread"
        ));
    }

    #[test]
    fn raw_thread_delete_removes_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_DELETE",
                "d": {
                    "id": "10",
                    "guild_id": "1",
                    "parent_id": "2",
                    "type": 11
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelDelete { guild_id, channel_id }]
                if *guild_id == Some(Id::new(1)) && *channel_id == Id::new(10)
        ));
    }

    #[test]
    fn raw_thread_list_sync_upserts_all_threads() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_LIST_SYNC",
                "d": {
                    "guild_id": "1",
                    "channel_ids": ["2"],
                    "threads": [
                        thread_payload(10, "release notes"),
                        thread_payload(11, "bug reports")
                    ],
                    "members": []
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(10) && channel.name == "release notes"
        ));
        assert!(matches!(
            &events[1],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(11) && channel.name == "bug reports"
        ));
    }

    #[test]
    fn message_update_parser_without_attachments_does_not_clear_cached_attachments() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited"
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { attachments, .. } = event else {
            panic!("expected message update event");
        };
        assert!(matches!(attachments, AttachmentUpdate::Unchanged));
    }

    #[test]
    fn message_update_parser_empty_attachments_clears_cached_attachments() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited",
            "attachments": []
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { attachments, .. } = event else {
            panic!("expected message update event");
        };
        assert!(matches!(attachments, AttachmentUpdate::Replace(values) if values.is_empty()));
    }

    #[test]
    fn guild_create_parser_keeps_custom_emojis() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "member_count": 123,
            "channels": [],
            "members": [],
            "presences": [],
            "emojis": [
                {
                    "id": "50",
                    "name": "party",
                    "animated": true,
                    "available": true
                },
                {
                    "id": "51",
                    "name": "sleep",
                    "available": false
                }
            ]
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate {
            member_count,
            emojis,
            ..
        } = event
        else {
            panic!("expected guild create event");
        };
        assert_eq!(member_count, Some(123));
        assert_eq!(emojis.len(), 2);
        assert_eq!(emojis[0].id, Id::new(50));
        assert_eq!(emojis[0].name, "party");
        assert!(emojis[0].animated);
        assert!(emojis[0].available);
        assert!(!emojis[1].available);
    }

    #[test]
    fn guild_create_parser_keeps_roles() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "members": [],
            "presences": [],
            "roles": [{
                "id": "90",
                "name": "Admin",
                "color": 16755200,
                "position": 10,
                "hoist": true
            }],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { roles, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].id, Id::new(90));
        assert_eq!(roles[0].name, "Admin");
        assert_eq!(roles[0].color, Some(16755200));
        assert_eq!(roles[0].position, 10);
        assert!(roles[0].hoist);
    }

    #[test]
    fn guild_create_parser_keeps_string_permission_bitfields() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "members": [],
            "presences": [],
            "roles": [{
                "id": "1",
                "name": "@everyone",
                "permissions": "1024",
                "position": 0,
                "hoist": false
            }],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { roles, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(roles[0].permissions, 0x400);
    }

    #[test]
    fn guild_create_parser_accepts_member_user_id_without_nested_user() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "members": [{
                "user_id": "10",
                "roles": [20]
            }],
            "presences": [],
            "roles": [],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { members, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, Id::new(10));
        assert_eq!(members[0].role_ids, vec![Id::new(20)]);
    }

    #[test]
    fn raw_guild_create_with_thin_current_member_hides_denied_channel() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "owner_id": "11",
            "channels": [{
                "id": "2",
                "type": 0,
                "name": "secret",
                "permission_overwrites": [{
                    "id": "1",
                    "type": 0,
                    "allow": "0",
                    "deny": "1024"
                }]
            }],
            "members": [{
                "user_id": "10",
                "roles": []
            }],
            "presences": [],
            "roles": [{
                "id": "1",
                "name": "@everyone",
                "permissions": "1024",
                "position": 0,
                "hoist": false
            }],
            "emojis": []
        }))
        .expect("guild create should parse");
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        });
        state.apply_event(&event);

        assert_eq!(
            state.channel_visibility_stats(Some(Id::new(1))),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        assert!(
            state
                .viewable_channels_for_guild(Some(Id::new(1)))
                .is_empty()
        );
    }

    #[test]
    fn raw_guild_create_with_thin_current_member_keeps_role_based_access() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "owner_id": "11",
            "channels": [{
                "id": "2",
                "type": 0,
                "name": "staff",
                "permission_overwrites": [{
                    "id": "1",
                    "type": 0,
                    "allow": "0",
                    "deny": "1024"
                }, {
                    "id": "20",
                    "type": 0,
                    "allow": "1024",
                    "deny": "0"
                }]
            }],
            "members": [{
                "user_id": "10",
                "roles": [20]
            }],
            "presences": [],
            "roles": [{
                "id": "1",
                "name": "@everyone",
                "permissions": "1024",
                "position": 0,
                "hoist": false
            }, {
                "id": "20",
                "name": "Staff",
                "permissions": "0",
                "position": 1,
                "hoist": false
            }],
            "emojis": []
        }))
        .expect("guild create should parse");
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        });
        state.apply_event(&event);

        assert_eq!(
            state.channel_visibility_stats(Some(Id::new(1))),
            ChannelVisibilityStats {
                visible: 1,
                hidden: 0,
            }
        );
        assert_eq!(state.viewable_channels_for_guild(Some(Id::new(1))).len(), 1);
    }

    #[test]
    fn guild_create_parser_keeps_active_threads() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "threads": [thread_payload(10, "release notes")],
            "members": [],
            "presences": [],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { channels, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].channel_id, Id::new(10));
        assert_eq!(channels[0].kind, "GuildPublicThread");
        assert_eq!(channels[0].name, "release notes");
    }

    #[test]
    fn raw_member_chunk_upserts_members_and_presences() {
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBERS_CHUNK",
                "d": {
                    "guild_id": "1",
                    "chunk_index": 0,
                    "chunk_count": 1,
                    "members": [
                        {
                            "nick": "Alice Nick",
                            "user": {
                                "id": "10",
                                "username": "alice",
                                "global_name": "Alice Global",
                                "avatar": "avatarhash"
                            }
                        },
                        {
                            "user": {
                                "id": "20",
                                "username": "bob",
                                "bot": true
                            }
                        }
                    ],
                    "presences": [
                        { "user": { "id": "10" }, "status": "online" },
                        { "user": { "id": "20" }, "status": "idle" }
                    ]
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.display_name == "Alice Nick"
                    && !member.is_bot
        ));
        assert!(matches!(
            &events[1],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(20)
                    && member.display_name == "bob"
                    && member.is_bot
        ));
        assert!(matches!(
            &events[2],
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(10)
                    && *status == PresenceStatus::Online
        ));
        assert!(matches!(
            &events[3],
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Idle
        ));
    }

    #[test]
    fn raw_member_add_keeps_real_join_semantics() {
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBER_ADD",
                "d": {
                    "guild_id": "1",
                    "nick": "Alice Nick",
                    "user": {
                        "id": "10",
                        "username": "alice"
                    }
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            AppEvent::GuildMemberAdd { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.display_name == "Alice Nick"
        ));
    }

    #[test]
    fn raw_ready_parser_keeps_guild_custom_emojis() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [{
                        "id": "1",
                        "name": "guild",
                        "channels": [],
                        "members": [],
                        "presences": [],
                        "emojis": [{
                            "id": "50",
                            "name": "party_time",
                            "animated": true,
                            "available": true
                        }]
                    }],
                    "private_channels": []
                }
            })
            .to_string(),
        );

        let guild_create = events
            .iter()
            .find_map(|event| match event {
                AppEvent::GuildCreate { emojis, .. } => Some(emojis),
                _ => None,
            })
            .expect("ready should emit a guild create event");

        assert_eq!(guild_create.len(), 1);
        assert_eq!(guild_create[0].id, Id::new(50));
        assert_eq!(guild_create[0].name, "party_time");
        assert!(guild_create[0].animated);
        assert!(guild_create[0].available);
    }

    #[test]
    fn guild_emojis_update_parser_replaces_custom_emojis() {
        let event = parse_guild_emojis_update(&json!({
            "guild_id": "1",
            "emojis": [
                {
                    "id": "60",
                    "name": "wave",
                    "animated": false,
                    "available": true
                }
            ]
        }))
        .expect("guild emojis update should parse");

        let AppEvent::GuildEmojisUpdate { guild_id, emojis } = event else {
            panic!("expected guild emojis update event");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(60));
        assert_eq!(emojis[0].name, "wave");
        assert!(emojis[0].available);
    }

    #[test]
    fn guild_update_parser_keeps_custom_emojis_when_present() {
        let event = parse_guild_update(&json!({
            "id": "1",
            "name": "guild renamed",
            "emojis": [{
                "id": "70",
                "name": "dance",
                "animated": true,
                "available": true
            }]
        }))
        .expect("guild update should parse");

        let AppEvent::GuildUpdate {
            guild_id,
            name,
            roles,
            emojis,
            ..
        } = event
        else {
            panic!("expected guild update event");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(name, "guild renamed");
        assert_eq!(roles, None);
        let emojis = emojis.expect("emoji field should be preserved when present");
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(70));
        assert_eq!(emojis[0].name, "dance");
        assert!(emojis[0].animated);
    }

    #[test]
    fn guild_update_parser_distinguishes_missing_custom_emojis() {
        let event = parse_guild_update(&json!({
            "id": "1",
            "name": "guild renamed"
        }))
        .expect("guild update should parse");

        let AppEvent::GuildUpdate { roles, emojis, .. } = event else {
            panic!("expected guild update event");
        };
        assert_eq!(roles, None);
        assert_eq!(emojis, None);
    }

    #[test]
    fn message_update_parser_keeps_mentions_when_present() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited <@40>",
            "mentions": [{ "id": "40", "username": "alice" }]
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { mentions, .. } = event else {
            panic!("expected message update event");
        };
        assert_eq!(mentions, Some(vec![mention_info(40, "alice")]));
    }

    #[test]
    fn message_update_parser_keeps_poll_results() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "poll": {
                "question": { "text": "오늘 뭐 먹지?" },
                "answers": [
                    { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                    { "answer_id": 2, "poll_media": { "text": "라멘" } }
                ],
                "results": {
                    "is_finalized": true,
                    "answer_counts": [
                        { "id": 1, "count": 5, "me_voted": true },
                        { "id": 2, "count": 3, "me_voted": false }
                    ]
                }
            }
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { poll, .. } = event else {
            panic!("expected message update event");
        };
        let poll = poll.expect("poll payload should be kept");
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].vote_count, Some(5));
        assert!(poll.answers[0].me_voted);
    }

    #[test]
    fn message_create_parser_keeps_image_attachments() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "cat.png",
                "url": "https://cdn.discordapp.com/cat.png",
                "proxy_url": "https://media.discordapp.net/cat.png",
                "content_type": "image/png",
                "size": 2048,
                "width": 640,
                "height": 480,
                "description": "cat"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "cat.png");
        assert_eq!(attachments[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(attachments[0].width, Some(640));
        assert_eq!(attachments[0].height, Some(480));
    }

    #[test]
    fn message_create_parser_keeps_regular_embeds() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "embeds": [{
                "type": "video",
                "color": 16711680,
                "provider": { "name": "YouTube" },
                "title": "Example Video",
                "description": "A video description",
                "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "thumbnail": {
                    "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                    "width": 480,
                    "height": 360
                },
                "image": {
                    "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
                    "width": 1280,
                    "height": 720
                },
                "video": { "url": "https://www.youtube.com/embed/dQw4w9WgXcQ" }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { embeds, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].color, Some(16711680));
        assert_eq!(embeds[0].provider_name.as_deref(), Some("YouTube"));
        assert_eq!(embeds[0].title.as_deref(), Some("Example Video"));
        assert_eq!(
            embeds[0].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        assert_eq!(embeds[0].thumbnail_width, Some(480));
        assert_eq!(embeds[0].thumbnail_height, Some(360));
        assert_eq!(
            embeds[0].image_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg")
        );
        assert_eq!(embeds[0].image_width, Some(1280));
        assert_eq!(embeds[0].image_height, Some(720));
        assert_eq!(
            embeds[0].video_url.as_deref(),
            Some("https://www.youtube.com/embed/dQw4w9WgXcQ")
        );
    }

    #[test]
    fn message_create_parser_keeps_message_type() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { message_kind, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message_kind, MessageKind::new(19));
    }

    #[test]
    fn message_create_parser_prefers_member_nick_for_author() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": "1",
            "author": { "id": "30", "global_name": "global", "username": "neo" },
            "member": { "nick": "server alias" },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { author, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(author, "server alias");
    }

    #[test]
    fn message_info_parser_keeps_author_role_ids_from_member_payload() {
        let message = parse_message_info(&json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": "1",
            "author": { "id": "30", "username": "neo" },
            "member": { "roles": ["90", "91"] },
            "content": "hello",
            "attachments": []
        }))
        .expect("message should parse");

        assert_eq!(message.author_role_ids, vec![Id::new(90), Id::new(91)]);
    }

    #[test]
    fn message_create_parser_builds_author_avatar_url() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": {
                "id": "30",
                "username": "neo",
                "avatar": "a_avatarhash"
            },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            author_avatar_url, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(
            author_avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/avatars/30/a_avatarhash.gif")
        );
    }

    #[test]
    fn message_create_parser_falls_back_to_global_name_without_member() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "global_name": "global alias", "username": "neo" },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            author, guild_id, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(guild_id, None);
        assert_eq!(author, "global alias");
    }

    #[test]
    fn message_create_parser_keeps_mention_display_names() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "hello <@40> <@41> <@42>",
            "mentions": [
                {
                    "id": "40",
                    "username": "alpha",
                    "global_name": "Alpha Global",
                    "member": { "nick": "Alpha Nick" }
                },
                {
                    "id": "41",
                    "username": "beta",
                    "global_name": "Beta Global"
                },
                {
                    "id": "42",
                    "username": "gamma"
                }
            ],
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { mentions, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            mentions,
            vec![
                mention_info_with_nick(40, "Alpha Nick"),
                mention_info(41, "Beta Global"),
                mention_info(42, "gamma"),
            ]
        );
    }

    #[test]
    fn message_create_parser_keeps_reply_preview() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": [],
            "referenced_message": {
                "id": "19",
                "channel_id": "10",
                "author": { "id": "31", "global_name": "Alex", "username": "alex" },
                "content": "잘되는군",
                "attachments": []
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply,
            Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                mentions: Vec::new(),
            })
        );
    }

    #[test]
    fn message_create_parser_keeps_reply_mentions() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": [],
            "referenced_message": {
                "id": "19",
                "channel_id": "10",
                "author": { "id": "31", "username": "alex" },
                "content": "hello <@40>",
                "mentions": [{ "id": "40", "username": "alice" }],
                "attachments": []
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply.and_then(|reply| reply.mentions.into_iter().next()),
            Some(mention_info(40, "alice"))
        );
    }

    #[test]
    fn message_create_parser_keeps_poll_payload() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 0,
            "content": "",
            "attachments": [],
            "poll": {
                "question": { "text": "오늘 뭐 먹지?" },
                "answers": [
                    { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                    { "answer_id": 2, "poll_media": { "text": "라멘" } }
                ],
                "results": {
                    "is_finalized": false,
                    "answer_counts": [
                        { "id": 1, "count": 2, "me_voted": true },
                        { "id": 2, "count": 1, "me_voted": false }
                    ]
                },
                "allow_multiselect": true
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll,
            Some(PollInfo {
                question: "오늘 뭐 먹지?".to_owned(),
                answers: vec![
                    PollAnswerInfo {
                        answer_id: 1,
                        text: "김치찌개".to_owned(),
                        vote_count: Some(2),
                        me_voted: true,
                    },
                    PollAnswerInfo {
                        answer_id: 2,
                        text: "라멘".to_owned(),
                        vote_count: Some(1),
                        me_voted: false,
                    },
                ],
                allow_multiselect: true,
                results_finalized: Some(false),
                total_votes: Some(3),
            })
        );
    }

    #[test]
    fn message_create_parser_keeps_poll_result_embed() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 46,
            "content": "",
            "attachments": [],
            "embeds": [{
                "type": "poll_result",
                "fields": [
                    { "name": "poll_question_text", "value": "오늘 뭐 먹지?" },
                    { "name": "victor_answer_id", "value": "1" },
                    { "name": "victor_answer_text", "value": "김치찌개" },
                    { "name": "victor_answer_votes", "value": "5" },
                    { "name": "total_votes", "value": "7" }
                ]
            }]
        }))
        .expect("poll result message should parse");

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll.expect("poll result should map to poll info")
                .total_votes,
            Some(7)
        );
    }

    #[test]
    fn message_create_parser_uses_proxy_url_when_url_is_missing() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "cat.png",
                "proxy_url": "https://media.discordapp.net/cat.png",
                "content_type": "image/png"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].url, "https://media.discordapp.net/cat.png");
        assert_eq!(
            attachments[0].proxy_url,
            "https://media.discordapp.net/cat.png"
        );
    }

    #[test]
    fn message_create_parser_keeps_video_attachment_metadata() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "clip.mp4",
                "url": "https://cdn.discordapp.com/clip.mp4",
                "proxy_url": "https://media.discordapp.net/clip.mp4",
                "content_type": "video/mp4",
                "size": 78364758,
                "width": 1920,
                "height": 1080,
                "description": "clip"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "clip.mp4");
        assert_eq!(attachments[0].content_type.as_deref(), Some("video/mp4"));
        assert_eq!(attachments[0].size, 78_364_758);
        assert_eq!(attachments[0].width, Some(1920));
        assert_eq!(attachments[0].height, Some(1080));
    }

    #[test]
    fn message_create_parser_keeps_forwarded_snapshot_content() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_reference": { "channel_id": "11" },
            "message_snapshots": [{
                "message": {
                    "content": "forwarded text",
                    "timestamp": "2026-04-30T12:34:56.000000+00:00",
                    "attachments": []
                }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 1);
        assert_eq!(
            forwarded_snapshots[0].content.as_deref(),
            Some("forwarded text")
        );
        assert_eq!(forwarded_snapshots[0].source_channel_id, Some(Id::new(11)));
        assert_eq!(
            forwarded_snapshots[0].timestamp.as_deref(),
            Some("2026-04-30T12:34:56.000000+00:00")
        );
    }

    #[test]
    fn message_create_parser_keeps_forwarded_snapshot_mentions() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_snapshots": [{
                "message": {
                    "content": "hello <@40>",
                    "mentions": [{ "id": "40", "username": "alice" }],
                    "attachments": []
                }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 1);
        assert_eq!(
            forwarded_snapshots[0].mentions,
            vec![mention_info(40, "alice")]
        );
    }

    #[test]
    fn message_create_parser_keeps_forwarded_snapshot_attachments() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_snapshots": [{
                "message": {
                    "content": "",
                    "attachments": [{
                        "id": "40",
                        "filename": "cat.png",
                        "url": "https://cdn.discordapp.com/cat.png",
                        "proxy_url": "https://media.discordapp.net/cat.png",
                        "content_type": "image/png",
                        "size": 2048,
                        "width": 640,
                        "height": 480
                    }]
                }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 1);
        assert_eq!(forwarded_snapshots[0].attachments.len(), 1);
        assert_eq!(forwarded_snapshots[0].attachments[0].filename, "cat.png");
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: None,
            display_name: display_name.to_owned(),
        }
    }

    fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: Some(nick.to_owned()),
            display_name: nick.to_owned(),
        }
    }

    fn thread_payload(id: u64, name: &str) -> serde_json::Value {
        json!({
            "id": id.to_string(),
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": name,
            "message_count": 12,
            "total_message_sent": 14,
            "thread_metadata": { "archived": false, "locked": false }
        })
    }

    #[test]
    fn parse_guild_create_reads_name_from_lazy_properties_object() {
        // With user-account capabilities containing LAZY_USER_NOTIFICATIONS,
        // Discord nests guild metadata under `properties` instead of placing
        // `name` / `owner_id` at the root. Concord must look in both places
        // or every guild renders as "unknown".
        let event = parse_guild_create(&json!({
            "id": "100",
            "member_count": 7,
            "channels": [],
            "roles": [],
            "emojis": [],
            "properties": {
                "name": "Lazy Server",
                "owner_id": "42",
            },
        }))
        .expect("guild_create payload should map");

        let AppEvent::GuildCreate {
            guild_id,
            name,
            owner_id,
            member_count,
            ..
        } = event
        else {
            panic!("expected GuildCreate event");
        };
        assert_eq!(guild_id, Id::new(100));
        assert_eq!(name, "Lazy Server");
        assert_eq!(owner_id, Some(Id::new(42)));
        assert_eq!(member_count, Some(7));
    }

    #[test]
    fn parse_guild_create_prefers_root_name_when_both_locations_set() {
        // Guard against future Discord shape drift: if both root-level and
        // nested name are present, the root wins (matches what the official
        // client does).
        let event = parse_guild_create(&json!({
            "id": "100",
            "name": "Root Name",
            "properties": {"name": "Properties Name"},
        }))
        .expect("guild_create payload should map");

        let AppEvent::GuildCreate { name, .. } = event else {
            panic!("expected GuildCreate event");
        };
        assert_eq!(name, "Root Name");
    }

    #[test]
    fn typing_start_extracts_channel_and_user_from_dm_payload() {
        // DM TYPING_START omits guild_id and embeds user_id directly.
        let events = parse_user_account_event(
            &json!({
                "t": "TYPING_START",
                "d": {
                    "channel_id": "12345",
                    "user_id": "99",
                    "timestamp": 1_700_000_000
                }
            })
            .to_string(),
        );
        assert!(matches!(
            events.as_slice(),
            [AppEvent::TypingStart { channel_id, user_id }]
                if *channel_id == Id::new(12345) && *user_id == Id::new(99)
        ));
    }

    #[test]
    fn typing_start_falls_back_to_member_user_id_when_top_level_missing() {
        // Some guild TYPING_START payloads only embed the user id under
        // `member.user.id`. Make sure we still surface the typer.
        let events = parse_user_account_event(
            &json!({
                "t": "TYPING_START",
                "d": {
                    "channel_id": "55",
                    "guild_id": "77",
                    "member": {
                        "user": { "id": "42" }
                    },
                    "timestamp": 1_700_000_000
                }
            })
            .to_string(),
        );
        assert!(matches!(
            events.as_slice(),
            [AppEvent::TypingStart { channel_id, user_id }]
                if *channel_id == Id::new(55) && *user_id == Id::new(42)
        ));
    }

    #[test]
    fn ready_hydrates_dm_recipients_from_dedupe_user_ids() {
        // With DEDUPE_USER_OBJECTS in capabilities, READY puts users at the
        // top level once and each private channel only carries
        // `recipient_ids`. The dashboard must still show the peer's name
        // and not `dm-{channel_id}`.
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": { "id": "10", "username": "me" },
                    "users": [
                        {
                            "id": "20",
                            "username": "asdf",
                            "global_name": "global",
                            "discriminator": "0",
                        }
                    ],
                    "private_channels": [
                        {
                            "id": "12345",
                            "type": 1,
                            "recipient_ids": ["20"]
                        }
                    ]
                }
            })
            .to_string(),
        );

        let dm = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(info) if info.kind == "dm" => Some(info),
                _ => None,
            })
            .expect("dm channel upsert should be emitted");
        assert_eq!(dm.name, "global");
        let recipients = dm.recipients.as_ref().expect("recipients hydrated");
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].display_name, "global");
        assert_eq!(recipients[0].username.as_deref(), Some("asdf"));
    }

    #[test]
    fn parse_guild_update_reads_name_from_lazy_properties_object() {
        let event = parse_guild_update(&json!({
            "id": "100",
            "properties": {
                "name": "Renamed Lazy",
                "owner_id": "9",
            },
        }))
        .expect("guild_update payload should map");

        let AppEvent::GuildUpdate {
            guild_id,
            name,
            owner_id,
            ..
        } = event
        else {
            panic!("expected GuildUpdate event");
        };
        assert_eq!(guild_id, Id::new(100));
        assert_eq!(name, "Renamed Lazy");
        assert_eq!(owner_id, Some(Id::new(9)));
    }
}
