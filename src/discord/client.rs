use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex, RwLock, RwLockReadGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

mod lifecycle;
mod rest_actions;

use crate::discord::VoiceParticipantPlaybackSettings;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use reqwest::header::HeaderValue;
use tokio::{
    sync::{Mutex as AsyncMutex, mpsc, watch},
    task::JoinHandle,
    time::{Duration, timeout},
};

use crate::{AppError, Result};

use super::{
    ActivityInfo, ApplicationCommandAutocompleteInvocation, ApplicationCommandInfo,
    ApplicationCommandInvocation, DiscordAction, DiscordAuthSession, DiscordPermission,
    PresenceStatus,
    application_commands::{
        application_command_autocomplete_from_invocation,
        application_command_interaction_from_invocation,
    },
    events::{AppEvent, SequencedAppEvent},
    fingerprint::{CLIENT_BUILD_NUMBER, ClientFingerprint, discord_http_client},
    gateway::{GatewayCommand, GatewayRuntime, GatewayVoiceStateUpdate, run_gateway},
    request_lifecycle::RequestLifecycle,
    rest::DiscordRest,
    state::{CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, SnapshotRevision},
    voice::{
        self, StreamBroadcastRequest, StreamCaptureTarget, VoiceAudioSettings, VoiceAudioSources,
        VoiceRuntimeEvent, VoiceScope,
    },
};

