use super::broadcast::{
    StreamBroadcastCaptureRegistry, StreamBroadcastGatewaySession, StreamBroadcastRuntimeState,
    run_stream_broadcast_session,
};
use super::stream::{StreamGatewaySession, StreamRuntimeState, run_stream_gateway_session};
use super::*;
use std::future::Future;
use tokio::sync::oneshot;

pub(super) const MAX_VOICE_RECONNECT_ATTEMPTS: u8 = 3;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum VoiceRuntimeAction {
    Connect(VoiceGatewaySession),
    Close,
}

struct VoiceRuntimeApplyResult {
    action: Option<VoiceRuntimeAction>,
    participant_playback_changed: bool,
}

struct ActiveBroadcastCaptureRequest {
    request_id: u64,
    stream_key: String,
}

impl ActiveBroadcastCaptureRequest {
    fn matches(&self, request_id: u64, stream_key: &str) -> bool {
        self.request_id == request_id && self.stream_key == stream_key
    }
}

struct BroadcastCapturePreparationTask {
    cancellation: capture::StreamCaptureCancellation,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct StreamWatchController {
    task: Option<JoinHandle<()>>,
    session: Option<StreamGatewaySession>,
}

impl StreamWatchController {
    async fn stop(&mut self, label: &str) -> Option<StreamGatewaySession> {
        stop_stream_connection_task(&mut self.task, &mut self.session, label).await
    }

    async fn replace(
        &mut self,
        session: StreamGatewaySession,
        events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
        status_publisher: VoiceStatusPublisher,
    ) {
        self.stop("stopping previous stream connection task before reconnect")
            .await;
        self.session = Some(session.clone());
        self.task = Some(tokio::spawn(run_stream_gateway_session(
            session,
            events_tx,
            status_publisher,
        )));
    }
}

#[derive(Default)]
struct StreamBroadcastController {
    task: Option<JoinHandle<()>>,
    session: Option<StreamBroadcastGatewaySession>,
    stop_tx: Option<oneshot::Sender<()>>,
    cleanup: Option<watch::Receiver<bool>>,
    capture_preparation: Option<BroadcastCapturePreparationTask>,
    capture_preparation_gate: Arc<Mutex<()>>,
    active_capture_request: Option<ActiveBroadcastCaptureRequest>,
    next_capture_request_id: u64,
    captures: StreamBroadcastCaptureRegistry,
}

impl StreamBroadcastController {
    fn is_current_capture_event(&self, event: &VoiceRuntimeEvent) -> bool {
        match event {
            VoiceRuntimeEvent::BroadcastStreamCaptureReady {
                request_id,
                stream_key,
            }
            | VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
                request_id,
                stream_key,
                ..
            } => self
                .active_capture_request
                .as_ref()
                .is_some_and(|active| active.matches(*request_id, stream_key)),
            _ => true,
        }
    }

    fn next_capture_request(
        &mut self,
        event: &VoiceRuntimeEvent,
    ) -> Option<(u64, String, StreamCaptureTarget)> {
        let VoiceRuntimeEvent::BroadcastStreamRequested(request) = event else {
            return None;
        };
        self.next_capture_request_id = self.next_capture_request_id.wrapping_add(1).max(1);
        Some((
            self.next_capture_request_id,
            request.stream_key.clone(),
            request.target.clone(),
        ))
    }

    fn connection_started(&self, event: &VoiceRuntimeEvent) -> bool {
        match (event, self.session.as_ref()) {
            (
                VoiceRuntimeEvent::BroadcastStreamConnectionEstablished {
                    connection_id,
                    stream_key,
                },
                Some(session),
            ) => {
                session.connection_id == *connection_id && session.request.stream_key == *stream_key
            }
            _ => false,
        }
    }

    fn stop(&mut self, stream_key: &str, retain_capture: bool) {
        if !retain_capture {
            if self
                .active_capture_request
                .as_ref()
                .is_some_and(|active| active.stream_key == stream_key)
            {
                self.active_capture_request = None;
                if let Some(preparation) = self.capture_preparation.as_ref() {
                    preparation.cancellation.cancel();
                }
            }
            self.captures.discard(stream_key);
        }
        let _ = stop_stream_broadcast_task(
            &mut self.task,
            &mut self.session,
            &mut self.stop_tx,
            &mut self.cleanup,
            "stopping active stream broadcast task",
        );
    }

    fn prepare_capture(
        &mut self,
        request_id: u64,
        stream_key: String,
        target: StreamCaptureTarget,
        events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    ) {
        if let Some(active) = self.active_capture_request.take() {
            self.captures.discard(&active.stream_key);
        }
        if let Some(previous) = self.capture_preparation.take() {
            previous.cancellation.cancel();
            previous.task.abort();
        }
        self.captures.activate(stream_key.clone(), request_id);
        self.active_capture_request = Some(ActiveBroadcastCaptureRequest {
            request_id,
            stream_key: stream_key.clone(),
        });
        let captures = self.captures.clone();
        let cancellation = capture::StreamCaptureCancellation::default();
        let preparation_cancellation = cancellation.clone();
        let task = tokio::spawn(run_broadcast_capture_preparation(
            request_id,
            stream_key,
            target,
            events_tx,
            captures,
            self.cleanup.clone(),
            Arc::clone(&self.capture_preparation_gate),
            preparation_cancellation,
        ));
        self.capture_preparation = Some(BroadcastCapturePreparationTask { cancellation, task });
    }

