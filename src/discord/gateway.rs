use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Duration,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use flate2::{Decompress, FlushDecompress, Status};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use reqwest::Url;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::{Instant, sleep, timeout};
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
    ActivityInfo, PresenceStatus, VoiceScope,
    client::AppEventPublisher,
    events::AppEvent,
    fingerprint::{
        CLIENT_BROWSER, CLIENT_BROWSER_VERSION, ClientFingerprint, DISCORD_REFERRER_CURRENT,
        DISCORD_REFERRING_DOMAIN_CURRENT, discord_gateway_headers,
    },
    state::{ClientCacheState, DiscordState},
};
use crate::logging;

mod parser;

pub(in crate::discord) use parser::parse_activity;
use parser::parse_user_account_dispatch;
pub(crate) use parser::{
    parse_channel_info, parse_member_info, parse_message_info, parse_thread_member_info,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayCommand {
    SearchGuildMembers {
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
        presences: bool,
        nonce: String,
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
        thread_id: Option<Id<ChannelMarker>>,
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
    WatchStream {
        stream_key: String,
    },
    CreateStream {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    DeleteStream {
        stream_key: String,
    },
    UpdatePresence {
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    Shutdown {
        voice_leave: Option<GatewayVoiceStateUpdate>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GatewayVoiceStateUpdate {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub self_mute: bool,
    pub self_deaf: bool,
}

#[derive(Clone)]
pub(crate) struct GatewayRuntime {
    pub(crate) fingerprint: Arc<ClientFingerprint>,
    pub(crate) state: Arc<RwLock<DiscordState>>,
    pub(crate) gateway_session_id: Arc<RwLock<Option<String>>>,
    pub(crate) event_publisher: AppEventPublisher,
}

/// Discord user-account gateway endpoint. We pin to `v=9` because the v9
/// dispatch shapes line up with everything `parse_user_account_event` already
/// understands. Discord's browser client uses the stateful `zlib-stream`
/// transport mode, which keeps large READY payloads bounded on the wire.
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json&compress=zlib-stream";

/// Bitmask Discord checks before delivering user-account-only payloads such as
/// `READY_SUPPLEMENTAL.merged_presences.friends` and per-friend
/// `PRESENCE_UPDATE` dispatches. Without these bits set Discord assumes the
/// session is a bot and silently drops friend presence streaming.
///
/// Bits enabled (sum 253):
///   0  LAZY_USER_NOTES
///   2  VERSIONED_READ_STATES
///   3  VERSIONED_USER_GUILD_SETTINGS
///   4  DEDUPE_USER_OBJECTS
///   5  PRIORITIZED_READY_PAYLOAD
///   6  MULTIPLE_GUILD_EXPERIMENT_POPULATIONS
///   7  NON_CHANNEL_READ_STATES
const USER_ACCOUNT_CAPABILITIES: u64 = 253;

// Some user-account READY payloads exceed tungstenite's default 16 MiB frame
// cap. Keep both compressed input and decompressed output bounded while still
// allowing large accounts to finish their initial sync.
const GATEWAY_WEBSOCKET_LIMIT: usize = 64 << 20;
const ZLIB_STREAM_SUFFIX: [u8; 4] = [0x00, 0x00, 0xff, 0xff];
const ZLIB_OUTPUT_CHUNK_SIZE: usize = 8 << 10;

const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(500);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
// Discord applies this budget to every JSON event on one Gateway connection.
// WebSocket control frames such as Pong and Close are not Gateway events.
const GATEWAY_SEND_LIMIT: usize = 120;
const GATEWAY_SEND_WINDOW: Duration = Duration::from_secs(60);
const GATEWAY_SHUTDOWN_LEAVE_TIMEOUT: Duration = Duration::from_millis(1_500);
const GUILD_MEMBER_REQUEST_RESPONSE_TTL: Duration = Duration::from_secs(2 * 60);
const MAX_PENDING_GUILD_MEMBER_REQUESTS: usize = 512;
const MAX_SENT_GUILD_MEMBER_REQUESTS: usize = 512;
const MAX_GATEWAY_RETRY_DELAY: Duration = Duration::from_secs(30 * 60);

type GatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Discord uses one zlib stream for the lifetime of a Gateway connection.
/// Individual JSON payloads end with a sync-flush marker and may span several
/// WebSocket binary messages, so neither the input buffer nor the inflater can
/// be recreated for each frame.
struct GatewayZlibDecoder {
    inflater: Decompress,
    pending: Vec<u8>,
}

impl Default for GatewayZlibDecoder {
    fn default() -> Self {
        Self {
            inflater: Decompress::new(true),
            pending: Vec::new(),
        }
    }
}

impl GatewayZlibDecoder {
    fn decode(&mut self, chunk: &[u8]) -> Result<Option<String>, String> {
        let pending_len = self
            .pending
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| "compressed gateway payload size overflow".to_owned())?;
        if pending_len > GATEWAY_WEBSOCKET_LIMIT {
            return Err(format!(
                "compressed gateway payload exceeds {GATEWAY_WEBSOCKET_LIMIT} bytes"
            ));
        }
        self.pending.extend_from_slice(chunk);
        if !self.pending.ends_with(&ZLIB_STREAM_SUFFIX) {
            return Ok(None);
        }

        let compressed = std::mem::take(&mut self.pending);
        let mut input_offset = 0;
        let mut output = Vec::new();
        loop {
            let mut buffer = [0; ZLIB_OUTPUT_CHUNK_SIZE];
            let input_before = self.inflater.total_in();
            let output_before = self.inflater.total_out();
            let status = self
                .inflater
                .decompress(
                    &compressed[input_offset..],
                    &mut buffer,
                    FlushDecompress::Sync,
                )
                .map_err(|error| format!("gateway zlib decode failed: {error}"))?;
            let consumed = usize::try_from(self.inflater.total_in() - input_before)
                .map_err(|_| "gateway zlib input count exceeds platform size".to_owned())?;
            let produced = usize::try_from(self.inflater.total_out() - output_before)
                .map_err(|_| "gateway zlib output count exceeds platform size".to_owned())?;
            input_offset += consumed;
            output.extend_from_slice(&buffer[..produced]);

            if output.len() > GATEWAY_WEBSOCKET_LIMIT {
                return Err(format!(
                    "decompressed gateway payload exceeds {GATEWAY_WEBSOCKET_LIMIT} bytes"
                ));
            }
            if matches!(status, Status::StreamEnd) {
                if input_offset != compressed.len() {
                    return Err(
                        "gateway zlib stream ended before the input was consumed".to_owned()
                    );
                }
                self.inflater = Decompress::new(true);
                break;
            }
            if input_offset == compressed.len() && produced < buffer.len() {
                break;
            }
            if consumed == 0 && produced == 0 {
                if input_offset == compressed.len() {
                    break;
                }
                return Err("gateway zlib decoder made no progress".to_owned());
            }
        }

        String::from_utf8(output)
            .map(Some)
            .map_err(|error| format!("gateway payload is not valid UTF-8: {error}"))
    }
}

/// Shared, lockable WebSocket sink. Both the heartbeat task and the main
/// dispatch loop need to send over the same connection, so the sink lives
/// behind a `Mutex<Arc<…>>` instead of being moved into either side.
type WriterHandle = Arc<Mutex<futures::stream::SplitSink<GatewayStream, WsMessage>>>;

#[derive(Clone)]
struct GatewaySender {
    // Heartbeats, session setup, and voice state use the urgent queue so a
    // backlog of UI commands cannot delay time-sensitive traffic.
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GuildMemberRequestKind {
    Search {
        query: String,
        limit: u16,
        presences: bool,
    },
    ByIds {
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GuildMemberRequest {
    guild_id: Id<GuildMarker>,
    nonce: String,
    kind: GuildMemberRequestKind,
}

#[derive(Clone, Debug)]
struct PendingGuildMemberRequest {
    request: GuildMemberRequest,
    send_at: Instant,
}

struct InFlightGuildMemberRequest {
    completion: oneshot::Receiver<Result<(), String>>,
}

struct ScheduledGuildMemberRequest {
    request: GuildMemberRequest,
    accepted: bool,
    retry_at: Option<Instant>,
}

struct SentGuildMemberRequest {
    request: GuildMemberRequest,
    sent_at: Instant,
}

struct GuildMemberRequestScheduler {
    pending: VecDeque<PendingGuildMemberRequest>,
    in_flight: Option<ScheduledGuildMemberRequest>,
    awaiting_response: VecDeque<SentGuildMemberRequest>,
    guild_rate_limit_until: HashMap<Id<GuildMarker>, Instant>,
    next_nonce: u64,
}

#[derive(Default)]
struct GatewaySessionResources {
    guild_member_requests: GuildMemberRequestScheduler,
    last_presence: Option<GatewayPresence>,
}

struct GatewayPresence {
    status: PresenceStatus,
    activities: Vec<ActivityInfo>,
}

impl Default for GuildMemberRequestScheduler {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            in_flight: None,
            awaiting_response: VecDeque::new(),
            guild_rate_limit_until: HashMap::new(),
            next_nonce: 1,
        }
    }
}

impl GuildMemberRequest {
    fn payload(&self) -> String {
        match &self.kind {
            GuildMemberRequestKind::Search {
                query,
                limit,
                presences,
            } => {
                search_guild_members_payload(self.guild_id, query, *limit, *presences, &self.nonce)
            }
            GuildMemberRequestKind::ByIds {
                user_ids,
                presences,
            } => request_guild_members_by_ids_payload(
                self.guild_id,
                user_ids,
                *presences,
                &self.nonce,
            ),
        }
    }

    fn is_search(&self) -> bool {
        matches!(&self.kind, GuildMemberRequestKind::Search { .. })
    }
}

impl GuildMemberRequestScheduler {
    fn enqueue_search(
        &mut self,
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
        presences: bool,
        nonce: String,
        now: Instant,
    ) -> bool {
        self.prune_expired(now);
        let request = GuildMemberRequest {
            guild_id,
            nonce,
            kind: GuildMemberRequestKind::Search {
                query,
                limit,
                presences,
            },
        };
        self.awaiting_response
            .retain(|sent| sent.request.nonce != request.nonce);

        if let Some(pending) = self
            .pending
            .iter_mut()
            .rev()
            .find(|pending| pending.request.guild_id == guild_id && pending.request.is_search())
        {
            pending.request = request;
            return true;
        }

        if self.pending.len() >= MAX_PENDING_GUILD_MEMBER_REQUESTS {
            let Some(position) = self
                .pending
                .iter()
                .position(|pending| pending.request.is_search())
            else {
                return false;
            };
            self.pending.remove(position);
        }

        self.enqueue_request(request, now)
    }

    fn enqueue_by_ids(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
        presences: bool,
        now: Instant,
    ) {
        self.prune_expired(now);
        let mut seen = BTreeSet::new();
        let mut remaining = user_ids
            .into_iter()
            .filter(|user_id| seen.insert(*user_id))
            .collect::<Vec<_>>();

        let compatible_requests = self.pending.iter().filter(|pending| {
            pending.request.guild_id == guild_id
                && matches!(
                    pending.request.kind,
                    GuildMemberRequestKind::ByIds {
                        presences: pending_presences,
                        ..
                    } if pending_presences == presences
                )
        });
        let mut available = 0usize;
        for pending in compatible_requests {
            let GuildMemberRequestKind::ByIds { user_ids, .. } = &pending.request.kind else {
                continue;
            };
            remaining.retain(|user_id| !user_ids.contains(user_id));
            available += 100usize.saturating_sub(user_ids.len());
        }
        let new_request_count = remaining.len().saturating_sub(available).div_ceil(100);
        self.make_room_for_by_ids(new_request_count);

        for pending in self.pending.iter_mut().filter(|pending| {
            pending.request.guild_id == guild_id
                && matches!(
                    pending.request.kind,
                    GuildMemberRequestKind::ByIds {
                        presences: pending_presences,
                        ..
                    } if pending_presences == presences
                )
        }) {
            let GuildMemberRequestKind::ByIds { user_ids, .. } = &mut pending.request.kind else {
                continue;
            };
            remaining.retain(|user_id| !user_ids.contains(user_id));
            let available = 100usize.saturating_sub(user_ids.len());
            let added = remaining.len().min(available);
            user_ids.extend(remaining.drain(..added));
            if remaining.is_empty() {
                return;
            }
        }

        for user_ids in remaining.chunks(100) {
            let request = GuildMemberRequest {
                guild_id,
                nonce: self.next_nonce(),
                kind: GuildMemberRequestKind::ByIds {
                    user_ids: user_ids.to_vec(),
                    presences,
                },
            };
            self.enqueue_by_ids_request(request, now);
        }
    }

    fn make_room_for_by_ids(&mut self, additional: usize) {
        let overflow = self
            .pending
            .len()
            .saturating_add(additional)
            .saturating_sub(MAX_PENDING_GUILD_MEMBER_REQUESTS);
        if overflow == 0 {
            return;
        }
        let positions = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| pending.request.is_search().then_some(index))
            .take(overflow)
            .collect::<Vec<_>>();
        for position in positions.into_iter().rev() {
            self.pending.remove(position);
        }
    }

    fn enqueue_request(&mut self, request: GuildMemberRequest, now: Instant) -> bool {
        if self.pending.len() >= MAX_PENDING_GUILD_MEMBER_REQUESTS {
            return false;
        }
        let send_at = self.guild_request_at(request.guild_id, now);
        self.pending
            .push_back(PendingGuildMemberRequest { request, send_at });
        true
    }

    fn enqueue_by_ids_request(&mut self, request: GuildMemberRequest, now: Instant) {
        // Hydration requests resolve concrete users already visible in the UI,
        // so losing them is worse than briefly exceeding the search queue's
        // soft memory bound. IDs are deduplicated and grouped into 100-member
        // payloads here. The shared Gateway writer enforces the connection-wide
        // send budget, while RATE_LIMITED dispatches provide the only guild
        // delay that targeted requests need.
        let send_at = self.guild_request_at(request.guild_id, now);
        self.pending
            .push_back(PendingGuildMemberRequest { request, send_at });
    }

    fn guild_request_at(&self, guild_id: Id<GuildMarker>, requested_at: Instant) -> Instant {
        self.guild_rate_limit_until
            .get(&guild_id)
            .copied()
            .unwrap_or(requested_at)
            .max(requested_at)
    }

    fn next_nonce(&mut self) -> String {
        let nonce = format!("concord-{:016x}", self.next_nonce);
        self.next_nonce = self.next_nonce.wrapping_add(1).max(1);
        nonce
    }

    fn next_delay(&self, now: Instant) -> Option<Duration> {
        self.pending
            .iter()
            .map(|request| request.send_at.saturating_duration_since(now))
            .min()
    }

    fn start_due(&mut self, now: Instant) -> Option<String> {
        if self.in_flight.is_some() {
            return None;
        }
        let index = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, request)| request.send_at <= now)
            .min_by_key(|(_, request)| request.send_at)
            .map(|(index, _)| index)?;
        let pending = self
            .pending
            .remove(index)
            .expect("due guild member request exists");
        let payload = pending.request.payload();
        self.in_flight = Some(ScheduledGuildMemberRequest {
            request: pending.request,
            accepted: false,
            retry_at: None,
        });
        Some(payload)
    }

    fn complete_send(&mut self, sent_at: Instant) {
        let completed = self
            .in_flight
            .take()
            .expect("sent guild member request exists");
        if let Some(retry_at) = completed.retry_at {
            self.pending.push_front(PendingGuildMemberRequest {
                request: completed.request,
                send_at: retry_at,
            });
            return;
        }

        if completed.accepted {
            return;
        }

        self.prune_expired(sent_at);
        if self.awaiting_response.len() >= MAX_SENT_GUILD_MEMBER_REQUESTS {
            self.awaiting_response.pop_front();
        }
        self.awaiting_response.push_back(SentGuildMemberRequest {
            request: completed.request,
            sent_at,
        });
    }

    fn cancel_in_flight(&mut self, now: Instant) {
        let Some(in_flight) = self.in_flight.take() else {
            return;
        };
        if in_flight.accepted {
            return;
        }
        let send_at = in_flight.retry_at.unwrap_or(now);
        self.pending.push_front(PendingGuildMemberRequest {
            request: in_flight.request,
            send_at,
        });
    }

    fn apply_rate_limit(
        &mut self,
        guild_id: Id<GuildMarker>,
        nonce: Option<&str>,
        retry_after: Duration,
        now: Instant,
    ) {
        self.prune_expired(now);
        let retry_at = now + retry_after;
        let has_newer_search = self
            .pending
            .iter()
            .any(|pending| pending.request.guild_id == guild_id && pending.request.is_search());

        let in_flight_matches = self.in_flight.as_ref().is_some_and(|in_flight| {
            in_flight.request.guild_id == guild_id
                && nonce.is_none_or(|nonce| in_flight.request.nonce == nonce)
        });
        if in_flight_matches {
            let in_flight = self
                .in_flight
                .as_mut()
                .expect("matching guild member request is in flight");
            if in_flight.request.is_search() && has_newer_search {
                in_flight.accepted = true;
                in_flight.retry_at = None;
            } else {
                in_flight.retry_at = Some(retry_at);
                in_flight.accepted = false;
            }
            self.apply_guild_rate_limit_until(guild_id, retry_at);
            return;
        }

        let position = match nonce {
            Some(nonce) => self
                .awaiting_response
                .iter()
                .rposition(|sent| sent.request.guild_id == guild_id && sent.request.nonce == nonce),
            None => self
                .awaiting_response
                .iter()
                .rposition(|sent| sent.request.guild_id == guild_id),
        };
        if let Some(position) = position {
            let sent = self
                .awaiting_response
                .remove(position)
                .expect("rate-limited guild member request exists");
            if !sent.request.is_search() || !has_newer_search {
                self.pending.push_front(PendingGuildMemberRequest {
                    request: sent.request,
                    send_at: retry_at,
                });
            }
        }
        self.apply_guild_rate_limit_until(guild_id, retry_at);
    }

    fn acknowledge(&mut self, nonce: &str) {
        if let Some(in_flight) = self.in_flight.as_mut()
            && in_flight.request.nonce == nonce
        {
            in_flight.accepted = true;
            return;
        }
        if let Some(position) = self
            .awaiting_response
            .iter()
            .rposition(|sent| sent.request.nonce == nonce)
        {
            self.awaiting_response.remove(position);
            return;
        }
        if let Some(position) = self
            .pending
            .iter()
            .rposition(|pending| pending.request.nonce == nonce)
        {
            self.pending.remove(position);
        }
    }

    fn apply_guild_rate_limit_until(&mut self, guild_id: Id<GuildMarker>, retry_at: Instant) {
        let earliest = *self
            .guild_rate_limit_until
            .entry(guild_id)
            .and_modify(|current| *current = (*current).max(retry_at))
            .or_insert(retry_at);
        self.delay_pending_guild_until(guild_id, earliest);
    }

    fn delay_pending_guild_until(&mut self, guild_id: Id<GuildMarker>, earliest: Instant) {
        for pending in self
            .pending
            .iter_mut()
            .filter(|pending| pending.request.guild_id == guild_id)
        {
            pending.send_at = pending.send_at.max(earliest);
        }
    }

    fn prune_expired(&mut self, now: Instant) {
        while self.awaiting_response.front().is_some_and(|sent| {
            now.saturating_duration_since(sent.sent_at) >= GUILD_MEMBER_REQUEST_RESPONSE_TTL
        }) {
            self.awaiting_response.pop_front();
        }
        self.guild_rate_limit_until
            .retain(|_, retry_at| *retry_at > now);
    }

    fn prepare_reconnect(&mut self, now: Instant) {
        self.prune_expired(now);
        self.cancel_in_flight(now);
        self.recover_awaiting(now);
    }

    fn recover_awaiting(&mut self, earliest: Instant) {
        let mut recovered = VecDeque::new();
        let mut recovered_guilds = HashSet::new();
        while let Some(sent) = self.awaiting_response.pop_back() {
            let superseded_search = sent.request.is_search()
                && self.pending.iter().chain(recovered.iter()).any(|pending| {
                    pending.request.guild_id == sent.request.guild_id && pending.request.is_search()
                });
            if !superseded_search {
                recovered_guilds.insert(sent.request.guild_id);
                recovered.push_front(PendingGuildMemberRequest {
                    request: sent.request,
                    send_at: earliest,
                });
            }
        }
        recovered.append(&mut self.pending);
        self.pending = recovered;
        for guild_id in recovered_guilds {
            self.delay_pending_guild_until(guild_id, earliest);
        }
    }

    fn start_new_session(&mut self, now: Instant) {
        self.prune_expired(now);
        self.cancel_in_flight(now);
        self.recover_awaiting(now);
        for pending in &mut self.pending {
            let earliest = self
                .guild_rate_limit_until
                .get(&pending.request.guild_id)
                .copied()
                .unwrap_or(now)
                .max(now);
            pending.send_at = pending.send_at.max(earliest);
        }
    }
}

impl InFlightGuildMemberRequest {
    async fn wait(&mut self) -> Result<(), String> {
        (&mut self.completion)
            .await
            .map_err(|_| "gateway writer task stopped before send completed".to_owned())?
    }
}

impl SubscriptionDeduper {
    fn should_send(&mut self, command: &GatewayCommand) -> bool {
        match command {
            GatewayCommand::SubscribeDirectMessage { channel_id } => {
                self.direct_messages.insert(*channel_id)
            }
            GatewayCommand::SubscribeGuildChannel {
                guild_id: _,
                channel_id: _,
            }
            | GatewayCommand::UpdateMemberListSubscription { .. } => true,
            _ => true,
        }
    }
}

#[derive(Clone, Copy)]
struct GatewayPublishContext<'a> {
    state: &'a Arc<RwLock<DiscordState>>,
    gateway_session_id: &'a Arc<RwLock<Option<String>>>,
    event_publisher: &'a AppEventPublisher,
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum GatewayHandshake {
    Identify,
    Resume { session_id: String, sequence: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GatewayConnectionPlan {
    url: String,
    handshake: GatewayHandshake,
    recovery_warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MalformedFrameRecovery {
    #[default]
    None,
    ResumeAttempted {
        after_sequence: Option<u64>,
    },
}

/// Mutable Gateway bookkeeping that survives reconnects. The session cursor
/// supports op-6 RESUME, while the recovery marker prevents a malformed replay
/// from reconnecting forever at the same confirmed sequence.
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
    malformed_frame_recovery: MalformedFrameRecovery,
}

impl SessionState {
    fn clear(&mut self) {
        self.session_id = None;
        self.resume_url = None;
        self.last_sequence = None;
        self.malformed_frame_recovery = MalformedFrameRecovery::None;
    }

    fn can_resume(&self) -> bool {
        self.session_id.is_some() && self.resume_url.is_some() && self.last_sequence.is_some()
    }

    fn next_connection(&mut self) -> GatewayConnectionPlan {
        if !self.can_resume() {
            let had_partial_resume_state = self.session_id.is_some()
                || self.resume_url.is_some()
                || self.last_sequence.is_some();
            self.clear();
            return GatewayConnectionPlan {
                url: GATEWAY_URL.to_owned(),
                handshake: GatewayHandshake::Identify,
                recovery_warning: had_partial_resume_state.then(|| {
                    "Gateway resume state is incomplete; starting a new session".to_owned()
                }),
            };
        }

        let resume_url = self
            .resume_url
            .as_deref()
            .expect("resume eligibility requires a resume URL");
        match normalized_resume_url(resume_url) {
            Ok(url) => GatewayConnectionPlan {
                url,
                handshake: GatewayHandshake::Resume {
                    session_id: self
                        .session_id
                        .clone()
                        .expect("resume eligibility requires a session id"),
                    sequence: self
                        .last_sequence
                        .expect("resume eligibility requires a sequence"),
                },
                recovery_warning: None,
            },
            Err(error) => {
                self.clear();
                GatewayConnectionPlan {
                    url: GATEWAY_URL.to_owned(),
                    handshake: GatewayHandshake::Identify,
                    recovery_warning: Some(error),
                }
            }
        }
    }

    fn malformed_frame_outcome(&mut self) -> FrameOutcome {
        if !self.can_resume() {
            return FrameOutcome::Reidentify;
        }
        let after_sequence = self.last_sequence;
        if self.malformed_frame_recovery
            == (MalformedFrameRecovery::ResumeAttempted { after_sequence })
        {
            return FrameOutcome::Reidentify;
        }
        self.malformed_frame_recovery = MalformedFrameRecovery::ResumeAttempted { after_sequence };
        FrameOutcome::Resume
    }

    fn record_sequence(&mut self, sequence: u64) {
        self.last_sequence = Some(sequence);
        self.malformed_frame_recovery = MalformedFrameRecovery::None;
    }

    fn abandon_failed_resume(&mut self, handshake: &GatewayHandshake) -> bool {
        if !matches!(handshake, GatewayHandshake::Resume { .. }) {
            return false;
        }
        self.clear();
        true
    }
}

fn normalized_resume_url(resume_url: &str) -> Result<String, String> {
    let mut url =
        Url::parse(resume_url).map_err(|error| format!("invalid Gateway resume URL: {error}"))?;
    if url.scheme() != "wss" {
        return Err("invalid Gateway resume URL: scheme must be wss".to_owned());
    }
    if url.host_str().is_none() {
        return Err("invalid Gateway resume URL: host is missing".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("invalid Gateway resume URL: credentials are not allowed".to_owned());
    }
    if url.fragment().is_some() {
        return Err("invalid Gateway resume URL: fragments are not allowed".to_owned());
    }
    let retained_query = url
        .query_pairs()
        .filter(|(name, _)| !matches!(name.as_ref(), "v" | "encoding" | "compress"))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(retained_query);
        query.append_pair("v", "9");
        query.append_pair("encoding", "json");
        query.append_pair("compress", "zlib-stream");
    }
    Ok(url.to_string())
}

pub async fn run_gateway(
    token: String,
    mut commands: mpsc::UnboundedReceiver<GatewayCommand>,
    runtime: GatewayRuntime,
) {
    let mut session = SessionState::default();
    let mut resources = GatewaySessionResources::default();
    let mut backoff = RECONNECT_BASE_DELAY;
    let mut publish_gateway_closed = true;

    loop {
        let publish = GatewayPublishContext {
            state: &runtime.state,
            gateway_session_id: &runtime.gateway_session_id,
            event_publisher: &runtime.event_publisher,
        };
        let outcome = match connect_and_run(
            &token,
            &mut commands,
            &mut session,
            &mut resources,
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
            ConnectionOutcome::Resume => {}
            ConnectionOutcome::Reidentify => {
                session.clear();
                clear_published_gateway_session(publish);
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
            state: &runtime.state,
            gateway_session_id: &runtime.gateway_session_id,
            event_publisher: &runtime.event_publisher,
        };
        publish_gateway_event(publish, AppEvent::GatewayClosed).await;
    }
}

async fn connect_and_run(
    token: &str,
    commands: &mut mpsc::UnboundedReceiver<GatewayCommand>,
    session: &mut SessionState,
    resources: &mut GatewaySessionResources,
    fingerprint: &ClientFingerprint,
    publish: GatewayPublishContext<'_>,
) -> Result<ConnectionOutcome, String> {
    let connection = session.next_connection();
    if let Some(error) = connection.recovery_warning.as_ref() {
        log_and_publish_gateway_error(publish, error.clone()).await;
        clear_published_gateway_session(publish);
    }
    logging::debug("gateway", format!("connecting to {}", connection.url));

    let request = gateway_request(&connection.url, fingerprint)
        .map_err(|error| gateway_setup_failure(session, &connection.handshake, publish, error))?;
    let (ws, _response) =
        connect_async_with_config(request, Some(gateway_websocket_config()), false)
            .await
            .map_err(|error| {
                gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    format!("websocket connect failed: {error}"),
                )
            })?;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let (sender, mut gateway_send_error_rx, gateway_writer_task) =
        spawn_gateway_sender(Arc::clone(&writer));
    let mut subscription_deduper = SubscriptionDeduper::default();
    let mut zlib_decoder = GatewayZlibDecoder::default();

    // Discord must speak first with op-10 HELLO carrying heartbeat_interval.
    // If the first frame is anything else, fail fast and try a clean
    // re-identify.
    let hello_frame = loop {
        match reader.next().await {
            Some(Ok(WsMessage::Text(text))) => break text.to_string(),
            Some(Ok(WsMessage::Binary(chunk))) => {
                match zlib_decoder.decode(&chunk).map_err(|error| {
                    gateway_setup_failure(session, &connection.handshake, publish, error)
                })? {
                    Some(text) => break text,
                    None => continue,
                }
            }
            Some(Ok(WsMessage::Close(frame))) => {
                let message =
                    websocket_close_message("websocket closed before HELLO", frame.as_ref());
                log_and_publish_gateway_error(publish, message).await;
                return Ok(ConnectionOutcome::Reidentify);
            }
            Some(Ok(_)) => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    "unexpected control frame before HELLO",
                ));
            }
            Some(Err(error)) => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    format!("read HELLO failed: {error}"),
                ));
            }
            None => {
                return Err(gateway_setup_failure(
                    session,
                    &connection.handshake,
                    publish,
                    "connection closed before HELLO",
                ));
            }
        }
    };
    let hello: Value = serde_json::from_str(&hello_frame).map_err(|error| {
        gateway_setup_failure(
            session,
            &connection.handshake,
            publish,
            format!("HELLO parse: {error}"),
        )
    })?;
    if hello.get("op").and_then(Value::as_u64) != Some(10) {
        return Err(gateway_setup_failure(
            session,
            &connection.handshake,
            publish,
            format!(
                "first frame was not HELLO: {}",
                hello.get("op").and_then(Value::as_u64).unwrap_or_default()
            ),
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
    match &connection.handshake {
        GatewayHandshake::Resume {
            session_id,
            sequence,
        } => {
            let payload = build_resume_payload(token, session_id, *sequence);
            send_text(&sender, payload).await.map_err(|error| {
                gateway_setup_failure(session, &connection.handshake, publish, error)
            })?;
            logging::debug("gateway", "RESUME sent");
        }
        GatewayHandshake::Identify => {
            resources
                .guild_member_requests
                .start_new_session(Instant::now());
            let client_state = {
                let state = publish
                    .state
                    .read()
                    .expect("discord state lock is not poisoned");
                if session.has_received_ready && resources.last_presence.is_none() {
                    resources.last_presence = current_gateway_presence(&state);
                }
                state.client_cache_state()
            };
            let reidentify_presence = session
                .has_received_ready
                .then_some(resources.last_presence.as_ref())
                .flatten();
            let payload =
                build_identify_payload(token, fingerprint, reidentify_presence, client_state);
            send_text(&sender, payload).await.map_err(|error| {
                gateway_setup_failure(session, &connection.handshake, publish, error)
            })?;
            logging::debug("gateway", "IDENTIFY sent");
        }
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
    let initial_jitter = Duration::from_millis(
        rand::thread_rng().gen_range(0..=heartbeat_interval.as_millis() as u64),
    );
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

    // Main loop: race incoming frames against outgoing work. Keep branch
    // polling fair because a busy command or dispatch stream must not starve a
    // due member request or its writer completion.
    let mut member_request_send: Option<InFlightGuildMemberRequest> = None;
    let outcome = loop {
        let member_request_delay = member_request_send
            .is_none()
            .then(|| resources.guild_member_requests.next_delay(Instant::now()))
            .flatten();
        tokio::select! {
            maybe_command = commands.recv() => {
                match maybe_command {
                    Some(command) => {
                        if let GatewayCommand::Shutdown { voice_leave } = command {
                            if let Some(voice_leave) = voice_leave {
                                let leave_result = timeout(
                                    GATEWAY_SHUTDOWN_LEAVE_TIMEOUT,
                                    sender.send_urgent(voice_state_update_payload(
                                        voice_leave.guild_id,
                                        voice_leave.channel_id,
                                        voice_leave.self_mute,
                                        voice_leave.self_deaf,
                                    )),
                                )
                                .await
                                .map_err(|_| {
                                    "voice leave timed out before gateway shutdown".to_owned()
                                })
                                .and_then(|result| result);
                                if let Err(error) = leave_result {
                                    log_and_publish_gateway_error(
                                        publish,
                                        format!(
                                            "voice leave before gateway shutdown failed: {error}"
                                        ),
                                    )
                                    .await;
                                }
                            }
                            if let Err(error) = close_websocket(&writer).await {
                                log_and_publish_gateway_error(
                                    publish,
                                    format!("gateway shutdown failed: {error}"),
                                )
                                .await;
                            }
                            break ConnectionOutcome::Stop;
                        } else if let Err(error) =
                            dispatch_command(
                                &sender,
                                command,
                                &mut subscription_deduper,
                                resources,
                            )
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
                        let frame_context = FrameContext {
                            sequence_cell: &sequence_cell,
                            heartbeat_ack: &heartbeat_ack,
                            sender: &sender,
                            fingerprint,
                            publish,
                        };
                        match handle_json_frame(
                            &text,
                            session,
                            resources,
                            frame_context,
                        ).await {
                            FrameOutcome::Continue => {}
                            FrameOutcome::Resume => break ConnectionOutcome::Resume,
                            FrameOutcome::Reidentify => break ConnectionOutcome::Reidentify,
                        }
                    }
                    Some(Ok(WsMessage::Binary(chunk))) => {
                        let text = match zlib_decoder.decode(&chunk) {
                            Ok(Some(text)) => text,
                            Ok(None) => continue,
                            Err(error) => {
                                log_and_publish_gateway_error(publish, error).await;
                                break ConnectionOutcome::Resume;
                            }
                        };
                        let frame_context = FrameContext {
                            sequence_cell: &sequence_cell,
                            heartbeat_ack: &heartbeat_ack,
                            sender: &sender,
                            fingerprint,
                            publish,
                        };
                        match handle_json_frame(
                            &text,
                            session,
                            resources,
                            frame_context,
                        ).await {
                            FrameOutcome::Continue => {}
                            FrameOutcome::Resume => break ConnectionOutcome::Resume,
                            FrameOutcome::Reidentify => break ConnectionOutcome::Reidentify,
                        }
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
            send_result = async {
                member_request_send
                    .as_mut()
                    .expect("guard ensures a guild member request is in flight")
                    .wait()
                    .await
            }, if member_request_send.is_some() => {
                member_request_send
                    .take()
                    .expect("completed guild member request exists");
                if let Err(error) = send_result {
                    let message = format!("guild member request send failed: {error}");
                    log_and_publish_gateway_error(publish, message).await;
                    break ConnectionOutcome::Resume;
                }
                resources
                    .guild_member_requests
                    .complete_send(Instant::now());
            }
            _ = sleep(member_request_delay.unwrap_or_default()), if member_request_delay.is_some() => {
                let Some(payload) = resources
                    .guild_member_requests
                    .start_due(Instant::now())
                else {
                    continue;
                };
                let completion = match sender.enqueue_normal(payload) {
                    Ok(completion) => completion,
                    Err(error) => {
                        let message = format!("guild member request send failed: {error}");
                        log_and_publish_gateway_error(publish, message).await;
                        break ConnectionOutcome::Resume;
                    }
                };
                member_request_send = Some(InFlightGuildMemberRequest { completion });
            }
        }
    };

    if matches!(
        outcome,
        ConnectionOutcome::Resume | ConnectionOutcome::Reidentify
    ) {
        resources
            .guild_member_requests
            .prepare_reconnect(Instant::now());
    } else {
        resources
            .guild_member_requests
            .cancel_in_flight(Instant::now());
    }
    heartbeat_task.abort();
    gateway_writer_task.abort();
    Ok(outcome)
}

async fn handle_json_frame(
    text: &str,
    session: &mut SessionState,
    resources: &mut GatewaySessionResources,
    frame_context: FrameContext<'_>,
) -> FrameOutcome {
    let value = match parse_gateway_frame(text, session) {
        Ok(value) => value,
        Err(failure) => {
            log_and_publish_gateway_error(frame_context.publish, failure.message).await;
            return failure.outcome;
        }
    };
    handle_frame(value, session, frame_context, resources).await
}

#[derive(Debug, Eq, PartialEq)]
struct MalformedGatewayFrame {
    message: String,
    outcome: FrameOutcome,
}

fn parse_gateway_frame(
    text: &str,
    session: &mut SessionState,
) -> Result<Value, MalformedGatewayFrame> {
    serde_json::from_str(text).map_err(|error| MalformedGatewayFrame {
        message: format!("gateway JSON parse failed: {error}"),
        // A single Resume can repair transient transport corruption. If the
        // same confirmed sequence fails again, abandon the replay buffer and
        // re-identify instead of reconnecting forever.
        outcome: session.malformed_frame_outcome(),
    })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameOutcome {
    Continue,
    Resume,
    Reidentify,
}

async fn handle_frame(
    value: Value,
    session: &mut SessionState,
    context: FrameContext<'_>,
    resources: &mut GatewaySessionResources,
) -> FrameOutcome {
    let op = value.get("op").and_then(Value::as_u64).unwrap_or_default();
    match op {
        // Dispatch
        0 => {
            if let Some(seq) = value.get("s").and_then(Value::as_u64) {
                session.record_sequence(seq);
                *context.sequence_cell.lock().await = Some(seq);
            }
            let dispatch_type = value.get("t").and_then(Value::as_str).unwrap_or("");
            let mut publish_reidentified = false;
            if dispatch_type == "RATE_LIMITED"
                && let Some(rate_limit) = gateway_guild_member_rate_limit(&value)
            {
                logging::debug(
                    "gateway",
                    format!(
                        "guild member requests rate limited: guild={} retry_after_ms={}",
                        rate_limit.guild_id.get(),
                        rate_limit.retry_after.as_millis()
                    ),
                );
                resources.guild_member_requests.apply_rate_limit(
                    rate_limit.guild_id,
                    rate_limit.nonce.as_deref(),
                    rate_limit.retry_after,
                    Instant::now(),
                );
            } else if dispatch_type == "GUILD_MEMBERS_CHUNK"
                && let Some(nonce) = gateway_guild_member_chunk_nonce(&value)
            {
                resources.guild_member_requests.acknowledge(nonce);
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
    context.event_publisher.publish(event).await;
}

fn clear_published_gateway_session(context: GatewayPublishContext<'_>) {
    *context
        .gateway_session_id
        .write()
        .expect("gateway session id lock is not poisoned") = None;
}

/// A Resume is only useful when its connection and initial handshake both
/// succeed. Transport failures after the socket opens must not select the same
/// failed Resume plan forever, so the next loop starts a fresh Identify.
fn gateway_setup_failure(
    session: &mut SessionState,
    handshake: &GatewayHandshake,
    context: GatewayPublishContext<'_>,
    error: impl Into<String>,
) -> String {
    if session.abandon_failed_resume(handshake) {
        clear_published_gateway_session(context);
    }
    error.into()
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
        4004 | 4010..=4016 => ConnectionOutcome::Fatal,
        4003 | 4007 | 4009 => ConnectionOutcome::Reidentify,
        4000..=4002 | 4005 | 4008 => ConnectionOutcome::Resume,
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
    resources: &mut GatewaySessionResources,
) -> Result<(), String> {
    if !subscription_deduper.should_send(&command) {
        logging::debug("gateway", "skipping duplicate channel subscription");
        return Ok(());
    }
    if let GatewayCommand::UpdatePresence { status, activities } = &command {
        resources.last_presence = Some(GatewayPresence {
            status: *status,
            activities: activities.clone(),
        });
    }
    let urgent = matches!(
        command,
        GatewayCommand::UpdateVoiceState { .. }
            | GatewayCommand::WatchStream { .. }
            | GatewayCommand::CreateStream { .. }
            | GatewayCommand::DeleteStream { .. }
    );
    let payload = match command {
        GatewayCommand::SearchGuildMembers {
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
            if !resources.guild_member_requests.enqueue_search(
                guild_id,
                query,
                limit,
                presences,
                nonce,
                Instant::now(),
            ) {
                logging::debug(
                    "gateway",
                    "dropping guild member search because the session queue is full",
                );
            }
            return Ok(());
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
            resources.guild_member_requests.enqueue_by_ids(
                guild_id,
                user_ids,
                presences,
                Instant::now(),
            );
            return Ok(());
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
            guild_channel_subscribe_payload(guild_id, channel_id, &[(0, 99)], None)
        }
        GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id,
            ranges,
        } => {
            logging::debug(
                "gateway",
                format!(
                    "updating member list ranges: guild={} channel={} thread={} ranges={:?}",
                    guild_id.get(),
                    channel_id.get(),
                    thread_id.map(Id::get).unwrap_or_default(),
                    ranges
                ),
            );
            let thread_member_lists = thread_id.into_iter().collect::<Vec<_>>();
            guild_channel_subscribe_payload(
                guild_id,
                channel_id,
                &ranges,
                Some(&thread_member_lists),
            )
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
        GatewayCommand::WatchStream { stream_key } => {
            logging::debug("gateway", format!("watching stream: {stream_key}"));
            watch_stream_payload(&stream_key)
        }
        GatewayCommand::CreateStream { scope, channel_id } => {
            logging::debug(
                "gateway",
                format!("creating stream: scope={scope:?} channel={channel_id}"),
            );
            create_stream_payload(scope, channel_id)
        }
        GatewayCommand::DeleteStream { stream_key } => {
            logging::debug("gateway", format!("deleting stream: {stream_key}"));
            delete_stream_payload(&stream_key)
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
        GatewayCommand::Shutdown { .. } => return Ok(()),
    };
    if urgent {
        sender.enqueue_urgent_text(payload)
    } else {
        sender.enqueue_text(payload)
    }
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

    fn enqueue_urgent_text(&self, payload: String) -> Result<(), String> {
        self.urgent_tx
            .send(GatewaySendRequest {
                payload,
                completion: None,
            })
            .map_err(|_| "gateway writer task stopped".to_owned())
    }

    fn enqueue_text(&self, payload: String) -> Result<(), String> {
        self.normal_tx
            .send(GatewaySendRequest {
                payload,
                completion: None,
            })
            .map_err(|_| "gateway writer task stopped".to_owned())
    }

    fn enqueue_normal(
        &self,
        payload: String,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        let (completion_tx, completion_rx) = oneshot::channel();
        self.normal_tx
            .send(GatewaySendRequest {
                payload,
                completion: Some(completion_tx),
            })
            .map_err(|_| "gateway writer task stopped".to_owned())?;
        Ok(completion_rx)
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

#[derive(Debug, Eq, PartialEq)]
struct GuildMemberRateLimit {
    guild_id: Id<GuildMarker>,
    nonce: Option<String>,
    retry_after: Duration,
}

fn gateway_guild_member_rate_limit(value: &Value) -> Option<GuildMemberRateLimit> {
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
    Some(GuildMemberRateLimit {
        guild_id,
        nonce: data
            .get("meta")
            .and_then(|meta| meta.get("nonce"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        retry_after: Duration::from_secs_f64(
            retry_after.min(MAX_GATEWAY_RETRY_DELAY.as_secs_f64()),
        ),
    })
}

fn gateway_guild_member_chunk_nonce(value: &Value) -> Option<&str> {
    value
        .get("d")
        .and_then(|data| data.get("nonce"))
        .and_then(Value::as_str)
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

fn build_identify_payload(
    token: &str,
    fingerprint: &ClientFingerprint,
    presence: Option<&GatewayPresence>,
    client_state: ClientCacheState,
) -> String {
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

    let presence = presence
        .map(|presence| gateway_presence_payload(&presence.status, &presence.activities))
        .unwrap_or_else(|| gateway_presence_payload(&PresenceStatus::Unknown, &[]));

    // Only reuse versions Concord actually tracks. The remaining conservative
    // defaults avoid claiming cache state that this process cannot verify.
    json!({
        "op": 2,
        "d": {
            "token": token,
            "capabilities": USER_ACCOUNT_CAPABILITIES,
            "properties": properties,
            "presence": presence,
            // `zlib-stream` is selected in the Gateway URL. The browser keeps
            // this separate Identify compression mode disabled.
            "compress": false,
            "client_state": {
                "guild_versions": {},
                "highest_last_message_id": client_state
                    .highest_guild_message_id
                    .map(Id::get)
                    .unwrap_or_default()
                    .to_string(),
                "read_state_version": client_state.read_state_version.unwrap_or_default(),
                "user_guild_settings_version": client_state
                    .user_guild_settings_version
                    .unwrap_or(-1),
                "user_settings_version": -1,
                "private_channels_version": client_state
                    .highest_private_message_id
                    .map(Id::get)
                    .unwrap_or_default()
                    .to_string(),
                "api_code_version": 0,
            },
        },
    })
    .to_string()
}

fn build_resume_payload(token: &str, session_id: &str, sequence: u64) -> String {
    json!({
        "op": 6,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence,
        },
    })
    .to_string()
}

fn search_guild_members_payload(
    guild_id: Id<GuildMarker>,
    query: &str,
    limit: u16,
    presences: bool,
    nonce: &str,
) -> String {
    json!({
        "op": 8,
        "d": {
            "guild_id": [guild_id.to_string()],
            "query": query,
            "limit": limit,
            "presences": presences,
            "nonce": nonce,
        },
    })
    .to_string()
}

fn request_guild_members_by_ids_payload(
    guild_id: Id<GuildMarker>,
    user_ids: &[Id<UserMarker>],
    presences: bool,
    nonce: &str,
) -> String {
    let user_ids = user_ids
        .iter()
        .take(100)
        .map(|user_id| user_id.to_string())
        .collect::<Vec<_>>();
    json!({
        "op": 8,
        "d": {
            "guild_id": [guild_id.to_string()],
            "user_ids": user_ids,
            "presences": presences,
            "nonce": nonce,
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
    thread_member_lists: Option<&[Id<ChannelMarker>]>,
) -> String {
    let ranges_json: Vec<[u32; 2]> = ranges.iter().map(|(start, end)| [*start, *end]).collect();
    let mut subscription = json!({
        "typing": true,
        "activities": true,
        "threads": true,
        "member_updates": true,
        "members": [],
        "channels": {
            channel_id.to_string(): ranges_json,
        },
    });
    if let Some(thread_member_lists) = thread_member_lists {
        subscription["thread_member_lists"] = Value::from(
            thread_member_lists
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
    }
    json!({
        "op": 37,
        "d": {
            "subscriptions": {
                guild_id.to_string(): subscription,
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

fn watch_stream_payload(stream_key: &str) -> String {
    json!({
        "op": 20,
        "d": {
            "stream_key": stream_key,
        },
    })
    .to_string()
}

fn create_stream_payload(scope: VoiceScope, channel_id: Id<ChannelMarker>) -> String {
    let (stream_type, guild_id) = match scope {
        VoiceScope::Guild(guild_id) => ("guild", Some(guild_id.to_string())),
        VoiceScope::Private(_) => ("call", None),
    };
    json!({
        "op": 18,
        "d": {
            "type": stream_type,
            "guild_id": guild_id,
            "channel_id": channel_id.to_string(),
            "preferred_region": Value::Null,
        },
    })
    .to_string()
}

fn delete_stream_payload(stream_key: &str) -> String {
    json!({
        "op": 19,
        "d": {
            "stream_key": stream_key,
        },
    })
    .to_string()
}

fn presence_update_payload(status: PresenceStatus, activities: &[ActivityInfo]) -> String {
    json!({
        "op": 3,
        "d": gateway_presence_payload(&status, activities),
    })
    .to_string()
}

fn gateway_presence_payload(status: &PresenceStatus, activities: &[ActivityInfo]) -> Value {
    json!({
        "since": 0,
        "activities": activities.iter().map(activity_gateway_payload).collect::<Vec<_>>(),
        "status": status.gateway_status(),
        "afk": false,
    })
}

fn current_gateway_presence(state: &DiscordState) -> Option<GatewayPresence> {
    let user_id = state.current_user_id()?;
    Some(GatewayPresence {
        status: state.user_presence(user_id)?,
        activities: state.user_activities(user_id).to_vec(),
    })
}

fn activity_gateway_payload(activity: &ActivityInfo) -> Value {
    let mut fields: serde_json::Map<String, Value> =
        activity.extra_fields.clone().into_iter().collect();
    fields.insert("name".to_owned(), json!(activity.name));
    fields.insert("type".to_owned(), json!(activity.kind.gateway_code()));
    if let Some(details) = activity.details.as_deref() {
        fields.insert("details".to_owned(), json!(details));
    }
    if let Some(details_url) = activity.details_url.as_deref() {
        fields.insert("details_url".to_owned(), json!(details_url));
    }
    if let Some(state) = activity.state.as_deref() {
        fields.insert("state".to_owned(), json!(state));
    }
    if let Some(state_url) = activity.state_url.as_deref() {
        fields.insert("state_url".to_owned(), json!(state_url));
    }
    if let Some(url) = activity.url.as_deref() {
        fields.insert("url".to_owned(), json!(url));
    }
    if let Some(platform) = activity.platform.as_deref() {
        fields.insert("platform".to_owned(), json!(platform));
    }
    if !activity.supported_platforms.is_empty() {
        fields.insert(
            "supported_platforms".to_owned(),
            json!(activity.supported_platforms),
        );
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
        fields.insert("emoji".to_owned(), node);
    }
    if let Some(application_id) = activity.application_id.as_deref() {
        fields.insert("application_id".to_owned(), json!(application_id));
    }
    if let Some(parent_application_id) = activity.parent_application_id.as_deref() {
        fields.insert(
            "parent_application_id".to_owned(),
            json!(parent_application_id),
        );
    }
    if let Some(status_display_type) = activity.status_display_type {
        fields.insert("status_display_type".to_owned(), json!(status_display_type));
    }
    if let Some(sync_id) = activity.sync_id.as_deref() {
        fields.insert("sync_id".to_owned(), json!(sync_id));
    }
    if let Some(timestamps) = activity.timestamps.as_ref() {
        let mut node = json!({});
        if let Some(start) = timestamps.start {
            node["start"] = json!(start);
        }
        if let Some(end) = timestamps.end {
            node["end"] = json!(end);
        }
        fields.insert("timestamps".to_owned(), node);
    }
    if let Some(assets) = activity.assets.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            assets.extra_fields.clone().into_iter().collect();
        if let Some(large_image) = assets.large_image.as_deref() {
            node.insert("large_image".to_owned(), json!(large_image));
        }
        if let Some(large_text) = assets.large_text.as_deref() {
            node.insert("large_text".to_owned(), json!(large_text));
        }
        if let Some(large_url) = assets.large_url.as_deref() {
            node.insert("large_url".to_owned(), json!(large_url));
        }
        if let Some(small_image) = assets.small_image.as_deref() {
            node.insert("small_image".to_owned(), json!(small_image));
        }
        if let Some(small_text) = assets.small_text.as_deref() {
            node.insert("small_text".to_owned(), json!(small_text));
        }
        if let Some(small_url) = assets.small_url.as_deref() {
            node.insert("small_url".to_owned(), json!(small_url));
        }
        if let Some(invite_cover_image) = assets.invite_cover_image.as_deref() {
            node.insert("invite_cover_image".to_owned(), json!(invite_cover_image));
        }
        fields.insert("assets".to_owned(), Value::Object(node));
    }
    if let Some(party) = activity.party.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            party.extra_fields.clone().into_iter().collect();
        if let Some(id) = party.id.as_deref() {
            node.insert("id".to_owned(), json!(id));
        }
        if let Some((current, max)) = party.size {
            node.insert("size".to_owned(), json!([current, max]));
        }
        if let Some(privacy) = party.privacy {
            node.insert("privacy".to_owned(), json!(privacy));
        }
        fields.insert("party".to_owned(), Value::Object(node));
    }
    if let Some(secrets) = activity.secrets.as_ref() {
        let mut node: serde_json::Map<String, Value> =
            secrets.extra_fields.clone().into_iter().collect();
        if let Some(join) = secrets.join.as_deref() {
            node.insert("join".to_owned(), json!(join));
        }
        if let Some(spectate) = secrets.spectate.as_deref() {
            node.insert("spectate".to_owned(), json!(spectate));
        }
        fields.insert("secrets".to_owned(), Value::Object(node));
    }

    let mut flags = activity.flags;
    if let Some(instance) = activity.instance {
        let updated = if instance {
            flags.unwrap_or_default() | 1
        } else {
            flags.unwrap_or_default() & !1
        };
        flags = Some(updated);
    }
    if let Some(flags) = flags {
        fields.insert("flags".to_owned(), json!(flags));
    }

    let mut metadata: serde_json::Map<String, Value> =
        activity.metadata.clone().into_iter().collect();
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
        fields.insert("buttons".to_owned(), json!(labels));
        metadata.insert("button_urls".to_owned(), json!(urls));
    }
    if !metadata.is_empty() {
        fields.insert("metadata".to_owned(), Value::Object(metadata));
    }

    Value::Object(fields)
}

#[cfg(test)]
mod tests;