const MEMBER_SEARCH_MIN_QUERY_CHARS: usize = 2;
const MEMBER_SEARCH_MAX_QUERY_CHARS: usize = 64;
const MEMBER_SEARCH_MAX_LIMIT: u16 = 10;
const OFFICIAL_WORDLE_APPLICATION_ID: u64 = 1_211_781_489_931_452_447;
const DISCORD_LOCAL_APPLICATION_ID: &str = "-1";
const GATEWAY_COMMAND_CHANNEL_CLOSED: &str = "gateway command channel closed";
const VOICE_RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

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
pub(crate) struct AppEventPublisher {
    effects_tx: mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    revision: Arc<RwLock<SnapshotRevision>>,
    publish_lock: Arc<AsyncMutex<()>>,
    application_command_requests: Arc<Mutex<HashMap<Option<Id<GuildMarker>>, RequestState>>>,
    application_commands: Arc<Mutex<ApplicationCommandCache>>,
    request_lifecycle: Arc<Mutex<RequestLifecycle>>,
    voice_events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
}

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    fingerprint: Arc<ClientFingerprint>,
    rest: DiscordRest,
    effects_rx: Arc<Mutex<Option<mpsc::Receiver<SequencedAppEvent>>>>,
    snapshots_tx: watch::Sender<SnapshotRevision>,
    state: Arc<RwLock<DiscordState>>,
    requested_voice: Arc<RwLock<Option<CurrentVoiceConnectionState>>>,
    push_to_talk: Arc<RwLock<bool>>,
    selected_rich_presence: Arc<RwLock<Option<String>>>,
    /// `application_id -> (external image url -> media-proxy path)`, so a url is
    /// registered with Discord only once.
    external_assets: Arc<Mutex<HashMap<String, HashMap<String, String>>>>,
    gateway_session_id: Arc<RwLock<Option<String>>>,
    application_command_requests: Arc<Mutex<HashMap<Option<Id<GuildMarker>>, RequestState>>>,
    application_commands: Arc<Mutex<ApplicationCommandCache>>,
    next_application_command_request: Arc<AtomicU64>,
    request_lifecycle: Arc<Mutex<RequestLifecycle>>,
    revision: Arc<RwLock<SnapshotRevision>>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    gateway_commands_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<GatewayCommand>>>>,
    voice_events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    voice_events_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<VoiceRuntimeEvent>>>>,
    voice_runtime_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    event_publisher: AppEventPublisher,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self> {
        Self::new_with_fingerprint(token, Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER)))
    }

    pub(crate) fn new_with_fingerprint(
        token: String,
        fingerprint: Arc<ClientFingerprint>,
    ) -> Result<Self> {
        let http = discord_http_client(&fingerprint);
        Self::new_with_fingerprint_and_http(token, fingerprint, http)
    }

    pub(crate) fn new_with_auth_session(
        token: String,
        auth_session: DiscordAuthSession,
    ) -> Result<Self> {
        Self::new_with_fingerprint_and_http(
            token,
            auth_session.fingerprint_arc(),
            auth_session.http(),
        )
    }

    fn new_with_fingerprint_and_http(
        token: String,
        fingerprint: Arc<ClientFingerprint>,
        http: reqwest::Client,
    ) -> Result<Self> {
        validate_token_header(&token)?;
        let rest = DiscordRest::new(token.clone(), http, Arc::clone(&fingerprint));
        let initial_state = DiscordState::default();
        let (effects_tx, effects_rx) = mpsc::channel(4096);
        let (snapshots_tx, _) = watch::channel(SnapshotRevision::default());
        let (gateway_commands_tx, gateway_commands_rx) = mpsc::unbounded_channel();
        let (voice_events_tx, voice_events_rx) = mpsc::unbounded_channel();
        let state = Arc::new(RwLock::new(initial_state));
        let revision = Arc::new(RwLock::new(SnapshotRevision::default()));
        let publish_lock = Arc::new(AsyncMutex::new(()));
        let application_command_requests = Arc::new(Mutex::new(HashMap::new()));
        let application_commands = Arc::new(Mutex::new(HashMap::new()));
        let request_lifecycle = Arc::new(Mutex::new(RequestLifecycle::default()));
        let event_publisher = AppEventPublisher {
            effects_tx: effects_tx.clone(),
            snapshots_tx: snapshots_tx.clone(),
            state: Arc::clone(&state),
            revision: Arc::clone(&revision),
            publish_lock: Arc::clone(&publish_lock),
            application_command_requests: Arc::clone(&application_command_requests),
            application_commands: Arc::clone(&application_commands),
            request_lifecycle: Arc::clone(&request_lifecycle),
            voice_events_tx: voice_events_tx.clone(),
        };

        Ok(Self {
            token,
            fingerprint,
            rest,
            effects_rx: Arc::new(Mutex::new(Some(effects_rx))),
            snapshots_tx,
            state,
            requested_voice: Arc::new(RwLock::new(None)),
            push_to_talk: Arc::new(RwLock::new(false)),
            selected_rich_presence: Arc::new(RwLock::new(None)),
            external_assets: Arc::new(Mutex::new(HashMap::new())),
            gateway_session_id: Arc::new(RwLock::new(None)),
            application_command_requests,
            application_commands,
            next_application_command_request: Arc::new(AtomicU64::new(1)),
            request_lifecycle,
            revision,
            gateway_commands_tx,
            gateway_commands_rx: Arc::new(Mutex::new(Some(gateway_commands_rx))),
            voice_events_tx,
            voice_events_rx: Arc::new(Mutex::new(Some(voice_events_rx))),
            voice_runtime_task: Arc::new(Mutex::new(None)),
            event_publisher,
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

    pub(super) fn read_state(&self) -> RwLockReadGuard<'_, DiscordState> {
        self.state
            .read()
            .expect("discord state lock is not poisoned")
    }

    pub fn current_discord_snapshot(&self) -> DiscordSnapshot {
        let state = self.read_state();
        let revision = *self
            .revision
            .read()
            .expect("snapshot revision lock is not poisoned");
        state.snapshot(revision)
    }

    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.read_state().current_user_id()
    }

    pub(crate) fn update_rest_page_referer(&self, channel_id: Option<Id<ChannelMarker>>) {
        let channel = channel_id.and_then(|channel_id| {
            self.read_state()
                .channel(channel_id)
                .map(|channel| (channel.guild_id, channel_id))
        });
        match channel {
            Some((guild_id, channel_id)) => self.rest.set_channel_referer(guild_id, channel_id),
            None => self.rest.reset_page_referer(),
        }
    }

    /// Current user `(id, username)` for the RPC READY handshake. RPC clients read
    /// `data.user.username` from it.
    pub fn current_user_rpc_identity(&self) -> Option<(String, String)> {
        let state = self.read_state();
        let id = state.current_user_id()?;
        let username = state.current_user().unwrap_or_default().to_owned();
        Some((id.to_string(), username))
    }

    pub async fn publish_event(&self, event: AppEvent) {
        self.event_publisher.publish(event).await;
    }

    pub fn start_gateway(&self, serve_rich_presence: bool) -> JoinHandle<()> {
        let token = self.token.clone();
        let state = Arc::clone(&self.state);
        let gateway_session_id = Arc::clone(&self.gateway_session_id);
        let fingerprint = Arc::clone(&self.fingerprint);
        let event_publisher = self.event_publisher.clone();
        let gateway_commands = self
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway can only be started once");
        let voice_events_tx = self.voice_events_tx.clone();
        let voice_status_publisher = voice::VoiceStatusPublisher::new(self.event_publisher.clone());
        let stream_preview_uploader = voice::StreamPreviewUploader::new(self.rest.clone());
        if let Some(voice_events) = self
            .voice_events_rx
            .lock()
            .expect("voice event receiver mutex is not poisoned")
            .take()
        {
            let task = tokio::spawn(voice::run_voice_runtime(
                voice_events,
                voice_events_tx.clone(),
                self.gateway_commands_tx.clone(),
                voice_status_publisher,
                stream_preview_uploader,
            ));
            *self
                .voice_runtime_task
                .lock()
                .expect("voice runtime task mutex is not poisoned") = Some(task);
        }

        // Best-effort, so it never blocks the gateway from starting.
        if serve_rich_presence {
            tokio::spawn(crate::discord::rpc::run_rpc_server(self.clone()));
        }

        tokio::spawn(async move {
            let runtime = GatewayRuntime {
                fingerprint,
                state,
                gateway_session_id,
                event_publisher,
            };
            run_gateway(token, gateway_commands, runtime).await;
        })
    }

    pub fn request_guild_members_by_ids(
        &self,
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
    ) -> std::result::Result<(), String> {
        if user_ids.is_empty() {
            return Ok(());
        }
        self.send_gateway_command(GatewayCommand::RequestGuildMembersByIds {
            guild_id,
            user_ids,
            presences: false,
        })
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
        let nonce = format!("mention-ac-{:016x}", query_hash(guild_id, &query));
        self.send_gateway_command(GatewayCommand::RequestGuildMembers {
            guild_id,
            query,
            limit,
            presences: true,
            nonce: Some(nonce),
        })
    }

    pub fn subscribe_direct_message(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.send_gateway_command(GatewayCommand::SubscribeDirectMessage { channel_id })
    }

    pub fn subscribe_guild_channel(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.send_gateway_command(GatewayCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        })
    }

    pub fn update_member_list_subscription(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    ) -> std::result::Result<(), String> {
        self.send_gateway_command(GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            ranges,
        })
    }

    pub fn update_voice_state(
        &self,
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
    ) -> std::result::Result<(), String> {
        self.update_voice_state_inner(scope, channel_id, self_mute, self_deaf, false)
    }

    pub fn request_voice_join(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        self_mute: bool,
        self_deaf: bool,
    ) -> std::result::Result<(), String> {
        self.update_voice_state_inner(scope, Some(channel_id), self_mute, self_deaf, true)
    }

    pub fn request_stream_watch(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        owner_id: Id<UserMarker>,
        display_name: String,
    ) -> std::result::Result<(), String> {
        let state = self.read_state();
        let current_voice = state
            .current_user_voice_connection()
            .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
            .ok_or_else(|| "join the streamer's voice channel first".to_owned())?;
        let participants = match scope {
            VoiceScope::Guild(guild_id) => {
                state.voice_participants_for_channel(guild_id, channel_id)
            }
            VoiceScope::Private(_) => state.voice_participants_for_private_channel(channel_id),
        };
        let participant = participants
            .iter()
            .find(|participant| participant.user_id == owner_id)
            .ok_or_else(|| "streamer is no longer in this voice channel".to_owned())?;
        if !participant.self_stream {
            return Err("selected participant is not streaming".to_owned());
        }
        if state.current_user_id() == Some(owner_id) {
            return Err("cannot watch your own stream".to_owned());
        }
        let stream_key = match scope {
            VoiceScope::Guild(guild_id) => format!(
                "guild:{}:{}:{}",
                guild_id.get(),
                current_voice.channel_id.get(),
                owner_id.get()
            ),
            VoiceScope::Private(_) => {
                format!("call:{}:{}", current_voice.channel_id.get(), owner_id.get())
            }
        };
        drop(state);

        let request = voice::StreamWatchRequest {
            stream_key: stream_key.clone(),
            scope,
            channel_id,
            owner_id,
            display_name,
        };
        self.voice_events_tx
            .send(VoiceRuntimeEvent::WatchStreamRequested(request))
            .map_err(|_| "voice runtime stopped".to_owned())?;
        if let Err(error) = self.send_gateway_command(GatewayCommand::WatchStream {
            stream_key: stream_key.clone(),
        }) {
            let _ = self
                .voice_events_tx
                .send(VoiceRuntimeEvent::WatchStreamCancelled { stream_key });
            return Err(error);
        }
        Ok(())
    }

    pub fn request_stream_start(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        target: StreamCaptureTarget,
    ) -> std::result::Result<(), String> {
        voice::ensure_stream_broadcast_available()?;
        let state = self.read_state();
        let current_voice = state
            .current_user_voice_connection()
            .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
            .ok_or_else(|| "join this voice channel before starting a stream".to_owned())?;
        if scope.guild_id().is_some() {
            let channel = state
                .channel(channel_id)
                .ok_or_else(|| "cannot verify stream permissions".to_owned())?;
            rest_actions::ensure_channel_action_policy(&state, channel, DiscordAction::StreamVoice)
                .map_err(|error| error.to_string())?;
        }
        let current_user_id = state
            .current_user_id()
            .ok_or_else(|| "current user is not ready".to_owned())?;
        let stream_key = stream_key(scope, current_voice.channel_id, current_user_id);
        drop(state);

        let request = StreamBroadcastRequest {
            stream_key,
            scope,
            channel_id,
            target,
        };
        self.voice_events_tx
            .send(VoiceRuntimeEvent::BroadcastStreamRequested(request))
            .map_err(|_| "voice runtime stopped".to_owned())
    }

    pub fn request_stream_stop(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        let state = self.read_state();
        let current_voice = state
            .current_user_voice_connection()
            .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
            .ok_or_else(|| "not connected to this voice channel".to_owned())?;
        let current_user_id = state
            .current_user_id()
            .ok_or_else(|| "current user is not ready".to_owned())?;
        let stream_key = stream_key(scope, current_voice.channel_id, current_user_id);
        drop(state);
        self.voice_events_tx
            .send(VoiceRuntimeEvent::BroadcastStreamStopRequested { stream_key })
            .map_err(|_| "voice runtime stopped".to_owned())
    }

    fn update_voice_state_inner(
        &self,
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        self_mute: bool,
        self_deaf: bool,
        manual_retry: bool,
    ) -> std::result::Result<(), String> {
        let mut requested = self
            .requested_voice
            .write()
            .expect("requested voice lock is not poisoned");
        if !manual_retry
            && voice_state_request_is_duplicate(*requested, scope, channel_id, self_mute, self_deaf)
        {
            return Ok(());
        }
        if let Some(channel_id) = channel_id {
            let requested_same_channel = requested
                .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                .is_some();
            if manual_retry || !requested_same_channel {
                let state = self.read_state();
                let current_same_channel = state
                    .current_user_voice_connection()
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .is_some();
                // Permission gates apply only to guild voice channels. DM and
                // group-DM calls have no guild permission model to check.
                if !current_same_channel && scope.guild_id().is_some() {
                    let Some(channel) = state.channel(channel_id) else {
                        return Err("cannot verify voice channel permissions".to_owned());
                    };
                    rest_actions::ensure_channel_action_policy(
                        &state,
                        channel,
                        DiscordAction::JoinVoiceChannel,
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }

        let result = self.send_gateway_command(GatewayCommand::UpdateVoiceState {
            guild_id: scope.guild_id(),
            channel_id,
            self_mute,
            self_deaf,
        });
        if result.is_ok() {
            if let Some(channel_id) = channel_id {
                let audio_settings = requested
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .map(CurrentVoiceConnectionState::audio_settings)
                    .unwrap_or_default();
                let voice = CurrentVoiceConnectionState {
                    scope,
                    channel_id,
                    self_mute,
                    self_deaf,
                    allow_microphone_transmit: audio_settings.allow_microphone_transmit,
                    noise_suppression: audio_settings.noise_suppression,
                    microphone_sensitivity: audio_settings.microphone_sensitivity,
                    microphone_volume: audio_settings.microphone_volume,
                    voice_output_volume: audio_settings.voice_output_volume,
                };
                *requested = Some(voice);
                let event = if manual_retry {
                    VoiceRuntimeEvent::ManualRetry(voice)
                } else {
                    VoiceRuntimeEvent::Requested(Some(voice))
                };
                let _ = self.voice_events_tx.send(event);
            } else if requested.is_some_and(|voice| voice.scope == scope) {
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
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        settings: VoiceAudioSettings,
    ) -> std::result::Result<(), String> {
        if settings.allow_microphone_transmit && scope.guild_id().is_some() {
            let state = self.read_state();
            let Some(channel) = state.channel(channel_id) else {
                return Err("cannot verify voice channel permissions".to_owned());
            };
            rest_actions::ensure_channel_action_policy(
                &state,
                channel,
                DiscordAction::TransmitMicrophone,
            )
            .map_err(|error| error.to_string())?;
            rest_actions::ensure_permission(
                &state,
                channel,
                DiscordAction::TransmitMicrophone,
                DiscordPermission::Speak,
            )
            .map_err(|error| error.to_string())?;
            let push_to_talk = *self
                .push_to_talk
                .read()
                .expect("push-to-talk lock is not poisoned");
            if !push_to_talk {
                rest_actions::ensure_permission(
                    &state,
                    channel,
                    DiscordAction::TransmitMicrophone,
                    DiscordPermission::UseVoiceActivity,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        let mut requested = self
            .requested_voice
            .write()
            .expect("requested voice lock is not poisoned");
        let Some(mut voice) = *requested else {
            return Ok(());
        };
        if voice.scope != scope || voice.channel_id != channel_id {
            return Ok(());
        }
        if voice.audio_settings() == settings {
            return Ok(());
        }

        voice.set_audio_settings(settings);
        *requested = Some(voice);
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::Requested(Some(voice)));
        Ok(())
    }

    pub(crate) fn update_voice_audio_sources(&self, sources: VoiceAudioSources) {
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::AudioSourcesChanged(sources));
    }

    #[cfg(feature = "voice-playback")]
    pub(crate) fn set_push_to_talk_enabled(&self, enabled: bool) {
        let mut current = self
            .push_to_talk
            .write()
            .expect("push-to-talk lock is not poisoned");
        if *current == enabled {
            return;
        }
        *current = enabled;
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::PushToTalkEnabledChanged(enabled));
    }

    #[cfg(feature = "voice-playback")]
    pub(crate) fn set_push_to_talk_pressed(&self, pressed: bool) {
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::PushToTalkPressed(pressed));
    }

    pub fn replace_voice_participant_playback_settings(
        &self,
        settings: Vec<(Id<UserMarker>, VoiceParticipantPlaybackSettings)>,
    ) {
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::ReplaceParticipantPlaybackSettings(
                settings,
            ));
    }

    pub fn update_voice_participant_playback_settings(
        &self,
        user_id: Id<UserMarker>,
        settings: VoiceParticipantPlaybackSettings,
    ) {
        let _ = self
            .voice_events_tx
            .send(VoiceRuntimeEvent::UpdateParticipantPlaybackSettings { user_id, settings });
    }

    pub async fn update_presence_status(
        &self,
        status: PresenceStatus,
    ) -> Result<Vec<ActivityInfo>> {
        let activities = self.current_user_activities();
        self.rest.update_current_user_status(status).await?;
        self.send_presence_update(status, activities.clone())?;
        Ok(activities)
    }

    pub fn current_user_activities(&self) -> Vec<ActivityInfo> {
        let state = self.read_state();
        state
            .current_user_id()
            .map(|user_id| state.user_activities(user_id).to_vec())
            .unwrap_or_default()
    }

    pub async fn application_display_name(&self, application_id: &str) -> Option<String> {
        match self.rest.application_rpc(application_id).await {
            Ok(info) => Some(info.name),
            Err(error) => {
                crate::logging::debug(
                    "rpc",
                    format!("resolve application {application_id} failed: {error}"),
                );
                None
            }
        }
    }

    /// Resolve an app's art assets to a `key -> id` map. `None` on failure (so the
    /// caller can retry). `Some` (possibly empty) means the asset set is known.
    pub async fn application_asset_ids(
        &self,
        application_id: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        match self.rest.application_assets(application_id).await {
            Ok(assets) => Some(
                assets
                    .into_iter()
                    .map(|asset| (asset.name, asset.id))
                    .collect(),
            ),
            Err(error) => {
                crate::logging::debug(
                    "rpc",
                    format!("resolve application {application_id} assets failed: {error}"),
                );
                None
            }
        }
    }

    /// Register an external image `url` and return its media-proxy path (used as
    /// `mp:{path}`). Only successful results are cached, so failures are retried.
    pub async fn register_external_asset(&self, application_id: &str, url: &str) -> Option<String> {
        if let Some(path) = self
            .external_assets
            .lock()
            .expect("external asset cache lock is not poisoned")
            .get(application_id)
            .and_then(|per_app| per_app.get(url))
        {
            return Some(path.clone());
        }
        let path = match self
            .rest
            .application_external_assets(application_id, &[url])
            .await
        {
            Ok(assets) => assets
                .into_iter()
                .next()
                .map(|asset| asset.external_asset_path)?,
            Err(error) => {
                crate::logging::debug(
                    "rpc",
                    format!("register external asset for {application_id} failed: {error}"),
                );
                return None;
            }
        };
        self.external_assets
            .lock()
            .expect("external asset cache lock is not poisoned")
            .entry(application_id.to_owned())
            .or_default()
            .insert(url.to_owned(), path.clone());
        Some(path)
    }

    /// Rewrite an activity's external image URLs to `mp:{path}` refs (asset keys and
    /// ids are left alone). Run before broadcasting. The gateway cannot render a raw URL.
    pub async fn resolve_activity_external_assets(&self, activity: &mut ActivityInfo) {
        let Some(application_id) = activity.application_id.clone() else {
            return;
        };
        let images = {
            let Some(assets) = activity.assets.as_ref() else {
                return;
            };
            [assets.large_image.clone(), assets.small_image.clone()]
        };
        let mut resolved: [Option<String>; 2] = [None, None];
        for (slot, image) in images.into_iter().enumerate() {
            let Some(url) = image else {
                continue;
            };
            if (url.starts_with("https://") || url.starts_with("http://"))
                && let Some(path) = self.register_external_asset(&application_id, &url).await
            {
                resolved[slot] = Some(format!("mp:{path}"));
            }
        }
        if let Some(assets) = activity.assets.as_mut() {
            if let Some(large) = resolved[0].take() {
                assets.large_image = Some(large);
            }
            if let Some(small) = resolved[1].take() {
                assets.small_image = Some(small);
            }
        }
    }

    /// Record which app's activity to broadcast. `None` means a manual/no
    /// activity that RPC updates must not override.
    pub fn select_rich_presence(&self, client_id: Option<String>) {
        *self
            .selected_rich_presence
            .write()
            .expect("selected rich presence lock is not poisoned") = client_id;
    }

    pub fn selected_rich_presence(&self) -> Option<String> {
        self.selected_rich_presence
            .read()
            .expect("selected rich presence lock is not poisoned")
            .clone()
    }

    /// So the RPC server can relay an activity without clobbering a manually chosen status.
    pub fn current_user_status(&self) -> PresenceStatus {
        let state = self.read_state();
        state
            .current_user_id()
            .and_then(|user_id| state.user_presence(user_id))
            .unwrap_or(PresenceStatus::Online)
    }

    pub fn update_presence_activity(
        &self,
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    ) -> Result<()> {
        self.send_presence_update(status, activities)
    }

    fn send_presence_update(
        &self,
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    ) -> Result<()> {
        self.send_gateway_command(GatewayCommand::UpdatePresence { status, activities })
            .map_err(AppError::DiscordRequest)?;
        Ok(())
    }

    pub fn current_or_requested_voice_connection(&self) -> Option<CurrentVoiceConnectionState> {
        self.read_state()
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

    pub async fn shutdown_voice_runtime(&self) -> std::result::Result<(), String> {
        let _ = self.voice_events_tx.send(VoiceRuntimeEvent::Shutdown);
        let task = self
            .voice_runtime_task
            .lock()
            .expect("voice runtime task mutex is not poisoned")
            .take();
        let Some(mut task) = task else {
            return Ok(());
        };

        match timeout(VOICE_RUNTIME_SHUTDOWN_TIMEOUT, &mut task).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(format!("voice runtime task ended unexpectedly: {error}")),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Err("voice runtime shutdown timed out after 8s".to_owned())
            }
        }
    }

    pub fn shutdown_gateway(&self) -> std::result::Result<(), String> {
        let voice_leave = self
            .requested_voice_connection()
            .map(|voice| GatewayVoiceStateUpdate {
                guild_id: voice.scope.guild_id(),
                channel_id: None,
                self_mute: voice.self_mute,
                self_deaf: voice.self_deaf,
            });
        self.send_gateway_command(GatewayCommand::Shutdown { voice_leave })
    }

    pub async fn load_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Result<Option<Vec<ApplicationCommandInfo>>> {
        let Some(request_id) = self.begin_application_command_request(guild_id) else {
            return Ok(None);
        };
        let result = self.rest.load_application_commands(guild_id).await;
        match result {
            Ok(commands) => {
                Ok(self.finish_application_command_request(guild_id, request_id, commands))
            }
            Err(error) => {
                self.clear_application_command_request(guild_id, request_id);
                Err(error)
            }
        }
    }

    pub async fn run_application_command(
        &self,
        invocation: &ApplicationCommandInvocation,
    ) -> Result<()> {
        self.ensure_can_run_application_command(invocation)?;
        let session_id = self
            .gateway_session_id
            .read()
            .expect("gateway session id lock is not poisoned")
            .clone()
            .ok_or_else(|| AppError::DiscordRequest("gateway session is not ready".to_owned()))?;
        let interaction = self.application_command_interaction(invocation)?;
        let nonce = interaction.nonce.clone();
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_application_command(nonce.clone(), Instant::now());
        let result = self
            .rest
            .run_application_command(&interaction, &session_id)
            .await;
        if result.is_err() {
            self.request_lifecycle
                .lock()
                .expect("request lifecycle lock is not poisoned")
                .clear_application_command(&nonce);
        }
        result
    }

    pub async fn request_application_command_autocomplete(
        &self,
        invocation: &ApplicationCommandAutocompleteInvocation,
    ) -> Result<()> {
        self.ensure_can_request_application_command_autocomplete(invocation.channel_id)?;
        let session_id = self
            .gateway_session_id
            .read()
            .expect("gateway session id lock is not poisoned")
            .clone()
            .ok_or_else(|| AppError::DiscordRequest("gateway session is not ready".to_owned()))?;
        let interaction = self.application_command_autocomplete_interaction(invocation)?;
        self.rest
            .request_application_command_autocomplete(&interaction, &session_id)
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
            .and_then(|commands| match invocation.command_identity {
                Some(identity) => commands
                    .iter()
                    .find(|command| command.identity() == identity),
                None => commands
                    .iter()
                    .find(|command| command.name == invocation.command_name),
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

    fn application_command_autocomplete_interaction(
        &self,
        invocation: &ApplicationCommandAutocompleteInvocation,
    ) -> Result<super::application_commands::ApplicationCommandAutocompleteInteraction> {
        let commands = self
            .application_commands
            .lock()
            .expect("application command cache lock is not poisoned");
        let command = commands
            .get(&invocation.guild_id)
            .and_then(|commands| {
                commands
                    .iter()
                    .find(|command| command.identity() == invocation.command_identity)
            })
            .ok_or_else(|| {
                AppError::DiscordRequest(format!(
                    "application command {} is not loaded",
                    invocation.command_name
                ))
            })?;
        application_command_autocomplete_from_invocation(invocation, command).ok_or_else(|| {
            AppError::DiscordRequest(format!(
                "application command {} autocomplete input is invalid",
                invocation.command_name
            ))
        })
    }

    fn send_gateway_command(&self, command: GatewayCommand) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(command)
            .map_err(|_| GATEWAY_COMMAND_CHANNEL_CLOSED.to_owned())
    }
}

async fn publish_app_event(
    effects_tx: &mpsc::Sender<SequencedAppEvent>,
    snapshots_tx: &watch::Sender<SnapshotRevision>,
    state: &Arc<RwLock<DiscordState>>,
    revision: &Arc<RwLock<SnapshotRevision>>,
    publish_lock: &Arc<AsyncMutex<()>>,
    event: &AppEvent,
) {
    let snapshot_areas = event.snapshot_areas();
    let needs_effect_delivery = event.needs_effect_delivery();

    let event_revision: SnapshotRevision;
    {
        let _publish_guard = publish_lock.lock().await;
        let voice_sounds = {
            let state = state.read().expect("discord state lock is not poisoned");
            match event {
                AppEvent::VoiceStateUpdate { state: voice_state } => state
                    .voice_sound_for_state_update(voice_state)
                    .into_iter()
                    .collect(),
                AppEvent::StreamUpdate { stream } => state.stream_viewer_sounds_for_update(stream),
                _ => Vec::new(),
            }
        };

        event_revision = if let Some(areas) = snapshot_areas {
            let next_revision = {
                let mut state = state.write().expect("discord state lock is not poisoned");
                let mut revision = revision
                    .write()
                    .expect("snapshot revision lock is not poisoned");
                *revision = state.apply_event_advancing(event, areas, *revision);
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
        for kind in voice_sounds {
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

fn stream_key(scope: VoiceScope, channel_id: Id<ChannelMarker>, user_id: Id<UserMarker>) -> String {
    match scope {
        VoiceScope::Guild(guild_id) => {
            format!("guild:{guild_id}:{channel_id}:{user_id}")
        }
        VoiceScope::Private(_) => format!("call:{channel_id}:{user_id}"),
    }
}

fn voice_state_request_is_duplicate(
    requested: Option<CurrentVoiceConnectionState>,
    scope: VoiceScope,
    channel_id: Option<Id<ChannelMarker>>,
    self_mute: bool,
    self_deaf: bool,
) -> bool {
    match (requested, channel_id) {
        (Some(voice), Some(channel_id)) => {
            voice.scope == scope
                && voice.channel_id == channel_id
                && voice.self_mute == self_mute
                && voice.self_deaf == self_deaf
        }
        (Some(voice), None) => voice.scope != scope,
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

fn query_hash(guild_id: Id<GuildMarker>, query: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    guild_id.hash(&mut hasher);
    query.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestState {
    Requested(u64),
    Loaded,
}

impl DiscordClient {
    fn begin_application_command_request(&self, guild_id: Option<Id<GuildMarker>>) -> Option<u64> {
        let mut requests = self
            .application_command_requests
            .lock()
            .expect("application command request lock is not poisoned");
        if matches!(requests.get(&guild_id), Some(RequestState::Requested(_))) {
            return None;
        }
        let request_id = self
            .next_application_command_request
            .fetch_add(1, Ordering::Relaxed);
        requests.insert(guild_id, RequestState::Requested(request_id));
        Some(request_id)
    }

    fn finish_application_command_request(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        request_id: u64,
        commands: Vec<ApplicationCommandInfo>,
    ) -> Option<Vec<ApplicationCommandInfo>> {
        let mut cached_commands = self
            .application_commands
            .lock()
            .expect("application command cache lock is not poisoned");
        let mut requests = self
            .application_command_requests
            .lock()
            .expect("application command request lock is not poisoned");
        if requests.get(&guild_id) != Some(&RequestState::Requested(request_id)) {
            return None;
        }
        let commands = commands
            .into_iter()
            .filter(|command| !is_hidden_default_application_command(command))
            .collect::<Vec<_>>();
        cached_commands.insert(guild_id, commands.clone());
        requests.insert(guild_id, RequestState::Loaded);
        Some(
            commands
                .into_iter()
                .map(ApplicationCommandInfo::without_raw)
                .collect(),
        )
    }

    fn clear_application_command_request(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        request_id: u64,
    ) {
        let mut requests = self
            .application_command_requests
            .lock()
            .expect("application command request lock is not poisoned");
        if requests.get(&guild_id) == Some(&RequestState::Requested(request_id)) {
            requests.remove(&guild_id);
        }
    }
}

fn is_hidden_default_application_command(command: &ApplicationCommandInfo) -> bool {
    match command.name.as_str() {
        "giphy" | "msg" | "play" => is_discord_default_application(command),
        "wordle" => command.application_id.get() == OFFICIAL_WORDLE_APPLICATION_ID,
        _ => false,
    }
}

fn is_discord_default_application(command: &ApplicationCommandInfo) -> bool {
    command
        .raw
        .get("application_id")
        .and_then(|value| value.as_str())
        .is_some_and(|id| id == DISCORD_LOCAL_APPLICATION_ID)
        || command
            .application_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("Discord"))
}

#[cfg(test)]
mod tests;
