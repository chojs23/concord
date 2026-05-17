use std::sync::{Arc, Mutex, RwLock};

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
    MessageAttachmentUpload, MessageInfo, ReactionEmoji, ReactionUserInfo, UserProfileInfo,
    commands::ForumPostArchiveState,
    events::{AppEvent, SequencedAppEvent},
    gateway::{GatewayCommand, GatewayRuntime, run_gateway},
    rest::{DiscordRest, ForumPostPage},
    state::{CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, SnapshotRevision},
    voice::{self, VoiceRuntimeEvent},
};

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    effects_tx: mpsc::Sender<SequencedAppEvent>,
    effects_rx: Arc<Mutex<Option<mpsc::Receiver<SequencedAppEvent>>>>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    requested_voice: Arc<RwLock<Option<CurrentVoiceConnectionState>>>,
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
            let mut requested = self
                .requested_voice
                .write()
                .expect("requested voice lock is not poisoned");
            if let Some(channel_id) = channel_id {
                let allow_microphone_transmit = requested
                    .filter(|voice| voice.guild_id == guild_id && voice.channel_id == channel_id)
                    .is_some_and(|voice| voice.allow_microphone_transmit);
                let voice = CurrentVoiceConnectionState {
                    guild_id,
                    channel_id,
                    self_mute,
                    self_deaf,
                    allow_microphone_transmit,
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
        if voice.allow_microphone_transmit == allow_microphone_transmit {
            return;
        }

        voice.allow_microphone_transmit = allow_microphone_transmit;
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

    let event_revision: SnapshotRevision;
    {
        let _publish_guard = publish_lock.lock().await;

        event_revision = if mutates_state {
            let next_revision = {
                let mut state = state.write().expect("discord state lock is not poisoned");
                state.apply_event(event);
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
}

pub(crate) fn validate_token_header(token: &str) -> Result<()> {
    HeaderValue::from_str(token)
        .map_err(|source| AppError::InvalidDiscordTokenHeader { source })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::discord::{AppEvent, ChannelInfo, MessageKind, VoiceStateInfo, ids::Id};

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
