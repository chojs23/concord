use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, RwLock},
};

mod lifecycle;
mod rest_actions;

use crate::config::{MicrophoneSensitivityDb, VoiceVolumePercent};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use reqwest::header::HeaderValue;
use tokio::{
    sync::{Mutex as AsyncMutex, mpsc, watch},
    task::JoinHandle,
};

use crate::{AppError, Result};

use super::{
    ActivityInfo, ApplicationCommandInfo, ApplicationCommandInvocation, PresenceStatus,
    application_commands::application_command_interaction_from_invocation,
    events::{AppEvent, SequencedAppEvent},
    gateway::{GatewayCommand, GatewayRuntime, run_gateway},
    request_lifecycle::RequestLifecycle,
    rest::DiscordRest,
    state::{CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, SnapshotRevision},
    voice::{self, VoiceRuntimeEvent, VoiceScope},
};

const MEMBER_SEARCH_MIN_QUERY_CHARS: usize = 2;
const MEMBER_SEARCH_MAX_QUERY_CHARS: usize = 64;
const MEMBER_SEARCH_MAX_LIMIT: u16 = 10;
const OFFICIAL_WORDLE_APPLICATION_ID: u64 = 1_211_781_489_931_452_447;
const DISCORD_LOCAL_APPLICATION_ID: &str = "-1";
const GATEWAY_COMMAND_CHANNEL_CLOSED: &str = "gateway command channel closed";

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

    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.state
            .read()
            .expect("discord state lock is not poisoned")
            .current_user_id()
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
        self.send_gateway_command(GatewayCommand::RequestGuildMembers {
            guild_id,
            query: String::new(),
            limit: 0,
            presences: true,
            nonce: None,
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
        let nonce = format!("mention-ac-{}-{:016x}", guild_id.get(), query_hash(&query));
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
        let mut requested = self
            .requested_voice
            .write()
            .expect("requested voice lock is not poisoned");
        if voice_state_request_is_duplicate(*requested, scope, channel_id, self_mute, self_deaf) {
            return Ok(());
        }
        if let Some(channel_id) = channel_id {
            let requested_same_channel = requested
                .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                .is_some();
            if !requested_same_channel {
                let state = self
                    .state
                    .read()
                    .expect("discord state lock is not poisoned");
                let current_same_channel = state
                    .current_user_voice_connection()
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .is_some();
                // Permission gates apply only to guild voice channels; DM and
                // group-DM calls have no guild permission model to check.
                if !current_same_channel
                    && scope.guild_id().is_some()
                    && let Some(channel) = state.channel(channel_id)
                    && !state.can_connect_voice_channel(channel)
                {
                    return Err("cannot connect to voice channel".to_owned());
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
                let allow_microphone_transmit = requested
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .is_some_and(|voice| voice.allow_microphone_transmit);
                let microphone_sensitivity = requested
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .map(|voice| voice.microphone_sensitivity)
                    .unwrap_or_default();
                let microphone_volume = requested
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .map(|voice| voice.microphone_volume)
                    .unwrap_or_default();
                let voice_output_volume = requested
                    .filter(|voice| voice.scope == scope && voice.channel_id == channel_id)
                    .map(|voice| voice.voice_output_volume)
                    .unwrap_or_default();
                let voice = CurrentVoiceConnectionState {
                    scope,
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
        if voice.scope != scope || voice.channel_id != channel_id {
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
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        state
            .current_user_id()
            .map(|user_id| state.user_activities(user_id).to_vec())
            .unwrap_or_default()
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
        self.send_gateway_command(GatewayCommand::Shutdown)
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

    fn send_gateway_command(&self, command: GatewayCommand) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(command)
            .map_err(|_| GATEWAY_COMMAND_CHANNEL_CLOSED.to_owned())
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
        let commands = commands
            .into_iter()
            .filter(|command| !is_hidden_default_application_command(command))
            .collect::<Vec<_>>();
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
