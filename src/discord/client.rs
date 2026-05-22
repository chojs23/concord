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
mod tests {
    use crate::{
        AppError,
        discord::{
            AppEvent, ChannelInfo, FriendStatus, MemberInfo, MentionInfo, MessageAttachmentUpload,
            MessageKind, RoleInfo, UserProfileInfo, VoiceSoundKind, VoiceStateInfo,
            gateway::GatewayCommand,
            ids::{
                Id,
                marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
            },
        },
    };
    use serde_json::{Value, json};

    use super::{
        DiscordClient, MEMBER_SEARCH_MAX_LIMIT, MEMBER_SEARCH_MAX_QUERY_CHARS,
        validate_token_header,
    };

    #[tokio::test]
    async fn publish_event_sends_matching_snapshot_and_effect_revisions() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client
            .publish_event(AppEvent::MessageHistoryLoaded {
                channel_id: Id::new(1),
                before: None,
                messages: Vec::new(),
            })
            .await;

        snapshots.changed().await.expect("snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        let effect = effects.recv().await.expect("effect is published");
        let state_snapshot = client.current_discord_snapshot();

        assert_eq!(snapshot.global, 1);
        assert_eq!(snapshot.message, 1);
        assert_eq!(snapshot.navigation, 0);
        assert_eq!(snapshot.detail, 0);
        assert_eq!(effect.revision, 1);
        assert_eq!(state_snapshot.revision.global, 1);
        assert_eq!(state_snapshot.revision.message, 1);
    }

    #[tokio::test]
    async fn message_create_publishes_matching_snapshot_and_effect_revisions() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client.publish_event(message_create_event(1)).await;

        snapshots.changed().await.expect("snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        let effect = effects.recv().await.expect("effect is published");

        assert_eq!(snapshot.global, 1);
        assert_eq!(snapshot.navigation, 1);
        assert_eq!(snapshot.message, 1);
        assert_eq!(snapshot.detail, 0);
        assert_eq!(effect.revision, 1);
        assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
    }

    #[tokio::test]
    async fn current_user_message_create_advances_detail_revision() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client
            .publish_event(AppEvent::Ready {
                user: "neo".to_owned(),
                user_id: Some(Id::new(99)),
            })
            .await;
        snapshots
            .changed()
            .await
            .expect("ready snapshot is published");
        drop(snapshots.borrow_and_update());

        client.publish_event(message_create_event(1)).await;

        snapshots
            .changed()
            .await
            .expect("message snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        let effect = effects.recv().await.expect("message effect is published");

        assert_eq!(snapshot.global, 2);
        assert_eq!(snapshot.navigation, 2);
        assert_eq!(snapshot.message, 2);
        assert_eq!(snapshot.detail, 2);
        assert_eq!(effect.revision, 2);
        assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
    }

    #[tokio::test]
    async fn mentioned_message_create_advances_detail_revision() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client
            .publish_event(AppEvent::Ready {
                user: "neo".to_owned(),
                user_id: Some(Id::new(42)),
            })
            .await;
        snapshots
            .changed()
            .await
            .expect("ready snapshot is published");
        drop(snapshots.borrow_and_update());

        let mut event = message_create_event(1);
        if let AppEvent::MessageCreate {
            content, mentions, ..
        } = &mut event
        {
            *content = Some("hello <@42>".to_owned());
            mentions.push(MentionInfo {
                user_id: Id::new(42),
                guild_nick: None,
                display_name: "neo".to_owned(),
            });
        }
        client.publish_event(event).await;

        snapshots
            .changed()
            .await
            .expect("message snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        let effect = effects.recv().await.expect("message effect is published");

        assert_eq!(snapshot.global, 2);
        assert_eq!(snapshot.navigation, 2);
        assert_eq!(snapshot.message, 2);
        assert_eq!(snapshot.detail, 2);
        assert_eq!(effect.revision, 2);
        assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
    }

    #[tokio::test]
    async fn normal_channel_upsert_updates_snapshot_without_effect_delivery() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client.publish_event(channel_upsert_event()).await;

        snapshots.changed().await.expect("snapshot is published");
        let snapshot = *snapshots.borrow_and_update();