    fn capture_ready(
        &mut self,
        request_id: u64,
        stream_key: String,
        destination: Option<(VoiceScope, Id<ChannelMarker>)>,
        gateway_commands_tx: &mpsc::UnboundedSender<GatewayCommand>,
        events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    ) {
        self.capture_preparation.take();
        if let Some((scope, channel_id)) = destination {
            if gateway_commands_tx
                .send(GatewayCommand::CreateStream { scope, channel_id })
                .is_err()
            {
                let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
                    request_id,
                    stream_key,
                    error: "gateway command channel closed".to_owned(),
                });
            }
        } else {
            self.active_capture_request = None;
            self.captures.discard(&stream_key);
        }
    }

    fn capture_failed(&mut self, stream_key: &str) {
        self.capture_preparation.take();
        self.active_capture_request = None;
        self.captures.discard(stream_key);
    }

    fn replace(
        &mut self,
        session: StreamBroadcastGatewaySession,
        events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
        status_publisher: VoiceStatusPublisher,
        stream_preview_uploader: StreamPreviewUploader,
    ) {
        replace_stream_broadcast_task(
            &mut self.task,
            &mut self.session,
            &mut self.stop_tx,
            &mut self.cleanup,
            session,
            events_tx,
            status_publisher,
            stream_preview_uploader,
            self.captures.clone(),
            "stopping previous stream broadcast task before reconnect",
        );
    }

    fn active_session(&self) -> Option<&StreamBroadcastGatewaySession> {
        self.session.as_ref()
    }

    async fn shutdown(&mut self) {
        if let Some(active) = self.active_capture_request.take() {
            self.captures.discard(&active.stream_key);
        }
        cancel_broadcast_capture_preparation(&mut self.capture_preparation).await;
        let _ = shutdown_stream_broadcast_task(
            &mut self.task,
            &mut self.session,
            &mut self.stop_tx,
            &mut self.cleanup,
            "stopping stream broadcast task during voice runtime shutdown",
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_broadcast_capture_preparation(
    request_id: u64,
    stream_key: String,
    target: StreamCaptureTarget,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    captures: StreamBroadcastCaptureRegistry,
    cleanup: Option<watch::Receiver<bool>>,
    preparation_gate: Arc<Mutex<()>>,
    cancellation: capture::StreamCaptureCancellation,
) {
    if let Some(cleanup) = cleanup {
        logging::debug(
            "stream",
            "waiting for previous stream broadcast cleanup before capture",
        );
        tokio::select! {
            () = wait_for_stream_broadcast_cleanup(cleanup) => {}
            () = wait_for_capture_preparation_cancellation(&cancellation) => return,
        }
    }

    let preparation_guard = tokio::select! {
        guard = Arc::clone(&preparation_gate).lock_owned() => guard,
        () = wait_for_capture_preparation_cancellation(&cancellation) => return,
    };
    if cancellation.is_cancelled() {
        return;
    }

    logging::debug(
        "stream",
        "preparing stream capture before creating Discord stream",
    );
    let prepared_stream_key = stream_key.clone();
    let capture_cancellation = cancellation.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _preparation_guard = preparation_guard;
        captures.prepare(
            prepared_stream_key,
            request_id,
            target,
            capture_cancellation,
        )
    })
    .await
    .map_err(|error| format!("stream capture preparation task failed: {error}"))
    .and_then(|result| result);
    if cancellation.is_cancelled() {
        return;
    }
    match result {
        Ok(true) => {
            let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamCaptureReady {
                request_id,
                stream_key,
            });
        }
        Ok(false) => {}
        Err(error) => {
            let _ = events_tx.send(VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
                request_id,
                stream_key,
                error,
            });
        }
    }
}

async fn wait_for_stream_broadcast_cleanup(mut cleanup: watch::Receiver<bool>) {
    while !*cleanup.borrow() && cleanup.changed().await.is_ok() {}
}

async fn wait_for_capture_preparation_cancellation(
    cancellation: &capture::StreamCaptureCancellation,
) {
    while !cancellation.is_cancelled() {
        sleep(Duration::from_millis(20)).await;
    }
}

