use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, RwLock},
};

use crate::config::{MicrophoneSensitivityDb, VoiceVolumePercent};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use chrono::{DateTime, Utc};
use reqwest::header::HeaderValue;
use tokio::{
    sync::{Mutex as AsyncMutex, mpsc, watch},
    task::JoinHandle,
};

use crate::{AppError, Result};

use super::{
    ApplicationCommandInfo, ApplicationCommandInvocation, MessageAttachmentUpload, MessageInfo,
    ReactionEmoji, ReactionUserInfo, UserProfileInfo,
    application_commands::application_command_interaction_from_invocation,
    commands::{AppCommand, ForumPostArchiveState},
    events::{AppEvent, SequencedAppEvent},
    gateway::{GatewayCommand, GatewayRuntime, run_gateway},
    request_lifecycle::{
        ForumPostRequestTarget, MemberListSubscriptionTarget, MentionMemberSearchTarget,
        RequestLifecycle,
    },
    rest::{DiscordRest, ForumPostPage},
    state::{CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, SnapshotRevision},
    voice::{self, VoiceRuntimeEvent},
};

const MEMBER_SEARCH_MIN_QUERY_CHARS: usize = 2;
const MEMBER_SEARCH_MAX_QUERY_CHARS: usize = 64;
const MEMBER_SEARCH_MAX_LIMIT: u16 = 10;