        assert_eq!(snapshot.global, 1);
        assert_eq!(snapshot.navigation, 1);
        assert_eq!(snapshot.message, 1);
        assert_eq!(snapshot.detail, 1);
        assert!(matches!(
            effects.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn thread_channel_upsert_is_delivered_as_effect_for_tui_derived_state() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client.publish_event(thread_channel_upsert_event()).await;

        snapshots.changed().await.expect("snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        let effect = effects.recv().await.expect("effect is published");

        assert_eq!(snapshot.global, 1);
        assert_eq!(snapshot.navigation, 1);
        assert_eq!(snapshot.message, 1);
        assert_eq!(snapshot.detail, 1);
        assert_eq!(effect.revision, 1);
        assert!(matches!(effect.event, AppEvent::ChannelUpsert(_)));
    }

    #[tokio::test]
    async fn concurrent_publishers_emit_ordered_effect_revisions() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        let mut tasks = Vec::new();
        for index in 0..32_u64 {
            let client = client.clone();
            tasks.push(tokio::spawn(async move {
                client
                    .publish_event(AppEvent::MessageHistoryLoaded {
                        channel_id: Id::new(index + 1),
                        before: None,
                        messages: Vec::new(),
                    })
                    .await;
            }));
        }

        for task in tasks {
            task.await.expect("publish task completes");
        }

        for expected_revision in 1..=32 {
            let effect = effects.recv().await.expect("effect is published");
            assert_eq!(effect.revision, expected_revision);
        }

        snapshots.changed().await.expect("snapshot is published");
        let snapshot = *snapshots.borrow_and_update();
        assert_eq!(snapshot.global, 32);
        assert_eq!(snapshot.message, 32);
        assert_eq!(client.current_discord_snapshot().revision.global, 32);
    }

    #[tokio::test]
    async fn effect_only_events_are_delivered_without_snapshots() {
        for event in [
            AppEvent::GatewayError {
                message: "boom".to_owned(),
            },
            AppEvent::ActivateChannel {
                channel_id: Id::new(42),
            },
        ] {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let client =
                DiscordClient::new("test-token".to_owned()).expect("token is valid header");
            let mut effects = client.take_effects();
            let snapshots = client.subscribe_snapshots();

            client.publish_event(event.clone()).await;

            let effect = effects.recv().await.expect("effect is published");
            assert_eq!(effect.revision, 0);
            assert_eq!(format!("{:?}", effect.event), format!("{event:?}"));
            assert!(!snapshots.has_changed().expect("snapshot stream is open"));
        }
    }

    #[test]
    fn requested_voice_state_tracks_shutdown_fallback() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

        client
            .update_voice_state(Id::new(1), Some(Id::new(10)), true, false)
            .expect("gateway command should queue");
        let voice = client
            .requested_voice_connection()
            .expect("requested voice state should be tracked");

        assert_eq!(voice.guild_id, Id::new(1));
        assert_eq!(voice.channel_id, Id::new(10));
        assert!(voice.self_mute);
        assert!(!voice.self_deaf);

        client
            .update_voice_state(Id::new(1), None, false, false)
            .expect("gateway command should queue");

        assert_eq!(client.requested_voice_connection(), None);
    }

    #[test]
    fn requested_voice_state_skips_duplicate_gateway_updates() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut gateway_commands = client
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway commands can be taken once");

        client
            .update_voice_state(Id::new(1), Some(Id::new(10)), true, false)
            .expect("initial join should queue");
        assert_voice_update(
            &mut gateway_commands,
            Id::new(1),
            Some(Id::new(10)),
            true,
            false,
        );

        client
            .update_voice_state(Id::new(1), Some(Id::new(10)), true, false)
            .expect("duplicate join is ignored without closing channel");
        assert!(matches!(
            gateway_commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        client
            .update_voice_state(Id::new(1), Some(Id::new(10)), false, false)
            .expect("mute change should queue");
        assert_voice_update(
            &mut gateway_commands,
            Id::new(1),
            Some(Id::new(10)),
            false,
            false,
        );

        client
            .update_voice_state(Id::new(1), None, false, false)
            .expect("leave should queue");
        assert_voice_update(&mut gateway_commands, Id::new(1), None, false, false);

        client
            .update_voice_state(Id::new(1), None, false, false)
            .expect("duplicate leave is ignored without closing channel");
        assert!(matches!(
            gateway_commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn send_message_rejects_explicit_missing_send_permission() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL).await;

        let error = client
            .send_message(Id::new(2), "hello", None, &[])
            .await
            .expect_err("missing SEND_MESSAGES should stop before REST");

        assert!(matches!(
            error,
            AppError::DiscordRequest(message) if message == "cannot send message in channel"
        ));
    }

    #[tokio::test]
    async fn send_message_rejects_explicit_missing_attach_permission() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | SEND_MESSAGES).await;
        let attachment = MessageAttachmentUpload::from_bytes("note.txt".to_owned(), b"x".to_vec());

        let error = client
            .send_message(Id::new(2), "hello", None, &[attachment])
            .await
            .expect_err("missing ATTACH_FILES should stop before REST");

        assert!(matches!(
            error,
            AppError::DiscordRequest(message) if message == "cannot attach files in channel"
        ));
    }