#[derive(Default)]
pub(super) struct VoiceRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    requested: Option<CurrentVoiceConnectionState>,
    current_voice: Option<ObservedSelfVoiceState>,
    server: Option<VoiceServerInfo>,
    active: Option<VoiceGatewaySession>,
    blocked: Option<VoiceGatewaySession>,
    reconnect_target: Option<VoiceGatewaySession>,
    reconnect_attempts: u8,
    push_to_talk: bool,
    push_to_talk_pressed: bool,
    audio_sources: VoiceAudioSources,
    audio_sources_generation: u64,
    participant_playback_settings: HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
    next_connection_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedSelfVoiceState {
    scope: VoiceScope,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

impl VoiceRuntimeState {
    #[cfg(test)]
    pub(super) fn apply(&mut self, event: VoiceRuntimeEvent) -> Option<VoiceRuntimeAction> {
        self.apply_with_changes(event).action
    }

    fn apply_with_changes(&mut self, event: VoiceRuntimeEvent) -> VoiceRuntimeApplyResult {
        let mut participant_playback_changed = false;
        match event {
            VoiceRuntimeEvent::Requested(requested) => {
                let target_changed = match (self.requested, requested) {
                    (Some(current), Some(next)) => {
                        current.scope != next.scope || current.channel_id != next.channel_id
                    }
                    (None, None) => false,
                    _ => true,
                };
                if target_changed {
                    self.push_to_talk_pressed = false;
                }
                if requested.is_none() || self.current_voice.is_none() {
                    self.blocked = None;
                }
                if let Some(next) = requested
                    && self.requested.is_some_and(|current| {
                        current.scope != next.scope || current.channel_id != next.channel_id
                    })
                {
                    self.server = None;
                }
                self.requested = requested;
                if self.requested.is_none() {
                    self.current_voice = None;
                    self.server = None;
                    return VoiceRuntimeApplyResult {
                        action: self.close_active(),
                        participant_playback_changed,
                    };
                }
            }
            VoiceRuntimeEvent::ManualRetry(requested) => {
                let target_changed = self.requested.is_none_or(|current| {
                    current.scope != requested.scope || current.channel_id != requested.channel_id
                });
                if target_changed {
                    self.server = None;
                    self.push_to_talk_pressed = false;
                }
                self.requested = Some(requested);
                self.blocked = None;
                self.reconnect_target = None;
                self.reconnect_attempts = 0;
            }
            VoiceRuntimeEvent::AudioSourcesChanged(sources) => {
                if self.audio_sources == sources {
                    return VoiceRuntimeApplyResult {
                        action: None,
                        participant_playback_changed,
                    };
                }
                self.audio_sources = sources;
                self.audio_sources_generation =
                    self.audio_sources_generation.wrapping_add(1).max(1);
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            VoiceRuntimeEvent::AudioSourcesApplyFailed {
                connection_id,
                generation,
                active_sources,
                ..
            } => {
                if self.audio_sources_generation == generation
                    && self
                        .active
                        .as_ref()
                        .is_some_and(|active| active.connection_id == connection_id)
                {
                    self.audio_sources = active_sources;
                }
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            #[cfg(feature = "voice-playback")]
            VoiceRuntimeEvent::PushToTalkEnabledChanged(enabled) => {
                if self.push_to_talk != enabled {
                    self.push_to_talk = enabled;
                    self.push_to_talk_pressed = false;
                }
            }
            #[cfg(feature = "voice-playback")]
            VoiceRuntimeEvent::PushToTalkPressed(pressed) => {
                self.push_to_talk_pressed = pressed;
            }
            VoiceRuntimeEvent::ReplaceParticipantPlaybackSettings(settings) => {
                let settings = settings
                    .into_iter()
                    .filter(|(_, settings)| {
                        *settings != VoiceParticipantPlaybackSettings::default()
                    })
                    .collect();
                participant_playback_changed = self.participant_playback_settings != settings;
                self.participant_playback_settings = settings;
            }
            VoiceRuntimeEvent::UpdateParticipantPlaybackSettings { user_id, settings } => {
                if settings == VoiceParticipantPlaybackSettings::default() {
                    participant_playback_changed = self
                        .participant_playback_settings
                        .remove(&user_id)
                        .is_some();
                } else {
                    participant_playback_changed =
                        self.participant_playback_settings.insert(user_id, settings)
                            != Some(settings);
                }
            }
            VoiceRuntimeEvent::CurrentUserReady(user_id) => {
                self.current_user_id = user_id;
            }
            VoiceRuntimeEvent::VoiceState(state) => {
                if let Some(action) = self.record_voice_state(state) {
                    return VoiceRuntimeApplyResult {
                        action: Some(action),
                        participant_playback_changed,
                    };
                }
            }
            VoiceRuntimeEvent::VoiceServer(server) => {
                if server.endpoint.is_none() {
                    self.server = None;
                    return VoiceRuntimeApplyResult {
                        action: self.close_active(),
                        participant_playback_changed,
                    };
                }
                self.server = Some(server);
            }
            VoiceRuntimeEvent::WatchStreamRequested(_)
            | VoiceRuntimeEvent::WatchStreamCancelled { .. }
            | VoiceRuntimeEvent::StreamCreate(_)
            | VoiceRuntimeEvent::StreamServer(_)
            | VoiceRuntimeEvent::StreamDelete(_)
            | VoiceRuntimeEvent::StreamConnectionEstablished { .. }
            | VoiceRuntimeEvent::StreamConnectionEnded { .. }
            | VoiceRuntimeEvent::BroadcastStreamRequested(_)
            | VoiceRuntimeEvent::BroadcastStreamCaptureReady { .. }
            | VoiceRuntimeEvent::BroadcastStreamCaptureFailed { .. }
            | VoiceRuntimeEvent::BroadcastStreamStopRequested { .. }
            | VoiceRuntimeEvent::BroadcastStreamConnectionEstablished { .. }
            | VoiceRuntimeEvent::BroadcastStreamConnectionStable { .. }
            | VoiceRuntimeEvent::BroadcastStreamConnectionEnded { .. } => {}
            #[cfg(test)]
            VoiceRuntimeEvent::BroadcastStreamCancelled { .. } => {}
            VoiceRuntimeEvent::ConnectionEstablished { connection_id } => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.connection_id == connection_id)
                {
                    self.reconnect_attempts = 0;
                }
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            VoiceRuntimeEvent::ConnectionEnded {
                connection_id,
                scope,
                channel_id,
                session_id,
                endpoint,
                outcome,
            } => {
                if let Some(active) = self
                    .active
                    .as_ref()
                    .filter(|active| {
                        active.matches_connection_end(
                            connection_id,
                            scope,
                            channel_id,
                            &session_id,
                            &endpoint,
                        )
                    })
                    .cloned()
                {
                    self.active = None;
                    if outcome == VoiceConnectionEnd::Stop {
                        self.blocked = Some(active);
                        return VoiceRuntimeApplyResult {
                            action: None,
                            participant_playback_changed,
                        };
                    }
                    if self.reconnect_attempts >= MAX_VOICE_RECONNECT_ATTEMPTS {
                        self.blocked = Some(active);
                        logging::debug(
                            "voice",
                            format!(
                                "voice reconnect limit reached after {} attempts",
                                MAX_VOICE_RECONNECT_ATTEMPTS
                            ),
                        );
                        return VoiceRuntimeApplyResult {
                            action: None,
                            participant_playback_changed,
                        };
                    }
                    self.reconnect_attempts += 1;
                    return VoiceRuntimeApplyResult {
                        action: self.connect_if_ready(),
                        participant_playback_changed,
                    };
                }
                return VoiceRuntimeApplyResult {
                    action: None,
                    participant_playback_changed,
                };
            }
            VoiceRuntimeEvent::Shutdown => {
                self.push_to_talk_pressed = false;
                return VoiceRuntimeApplyResult {
                    action: self.close_active(),
                    participant_playback_changed,
                };
            }
        }

        VoiceRuntimeApplyResult {
            action: self.connect_if_ready(),
            participant_playback_changed,
        }
    }

    fn record_voice_state(&mut self, state: VoiceStateInfo) -> Option<VoiceRuntimeAction> {
        if self.current_user_id != Some(state.user_id) {
            return None;
        }
        let requested = self.requested?;
        // A leave clears the channel; for a DM that also clears the scope, so we
        // treat any channel-less state for the current user as a disconnect.
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            self.server = None;
            self.push_to_talk_pressed = false;
            return self.close_active();
        };
        if state.scope() != Some(requested.scope) {
            return None;
        }
        let session_id = state
            .session_id
            .filter(|session_id| !session_id.is_empty())?;
        self.current_voice = Some(ObservedSelfVoiceState {
            scope: requested.scope,
            channel_id,
            session_id,
        });
        None
    }

    fn connect_if_ready(&mut self) -> Option<VoiceRuntimeAction> {
        let requested = self.requested?;
        let voice = self.current_voice.as_ref()?;
        if requested.scope != voice.scope || requested.channel_id != voice.channel_id {
            return self.close_active();
        }
        let server = self.server.as_ref()?;
        if server.scope() != Some(requested.scope) {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }
        let mut session = VoiceGatewaySession {
            connection_id: 0,
            scope: requested.scope,
            channel_id: requested.channel_id,
            user_id: self.current_user_id?,
            session_id: voice.session_id.clone(),
            endpoint,
            token: server.token.clone(),
        };
        if self.reconnect_target.as_ref() != Some(&session) {
            self.reconnect_target = Some(session.clone());
            self.reconnect_attempts = 0;
        }
        if self.active.as_ref() == Some(&session) {
            return None;
        }
        if self.blocked.as_ref() == Some(&session) {
            return None;
        }
        self.blocked = None;
        self.next_connection_id = self.next_connection_id.wrapping_add(1).max(1);
        session.connection_id = self.next_connection_id;
        self.active = Some(session.clone());
        Some(VoiceRuntimeAction::Connect(session))
    }

    fn close_active(&mut self) -> Option<VoiceRuntimeAction> {
        self.active.take().map(|_| VoiceRuntimeAction::Close)
    }

    pub(super) fn capture_gate(&self) -> Option<VoiceCaptureGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.scope != requested.scope || active.channel_id != requested.channel_id {
            return None;
        }
        let capture_enabled = requested.allow_microphone_transmit && !requested.self_mute;
        Some(VoiceCaptureGate {
            capture_enabled,
            transmit_enabled: capture_enabled && (!self.push_to_talk || self.push_to_talk_pressed),
            use_voice_activity: !self.push_to_talk,
            noise_suppression: requested.noise_suppression,
            microphone_sensitivity: requested.microphone_sensitivity,
            microphone_volume: requested.microphone_volume,
        })
    }

    pub(super) fn playback_gate(&self) -> Option<VoicePlaybackGate> {
        let active = self.active.as_ref()?;
        let requested = self.requested?;
        if active.scope != requested.scope || active.channel_id != requested.channel_id {
            return None;
        }
        Some(VoicePlaybackGate {
            enabled: !requested.self_deaf,
            volume: requested.voice_output_volume,
        })
    }

    pub(super) fn audio_source_selection(&self) -> VoiceAudioSourceSelection {
        VoiceAudioSourceSelection {
            generation: self.audio_sources_generation,
            sources: self.audio_sources.clone(),
        }
    }
}

