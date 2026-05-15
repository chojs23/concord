use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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

use crate::{AppError, Result, logging};

use super::{
    MessageAttachmentUpload, MessageInfo, ReactionEmoji, ReactionUserInfo, UserProfileInfo,
    commands::ForumPostArchiveState,
    events::{AppEvent, SequencedAppEvent},
    gateway::{GatewayCommand, run_gateway},
    rest::{DiscordRest, ForumPostPage},
    state::{DiscordSnapshot, DiscordState, SnapshotRevision},
};

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    effects_tx: mpsc::Sender<SequencedAppEvent>,
    effects_rx: Arc<Mutex<Option<mpsc::Receiver<SequencedAppEvent>>>>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    revision: Arc<RwLock<SnapshotRevision>>,
    publish_lock: Arc<AsyncMutex<()>>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    gateway_commands_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<GatewayCommand>>>>,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self> {
        validate_token_header(&token)?;
        let rest = DiscordRest::new(token.clone());
        let initial_state = DiscordState::default();
        let (effects_tx, effects_rx) = mpsc::channel(4096);
        let (snapshots_tx, _) = watch::channel(SnapshotRevision::default());
        let (gateway_commands_tx, gateway_commands_rx) = mpsc::unbounded_channel();

        Ok(Self {
            token,
            rest,
            effects_tx,
            effects_rx: Arc::new(Mutex::new(Some(effects_rx))),
            snapshots_tx,
            state: Arc::new(RwLock::new(initial_state)),
            revision: Arc::new(RwLock::new(SnapshotRevision::default())),
            publish_lock: Arc::new(AsyncMutex::new(())),
            gateway_commands_tx,
            gateway_commands_rx: Arc::new(Mutex::new(Some(gateway_commands_rx))),
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
        let started = Instant::now();
        if logging::debug_logging_enabled() {
            let state = self
                .state
                .read()
                .expect("discord state lock is not poisoned");
            let lock_duration = started.elapsed();
            let revision = *self
                .revision
                .read()
                .expect("snapshot revision lock is not poisoned");
            let counts = state.cache_counts();
            let clone_started = Instant::now();
            let snapshot = state.snapshot(revision);
            let clone_duration = clone_started.elapsed();
            let total_duration = started.elapsed();
            drop(state);
            logging::debug(
                "snapshot",
                format!(
                    "op=current_discord_snapshot revision={} navigation_revision={} \
                     message_revision={} detail_revision={} lock_ms={:.2} \
                     clone_ms={:.2} total_ms={:.2} {}",
                    revision.global,
                    revision.navigation,
                    revision.message,
                    revision.detail,
                    lock_duration.as_secs_f64() * 1_000.0,
                    clone_duration.as_secs_f64() * 1_000.0,
                    total_duration.as_secs_f64() * 1_000.0,
                    counts.log_fields(),
                ),
            );
            snapshot
        } else {
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
    }

    pub async fn publish_event(&self, event: AppEvent) {
        publish_app_event(
            &self.effects_tx,
            &self.snapshots_tx,
            &self.state,
            &self.revision,
            &self.publish_lock,
            &event,
        )
        .await;
    }

    pub fn start_gateway(&self) -> JoinHandle<()> {
        let token = self.token.clone();
        let effects_tx = self.effects_tx.clone();
        let snapshots_tx = self.snapshots_tx.clone();
        let state = Arc::clone(&self.state);
        let revision = Arc::clone(&self.revision);
        let publish_lock = Arc::clone(&self.publish_lock);
        let gateway_commands = self
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway can only be started once");

        tokio::spawn(async move {
            run_gateway(
                token,
                effects_tx,
                snapshots_tx,
                gateway_commands,
                state,
                revision,
                publish_lock,
            )
            .await;
        })
    }

    pub fn request_guild_members(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::RequestGuildMembers { guild_id })
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

    pub fn update_presence(&self, status: &'static str) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::UpdatePresence { status })
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
        self.rest
            .send_message(channel_id, content, reply_to, attachments)
            .await
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

    pub async fn set_presence(&self, status: &str) -> Result<()> {
        self.rest.set_presence(status).await
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
    let started = Instant::now();
    let log_metrics = logging::debug_logging_enabled();
    let event_name = log_metrics.then(|| app_event_metric_name(event));
    let mutates_state = event.mutates_discord_state();
    let needs_effect_delivery = event.needs_effect_delivery();
    let mut state_lock_duration = Duration::ZERO;
    let mut state_mutation_duration = Duration::ZERO;
    let mut cache_counts = None;

    let publish_lock_started = Instant::now();
    let event_revision: SnapshotRevision;
    let publish_lock_duration;
    {
        let _publish_guard = publish_lock.lock().await;
        publish_lock_duration = publish_lock_started.elapsed();

        event_revision = if mutates_state {
            let state_lock_started = Instant::now();
            let next_revision = {
                let mut state = state.write().expect("discord state lock is not poisoned");
                state_lock_duration = state_lock_started.elapsed();
                let mutation_started = Instant::now();
                state.apply_event(event);
                state_mutation_duration = mutation_started.elapsed();
                if log_metrics {
                    cache_counts = Some(state.cache_counts());
                }
                let mut revision = revision
                    .write()
                    .expect("snapshot revision lock is not poisoned");
                if let Some(areas) = DiscordState::snapshot_areas_for_event(event) {
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
    }

    if let Some(event_name) = event_name {
        let cache_counts = cache_counts
            .map(|counts| format!(" {}", counts.log_fields()))
            .unwrap_or_default();
        logging::debug(
            "snapshot",
            format!(
                "op=publish_app_event event={event_name} revision={} \
                 navigation_revision={} message_revision={} detail_revision={} \
                 mutates={mutates_state} effect={needs_effect_delivery} \
                 publish_lock_ms={:.2} state_lock_ms={:.2} mutate_ms={:.2} \
                 total_ms={:.2}{cache_counts}",
                event_revision.global,
                event_revision.navigation,
                event_revision.message,
                event_revision.detail,
                duration_ms(publish_lock_duration),
                duration_ms(state_lock_duration),
                duration_ms(state_mutation_duration),
                duration_ms(started.elapsed()),
            ),
        );
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn app_event_metric_name(event: &AppEvent) -> &'static str {
    match event {
        AppEvent::Ready { .. } => "Ready",
        AppEvent::CurrentUserCapabilities { .. } => "CurrentUserCapabilities",
        AppEvent::GuildCreate { .. } => "GuildCreate",
        AppEvent::GuildUpdate { .. } => "GuildUpdate",
        AppEvent::GuildRolesUpdate { .. } => "GuildRolesUpdate",
        AppEvent::GuildEmojisUpdate { .. } => "GuildEmojisUpdate",
        AppEvent::GuildDelete { .. } => "GuildDelete",
        AppEvent::SelectedGuildChanged { .. } => "SelectedGuildChanged",
        AppEvent::SelectedMessageChannelChanged { .. } => "SelectedMessageChannelChanged",
        AppEvent::ChannelUpsert(_) => "ChannelUpsert",
        AppEvent::ChannelDelete { .. } => "ChannelDelete",
        AppEvent::MessageCreate { .. } => "MessageCreate",
        AppEvent::MessageHistoryLoaded { .. } => "MessageHistoryLoaded",
        AppEvent::ThreadPreviewLoaded { .. } => "ThreadPreviewLoaded",
        AppEvent::ThreadPreviewLoadFailed { .. } => "ThreadPreviewLoadFailed",
        AppEvent::ForumPostsLoaded { .. } => "ForumPostsLoaded",
        AppEvent::ForumPostsLoadFailed { .. } => "ForumPostsLoadFailed",
        AppEvent::MessageHistoryLoadFailed { .. } => "MessageHistoryLoadFailed",
        AppEvent::MessageUpdate { .. } => "MessageUpdate",
        AppEvent::MessageDelete { .. } => "MessageDelete",
        AppEvent::GuildMemberListCounts { .. } => "GuildMemberListCounts",
        AppEvent::GuildMemberUpsert { .. } => "GuildMemberUpsert",
        AppEvent::GuildMemberAdd { .. } => "GuildMemberAdd",
        AppEvent::GuildMemberRemove { .. } => "GuildMemberRemove",
        AppEvent::PresenceUpdate { .. } => "PresenceUpdate",
        AppEvent::UserPresenceUpdate { .. } => "UserPresenceUpdate",
        AppEvent::TypingStart { .. } => "TypingStart",
        AppEvent::CurrentUserReactionAdd { .. } => "CurrentUserReactionAdd",
        AppEvent::CurrentUserReactionRemove { .. } => "CurrentUserReactionRemove",
        AppEvent::MessageReactionAdd { .. } => "MessageReactionAdd",
        AppEvent::MessageReactionRemove { .. } => "MessageReactionRemove",
        AppEvent::MessageReactionRemoveAll { .. } => "MessageReactionRemoveAll",
        AppEvent::MessageReactionRemoveEmoji { .. } => "MessageReactionRemoveEmoji",
        AppEvent::MessagePinnedUpdate { .. } => "MessagePinnedUpdate",
        AppEvent::PinnedMessagesLoaded { .. } => "PinnedMessagesLoaded",
        AppEvent::PinnedMessagesLoadFailed { .. } => "PinnedMessagesLoadFailed",
        AppEvent::CurrentUserPollVoteUpdate { .. } => "CurrentUserPollVoteUpdate",
        AppEvent::ReactionUsersLoaded { .. } => "ReactionUsersLoaded",
        AppEvent::GuildFoldersUpdate { .. } => "GuildFoldersUpdate",
        AppEvent::UserGuildNotificationSettingsInit { .. } => "UserGuildNotificationSettingsInit",
        AppEvent::UserGuildNotificationSettingsUpdate { .. } => {
            "UserGuildNotificationSettingsUpdate"
        }
        AppEvent::GatewayError { .. } => "GatewayError",
        AppEvent::AttachmentDownloadCompleted { .. } => "AttachmentDownloadCompleted",
        AppEvent::UpdateAvailable { .. } => "UpdateAvailable",
        AppEvent::AttachmentPreviewLoaded { .. } => "AttachmentPreviewLoaded",
        AppEvent::AttachmentPreviewLoadFailed { .. } => "AttachmentPreviewLoadFailed",
        AppEvent::UserProfileLoaded { .. } => "UserProfileLoaded",
        AppEvent::UserProfileLoadFailed { .. } => "UserProfileLoadFailed",
        AppEvent::UserNoteLoaded { .. } => "UserNoteLoaded",
        AppEvent::RelationshipsLoaded { .. } => "RelationshipsLoaded",
        AppEvent::RelationshipUpsert { .. } => "RelationshipUpsert",
        AppEvent::RelationshipRemove { .. } => "RelationshipRemove",
        AppEvent::ActivateChannel { .. } => "ActivateChannel",
        AppEvent::ReadStateInit { .. } => "ReadStateInit",
        AppEvent::MessageAck { .. } => "MessageAck",
        AppEvent::GatewayClosed => "GatewayClosed",
        AppEvent::SelfPresenceUpdate { .. } => "SelfPresenceUpdate",
    }
}

pub(crate) fn validate_token_header(token: &str) -> Result<()> {
    HeaderValue::from_str(token)
        .map_err(|source| AppError::InvalidDiscordTokenHeader { source })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::discord::{AppEvent, ChannelInfo, MessageKind, ids::Id};

    use super::{DiscordClient, validate_token_header};

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
        assert_eq!(snapshot.detail, 1);
        assert_eq!(effect.revision, 1);
        assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
    }

    #[tokio::test]
    async fn channel_upsert_is_delivered_as_effect_for_tui_derived_state() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let mut snapshots = client.subscribe_snapshots();

        client.publish_event(channel_upsert_event()).await;

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
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
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

    fn channel_upsert_event() -> AppEvent {
        AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
    }
}