type ApplicationCommandCache = HashMap<Option<Id<GuildMarker>>, Vec<ApplicationCommandInfo>>;
type MemberListRange = (u32, u32);
type MemberListSubscriptionRequest = (
    Id<GuildMarker>,
    Id<ChannelMarker>,
    u32,
    Vec<MemberListRange>,
);
type DueMemberListSubscription = (Id<GuildMarker>, Id<ChannelMarker>, Vec<MemberListRange>);

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    effects_tx: mpsc::Sender<SequencedAppEvent>,
    effects_rx: Arc<Mutex<Option<mpsc::Receiver<SequencedAppEvent>>>>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    requested_voice: Arc<RwLock<Option<CurrentVoiceConnectionState>>>,
    gateway_session_id: Arc<RwLock<Option<String>>>,
    application_command_requests: Arc<Mutex<HashMap<Option<Id<GuildMarker>>, RequestState>>>,
    application_commands: Arc<Mutex<ApplicationCommandCache>>,
    request_lifecycle: Arc<Mutex<RequestLifecycle>>,
    revision: Arc<RwLock<SnapshotRevision>>,
    publish_lock: Arc<AsyncMutex<()>>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    gateway_commands_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<GatewayCommand>>>>,
    voice_events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    voice_events_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<VoiceRuntimeEvent>>>>,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self> {
        validate_token_header(&token)?;
        let rest = DiscordRest::new(token.clone());
        let initial_state = DiscordState::default();
        let (effects_tx, effects_rx) = mpsc::channel(4096);
        let (snapshots_tx, _) = watch::channel(SnapshotRevision::default());
        let (gateway_commands_tx, gateway_commands_rx) = mpsc::unbounded_channel();
        let (voice_events_tx, voice_events_rx) = mpsc::unbounded_channel();

        Ok(Self {
            token,
            rest,
            effects_tx,
            effects_rx: Arc::new(Mutex::new(Some(effects_rx))),
            snapshots_tx,
            state: Arc::new(RwLock::new(initial_state)),
            requested_voice: Arc::new(RwLock::new(None)),
            gateway_session_id: Arc::new(RwLock::new(None)),
            application_command_requests: Arc::new(Mutex::new(HashMap::new())),
            application_commands: Arc::new(Mutex::new(HashMap::new())),
            request_lifecycle: Arc::new(Mutex::new(RequestLifecycle::default())),
            revision: Arc::new(RwLock::new(SnapshotRevision::default())),
            publish_lock: Arc::new(AsyncMutex::new(())),
            gateway_commands_tx,
            gateway_commands_rx: Arc::new(Mutex::new(Some(gateway_commands_rx))),
            voice_events_tx,
            voice_events_rx: Arc::new(Mutex::new(Some(voice_events_rx))),
        })
    }

    pub fn take_effects(&self) -> mpsc::Receiver<SequencedAppEvent> {
        self.effects_rx
            .lock()
            .expect("effect receiver mutex is not poisoned")
            .take()
            .expect("effect stream can only be taken once")
    }

    pub fn subscribe_snapshots(&self) -> watch::Receiver<SnapshotRevision> {
        self.snapshots_tx.subscribe()
    }

    pub fn current_discord_snapshot(&self) -> DiscordSnapshot {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let revision = *self
            .revision
            .read()
            .expect("snapshot revision lock is not poisoned");
        state.snapshot(revision)
    }

    pub async fn publish_event(&self, event: AppEvent) {
        self.record_request_lifecycle_event(&event);
        publish_app_event(
            &self.effects_tx,
            &self.snapshots_tx,
            &self.state,
            &self.revision,
            &self.publish_lock,
            &event,
        )
        .await;
        voice::forward_app_event(&self.voice_events_tx, &event);
    }

    pub(crate) fn record_request_lifecycle_event(&self, event: &AppEvent) {
        if let AppEvent::ApplicationCommandsLoaded { guild_id, .. } = event {
            self.record_application_commands_loaded(*guild_id);
        }
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .record_event(event);
    }

    pub(crate) fn next_message_history_request(
        &self,
        channel_id: Option<Id<ChannelMarker>>,
        force_reload: bool,
    ) -> Option<Id<ChannelMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_history_request(channel_id, force_reload)
    }

    pub(crate) fn mark_message_history_request_failed(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_history_failed(channel_id);
    }

    pub(crate) fn begin_older_message_history_request(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Id<MessageMarker>,
    ) -> bool {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_older_history_request(channel_id, before)
    }

    pub(crate) fn begin_newer_message_history_request(
        &self,
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
    ) -> bool {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_newer_history_request(channel_id, after)
    }

    pub(crate) fn next_forum_post_request(
        &self,
        target: Option<(Id<GuildMarker>, Id<ChannelMarker>, bool)>,
    ) -> Option<(
        Id<GuildMarker>,
        Id<ChannelMarker>,
        ForumPostArchiveState,
        usize,
    )> {
        let target =
            target.map(
                |(guild_id, channel_id, should_load_more)| ForumPostRequestTarget {
                    guild_id,
                    channel_id,
                    should_load_more,
                },
            );
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_forum_post_request(target)
    }

    pub(crate) fn mark_forum_post_request_failed(
        &self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_forum_post_failed(channel_id, archive_state, offset);
    }

    pub(crate) fn next_pinned_message_request(
        &self,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_pinned_message_request(channel_id)
    }

    pub(crate) fn mark_pinned_message_request_failed(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_pinned_message_failed(channel_id);
    }

    pub(crate) fn next_message_author_member_requests(
        &self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: std::time::Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_message_author_member_requests(missing, now)
    }

    pub(crate) fn next_initial_unknown_member_requests(
        &self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: std::time::Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_initial_unknown_member_requests(missing, now)
    }

    pub(crate) fn next_member_request(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<Id<GuildMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_member_request(guild_id)
    }

    pub(crate) fn remove_member_request(&self, guild_id: Id<GuildMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .remove_member_request(guild_id);
    }

    pub(crate) fn set_mention_member_search_target(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        query: Option<&str>,
        now: std::time::Instant,
    ) {
        let target = guild_id
            .zip(query)
            .map(|(guild_id, query)| MentionMemberSearchTarget {
                guild_id,
                query: query.to_owned(),
            });
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .set_mention_member_search_target(target, now);
    }

    pub(crate) fn mention_member_search_deadline(&self) -> Option<std::time::Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mention_member_search_deadline()
    }

    pub(crate) fn next_due_mention_member_search(
        &self,
        now: std::time::Instant,
    ) -> Option<(Id<GuildMarker>, String)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_due_mention_member_search(now)
            .map(|target| (target.guild_id, target.query))
    }

    pub(crate) fn set_member_list_subscription_target(
        &self,
        target: Option<MemberListSubscriptionRequest>,
        now: std::time::Instant,
    ) {
        let target =
            target.map(
                |(guild_id, channel_id, bucket, ranges)| MemberListSubscriptionTarget {
                    guild_id,
                    channel_id,
                    bucket,
                    ranges,
                },
            );
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .set_member_list_subscription_target(target, now);
    }

    pub(crate) fn member_list_subscription_deadline(&self) -> Option<std::time::Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .member_list_subscription_deadline()
    }

    pub(crate) fn next_due_member_list_subscription(
        &self,
        now: std::time::Instant,
    ) -> Option<DueMemberListSubscription> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_due_member_list_subscription(now)
            .map(|target| (target.guild_id, target.channel_id, target.ranges))
    }

    pub(crate) fn next_thread_preview_requests(
        &self,
        missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_thread_preview_requests(missing)
    }

    pub(crate) fn remove_thread_preview_request(
        &self,
        key: (Id<ChannelMarker>, Id<MessageMarker>),
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .remove_thread_preview_request(key);
    }

    pub(crate) fn next_user_profile_request(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<(Id<UserMarker>, Option<Id<GuildMarker>>, bool)> {
        let is_self = {
            let state = self
                .state
                .read()
                .expect("discord state lock is not poisoned");
            if state.user_profile(user_id, guild_id).is_some() {
                return None;
            }
            state.current_user_id() == Some(user_id)
        };
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_user_profile_request(user_id, guild_id)
            .then_some((user_id, guild_id, is_self))
    }

    pub(crate) fn next_user_note_request(&self, user_id: Id<UserMarker>) -> Option<Id<UserMarker>> {
        {
            let state = self
                .state
                .read()
                .expect("discord state lock is not poisoned");
            if state.is_note_fetched(user_id) {
                return None;
            }
        }
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_user_note_request(user_id)
            .then_some(user_id)
    }

    pub(crate) fn mark_user_note_request_failed(&self, user_id: Id<UserMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_user_note_failed(user_id);
    }

    pub(crate) fn schedule_read_ack(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        now: std::time::Instant,
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .schedule_read_ack(channel_id, message_id, now);
    }

    pub(crate) async fn publish_optimistic_read_ack(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.publish_event(AppEvent::MessageAck {
            channel_id,
            message_id,
            mention_count: 0,
        })
        .await;
    }

    pub(crate) async fn publish_optimistic_read_acks(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) {
        for (channel_id, message_id) in targets.iter().copied() {
            self.publish_optimistic_read_ack(channel_id, message_id)
                .await;
        }
    }

    pub(crate) fn clear_read_ack(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .clear_read_ack(channel_id);
    }

    pub(crate) fn clear_read_acks(&self, channel_ids: impl IntoIterator<Item = Id<ChannelMarker>>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .clear_read_acks(channel_ids);
    }

    pub(crate) fn next_read_ack_deadline(&self) -> Option<std::time::Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_read_ack_deadline()
    }

    pub(crate) fn flush_due_read_acks(
        &self,
        now: std::time::Instant,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .flush_due_read_acks(now)
    }

    pub(crate) fn due_read_ack_commands(&self, now: std::time::Instant) -> Vec<AppCommand> {
        self.flush_due_read_acks(now)
            .into_iter()
            .map(|(channel_id, message_id)| AppCommand::AckChannel {
                channel_id,
                message_id,
            })
            .collect()
    }

    pub fn start_gateway(&self) -> JoinHandle<()> {
        let token = self.token.clone();
        let effects_tx = self.effects_tx.clone();
        let snapshots_tx = self.snapshots_tx.clone();
        let state = Arc::clone(&self.state);
        let revision = Arc::clone(&self.revision);
        let gateway_session_id = Arc::clone(&self.gateway_session_id);
        let publish_lock = Arc::clone(&self.publish_lock);
        let gateway_commands = self
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway can only be started once");
        let voice_events_tx = self.voice_events_tx.clone();
        let voice_status_publisher = voice::VoiceStatusPublisher::new(
            self.effects_tx.clone(),
            self.snapshots_tx.clone(),
            Arc::clone(&self.state),
            Arc::clone(&self.revision),
            Arc::clone(&self.publish_lock),
        );
        if let Some(voice_events) = self
            .voice_events_rx
            .lock()
            .expect("voice event receiver mutex is not poisoned")
            .take()
        {
            tokio::spawn(voice::run_voice_runtime(
                voice_events,
                voice_events_tx.clone(),
                voice_status_publisher,
            ));
        }

        tokio::spawn(async move {
            let runtime = GatewayRuntime {
                effects_tx,
                snapshots_tx,
                state,
                revision,
                gateway_session_id,
                publish_lock,
                voice_events_tx,
            };
            run_gateway(token, gateway_commands, runtime).await;
        })
    }

    pub fn request_guild_members(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::RequestGuildMembers {
                guild_id,
                query: String::new(),
                limit: 0,
                presences: true,
                nonce: None,
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn request_guild_members_by_ids(
        &self,
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
    ) -> std::result::Result<(), String> {
        if user_ids.is_empty() {
            return Ok(());
        }
        self.gateway_commands_tx
            .send(GatewayCommand::RequestGuildMembersByIds {
                guild_id,
                user_ids,
                presences: false,
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn search_guild_members(
        &self,
        guild_id: Id<GuildMarker>,
        query: String,
        limit: u16,
    ) -> std::result::Result<(), String> {
        let Some(query) = normalize_member_search_query(&query) else {
            return Ok(());
        };
        let limit = limit.min(MEMBER_SEARCH_MAX_LIMIT);
        let nonce = format!("mention-ac-{}-{:016x}", guild_id.get(), query_hash(&query));
        self.gateway_commands_tx
            .send(GatewayCommand::RequestGuildMembers {
                guild_id,
                query,
                limit,
                presences: true,
                nonce: Some(nonce),
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn subscribe_direct_message(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::SubscribeDirectMessage { channel_id })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn subscribe_guild_channel(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::SubscribeGuildChannel {
                guild_id,
                channel_id,
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn update_member_list_subscription(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::UpdateMemberListSubscription {
                guild_id,
                channel_id,
                ranges,
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn update_voice_state(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
    ) -> std::result::Result<(), String> {
        let mut requested = self
            .requested_voice
            .write()
            .expect("requested voice lock is not poisoned");
        if voice_state_request_is_duplicate(*requested, guild_id, channel_id, self_mute, self_deaf)
        {
            return Ok(());
        }
        if let Some(channel_id) = channel_id {
            let requested_same_channel = requested
                .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                .is_some();
            if !requested_same_channel {
                let state = self
                    .state
                    .read()
                    .expect("discord state lock is not poisoned");
                let current_same_channel = state
                    .current_user_voice_connection()
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .is_some();
                if !current_same_channel
                    && let Some(channel) = state.channel(channel_id)
                    && !state.can_connect_voice_channel(channel)
                {
                    return Err("cannot connect to voice channel".to_owned());
                }
            }
        }

        let result = self
            .gateway_commands_tx
            .send(GatewayCommand::UpdateVoiceState {
                guild_id,
                channel_id,
                self_mute,
                self_deaf,
            })
            .map_err(|_| "gateway command channel closed".to_owned());
        if result.is_ok() {
            if let Some(channel_id) = channel_id {
                let allow_microphone_transmit = requested
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .is_some_and(|voice| voice.allow_microphone_transmit);
                let microphone_sensitivity = requested
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .map(|voice| voice.microphone_sensitivity)
                    .unwrap_or_default();
                let microphone_volume = requested
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .map(|voice| voice.microphone_volume)
                    .unwrap_or_default();
                let voice_output_volume = requested
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .map(|voice| voice.voice_output_volume)
                    .unwrap_or_default();
                let voice = CurrentVoiceConnectionState {
                    guild_id,
                    channel_id,
                    self_mute,
                    self_deaf,
                    allow_microphone_transmit,
                    microphone_sensitivity,
                    microphone_volume,
                    voice_output_volume,
                };
                *requested = Some(voice);
                let _ = self
                    .voice_events_tx
                    .send(VoiceRuntimeEvent::Requested(Some(voice)));
            } else if requested.is_some_and(|voice| voice.guild_id == guild_id) {
                *requested = None;
                let _ = self
                    .voice_events_tx
                    .send(VoiceRuntimeEvent::Requested(None));
            }
        }
        result
    }

    pub fn update_voice_capture_permission(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        allow_microphone_transmit: bool,
        microphone_sensitivity: MicrophoneSensitivityDb,
        microphone_volume: VoiceVolumePercent,
        voice_output_volume: VoiceVolumePercent,
    ) {
        let mut requested = self
            .requested_voice
            .write()
            .expect("requested voice lock is not poisoned");
        let Some(mut voice) = *requested else {
            return;
        };
        if voice.guild_id != guild_id || voice.channel_id != channel_id {
            return;
        }
        if voice.allow_microphone_transmit == allow_microphone_transmit
            && voice.microphone_sensitivity == microphone_sensitivity
            && voice.microphone_volume == microphone_volume
            && voice.voice_output_volume == voice_output_volume
        {
            return;
        }

        voice.allow_microphone_transmit = allow_microphone_transmit;
        voice.microphone_sensitivity = microphone_sensitivity;
        voice.microphone_volume = microphone_volume;
        voice.voice_output_volume = voice_output_volume;
        *requested = Some(voice);
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::Requested(Some(voice)));
    }

    pub fn current_or_requested_voice_connection(&self) -> Option<CurrentVoiceConnectionState> {
        self.state
            .read()
            .expect("discord state lock is not poisoned")
            .current_user_voice_connection()
            .or_else(|| {
                *self
                    .requested_voice
                    .read()
                    .expect("requested voice lock is not poisoned")
            })
    }

    pub fn requested_voice_connection(&self) -> Option<CurrentVoiceConnectionState> {
        *self
            .requested_voice
            .read()
            .expect("requested voice lock is not poisoned")
    }

    pub fn shutdown_gateway(&self) -> std::result::Result<(), String> {
        let _ = self.voice_events_tx.send(VoiceRuntimeEvent::Shutdown);
        self.gateway_commands_tx
            .send(GatewayCommand::Shutdown)
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub async fn prime_rest_pool(&self) -> Result<()> {
        self.rest.prime_connection_pool().await
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<Id<MessageMarker>>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<MessageInfo> {
        self.ensure_can_send_message(channel_id, attachments)?;
        self.rest
            .send_message(channel_id, content, reply_to, attachments)
            .await
    }

    fn ensure_can_send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let Some(channel) = state.channel(channel_id) else {
            return Ok(());
        };
        if !state.can_send_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot send message in channel".to_owned(),
            ));
        }
        if !attachments.is_empty() && !state.can_attach_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot attach files in channel".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.rest
            .edit_message(channel_id, message_id, content)
            .await
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.rest.delete_message(channel_id, message_id).await
    }

    pub async fn leave_guild(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        self.rest.leave_guild(guild_id).await
    }

    pub async fn load_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Result<Option<Vec<ApplicationCommandInfo>>> {
        if !self.begin_application_command_request(guild_id) {
            return Ok(None);
        }
        let result = self.rest.load_application_commands(guild_id).await;
        match result {
            Ok(commands) => Ok(Some(
                self.record_application_commands_for_tui(guild_id, commands),
            )),
            Err(error) => {
                self.clear_application_command_request(guild_id);
                Err(error)
            }
        }
    }

    pub async fn run_application_command(
        &self,
        invocation: &ApplicationCommandInvocation,
    ) -> Result<()> {
        let session_id = self
            .gateway_session_id
            .read()
            .expect("gateway session id lock is not poisoned")
            .clone()
            .ok_or_else(|| AppError::DiscordRequest("gateway session is not ready".to_owned()))?;
        let interaction = self.application_command_interaction(invocation)?;
        self.rest
            .run_application_command(&interaction, &session_id)
            .await
    }

    fn application_command_interaction(
        &self,
        invocation: &ApplicationCommandInvocation,
    ) -> Result<super::ApplicationCommandInteraction> {
        let commands = self
            .application_commands
            .lock()
            .expect("application command cache lock is not poisoned");
        let command = commands
            .get(&invocation.guild_id)
            .and_then(|commands| {
                commands
                    .iter()
                    .find(|command| command.name == invocation.command_name)
            })
            .ok_or_else(|| {
                AppError::DiscordRequest(format!(
                    "application command {} is not loaded",
                    invocation.command_name
                ))
            })?;
        application_command_interaction_from_invocation(invocation, command).ok_or_else(|| {
            AppError::DiscordRequest(format!(
                "application command {} options are incomplete or invalid",
                invocation.command_name
            ))
        })
    }

    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.rest.ack_channel(channel_id, message_id).await
    }

    pub async fn set_guild_muted(
        &self,
        guild_id: Id<GuildMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
            .await
    }

    pub async fn set_channel_muted(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_channel_muted(
                guild_id,
                channel_id,
                muted,
                mute_end_time,
                selected_time_window,
            )
            .await
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        self.rest.ack_channels(targets).await
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest
            .load_message_history(channel_id, before, limit)
            .await
    }

    pub async fn load_message_history_around(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest
            .load_message_history_around(channel_id, message_id, limit)
            .await
    }

    pub async fn load_message_history_after(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest
            .load_message_history_after(channel_id, message_id, limit)
            .await
    }

    pub async fn search_messages(
        &self,
        query: crate::discord::MessageSearchQuery,
    ) -> Result<crate::discord::MessageSearchPage> {
        self.rest.search_messages(query).await
    }

    pub async fn load_forum_posts(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) -> Result<ForumPostPage> {
        self.rest
            .load_forum_posts(guild_id, channel_id, archive_state, offset)
            .await
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.rest.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.rest
            .remove_current_user_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_reaction_users(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<Vec<ReactionUserInfo>> {
        self.rest
            .load_reaction_users(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        self.rest.load_pinned_messages(channel_id).await
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        self.rest
            .set_message_pinned(channel_id, message_id, pinned)
            .await
    }

    pub async fn vote_poll(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) -> Result<()> {
        self.rest
            .vote_poll(channel_id, message_id, answer_ids)
            .await
    }

    pub async fn load_user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        is_self: bool,
    ) -> Result<UserProfileInfo> {
        self.rest
            .load_user_profile(user_id, guild_id, is_self)
            .await
    }

    pub async fn load_user_note(&self, user_id: Id<UserMarker>) -> Result<Option<String>> {
        self.rest.load_user_note(user_id).await
    }
}

pub(super) async fn publish_app_event(
    effects_tx: &mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: &watch::Sender<SnapshotRevision>,
    state: &Arc<RwLock<DiscordState>>,
    revision: &Arc<RwLock<SnapshotRevision>>,
    publish_lock: &Arc<AsyncMutex<()>>,
    event: &AppEvent,
) {
    let mutates_state = event.mutates_discord_state();
    let needs_effect_delivery = event.needs_effect_delivery();
    let voice_sound = {
        let state = state.read().expect("discord state lock is not poisoned");
        match event {
            AppEvent::VoiceStateUpdate { state: voice_state } => {
                state.voice_sound_for_state_update(voice_state)
            }
            _ => None,
        }
    };

    let event_revision: SnapshotRevision;
    {
        let _publish_guard = publish_lock.lock().await;

        event_revision = if mutates_state {
            let next_revision = {
                let mut state = state.write().expect("discord state lock is not poisoned");
                let detail_revision_before = matches!(event, AppEvent::MessageCreate { .. })
                    .then(|| state.detail_revision_signature());
                state.apply_event(event);
                let mut revision = revision
                    .write()
                    .expect("snapshot revision lock is not poisoned");
                if let Some(mut areas) = DiscordState::snapshot_areas_for_event(event) {
                    if let Some(before) = detail_revision_before {
                        areas.detail = state.detail_revision_signature() != before;
                    }
                    *revision = revision.advance(areas);
                }
                *revision
            };
            let _ = snapshots_tx.send(next_revision);
            next_revision
        } else {
            *revision
                .read()
                .expect("snapshot revision lock is not poisoned")
        };

        if needs_effect_delivery {
            let _ = effects_tx
                .send(SequencedAppEvent {
                    revision: event_revision.global,
                    event: event.clone(),
                })
                .await;
        }
        if let Some(kind) = voice_sound {
            let _ = effects_tx
                .send(SequencedAppEvent {
                    revision: event_revision.global,
                    event: AppEvent::VoiceSound { kind },
                })
                .await;
        }
    }
}

pub(crate) fn validate_token_header(token: &str) -> Result<()> {
    HeaderValue::from_str(token)
        .map_err(|source| AppError::InvalidDiscordTokenHeader { source })?;
    Ok(())
}

fn voice_state_request_is_duplicate(
    requested: Option<CurrentVoiceConnectionState>,
    guild_id: Id<GuildMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    self_mute: bool,
    self_deaf: bool,
) -> bool {
    match (requested, channel_id) {
        (Some(voice), Some(channel_id)) => {
            voice.guild_id == guild_id
                && voice.channel_id == channel_id
                && voice.self_mute == self_mute
                && voice.self_deaf == self_deaf
        }
        (Some(voice), None) => voice.guild_id != guild_id,
        (None, None) => true,
        (None, Some(_)) => false,
    }
}

fn normalize_member_search_query(query: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut count = 0usize;
    for ch in query.trim().chars() {
        for lowered in ch.to_lowercase() {
            if count >= MEMBER_SEARCH_MAX_QUERY_CHARS {
                return (normalized.chars().count() >= MEMBER_SEARCH_MIN_QUERY_CHARS)
                    .then_some(normalized);
            }
            normalized.push(lowered);
            count += 1;
        }
    }
    (normalized.chars().count() >= MEMBER_SEARCH_MIN_QUERY_CHARS).then_some(normalized)
}

fn query_hash(query: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestState {
    Requested,
    Loaded,
}

impl DiscordClient {
    fn begin_application_command_request(&self, guild_id: Option<Id<GuildMarker>>) -> bool {
        let mut requests = self
            .application_command_requests
            .lock()
            .expect("application command request lock is not poisoned");
        if requests.contains_key(&guild_id) {
            return false;
        }
        requests.insert(guild_id, RequestState::Requested);
        true
    }

    fn record_application_commands_loaded(&self, guild_id: Option<Id<GuildMarker>>) {
        self.application_command_requests
            .lock()
            .expect("application command request lock is not poisoned")
            .insert(guild_id, RequestState::Loaded);
    }

    fn record_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        commands: Vec<ApplicationCommandInfo>,
    ) {
        self.application_commands
            .lock()
            .expect("application command cache lock is not poisoned")
            .insert(guild_id, commands);
    }

    fn record_application_commands_for_tui(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        commands: Vec<ApplicationCommandInfo>,
    ) -> Vec<ApplicationCommandInfo> {
        self.record_application_commands(guild_id, commands.clone());
        commands
            .into_iter()
            .map(ApplicationCommandInfo::without_raw)
            .collect()
    }

    fn clear_application_command_request(&self, guild_id: Option<Id<GuildMarker>>) {
        self.application_command_requests
            .lock()
            .expect("application command request lock is not poisoned")
            .remove(&guild_id);
    }
}

#[cfg(test)]
mod tests;