pub(crate) fn forward_app_event(
    sender: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    event: &AppEvent,
) {
    let runtime_event = match event {
        AppEvent::Ready { user_id, .. } => VoiceRuntimeEvent::CurrentUserReady(*user_id),
        AppEvent::VoiceStateUpdate { state } => VoiceRuntimeEvent::VoiceState(state.clone()),
        AppEvent::VoiceServerUpdate { server } => VoiceRuntimeEvent::VoiceServer(server.clone()),
        AppEvent::StreamCreate { stream } => VoiceRuntimeEvent::StreamCreate(stream.clone()),
        AppEvent::StreamServerUpdate { server } => VoiceRuntimeEvent::StreamServer(server.clone()),
        AppEvent::StreamDelete { stream } => VoiceRuntimeEvent::StreamDelete(stream.clone()),
        _ => return,
    };
    let _ = sender.send(runtime_event);
}

pub(crate) async fn run_voice_runtime(
    mut events: mpsc::UnboundedReceiver<VoiceRuntimeEvent>,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    status_publisher: VoiceStatusPublisher,
    stream_preview_uploader: StreamPreviewUploader,
) {
    let mut state = VoiceRuntimeState::default();
    let mut stream_state = StreamRuntimeState::default();
    let mut broadcast_state = StreamBroadcastRuntimeState::default();
    let mut connection_task: Option<JoinHandle<()>> = None;
    let mut connection_session: Option<VoiceGatewaySession> = None;
    let mut audio_sources_tx: Option<watch::Sender<VoiceAudioSourceSelection>> = None;
    let mut capture_gate_tx: Option<mpsc::UnboundedSender<VoiceCaptureGate>> = None;
    let mut playback_gate_tx: Option<mpsc::UnboundedSender<VoicePlaybackGate>> = None;
    let mut participant_playback_tx: Option<
        watch::Sender<HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>>,
    > = None;
    let mut stream_controller = StreamWatchController::default();
    let mut broadcast_controller = StreamBroadcastController::default();

    while let Some(event) = events.recv().await {
        if !broadcast_controller.is_current_capture_event(&event) {
            logging::debug("stream", "ignoring stale stream capture preparation result");
            continue;
        }
        let shutdown = matches!(event, VoiceRuntimeEvent::Shutdown);
        let broadcast_capture_request = broadcast_controller.next_capture_request(&event);
        let broadcast_capture_ready = match &event {
            VoiceRuntimeEvent::BroadcastStreamCaptureReady {
                request_id,
                stream_key,
            } => Some((*request_id, stream_key.clone())),
            _ => None,
        };
        let broadcast_capture_failed = match &event {
            VoiceRuntimeEvent::BroadcastStreamCaptureFailed {
                request_id,
                stream_key,
                ..
            } => Some((*request_id, stream_key.clone())),
            _ => None,
        };
        let changed_audio_sources = matches!(
            &event,
            VoiceRuntimeEvent::AudioSourcesChanged(sources) if state.audio_sources != *sources
        );
        let audio_sources_apply_failure = match &event {
            VoiceRuntimeEvent::AudioSourcesApplyFailed {
                connection_id,
                generation,
                requested_sources,
                active_sources,
                message,
            } if state.audio_sources_generation == *generation
                && state
                    .active
                    .as_ref()
                    .is_some_and(|active| active.connection_id == *connection_id) =>
            {
                Some((
                    requested_sources.clone(),
                    active_sources.clone(),
                    message.clone(),
                ))
            }
            _ => None,
        };
        let broadcast_started = broadcast_controller.connection_started(&event);
        let stream_update = stream_state.apply(&event);
        let broadcast_update = broadcast_state.apply(&event);
        let VoiceRuntimeApplyResult {
            action,
            participant_playback_changed,
        } = state.apply_with_changes(event);
        if let Some((requested_sources, active_sources, message)) = audio_sources_apply_failure {
            status_publisher
                .publish_audio_sources_apply_failed(requested_sources, active_sources, message)
                .await;
        }
        if let Some(error) = stream_update.error {
            status_publisher.publish_error(error).await;
        }
        if let Some(ended) = stream_update.playback_ended {
            status_publisher
                .publish_stream_playback_ended(
                    ended.request.scope,
                    ended.request.channel_id,
                    ended.request.owner_id,
                    ended.reconnecting,
                )
                .await;
        }
        if let Some(stream_key) = stream_update.close_stream_key {
            let _ = stream_controller
                .stop("stopping active stream connection task")
                .await;
            if stream_update.send_delete {
                let _ = gateway_commands_tx.send(GatewayCommand::DeleteStream { stream_key });
            }
        }
        if let Some(session) = stream_update.connect {
            stream_controller
                .replace(session, events_tx.clone(), status_publisher.clone())
                .await;
        }
        if let Some(error) = broadcast_update.error {
            status_publisher.publish_error(error).await;
        }
        if let Some(ended) = broadcast_update.broadcast_ended {
            status_publisher
                .publish_stream_broadcast_ended(ended.scope, ended.channel_id)
                .await;
        }
        let retaining_broadcast_capture = broadcast_update.retain_capture;
        if let Some(stream_key) = broadcast_update.close_stream_key {
            broadcast_controller.stop(&stream_key, retaining_broadcast_capture);
            if broadcast_update.send_delete {
                let _ = gateway_commands_tx.send(GatewayCommand::DeleteStream { stream_key });
            }
        }
        if let Some((request_id, stream_key, target)) = broadcast_capture_request {
            broadcast_controller.prepare_capture(request_id, stream_key, target, events_tx.clone());
        }
        if let Some((request_id, stream_key)) = broadcast_capture_ready {
            let destination = broadcast_state.requested_destination(&stream_key);
            broadcast_controller.capture_ready(
                request_id,
                stream_key,
                destination,
                &gateway_commands_tx,
                &events_tx,
            );
        }
        if let Some((_request_id, stream_key)) = broadcast_capture_failed {
            broadcast_controller.capture_failed(&stream_key);
        }
        if let Some(session) = broadcast_update.connect {
            broadcast_controller.replace(
                session,
                events_tx.clone(),
                status_publisher.clone(),
                stream_preview_uploader.clone(),
            );
        }
        if broadcast_started && let Some(session) = broadcast_controller.active_session() {
            status_publisher
                .publish_stream_broadcast_started(session.request.scope, session.request.channel_id)
                .await;
        }
        let connected_this_event = matches!(&action, Some(VoiceRuntimeAction::Connect(_)));
        if let Some(action) = action {
            match action {
                VoiceRuntimeAction::Connect(session) => {
                    if let Some(stopped_session) = stop_voice_connection_task(
                        &mut connection_task,
                        &mut connection_session,
                        &mut audio_sources_tx,
                        &mut capture_gate_tx,
                        &mut playback_gate_tx,
                        "stopping previous voice connection task before reconnect",
                    )
                    .await
                    {
                        status_publisher
                            .publish_speaking(&stopped_session, stopped_session.user_id, false)
                            .await;
                    }
                    let (next_capture_gate_tx, capture_gate_rx) = mpsc::unbounded_channel();
                    let (next_playback_gate_tx, playback_gate_rx) = mpsc::unbounded_channel();
                    let (next_audio_sources_tx, audio_sources_rx) =
                        watch::channel(state.audio_source_selection());
                    let (next_participant_playback_tx, participant_playback_rx) =
                        watch::channel(state.participant_playback_settings.clone());
                    capture_gate_tx = Some(next_capture_gate_tx);
                    playback_gate_tx = Some(next_playback_gate_tx);
                    audio_sources_tx = Some(next_audio_sources_tx);
                    participant_playback_tx = Some(next_participant_playback_tx);
                    let initial_capture_gate = state.capture_gate().unwrap_or(VoiceCaptureGate {
                        capture_enabled: false,
                        transmit_enabled: false,
                        use_voice_activity: true,
                        noise_suppression: false,
                        microphone_sensitivity: MicrophoneSensitivityDb::default(),
                        microphone_volume: VoiceVolumePercent::default(),
                    });
                    let initial_playback_gate =
                        state.playback_gate().unwrap_or(VoicePlaybackGate {
                            enabled: true,
                            volume: VoiceVolumePercent::default(),
                        });
                    connection_session = Some(session.clone());
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(
                        session,
                        events_tx.clone(),
                        status_publisher.clone(),
                        VoiceGatewayControls {
                            audio_sources_rx,
                            initial_capture_gate,
                            capture_gate_rx,
                            initial_playback_gate,
                            playback_gate_rx,
                            participant_playback_rx,
                        },
                    )));
                }
                VoiceRuntimeAction::Close => {
                    if let Some(stopped_session) = stop_voice_connection_task(
                        &mut connection_task,
                        &mut connection_session,
                        &mut audio_sources_tx,
                        &mut capture_gate_tx,
                        &mut playback_gate_tx,
                        "stopping active voice connection task",
                    )
                    .await
                    {
                        status_publisher
                            .publish_speaking(&stopped_session, stopped_session.user_id, false)
                            .await;
                    }
                    participant_playback_tx = None;
                }
            }
        }
        if state.active.is_none() {
            audio_sources_tx = None;
            capture_gate_tx = None;
            playback_gate_tx = None;
            participant_playback_tx = None;
        }
        if let (Some(capture_gate_tx), Some(capture_gate)) =
            (capture_gate_tx.as_ref(), state.capture_gate())
        {
            let _ = capture_gate_tx.send(capture_gate);
        }
        if let (Some(playback_gate_tx), Some(playback_gate)) =
            (playback_gate_tx.as_ref(), state.playback_gate())
        {
            let _ = playback_gate_tx.send(playback_gate);
        }
        if !connected_this_event
            && changed_audio_sources
            && let Some(audio_sources_tx) = audio_sources_tx.as_mut()
        {
            audio_sources_tx.send_replace(state.audio_source_selection());
        }
        if participant_playback_changed
            && !connected_this_event
            && let Some(participant_playback_tx) = participant_playback_tx.as_mut()
        {
            participant_playback_tx.send_replace(state.participant_playback_settings.clone());
        }
        if shutdown {
            break;
        }
    }

    if let Some(stopped_session) = stop_voice_connection_task(
        &mut connection_task,
        &mut connection_session,
        &mut audio_sources_tx,
        &mut capture_gate_tx,
        &mut playback_gate_tx,
        "stopping voice connection task during voice runtime shutdown",
    )
    .await
    {
        status_publisher
            .publish_speaking(&stopped_session, stopped_session.user_id, false)
            .await;
    }
    let _ = stream_controller
        .stop("stopping stream connection task during voice runtime shutdown")
        .await;
    broadcast_controller.shutdown().await;
}