    #[test]
    fn send_message_guard_allows_unknown_channels_while_state_hydrates() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

        client
            .ensure_can_send_message(Id::new(99), &[])
            .expect("unknown channel should stay optimistic");
    }

    #[tokio::test]
    async fn voice_join_rejects_explicit_missing_connect_permission() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL).await;
        let mut gateway_commands = client
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway commands can be taken once");

        let error = client
            .update_voice_state(Id::new(1), Some(Id::new(2)), false, false)
            .expect_err("missing CONNECT should stop before gateway command");

        assert_eq!(error, "cannot connect to voice channel");
        assert_eq!(client.requested_voice_connection(), None);
        assert!(matches!(
            gateway_commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn voice_state_update_allows_current_channel_mute_change_without_connect_permission() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL).await;
        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: VoiceStateInfo {
                    guild_id: Id::new(1),
                    channel_id: Some(Id::new(2)),
                    user_id: Id::new(10),
                    session_id: Some("current-voice-session".to_owned()),
                    member: None,
                    deaf: false,
                    mute: false,
                    self_deaf: false,
                    self_mute: false,
                    self_stream: false,
                },
            })
            .await;
        let mut gateway_commands = client
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway commands can be taken once");

        client
            .update_voice_state(Id::new(1), Some(Id::new(2)), true, true)
            .expect("current channel mute and deaf changes should still queue");

        assert_voice_update(
            &mut gateway_commands,
            Id::new(1),
            Some(Id::new(2)),
            true,
            true,
        );
    }

    #[test]
    fn application_command_requests_are_deduped_until_loaded() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let guild_id = Some(Id::new(1));

        assert!(client.begin_application_command_request(guild_id));
        assert!(!client.begin_application_command_request(guild_id));

        client.record_application_commands_loaded(guild_id);
        assert!(!client.begin_application_command_request(guild_id));

        let retry_guild_id = Some(Id::new(2));
        assert!(client.begin_application_command_request(retry_guild_id));
        assert!(!client.begin_application_command_request(retry_guild_id));
        client.clear_application_command_request(retry_guild_id);
        assert!(client.begin_application_command_request(retry_guild_id));

        assert!(client.begin_application_command_request(None));
        assert!(!client.begin_application_command_request(None));
        client.record_application_commands_loaded(None);
        assert!(!client.begin_application_command_request(None));
    }

    #[test]
    fn application_command_metadata_keeps_raw_backend_owned() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let guild_id = Some(Id::new(1));
        let command = application_command("echo");

        let tui_commands = client.record_application_commands_for_tui(guild_id, vec![command]);

        assert_eq!(tui_commands[0].raw, Value::Null);
        let commands = client
            .application_commands
            .lock()
            .expect("application command cache lock is not poisoned");
        assert_eq!(
            commands.get(&guild_id).expect("backend cache")[0].raw["name"],
            "echo"
        );
    }

    #[tokio::test]
    async fn user_profile_requests_are_gated_by_backend_lifecycle_and_cache() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let user_id = Id::new(10);
        let guild_id = Some(Id::new(1));

        assert_eq!(
            client.next_user_profile_request(user_id, guild_id),
            Some((user_id, guild_id, false))
        );
        assert_eq!(client.next_user_profile_request(user_id, guild_id), None);

        client
            .publish_event(AppEvent::UserProfileLoadFailed {
                user_id,
                guild_id,
                message: "temporary failure".to_owned(),
            })
            .await;
        assert_eq!(
            client.next_user_profile_request(user_id, guild_id),
            Some((user_id, guild_id, false))
        );

        client
            .publish_event(AppEvent::UserProfileLoaded {
                guild_id,
                profile: user_profile(user_id),
            })
            .await;
        assert_eq!(client.next_user_profile_request(user_id, guild_id), None);
    }

    #[tokio::test]
    async fn user_note_requests_are_gated_by_backend_lifecycle_and_cache() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let user_id = Id::new(10);

        assert_eq!(client.next_user_note_request(user_id), Some(user_id));
        assert_eq!(client.next_user_note_request(user_id), None);

        client.mark_user_note_request_failed(user_id);
        assert_eq!(client.next_user_note_request(user_id), Some(user_id));

        client
            .publish_event(AppEvent::UserNoteLoaded {
                user_id,
                note: Some("note".to_owned()),
            })
            .await;
        assert_eq!(client.next_user_note_request(user_id), None);
    }

    #[test]
    fn guild_member_search_validates_query_and_caps_limit() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut gateway_commands = client
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway commands can be taken once");

        client
            .search_guild_members(Id::new(1), " a ".to_owned(), 10)
            .expect("short search is ignored without closing channel");
        assert!(matches!(
            gateway_commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let long_query = "İ".repeat(MEMBER_SEARCH_MAX_QUERY_CHARS + 10);
        client
            .search_guild_members(Id::new(1), long_query, 99)
            .expect("valid search should queue");

        let command = gateway_commands
            .try_recv()
            .expect("search command should be queued");
        let GatewayCommand::RequestGuildMembers {
            guild_id,
            query,
            limit,
            presences,
            nonce,
        } = command
        else {
            panic!("expected guild member search command");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(query.chars().count(), MEMBER_SEARCH_MAX_QUERY_CHARS);
        assert_eq!(limit, MEMBER_SEARCH_MAX_LIMIT);
        assert!(presences);
        let nonce = nonce.expect("member search should include nonce");
        assert!(nonce.starts_with("mention-ac-1-"));
        assert!(!nonce.contains(&query));
    }

    #[test]
    fn guild_member_request_by_ids_queues_gateway_command() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut gateway_commands = client
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway commands can be taken once");

        client
            .request_guild_members_by_ids(Id::new(1), Vec::new())
            .expect("empty request is ignored without closing channel");
        assert!(matches!(
            gateway_commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        client
            .request_guild_members_by_ids(Id::new(1), vec![Id::new(20), Id::new(30)])
            .expect("valid request should queue");

        let command = gateway_commands
            .try_recv()
            .expect("member request should be queued");
        let GatewayCommand::RequestGuildMembersByIds {
            guild_id,
            user_ids,
            presences,
        } = command
        else {
            panic!("expected guild member id request command");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(user_ids, vec![Id::new(20), Id::new(30)]);
        assert!(!presences);
    }

    #[tokio::test]
    async fn requested_voice_state_ignores_observed_other_client_voice() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

        client
            .publish_event(AppEvent::Ready {
                user: "me".to_owned(),
                user_id: Some(Id::new(10)),
            })
            .await;
        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: VoiceStateInfo {
                    guild_id: Id::new(1),
                    channel_id: Some(Id::new(10)),
                    user_id: Id::new(10),
                    session_id: Some("other-client-voice-session".to_owned()),
                    member: None,
                    deaf: false,
                    mute: false,
                    self_deaf: false,
                    self_mute: false,
                    self_stream: false,
                },
            })
            .await;

        assert_eq!(client.requested_voice_connection(), None);
        assert!(client.current_or_requested_voice_connection().is_some());
    }

    #[tokio::test]
    async fn voice_state_transitions_publish_join_and_leave_sound_effects() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();

        client
            .publish_event(AppEvent::Ready {
                user: "me".to_owned(),
                user_id: Some(Id::new(10)),
            })
            .await;
        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: voice_state(10, Some(11)),
            })
            .await;
        assert_voice_sound(&mut effects, VoiceSoundKind::Join).await;

        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: voice_state(20, Some(11)),
            })
            .await;
        assert_voice_sound(&mut effects, VoiceSoundKind::Join).await;

        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: voice_state(20, None),
            })
            .await;
        assert_voice_sound(&mut effects, VoiceSoundKind::Leave).await;

        client
            .publish_event(AppEvent::VoiceStateUpdate {
                state: voice_state(10, None),
            })
            .await;
        assert_voice_sound(&mut effects, VoiceSoundKind::Leave).await;
    }

    #[test]
    fn validates_token_header_values() {
        validate_token_header("raw-user-token").expect("raw user token must be accepted");
        validate_token_header("invalid\nuser-token")
            .expect_err("newlines are not valid authorization header values");
    }

    fn message_create_event(message_id: u64) -> AppEvent {
        AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_is_bot: false,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            interaction: None,
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {message_id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }

    const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
    const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;

    async fn publish_permission_fixture(
        client: &DiscordClient,
        channel_kind: &str,
        everyone_permissions: u64,
    ) {
        client
            .publish_event(AppEvent::Ready {
                user: "me".to_owned(),
                user_id: Some(Id::new(10)),
            })
            .await;
        client
            .publish_event(AppEvent::GuildCreate {
                guild_id: Id::new(1),
                name: "guild".to_owned(),
                member_count: Some(1),
                owner_id: Some(Id::new(99)),
                channels: vec![permission_fixture_channel(
                    Id::new(1),
                    Id::new(2),
                    channel_kind,
                )],
                members: vec![permission_fixture_member(Id::new(10))],
                presences: Vec::new(),
                roles: vec![permission_fixture_role(
                    Id::new(1),
                    "@everyone",
                    everyone_permissions,
                )],
                emojis: Vec::new(),
            })
            .await;
    }

    fn permission_fixture_channel(
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        kind: &str,
    ) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "guarded".to_owned(),
            kind: kind.to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }

    fn permission_fixture_member(user_id: Id<UserMarker>) -> MemberInfo {
        MemberInfo {
            user_id,
            display_name: "me".to_owned(),
            username: Some("me".to_owned()),
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }
    }

    fn permission_fixture_role(id: Id<RoleMarker>, name: &str, permissions: u64) -> RoleInfo {
        RoleInfo {
            id,
            name: name.to_owned(),
            color: None,
            position: 0,
            hoist: false,
            permissions,
        }
    }

    fn user_profile(user_id: Id<UserMarker>) -> UserProfileInfo {
        UserProfileInfo {
            user_id,
            username: "neo".to_owned(),
            global_name: None,
            guild_nick: None,
            role_ids: Vec::new(),
            avatar_url: None,
            bio: None,
            pronouns: None,
            mutual_guilds: Vec::new(),
            mutual_friends_count: 0,
            friend_status: FriendStatus::None,
            note: None,
        }
    }

    fn application_command(name: &str) -> crate::discord::ApplicationCommandInfo {
        crate::discord::ApplicationCommandInfo {
            id: Id::new(100),
            application_id: Id::new(200),
            version: "1".to_owned(),
            name: name.to_owned(),
            application_name: Some("TestBot".to_owned()),
            description: format!("{name} command"),
            options: Vec::new(),
            raw: json!({
                "id": "100",
                "application_id": "200",
                "version": "1",
                "name": name,
            }),
        }
    }

    fn channel_upsert_event() -> AppEvent {
        AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            parent_id: Some(Id::new(10)),
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
    }

    fn voice_state(user_id: u64, channel_id: Option<u64>) -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Id::new(1),
            channel_id: channel_id.map(Id::new),
            user_id: Id::new(user_id),
            session_id: None,
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }

    async fn assert_voice_sound(
        effects: &mut tokio::sync::mpsc::Receiver<crate::discord::SequencedAppEvent>,
        expected: VoiceSoundKind,
    ) {
        let effect = effects
            .recv()
            .await
            .expect("voice sound effect is published");
        assert!(matches!(effect.event, AppEvent::VoiceSound { kind } if kind == expected));
    }

    fn assert_voice_update(
        gateway_commands: &mut tokio::sync::mpsc::UnboundedReceiver<GatewayCommand>,
        expected_guild_id: Id<crate::discord::ids::marker::GuildMarker>,
        expected_channel_id: Option<Id<crate::discord::ids::marker::ChannelMarker>>,
        expected_self_mute: bool,
        expected_self_deaf: bool,
    ) {
        let command = gateway_commands
            .try_recv()
            .expect("voice command should be queued");
        let GatewayCommand::UpdateVoiceState {
            guild_id,
            channel_id,
            self_mute,
            self_deaf,
        } = command
        else {
            panic!("expected voice update command");
        };

        assert_eq!(guild_id, expected_guild_id);
        assert_eq!(channel_id, expected_channel_id);
        assert_eq!(self_mute, expected_self_mute);
        assert_eq!(self_deaf, expected_self_deaf);
    }

    fn thread_channel_upsert_event() -> AppEvent {
        AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(3),
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "new-thread".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
    }
}