async fn stop_stream_connection_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamGatewaySession>,
    label: &str,
) -> Option<StreamGatewaySession> {
    let stopped_session = stream_session.take();
    let Some(mut task) = stream_task.take() else {
        return stopped_session;
    };
    logging::debug("stream", label);
    task.abort();
    let _ = timeout(Duration::from_millis(100), &mut task).await;
    stopped_session
}

async fn cancel_broadcast_capture_preparation(
    preparation: &mut Option<BroadcastCapturePreparationTask>,
) {
    let Some(mut preparation) = preparation.take() else {
        return;
    };
    preparation.cancellation.cancel();
    if timeout(VOICE_CONNECTION_SHUTDOWN_TIMEOUT, &mut preparation.task)
        .await
        .is_err()
    {
        logging::debug(
            "stream",
            "stream capture preparation did not stop before shutdown timeout",
        );
        preparation.task.abort();
    }
}

fn stop_stream_broadcast_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamBroadcastGatewaySession>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    cleanup: &mut Option<watch::Receiver<bool>>,
    label: &str,
) -> Option<StreamBroadcastGatewaySession> {
    let stopped_session = stream_session.take();
    let stopping_active_task = stopped_session.is_some() || stop_tx.is_some();
    if let Some(stop_tx) = stop_tx.take() {
        let _ = stop_tx.send(());
    }
    if !stopping_active_task {
        return stopped_session;
    }
    let Some(mut task) = stream_task.take() else {
        cleanup.take();
        return stopped_session;
    };
    logging::debug("stream", label);
    let (cleanup_tx, cleanup_rx) = watch::channel(false);
    *cleanup = Some(cleanup_rx);
    *stream_task = Some(tokio::spawn(async move {
        reap_stream_broadcast_task(&mut task).await;
        cleanup_tx.send_replace(true);
    }));
    stopped_session
}

async fn shutdown_stream_broadcast_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamBroadcastGatewaySession>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    cleanup: &mut Option<watch::Receiver<bool>>,
    label: &str,
) -> Option<StreamBroadcastGatewaySession> {
    cleanup.take();
    let stopped_session = stream_session.take();
    let stopping_active_task = stopped_session.is_some() || stop_tx.is_some();
    if let Some(stop_tx) = stop_tx.take() {
        let _ = stop_tx.send(());
    }
    let Some(mut task) = stream_task.take() else {
        return stopped_session;
    };
    logging::debug("stream", label);
    if stopping_active_task {
        reap_stream_broadcast_task(&mut task).await;
    } else {
        // A normal stop keeps its bounded reaper in `stream_task` so a later
        // replacement can wait for it. Final shutdown waits for that reaper
        // directly rather than detaching it with the runtime.
        match task.await {
            Ok(()) => logging::debug("stream", "stream broadcast cleanup finished"),
            Err(error) => {
                logging::debug("stream", format!("stream broadcast cleanup ended: {error}"));
            }
        }
    }
    stopped_session
}

#[allow(clippy::too_many_arguments)]
fn replace_stream_broadcast_task(
    stream_task: &mut Option<JoinHandle<()>>,
    stream_session: &mut Option<StreamBroadcastGatewaySession>,
    stop_tx: &mut Option<oneshot::Sender<()>>,
    cleanup: &mut Option<watch::Receiver<bool>>,
    session: StreamBroadcastGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    stream_preview_uploader: StreamPreviewUploader,
    broadcast_captures: StreamBroadcastCaptureRegistry,
    label: &str,
) {
    cleanup.take();
    if let Some(stop_tx) = stop_tx.take() {
        let _ = stop_tx.send(());
    }
    let previous = stream_task.take();
    if previous.is_some() {
        logging::debug("stream", label);
    }
    let (next_stop_tx, stop_rx) = oneshot::channel();
    *stop_tx = Some(next_stop_tx);
    *stream_session = Some(session.clone());
    *stream_task = Some(tokio::spawn(run_after_stream_broadcast_task(
        previous,
        run_stream_broadcast_session(
            session,
            events_tx,
            status_publisher,
            stream_preview_uploader,
            broadcast_captures,
            stop_rx,
        ),
    )));
}

async fn run_after_stream_broadcast_task(
    previous: Option<JoinHandle<()>>,
    next: impl Future<Output = ()>,
) {
    if let Some(mut previous) = previous {
        reap_stream_broadcast_task(&mut previous).await;
    }
    next.await;
}

async fn reap_stream_broadcast_task(task: &mut JoinHandle<()>) {
    match timeout(VOICE_CONNECTION_SHUTDOWN_TIMEOUT, &mut *task).await {
        Ok(Ok(())) => {
            logging::debug("stream", "stream broadcast task stopped cleanly");
        }
        Ok(Err(error)) => {
            logging::debug("stream", format!("stream broadcast task ended: {error}"));
        }
        Err(_) => {
            logging::debug("stream", "stream broadcast graceful stop timed out");
            task.abort();
            let _ = timeout(Duration::from_millis(100), &mut *task).await;
        }
    }
}

pub(super) async fn stop_voice_connection_task(
    connection_task: &mut Option<JoinHandle<()>>,
    connection_session: &mut Option<VoiceGatewaySession>,
    audio_sources_tx: &mut Option<watch::Sender<VoiceAudioSourceSelection>>,
    capture_gate_tx: &mut Option<mpsc::UnboundedSender<VoiceCaptureGate>>,
    playback_gate_tx: &mut Option<mpsc::UnboundedSender<VoicePlaybackGate>>,
    label: &str,
) -> Option<VoiceGatewaySession> {
    audio_sources_tx.take();
    capture_gate_tx.take();
    playback_gate_tx.take();
    let stopped_session = connection_session.take();
    let Some(mut task) = connection_task.take() else {
        return stopped_session;
    };
    logging::debug("voice", label);
    match timeout(VOICE_CONNECTION_SHUTDOWN_TIMEOUT, &mut task).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            logging::debug("voice", format!("voice connection task ended: {error}"));
            stopped_session
        }
        Err(_) => {
            logging::debug("voice", "voice connection graceful stop timed out");
            task.abort();
            let _ = timeout(Duration::from_millis(100), &mut task).await;
            stopped_session
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DropNotice(Option<oneshot::Sender<()>>);

    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn stopping_stream_watch_aborts_its_gateway_task() {
        let (started_tx, started_rx) = oneshot::channel();
        let (stopped_tx, stopped_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _notice = DropNotice(Some(stopped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        let mut controller = StreamWatchController {
            task: Some(task),
            session: None,
        };
        started_rx.await.expect("stream watch task should start");

        controller.stop("stopping test stream watch").await;

        timeout(Duration::from_secs(1), stopped_rx)
            .await
            .expect("stream watch task should stop")
            .expect("stream watch task should report cleanup");
        assert!(controller.task.is_none());
    }

    #[tokio::test]
    async fn cancelling_capture_preparation_signals_and_joins_the_task() {
        let cancellation = capture::StreamCaptureCancellation::default();
        let observed_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            while !observed_cancellation.is_cancelled() {
                tokio::task::yield_now().await;
            }
        });
        let mut preparation = Some(BroadcastCapturePreparationTask { cancellation, task });

        cancel_broadcast_capture_preparation(&mut preparation).await;

        assert!(preparation.is_none());
    }

    #[tokio::test]
    async fn broadcast_stop_does_not_block_voice_runtime_cleanup() {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (cleaned_tx, cleaned_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            let _ = stop_rx.await;
            let _ = release_rx.await;
            let _ = cleaned_tx.send(());
        });
        let mut controller = StreamBroadcastController {
            task: Some(task),
            stop_tx: Some(stop_tx),
            ..StreamBroadcastController::default()
        };
        started_rx.await.expect("test broadcast task should start");

        controller.stop("guild:10:20:30", false);

        release_tx
            .send(())
            .expect("test broadcast cleanup should be released");
        cleaned_rx
            .await
            .expect("broadcast task should still clean up asynchronously");
        let cleanup_task = controller
            .task
            .take()
            .expect("broadcast cleanup task should remain tracked");
        cleanup_task
            .await
            .expect("broadcast cleanup tracker should finish");
        assert!(controller.stop_tx.is_none());
    }

    #[tokio::test]
    async fn replacement_broadcast_waits_for_previous_cleanup() {
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let (cleaned_tx, cleaned_rx) = tokio::sync::oneshot::channel();
        let previous = tokio::spawn(async move {
            let _ = release_rx.await;
            let _ = cleaned_tx.send(());
        });
        let (replacement_tx, mut replacement_rx) = tokio::sync::oneshot::channel();
        let replacement = tokio::spawn(run_after_stream_broadcast_task(
            Some(previous),
            async move {
                let _ = replacement_tx.send(());
            },
        ));

        assert!(
            timeout(Duration::from_millis(25), &mut replacement_rx)
                .await
                .is_err(),
            "replacement must not start while previous cleanup is pending"
        );
        release_tx
            .send(())
            .expect("previous broadcast cleanup should be released");
        cleaned_rx
            .await
            .expect("previous broadcast should report cleanup");
        timeout(Duration::from_secs(1), &mut replacement_rx)
            .await
            .expect("replacement should start after cleanup")
            .expect("replacement should report startup");
        replacement
            .await
            .expect("replacement sequencing task should finish");
    }

    #[tokio::test]
    async fn capture_preparation_waits_for_tracked_broadcast_cleanup() {
        let (cleanup_tx, cleanup_rx) = watch::channel(false);
        let wait = wait_for_stream_broadcast_cleanup(cleanup_rx);
        tokio::pin!(wait);

        assert!(
            timeout(Duration::from_millis(25), &mut wait).await.is_err(),
            "capture preparation must wait while the previous portal session is cleaning up"
        );
        cleanup_tx.send_replace(true);
        timeout(Duration::from_secs(1), &mut wait)
            .await
            .expect("capture preparation cleanup barrier should finish");
    }

    #[tokio::test]
    async fn controller_starts_capture_preparation_without_waiting_for_cleanup() {
        let (_cleanup_tx, cleanup_rx) = watch::channel(false);
        let mut controller = StreamBroadcastController {
            cleanup: Some(cleanup_rx),
            ..StreamBroadcastController::default()
        };
        let (events_tx, _events_rx) = mpsc::unbounded_channel();

        controller.prepare_capture(
            1,
            "guild:10:20:30".to_owned(),
            StreamCaptureTarget {
                kind: StreamCaptureTargetKind::Portal,
                id: 0,
                title: "Screen or window...".to_owned(),
            },
            events_tx,
        );

        let preparation = controller
            .capture_preparation
            .take()
            .expect("capture preparation should be tracked immediately");
        preparation.cancellation.cancel();
        preparation.task.abort();
        let _ = preparation.task.await;
    }

    #[tokio::test]
    async fn final_broadcast_shutdown_waits_for_active_and_tracked_cleanup() {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (cleaned_tx, cleaned_rx) = tokio::sync::oneshot::channel();
        let active_task = tokio::spawn(async move {
            let _ = stop_rx.await;
            let _ = cleaned_tx.send(());
        });
        let mut active_controller = StreamBroadcastController {
            task: Some(active_task),
            stop_tx: Some(stop_tx),
            ..StreamBroadcastController::default()
        };

        active_controller.shutdown().await;
        cleaned_rx
            .await
            .expect("active broadcast cleanup should finish before shutdown returns");
        assert!(active_controller.task.is_none());

        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let tracked_cleanup = tokio::spawn(async move {
            let _ = release_rx.await;
        });
        let shutdown = tokio::spawn(async move {
            let mut controller = StreamBroadcastController {
                task: Some(tracked_cleanup),
                ..StreamBroadcastController::default()
            };
            controller.shutdown().await;
        });
        tokio::pin!(shutdown);

        assert!(
            timeout(Duration::from_millis(25), &mut shutdown)
                .await
                .is_err(),
            "final shutdown must wait for a previously tracked cleanup task"
        );
        release_tx
            .send(())
            .expect("tracked broadcast cleanup should be released");
        timeout(Duration::from_secs(1), &mut shutdown)
            .await
            .expect("tracked broadcast cleanup should complete")
            .expect("final shutdown task should join");
    }
}
