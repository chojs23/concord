use std::net::{Ipv4Addr, SocketAddrV4};

use socket2::{Domain, Protocol, Socket, Type};

use super::media::GatewayChildTasks;
use super::*;

const VOICE_UDP_RECEIVE_BUFFER_BYTES: usize = 4 * 1024 * 1024;

pub(super) async fn run_voice_gateway_session(
    session: VoiceGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    controls: VoiceGatewayControls,
) {
    let connection = connect_voice_gateway(&session, &events_tx, &status_publisher, controls).await;
    let outcome = match connection {
        Ok(outcome) => {
            status_publisher
                .publish(
                    &session,
                    VoiceConnectionStatus::Disconnected,
                    "Voice gateway disconnected",
                )
                .await;
            outcome
        }
        Err(error) => {
            logging::error("voice", &error);
            status_publisher
                .publish(&session, VoiceConnectionStatus::Failed, error)
                .await;
            VoiceConnectionEnd::Reconnect
        }
    };
    let _ = events_tx.send(session.connection_ended_event(outcome));
}

pub(super) async fn connect_voice_gateway(
    session: &VoiceGatewaySession,
    events_tx: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: &VoiceStatusPublisher,
    controls: VoiceGatewayControls,
) -> Result<VoiceConnectionEnd, String> {
    let VoiceGatewayControls {
        mut audio_sources_rx,
        initial_capture_gate,
        mut capture_gate_rx,
        initial_playback_gate,
        mut playback_gate_rx,
        participant_playback_rx,
    } = controls;
    let url = voice_gateway_url(&session.endpoint)?;
    logging::debug("voice", format!("connecting voice websocket: {url}"));
    let connect_started = Instant::now();
    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(&url))
        .await
        .map_err(|_| "voice websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("voice websocket connect failed: {error}"))?;
    logging::debug(
        "voice",
        format!(
            "voice websocket connected: status={} elapsed_ms={}",
            response.status(),
            connect_started.elapsed().as_millis()
        ),
    );
    status_publisher
        .publish(
            session,
            VoiceConnectionStatus::Connected,
            "Voice gateway connected",
        )
        .await;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut child_tasks = VoiceChildTasks::default();
    let initial_audio_sources = audio_sources_rx.borrow_and_update().clone();
    let requested_audio_sources = initial_audio_sources.sources;
    let initial_audio_sources_outcome =
        child_tasks.set_voice_audio_sources(requested_audio_sources.clone(), initial_capture_gate);
    let mut current_audio_sources = initial_audio_sources_outcome.active_sources;
    if let Some(message) = initial_audio_sources_outcome.error {
        let _ = events_tx.send(VoiceRuntimeEvent::AudioSourcesApplyFailed {
            connection_id: session.connection_id,
            generation: initial_audio_sources.generation,
            requested_sources: requested_audio_sources,
            active_sources: current_audio_sources.clone(),
            message,
        });
    }
    let audio_runtime = VoiceAudioRuntime::start()?;
    let audio_handle = audio_runtime.handle().clone();
    child_tasks.audio_runtime = Some(audio_runtime);
    let mut speaking_tracker = VoiceSpeakingTracker::new(session.user_id);
    let mut speaking_sweep = tokio::time::interval(VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL);
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
    let (local_speaking_tx, mut local_speaking_rx) = mpsc::unbounded_channel();
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
    let (transmit_failure_tx, mut transmit_failure_rx) = mpsc::unbounded_channel::<String>();
    let (remote_speaking_tx, mut remote_speaking_rx) = mpsc::unbounded_channel();
    #[cfg_attr(
        not(feature = "voice-playback"),
        allow(unused_mut, unused_variables, unused_assignments)
    )]
    let mut current_capture_gate = initial_capture_gate;
    let mut current_playback_gate = initial_playback_gate;
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    let mut current_session_description: Option<VoiceSessionDescription> = None;
    #[cfg_attr(
        not(feature = "voice-playback"),
        allow(unused_mut, unused_variables, unused_assignments)
    )]
    let mut voice_ready: Option<VoiceTransportSession> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let heartbeat_ack = Arc::new(Mutex::new(VoiceHeartbeatAckState::default()));
    let (heartbeat_timeout_tx, mut heartbeat_timeout_rx) =
        mpsc::unbounded_channel::<VoiceHeartbeatTimeout>();
    let dave_state = Arc::new(Mutex::new(VoiceDaveState::new(session)));

    let result: Result<VoiceConnectionEnd, String> = async {
    send_voice_text(&writer, voice_identify_payload(session)).await?;
    logging::debug("voice", "voice identify sent");
    logging::debug("voice", "voice websocket read loop started");
    let mut resume_pending = false;
    let mut resume_deadline: Option<Instant> = None;
    let mut connection_generation = 0u64;
    let mut connection_stable_deadline: Option<Instant> = None;

    loop {
        let frame = tokio::select! {
            audio_sources = audio_sources_rx.changed() => {
                match audio_sources {
                    Ok(()) => {
                        let selection = audio_sources_rx.borrow_and_update().clone();
                        let requested_audio_sources = selection.sources;
                        let outcome = child_tasks.set_voice_audio_sources(
                            requested_audio_sources.clone(),
                            current_capture_gate,
                        );
                        current_audio_sources = outcome.active_sources;
                        if let Some(message) = outcome.error {
                            let _ = events_tx.send(VoiceRuntimeEvent::AudioSourcesApplyFailed {
                                connection_id: session.connection_id,
                                generation: selection.generation,
                                requested_sources: requested_audio_sources,
                                active_sources: current_audio_sources.clone(),
                                message,
                            });
                        }
                        continue;
                    }
                    Err(_) => break,
                }
            }
            capture_gate = capture_gate_rx.recv() => {
                match capture_gate {
                    Some(capture_gate) => {
                        #[cfg(feature = "voice-playback")]
                        {
                            current_capture_gate = capture_gate;
                        }
                        child_tasks.set_voice_transmit_gate(capture_gate);
                        continue;
                    }
                    None => {
                        child_tasks.set_voice_transmit_gate(VoiceCaptureGate {
                            capture_enabled: false,
                            transmit_enabled: false,
                            use_voice_activity: true,
                            noise_suppression: false,
                            microphone_sensitivity: MicrophoneSensitivityDb::default(),
                            microphone_volume: VoiceVolumePercent::default(),
                        });
                        break;
                    }
                }
            }
            playback_gate = playback_gate_rx.recv() => {
                match playback_gate {
                    Some(playback_gate) => {
                        current_playback_gate = playback_gate;
                        child_tasks.set_voice_playback_gate(playback_gate);
                        continue;
                    }
                    None => {
                        child_tasks.set_voice_playback_gate(VoicePlaybackGate {
                            enabled: false,
                            volume: VoiceVolumePercent::default(),
                        });
                        break;
                    }
                }
            }
            local_speaking = local_speaking_rx.recv() => {
                let Some(local_speaking) = local_speaking else {
                    break;
                };
                if let Some(speaking) = speaking_tracker.record_local(local_speaking) {
                    status_publisher
                        .publish_speaking(session, session.user_id, speaking)
                        .await;
                }
                continue;
            }
            remote_speaking = remote_speaking_rx.recv() => {
                let Some(user_id) = remote_speaking else {
                    break;
                };
                if let Some(speaking) = speaking_tracker.record_remote(user_id, true, Instant::now()) {
                    status_publisher.publish_speaking(session, user_id, speaking).await;
                }
                continue;
            }
            transmit_failure = transmit_failure_rx.recv() => {
                let Some(error) = transmit_failure else {
                    break;
                };
                logging::error(
                    "voice",
                    format!("voice UDP transmit failed, reconnecting: {error}"),
                );
                return Ok(VoiceConnectionEnd::Reconnect);
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                connection_stable_deadline.unwrap_or_else(Instant::now)
            )), if connection_stable_deadline.is_some() => {
                connection_stable_deadline = None;
                let _ = events_tx.send(session.connection_established_event());
                continue;
            }
            _ = speaking_sweep.tick() => {
                for user_id in speaking_tracker.expire_remote(Instant::now()) {
                    status_publisher.publish_speaking(session, user_id, false).await;
                }
                continue;
            }
            heartbeat_timeout = heartbeat_timeout_rx.recv() => {
                match heartbeat_timeout {
                    Some(timeout) if timeout.generation == connection_generation => None,
                    Some(_) => continue,
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(
                resume_deadline.unwrap_or_else(Instant::now)
            )), if resume_deadline.is_some() => {
                logging::debug("voice", "voice resume handshake timed out");
                return Ok(VoiceConnectionEnd::Reconnect);
            }
            frame = reader.next() => Some(frame),
        };
        let Some(frame) = frame else {
            if resume_pending {
                return Ok(VoiceConnectionEnd::Reconnect);
            }
            connection_generation = connection_generation.wrapping_add(1);
            reader = resume_voice_gateway(
                &url,
                &writer,
                session,
                &last_sequence,
                &heartbeat_ack,
                &mut child_tasks,
                "heartbeat ACK timed out",
            )
            .await?;
            resume_pending = true;
            resume_deadline = Some(Instant::now() + VOICE_RESUME_HANDSHAKE_TIMEOUT);
            continue;
        };
        let Some(frame) = frame else {
            if resume_pending {
                return Ok(VoiceConnectionEnd::Reconnect);
            }
            connection_generation = connection_generation.wrapping_add(1);
            reader = resume_voice_gateway(
                &url,
                &writer,
                session,
                &last_sequence,
                &heartbeat_ack,
                &mut child_tasks,
                "websocket stream ended",
            )
            .await?;
            resume_pending = true;
            resume_deadline = Some(Instant::now() + VOICE_RESUME_HANDSHAKE_TIMEOUT);
            continue;
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                if resume_pending {
                    logging::debug(
                        "voice",
                        format!("voice resume websocket failed before completion: {error}"),
                    );
                    return Ok(VoiceConnectionEnd::Reconnect);
                }
                connection_generation = connection_generation.wrapping_add(1);
                reader = resume_voice_gateway(
                    &url,
                    &writer,
                    session,
                    &last_sequence,
                    &heartbeat_ack,
                    &mut child_tasks,
                    &format!("websocket read failed: {error}"),
                )
                .await?;
                resume_pending = true;
                resume_deadline = Some(Instant::now() + VOICE_RESUME_HANDSHAKE_TIMEOUT);
                continue;
            }
        };
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("voice websocket JSON parse failed: {error}"))?;
                if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
                    *last_sequence.lock().await = Some(sequence);
                }
                let opcode = value.get("op").and_then(Value::as_u64).unwrap_or_default() as u8;
                match opcode {
                    VOICE_OP_READY => {
                        let (socket, ready) =
                            establish_voice_transport(&value, &writer, &audio_handle).await?;
                        udp_socket = Some(socket);
                        #[cfg(feature = "voice-playback")]
                        {
                            voice_ready = Some(ready);
                        }
                        #[cfg(not(feature = "voice-playback"))]
                        let _ = ready;
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = parse_voice_session_description(&value)?;
                        logging::debug(
                            "voice",
                            format!("voice session description received: {description:?}"),
                        );
                        if let Some(current) = current_session_description.as_ref() {
                            if current.secret_key == description.secret_key
                                && current.mode != description.mode
                            {
                                return Err(
                                    "voice transport mode changed without a new secret key"
                                        .to_owned(),
                                );
                            }
                            if current.uses_same_transport(&description) {
                                if current.dave_protocol_version
                                    != description.dave_protocol_version
                                    && let Some(dave_protocol_version) =
                                        description.dave_protocol_version
                                {
                                    let dave_protocol_version =
                                        u16::try_from(dave_protocol_version).map_err(|_| {
                                            "DAVE protocol version does not fit u16".to_owned()
                                        })?;
                                    dave_state.lock().await.reinit(dave_protocol_version)?;
                                }
                                current_session_description = Some(description);
                                logging::debug(
                                    "voice",
                                    "keeping current voice transport for duplicate session description",
                                );
                                continue;
                            }
                        }
                        if let Some(dave_protocol_version) = description.dave_protocol_version {
                            let dave_protocol_version = u16::try_from(dave_protocol_version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.lock().await.reinit(dave_protocol_version)?;
                        }
                        let Some(socket) = udp_socket.as_ref() else {
                            return Err(
                                "voice session description received before transport ready"
                                    .to_owned(),
                            );
                        };
                        #[cfg(feature = "voice-playback")]
                        if child_tasks
                            .stop_udp_transmit_gracefully(
                                "stopping previous voice UDP transmit task",
                            )
                            .await
                        {
                            while local_speaking_rx.try_recv().is_ok() {}
                            while transmit_failure_rx.try_recv().is_ok() {}
                            if let Some(speaking) = speaking_tracker.record_local(false) {
                                status_publisher
                                    .publish_speaking(session, session.user_id, speaking)
                                    .await;
                            }
                        }
                        start_voice_session_audio(
                            description.clone(),
                            &mut child_tasks,
                            VoiceSessionAudio {
                                socket,
                                #[cfg(feature = "voice-playback")]
                                writer: &writer,
                                audio_handle: &audio_handle,
                                dave_state: &dave_state,
                                remote_speaking_tx: &remote_speaking_tx,
                                current_playback_gate,
                                participant_playback_rx: participant_playback_rx.clone(),
                                output_source: current_audio_sources.output.as_deref(),
                                #[cfg(feature = "voice-playback")]
                                voice_ready: voice_ready.as_ref(),
                                #[cfg(feature = "voice-playback")]
                                current_capture_gate,
                                #[cfg(feature = "voice-playback")]
                                local_speaking_tx: &local_speaking_tx,
                                #[cfg(feature = "voice-playback")]
                                transmit_failure_tx: &transmit_failure_tx,
                            },
                        );
                        current_session_description = Some(description);
                        if connection_stable_deadline.is_none() {
                            connection_stable_deadline =
                                Some(Instant::now() + VOICE_CONNECTION_STABLE_INTERVAL);
                        }
                    }
                    VOICE_OP_HEARTBEAT_ACK => {
                        heartbeat_ack.lock().await.mark_acknowledged();
                    }
                    VOICE_OP_RESUMED => {
                        resume_pending = false;
                        resume_deadline = None;
                        logging::debug("voice", "voice gateway session resumed");
                    }
                    VOICE_OP_HELLO => {
                        handle_voice_hello(
                            &value,
                            &writer,
                            &last_sequence,
                            &heartbeat_ack,
                            &heartbeat_timeout_tx,
                            connection_generation,
                            &mut child_tasks,
                        )
                        .await?;
                    }
                    VOICE_OP_CLIENTS_CONNECT
                    | VOICE_OP_CLIENT_DISCONNECT
                    | VOICE_OP_MEDIA_SINK_WANTS
                    | VOICE_OP_CLIENT_FLAGS
                    | VOICE_OP_CLIENT_PLATFORM
                    | VOICE_OP_DAVE_PREPARE_TRANSITION
                    | VOICE_OP_DAVE_EXECUTE_TRANSITION
                    | VOICE_OP_DAVE_PREPARE_EPOCH => {
                        dave_state
                            .lock()
                            .await
                            .handle_json_op(&writer, opcode, &value)
                            .await?;
                    }
                    VOICE_OP_SPEAKING => {
                        handle_voice_speaking(
                            &value,
                            session,
                            &dave_state,
                            &mut speaking_tracker,
                            status_publisher,
                        )
                        .await;
                    }
                    other => logging::debug("voice", format!("unhandled voice gateway op={other}")),
                }
            }
            WsMessage::Ping(payload) => {
                let mut writer = writer.lock().await;
                writer
                    .send(WsMessage::Pong(payload))
                    .await
                    .map_err(|error| format!("voice websocket pong failed: {error}"))?;
            }
            WsMessage::Close(frame) => {
                let close_action = frame
                    .as_ref()
                    .map(|frame| voice_close_action(u16::from(frame.code)))
                    .unwrap_or(VoiceCloseAction::Resume);
                if let Some(frame) = frame {
                    logging::debug(
                        "voice",
                        format!(
                            "voice websocket closed: code={} reason={}",
                            frame.code, frame.reason
                        ),
                    );
                } else {
                    logging::debug("voice", "voice websocket closed without close frame");
                }
                match close_action {
                    VoiceCloseAction::Stop => return Ok(VoiceConnectionEnd::Stop),
                    VoiceCloseAction::Reconnect => return Ok(VoiceConnectionEnd::Reconnect),
                    VoiceCloseAction::Resume if resume_pending => {
                        return Ok(VoiceConnectionEnd::Reconnect);
                    }
                    VoiceCloseAction::Resume => {}
                }
                connection_generation = connection_generation.wrapping_add(1);
                reader = resume_voice_gateway(
                    &url,
                    &writer,
                    session,
                    &last_sequence,
                    &heartbeat_ack,
                    &mut child_tasks,
                    "websocket closed",
                )
                .await?;
                resume_pending = true;
                resume_deadline = Some(Instant::now() + VOICE_RESUME_HANDSHAKE_TIMEOUT);
            }
            WsMessage::Binary(payload) => {
                let frame = parse_voice_binary_frame(&payload)?;
                *last_sequence.lock().await = Some(frame.sequence);
                dave_state
                    .lock()
                    .await
                    .handle_binary_frame(&writer, frame)
                    .await?;
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }

    Ok(VoiceConnectionEnd::Stop)
    }
    .await;

    child_tasks.shutdown_all().await;
    for user_id in speaking_tracker.clear_all() {
        status_publisher
            .publish_speaking(session, user_id, false)
            .await;
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VoiceCloseAction {
    Resume,
    Reconnect,
    Stop,
}

pub(super) struct StreamVoiceGatewayControl {
    writer: VoiceWriter,
    last_sequence: Arc<Mutex<Option<i64>>>,
    heartbeat_ack: Arc<Mutex<VoiceHeartbeatAckState>>,
    heartbeat_timeout_tx: mpsc::UnboundedSender<VoiceHeartbeatTimeout>,
    heartbeat_timeout_rx: mpsc::UnboundedReceiver<VoiceHeartbeatTimeout>,
    dave_state: Arc<Mutex<VoiceDaveState>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamVoiceGatewayFrameAction {
    Payload,
    Continue,
    End(VoiceConnectionEnd),
}

impl StreamVoiceGatewayControl {
    pub(super) fn new(
        writer: VoiceWriter,
        current_user_id: Id<UserMarker>,
        rtc_server_id: &str,
    ) -> Result<Self, String> {
        let (heartbeat_timeout_tx, heartbeat_timeout_rx) = mpsc::unbounded_channel();
        Ok(Self {
            writer,
            last_sequence: Arc::new(Mutex::new(None)),
            heartbeat_ack: Arc::new(Mutex::new(VoiceHeartbeatAckState::default())),
            heartbeat_timeout_tx,
            heartbeat_timeout_rx,
            dave_state: Arc::new(Mutex::new(VoiceDaveState::new_for_stream(
                current_user_id,
                rtc_server_id,
            )?)),
        })
    }

    pub(super) fn dave_state(&self) -> Arc<Mutex<VoiceDaveState>> {
        Arc::clone(&self.dave_state)
    }

    pub(super) async fn heartbeat_timed_out(&mut self) {
        let _ = self.heartbeat_timeout_rx.recv().await;
    }

    pub(super) async fn record_sequence(&self, value: &Value) {
        if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
            *self.last_sequence.lock().await = Some(sequence);
        }
    }

    pub(super) async fn handle_json_op(
        &self,
        opcode: u8,
        value: &Value,
        child_tasks: &mut GatewayChildTasks,
    ) -> Result<bool, String> {
        match opcode {
            VOICE_OP_HEARTBEAT_ACK => {
                self.heartbeat_ack.lock().await.mark_acknowledged();
            }
            VOICE_OP_HELLO => {
                let interval = value
                    .get("d")
                    .and_then(|data| data.get("heartbeat_interval"))
                    .and_then(Value::as_u64)
                    .map(Duration::from_millis)
                    .ok_or_else(|| "stream voice hello missing heartbeat interval".to_owned())?;
                self.heartbeat_ack.lock().await.reset();
                child_tasks
                    .replace_heartbeat(tokio::spawn(run_voice_heartbeat(
                        Arc::clone(&self.writer),
                        interval,
                        Arc::clone(&self.last_sequence),
                        Arc::clone(&self.heartbeat_ack),
                        self.heartbeat_timeout_tx.clone(),
                        0,
                    )))
                    .await;
            }
            opcode if dave::handles_gateway_json_op(opcode) => {
                self.dave_state
                    .lock()
                    .await
                    .handle_json_op(&self.writer, opcode, value)
                    .await?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub(super) async fn handle_binary(&self, payload: &[u8]) -> Result<(), String> {
        let frame = parse_voice_binary_frame(payload)?;
        *self.last_sequence.lock().await = Some(frame.sequence);
        self.dave_state
            .lock()
            .await
            .handle_binary_frame(&self.writer, frame)
            .await
    }

    pub(super) async fn frame_action(
        &self,
        frame: &WsMessage,
    ) -> Result<StreamVoiceGatewayFrameAction, String> {
        match frame {
            WsMessage::Text(_) | WsMessage::Binary(_) => Ok(StreamVoiceGatewayFrameAction::Payload),
            WsMessage::Ping(payload) => {
                self.writer
                    .lock()
                    .await
                    .send(WsMessage::Pong(payload.clone()))
                    .await
                    .map_err(|error| format!("stream voice websocket pong failed: {error}"))?;
                Ok(StreamVoiceGatewayFrameAction::Continue)
            }
            WsMessage::Close(frame) => {
                let action = frame
                    .as_ref()
                    .map(|frame| voice_close_action(u16::from(frame.code)))
                    .unwrap_or(VoiceCloseAction::Reconnect);
                let outcome = match action {
                    VoiceCloseAction::Stop => VoiceConnectionEnd::Stop,
                    VoiceCloseAction::Resume | VoiceCloseAction::Reconnect => {
                        VoiceConnectionEnd::Reconnect
                    }
                };
                Ok(StreamVoiceGatewayFrameAction::End(outcome))
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => Ok(StreamVoiceGatewayFrameAction::Continue),
        }
    }
}

pub(super) fn voice_close_action(code: u16) -> VoiceCloseAction {
    match code {
        4013 | 4015 => VoiceCloseAction::Resume,
        4006 | 4009 => VoiceCloseAction::Reconnect,
        4014 | 4021 | 4022 => VoiceCloseAction::Stop,
        _ => VoiceCloseAction::Stop,
    }
}

async fn resume_voice_gateway(
    url: &str,
    writer: &VoiceWriter,
    session: &VoiceGatewaySession,
    last_sequence: &Arc<Mutex<Option<i64>>>,
    heartbeat_ack: &Arc<Mutex<VoiceHeartbeatAckState>>,
    child_tasks: &mut VoiceChildTasks,
    reason: &str,
) -> Result<VoiceReader, String> {
    logging::debug(
        "voice",
        format!("resuming voice websocket after {reason}: {url}"),
    );
    child_tasks.heartbeat.abort();
    heartbeat_ack.lock().await.reset();

    let (ws, response) = timeout(VOICE_WEBSOCKET_CONNECT_TIMEOUT, connect_async(url))
        .await
        .map_err(|_| "voice resume websocket connect timed out after 10s".to_owned())?
        .map_err(|error| format!("voice resume websocket connect failed: {error}"))?;
    logging::debug(
        "voice",
        format!(
            "voice resume websocket connected: status={}",
            response.status()
        ),
    );

    let (resumed_writer, reader) = ws.split();
    *writer.lock().await = resumed_writer;
    let sequence = last_sequence.lock().await.unwrap_or(-1);
    send_voice_text(writer, voice_resume_payload(session, sequence)).await?;
    logging::debug("voice", format!("voice resume sent: seq_ack={sequence}"));
    Ok(reader)
}

async fn establish_voice_transport(
    value: &Value,
    writer: &VoiceWriter,
    audio_handle: &tokio::runtime::Handle,
) -> Result<(Arc<UdpSocket>, VoiceTransportSession), String> {
    let ready = parse_voice_ready_payload(value)?;
    logging::debug(
        "voice",
        format!(
            "voice ready received: ssrc={} udp={}:{} modes={}",
            ready.ssrc,
            ready.ip,
            ready.port,
            ready.modes.len()
        ),
    );
    let mode = choose_encryption_mode(&ready.modes)?;
    logging::debug("voice", format!("voice encryption mode selected: {mode}"));
    // Bind on the audio runtime so subsequent UDP I/O stays on the dedicated
    // thread instead of competing with the TUI.
    let ready_for_discover = ready.clone();
    let (socket, discovered) = audio_handle
        .spawn(async move { discover_voice_udp_address(&ready_for_discover).await })
        .await
        .map_err(|error| format!("voice UDP discovery task join failed: {error}"))??;
    send_voice_text(writer, voice_select_protocol_payload(&discovered, &mode)).await?;
    logging::debug(
        "voice",
        format!(
            "voice select protocol sent: address={} port={} mode={}",
            discovered.address, discovered.port, mode
        ),
    );
    logging::debug("voice", "voice UDP discovery completed");
    Ok((socket, ready))
}

struct VoiceSessionAudio<'a> {
    socket: &'a Arc<UdpSocket>,
    #[cfg(feature = "voice-playback")]
    writer: &'a VoiceWriter,
    audio_handle: &'a tokio::runtime::Handle,
    dave_state: &'a Arc<Mutex<VoiceDaveState>>,
    remote_speaking_tx: &'a mpsc::UnboundedSender<Id<UserMarker>>,
    current_playback_gate: VoicePlaybackGate,
    participant_playback_rx:
        watch::Receiver<HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>>,
    output_source: Option<&'a str>,
    #[cfg(feature = "voice-playback")]
    voice_ready: Option<&'a VoiceTransportSession>,
    #[cfg(feature = "voice-playback")]
    current_capture_gate: VoiceCaptureGate,
    #[cfg(feature = "voice-playback")]
    local_speaking_tx: &'a mpsc::UnboundedSender<bool>,
    #[cfg(feature = "voice-playback")]
    transmit_failure_tx: &'a mpsc::UnboundedSender<String>,
}

fn start_voice_session_audio(
    description: VoiceSessionDescription,
    child_tasks: &mut VoiceChildTasks,
    audio: VoiceSessionAudio<'_>,
) {
    logging::debug("voice", "starting voice UDP receive task");
    let opus_decode = VoiceOpusDecode::start(
        audio.current_playback_gate,
        audio.participant_playback_rx,
        audio.audio_handle,
        audio.output_source,
    );
    let playback_tx = Some(opus_decode.frames_tx.clone());
    child_tasks.replace_opus_decode(opus_decode);
    child_tasks.set_voice_playback_gate(audio.current_playback_gate);
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
    let transmit_description = description.clone();
    child_tasks.replace_udp_receive(audio.audio_handle.spawn(run_voice_udp_receive(
        Arc::clone(audio.socket),
        description,
        Arc::clone(audio.dave_state),
        playback_tx,
        audio.remote_speaking_tx.clone(),
    )));
    child_tasks.replace_udp_keepalive(
        audio
            .audio_handle
            .spawn(run_voice_udp_keepalive(Arc::clone(audio.socket))),
    );
    #[cfg(feature = "voice-playback")]
    if let Some(ready) = audio.voice_ready {
        let (pcm_tx, pcm_rx) = mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);
        let (gate_tx, gate_rx) = watch::channel(audio.current_capture_gate);
        let transmit_failure_tx = audio.transmit_failure_tx.clone();
        let transmit_context = VoiceUdpTransmitContext {
            udp_socket: Arc::clone(audio.socket),
            writer: Arc::clone(audio.writer),
            description: transmit_description,
            ssrc: ready.ssrc,
            dave_state: Arc::clone(audio.dave_state),
            local_speaking_tx: audio.local_speaking_tx.clone(),
        };
        child_tasks.install_udp_transmit(
            audio.audio_handle.spawn(async move {
                let result = run_voice_udp_transmit(pcm_rx, gate_rx, transmit_context).await;
                publish_voice_udp_transmit_failure(result, &transmit_failure_tx);
            }),
            gate_tx,
            pcm_tx,
        );
        child_tasks.set_voice_transmit_gate(audio.current_capture_gate);
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn publish_voice_udp_transmit_failure(
    result: Result<(), String>,
    transmit_failure_tx: &mpsc::UnboundedSender<String>,
) {
    if let Err(error) = result {
        let _ = transmit_failure_tx.send(error);
    }
}

pub(super) async fn run_voice_udp_keepalive(socket: Arc<UdpSocket>) {
    let mut interval = tokio::time::interval(UDP_KEEPALIVE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut counter = 0u32;

    loop {
        interval.tick().await;
        if let Err(error) = socket.send(&udp_keepalive_packet(counter)).await {
            logging::error("voice", format!("voice UDP keepalive failed: {error}"));
            break;
        }
        if counter == 0 || counter.is_multiple_of(12) {
            logging::debug(
                "voice",
                format!("voice UDP keepalive sent: counter={counter}"),
            );
        }
        counter = counter.wrapping_add(1);
    }
}

async fn handle_voice_hello(
    value: &Value,
    writer: &VoiceWriter,
    last_sequence: &Arc<Mutex<Option<i64>>>,
    heartbeat_ack: &Arc<Mutex<VoiceHeartbeatAckState>>,
    heartbeat_timeout_tx: &mpsc::UnboundedSender<VoiceHeartbeatTimeout>,
    generation: u64,
    child_tasks: &mut VoiceChildTasks,
) -> Result<(), String> {
    let interval = value
        .get("d")
        .and_then(|data| data.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
        .ok_or_else(|| "voice hello missing heartbeat interval".to_owned())?;
    logging::debug(
        "voice",
        format!(
            "voice hello received: heartbeat_interval_ms={}",
            interval.as_millis()
        ),
    );
    let writer = Arc::clone(writer);
    let last_sequence = Arc::clone(last_sequence);
    let heartbeat_ack = Arc::clone(heartbeat_ack);
    let heartbeat_timeout_tx = heartbeat_timeout_tx.clone();
    heartbeat_ack.lock().await.reset();
    child_tasks.replace_heartbeat(tokio::spawn(run_voice_heartbeat(
        writer,
        interval,
        last_sequence,
        heartbeat_ack,
        heartbeat_timeout_tx,
        generation,
    )));
    logging::debug("voice", "voice heartbeat task started");
    Ok(())
}

async fn handle_voice_speaking(
    value: &Value,
    session: &VoiceGatewaySession,
    dave_state: &Arc<Mutex<VoiceDaveState>>,
    speaking_tracker: &mut VoiceSpeakingTracker,
    status_publisher: &VoiceStatusPublisher,
) {
    let speaking = dave_state.lock().await.handle_speaking_op(value);
    if let (Some(user_id), Some(speaking)) = (
        speaking.user_id.and_then(Id::<UserMarker>::new_checked),
        speaking.speaking,
    ) && let Some(speaking) = speaking_tracker.record_remote(
        user_id,
        voice_speaking_microphone_active(speaking),
        Instant::now(),
    ) {
        status_publisher
            .publish_speaking(session, user_id, speaking)
            .await;
    }
}

pub(super) async fn discover_voice_udp_address(
    ready: &VoiceTransportSession,
) -> Result<(Arc<UdpSocket>, DiscoveredVoiceAddress), String> {
    logging::debug("voice", "binding voice UDP socket");
    let socket = bind_voice_udp_socket()?;
    if let Ok(local_addr) = socket.local_addr() {
        logging::debug(
            "voice",
            format!("voice UDP socket bound: local={local_addr}"),
        );
    }
    logging::debug(
        "voice",
        format!(
            "connecting voice UDP socket: remote={}:{}",
            ready.ip, ready.port
        ),
    );
    socket
        .connect((ready.ip.as_str(), ready.port))
        .await
        .map_err(|error| format!("voice UDP connect failed: {error}"))?;
    logging::debug("voice", "voice UDP socket connected");
    logging::debug(
        "voice",
        format!("sending voice UDP discovery request: ssrc={}", ready.ssrc),
    );
    socket
        .send(&udp_discovery_request(ready.ssrc))
        .await
        .map_err(|error| format!("voice UDP discovery send failed: {error}"))?;

    let mut response = [0u8; UDP_DISCOVERY_PACKET_LEN];
    logging::debug("voice", "waiting for voice UDP discovery response");
    let len = timeout(UDP_DISCOVERY_TIMEOUT, socket.recv(&mut response))
        .await
        .map_err(|_| "voice UDP discovery timed out".to_owned())?
        .map_err(|error| format!("voice UDP discovery receive failed: {error}"))?;
    let discovered = parse_udp_discovery_response(&response[..len], ready.ssrc)?;
    logging::debug(
        "voice",
        format!(
            "voice UDP discovery response received: address={} port={}",
            discovered.address, discovered.port
        ),
    );
    Ok((Arc::new(socket), discovered))
}

fn bind_voice_udp_socket() -> Result<UdpSocket, String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| format!("voice UDP socket creation failed: {error}"))?;
    if let Err(error) = socket.set_recv_buffer_size(VOICE_UDP_RECEIVE_BUFFER_BYTES) {
        logging::debug(
            "voice",
            format!(
                "voice UDP receive buffer configuration failed: requested_bytes={VOICE_UDP_RECEIVE_BUFFER_BYTES} error={error}"
            ),
        );
    }
    match socket.recv_buffer_size() {
        Ok(applied_bytes) => logging::debug(
            "voice",
            format!(
                "voice UDP receive buffer size: requested_bytes={VOICE_UDP_RECEIVE_BUFFER_BYTES} applied_bytes={applied_bytes}"
            ),
        ),
        Err(error) => logging::debug(
            "voice",
            format!("read applied voice UDP receive buffer failed: {error}"),
        ),
    }
    socket
        .set_nonblocking(true)
        .map_err(|error| format!("set voice UDP socket nonblocking failed: {error}"))?;
    socket
        .bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into())
        .map_err(|error| format!("voice UDP bind failed: {error}"))?;
    UdpSocket::from_std(socket.into())
        .map_err(|error| format!("register voice UDP socket failed: {error}"))
}

pub(super) async fn run_voice_udp_receive(
    socket: Arc<UdpSocket>,
    description: VoiceSessionDescription,
    dave_state: Arc<Mutex<VoiceDaveState>>,
    playback_tx: Option<mpsc::Sender<VoicePlaybackFrame>>,
    remote_speaking_tx: mpsc::UnboundedSender<Id<UserMarker>>,
) {
    let mode = description.mode.clone();
    let decryptor = match VoiceRtpDecryptor::new(&description.mode, &description.secret_key) {
        Ok(decryptor) => decryptor,
        Err(error) => {
            logging::error("voice", format!("voice RTP decrypt setup failed: {error}"));
            return;
        }
    };
    logging::debug(
        "voice",
        format!("voice UDP receive decrypt active: mode={mode}"),
    );
    let mut packet = vec![0u8; 2048];
    let mut rtp_packets = 0u64;
    let mut decrypted_packets = 0u64;
    let mut dave_decrypted_packets = 0u64;
    let mut dave_pending_packets = 0u64;
    let mut decrypt_failures = 0u64;
    let mut non_audio_packets = 0u64;
    let mut rtcp_packets = 0u64;
    let mut malformed_packets = 0u64;
    let mut keepalive_acks = 0u64;
    loop {
        match socket.recv(&mut packet).await {
            Ok(len) => {
                if let Some(counter) = parse_udp_keepalive_response(&packet[..len]) {
                    keepalive_acks = keepalive_acks.saturating_add(1);
                    if keepalive_acks == 1 || keepalive_acks.is_multiple_of(12) {
                        logging::debug(
                            "voice",
                            format!(
                                "voice UDP keepalive acknowledged: count={keepalive_acks} counter={counter}"
                            ),
                        );
                    }
                    continue;
                }
                if looks_like_rtcp_packet(&packet[..len]) {
                    rtcp_packets = rtcp_packets.saturating_add(1);
                    if rtcp_packets == 1 || rtcp_packets.is_multiple_of(100) {
                        logging::debug(
                            "voice",
                            format!(
                                "ignoring RTCP UDP packet: count={} packet_type={} length={} sender_ssrc={:?}",
                                rtcp_packets,
                                packet[1],
                                len,
                                rtcp_sender_ssrc(&packet[..len])
                            ),
                        );
                    }
                    continue;
                }
                match parse_rtp_header(&packet[..len]) {
                    Ok(header) => {
                        rtp_packets = rtp_packets.saturating_add(1);
                        if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE {
                            non_audio_packets = non_audio_packets.saturating_add(1);
                            if non_audio_packets == 1 || non_audio_packets.is_multiple_of(100) {
                                logging::debug(
                                    "voice",
                                    format!(
                                        "ignoring non-audio RTP packet: count={} payload_type={} ssrc={} seq={} timestamp={}",
                                        non_audio_packets,
                                        header.payload_type,
                                        header.ssrc,
                                        header.sequence,
                                        header.timestamp
                                    ),
                                );
                            }
                            continue;
                        }
                        match decryptor.decrypt_packet(&packet[..len], &header) {
                            Ok(payload) => {
                                decrypted_packets = decrypted_packets.saturating_add(1);
                                let (remote_user_id, media) = {
                                    let mut dave_state = dave_state.lock().await;
                                    let remote_user_id = dave_state.user_id_for_ssrc(header.ssrc);
                                    let media = dave_state.unwrap_media_payload_for_ssrc(
                                        header.ssrc,
                                        &payload.media_payload,
                                    );
                                    (remote_user_id, media)
                                };
                                let media_payload_len = match &media {
                                    VoiceMediaPayload::Plain(payload) => payload.len(),
                                    VoiceMediaPayload::DaveUnexpectedPlain { payload_len }
                                    | VoiceMediaPayload::DaveMissingUser { payload_len }
                                    | VoiceMediaPayload::DaveNotReady { payload_len, .. } => {
                                        dave_pending_packets =
                                            dave_pending_packets.saturating_add(1);
                                        if dave_pending_packets == 1
                                            || dave_pending_packets.is_multiple_of(100)
                                        {
                                            logging::debug(
                                                "voice",
                                                format!(
                                                    "DAVE media decrypt pending: count={} ssrc={} seq={} reason={}",
                                                    dave_pending_packets,
                                                    header.ssrc,
                                                    header.sequence,
                                                    media.pending_reason()
                                                ),
                                            );
                                        }
                                        *payload_len
                                    }
                                    VoiceMediaPayload::DaveDecryptFailed { message, .. } => {
                                        decrypt_failures = decrypt_failures.saturating_add(1);
                                        if decrypt_failures == 1
                                            || decrypt_failures.is_multiple_of(100)
                                        {
                                            logging::debug(
                                                "voice",
                                                format!(
                                                    "DAVE media decrypt failed: count={} ssrc={} seq={} error={}",
                                                    decrypt_failures,
                                                    header.ssrc,
                                                    header.sequence,
                                                    message
                                                ),
                                            );
                                        }
                                        payload.media_payload.len()
                                    }
                                    VoiceMediaPayload::DaveDecrypted { opus, .. } => {
                                        dave_decrypted_packets =
                                            dave_decrypted_packets.saturating_add(1);
                                        opus.len()
                                    }
                                };
                                if (dave_decrypted_packets == 1
                                    || dave_decrypted_packets.is_multiple_of(500))
                                    && let VoiceMediaPayload::DaveDecrypted { user_id, .. } = &media
                                {
                                    logging::debug(
                                        "voice",
                                        format!(
                                            "DAVE media decrypted: count={} user_id={} ssrc={} seq={} opus_len={}",
                                            dave_decrypted_packets,
                                            user_id,
                                            header.ssrc,
                                            header.sequence,
                                            media_payload_len
                                        ),
                                    );
                                }
                                if let Some(frame) =
                                    voice_playback_frame(&media, &header, remote_user_id)
                                    && let Some(tx) = playback_tx.as_ref()
                                {
                                    let _ = tx.try_send(frame);
                                }
                                if let Some(user_id) = remote_user_id
                                    && voice_media_payload_counts_as_remote_activity(&media)
                                {
                                    let _ = remote_speaking_tx.send(user_id);
                                }
                                if decrypted_packets == 1 || decrypted_packets.is_multiple_of(500) {
                                    logging::debug(
                                        "voice",
                                        format!(
                                            "decrypted RTP packet: count={} ssrc={} seq={} timestamp={} payload_type={} payload_len={} extension_body_len={}",
                                            decrypted_packets,
                                            header.ssrc,
                                            header.sequence,
                                            header.timestamp,
                                            header.payload_type,
                                            media_payload_len,
                                            payload.encrypted_extension_body_len
                                        ),
                                    );
                                }
                            }
                            Err(error) => {
                                decrypt_failures = decrypt_failures.saturating_add(1);
                                if decrypt_failures == 1 || decrypt_failures.is_multiple_of(100) {
                                    logging::debug(
                                        "voice",
                                        format!(
                                            "RTP decrypt failed: count={} ssrc={} seq={} timestamp={} error={}",
                                            decrypt_failures,
                                            header.ssrc,
                                            header.sequence,
                                            header.timestamp,
                                            error
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    Err(error) => {
                        malformed_packets = malformed_packets.saturating_add(1);
                        if malformed_packets == 1 || malformed_packets.is_multiple_of(100) {
                            logging::debug(
                                "voice",
                                format!(
                                    "ignoring non-RTP UDP packet: count={malformed_packets} error={error}"
                                ),
                            );
                        }
                    }
                }
            }
            Err(error) => {
                logging::error("voice", format!("voice UDP receive failed: {error}"));
                break;
            }
        }
    }
}

#[allow(dead_code)]
pub(super) async fn run_voice_heartbeat(
    writer: VoiceWriter,
    interval: Duration,
    last_sequence: Arc<Mutex<Option<i64>>>,
    heartbeat_ack: Arc<Mutex<VoiceHeartbeatAckState>>,
    heartbeat_timeout_tx: mpsc::UnboundedSender<VoiceHeartbeatTimeout>,
    generation: u64,
) {
    loop {
        if !heartbeat_ack.lock().await.mark_sent() {
            let _ = heartbeat_timeout_tx.send(VoiceHeartbeatTimeout { generation });
            break;
        }
        let sequence = last_sequence.lock().await.unwrap_or(-1);
        if let Err(error) = send_voice_text(&writer, voice_heartbeat_payload(sequence)).await {
            logging::error("voice", format!("voice heartbeat send failed: {error}"));
            let _ = heartbeat_timeout_tx.send(VoiceHeartbeatTimeout { generation });
            break;
        }
        sleep(interval).await;
    }
}

pub(super) async fn send_voice_text(writer: &VoiceWriter, payload: String) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Text(payload.into()))
        .await
        .map_err(|error| format!("voice websocket send failed: {error}"))
}

pub(super) async fn send_voice_binary(
    writer: &VoiceWriter,
    opcode: u8,
    mut payload: Vec<u8>,
) -> Result<(), String> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(opcode);
    frame.append(&mut payload);
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Binary(frame.into()))
        .await
        .map_err(|error| format!("voice websocket binary send failed: {error}"))
}

pub(super) fn voice_gateway_url(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint
        .trim()
        .trim_start_matches("wss://")
        .trim_start_matches("https://")
        .trim_start_matches("ws://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if endpoint.is_empty() {
        return Err("voice endpoint is empty".to_owned());
    }
    Ok(format!("wss://{endpoint}/?v={VOICE_GATEWAY_VERSION}"))
}

pub(super) fn voice_identify_payload(session: &VoiceGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.scope.server_id_string(),
            "user_id": session.user_id.to_string(),
            "channel_id": session.channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "max_dave_protocol_version": davey::DAVE_PROTOCOL_VERSION,
        },
    })
    .to_string()
}

pub(super) fn voice_heartbeat_payload(sequence: i64) -> String {
    json!({
        "op": VOICE_OP_HEARTBEAT,
        "d": {
            "t": chrono::Utc::now().timestamp_millis(),
            "seq_ack": sequence,
        },
    })
    .to_string()
}

pub(super) fn voice_resume_payload(session: &VoiceGatewaySession, sequence: i64) -> String {
    json!({
        "op": VOICE_OP_RESUME,
        "d": {
            "server_id": session.scope.server_id_string(),
            "channel_id": session.channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "seq_ack": sequence,
        },
    })
    .to_string()
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_speaking_payload(ssrc: u32, speaking: bool) -> String {
    json!({
        "op": VOICE_OP_SPEAKING,
        "d": {
            "speaking": if speaking { 1 } else { 0 },
            "delay": 0,
            "ssrc": ssrc,
        },
    })
    .to_string()
}

pub(super) fn parse_voice_ready_payload(value: &Value) -> Result<VoiceTransportSession, String> {
    let data = value
        .get("d")
        .ok_or_else(|| "voice ready missing data".to_owned())?;
    let ssrc = data
        .get("ssrc")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "voice ready missing ssrc".to_owned())?;
    let ip = data
        .get("ip")
        .and_then(Value::as_str)
        .filter(|ip| !ip.is_empty())
        .ok_or_else(|| "voice ready missing UDP ip".to_owned())?
        .to_owned();
    let port = data
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "voice ready missing UDP port".to_owned())?;
    let modes = data
        .get("modes")
        .and_then(Value::as_array)
        .ok_or_else(|| "voice ready missing encryption modes".to_owned())?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();

    Ok(VoiceTransportSession {
        ssrc,
        ip,
        port,
        modes,
    })
}

pub(super) fn choose_encryption_mode(modes: &[String]) -> Result<String, String> {
    for candidate in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        if modes.iter().any(|mode| mode == candidate) {
            return Ok(candidate.to_owned());
        }
    }
    Err("voice ready did not offer a supported encryption mode".to_owned())
}

pub(super) fn udp_discovery_request(ssrc: u32) -> [u8; UDP_DISCOVERY_PACKET_LEN] {
    let mut packet = [0u8; UDP_DISCOVERY_PACKET_LEN];
    packet[0..2].copy_from_slice(&1u16.to_be_bytes());
    packet[2..4].copy_from_slice(&70u16.to_be_bytes());
    packet[4..8].copy_from_slice(&ssrc.to_be_bytes());
    packet
}

pub(super) fn udp_keepalive_packet(counter: u32) -> [u8; UDP_KEEPALIVE_PACKET_LEN] {
    let mut packet = [0u8; UDP_KEEPALIVE_PACKET_LEN];
    packet[..size_of::<u32>()].copy_from_slice(&counter.to_le_bytes());
    packet
}

pub(super) fn parse_udp_keepalive_response(packet: &[u8]) -> Option<u32> {
    let counter = packet.get(..size_of::<u32>())?.try_into().ok()?;
    (packet.len() == UDP_KEEPALIVE_PACKET_LEN).then(|| u32::from_le_bytes(counter))
}

pub(super) fn parse_udp_discovery_response(
    packet: &[u8],
    expected_ssrc: u32,
) -> Result<DiscoveredVoiceAddress, String> {
    if packet.len() < UDP_DISCOVERY_PACKET_LEN {
        return Err("voice UDP discovery response is too short".to_owned());
    }
    let packet_type = u16::from_be_bytes([packet[0], packet[1]]);
    if packet_type != 2 {
        return Err("voice UDP discovery response has invalid type".to_owned());
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]);
    if length != 70 {
        return Err("voice UDP discovery response has invalid length".to_owned());
    }
    let ssrc = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
    if ssrc != expected_ssrc {
        return Err("voice UDP discovery response has unexpected SSRC".to_owned());
    }
    let address_end = packet[8..72]
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| 8 + index)
        .unwrap_or(72);
    let address = std::str::from_utf8(&packet[8..address_end])
        .map_err(|error| format!("voice UDP discovery address is invalid UTF-8: {error}"))?
        .to_owned();
    if address.is_empty() {
        return Err("voice UDP discovery response has empty address".to_owned());
    }
    let port = u16::from_be_bytes([packet[72], packet[73]]);
    Ok(DiscoveredVoiceAddress { address, port })
}

pub(super) fn voice_select_protocol_payload(
    discovered: &DiscoveredVoiceAddress,
    mode: &str,
) -> String {
    json!({
        "op": 1,
        "d": {
            "protocol": "udp",
            "data": {
                "address": discovered.address,
                "port": discovered.port,
                "mode": mode,
            },
        },
    })
    .to_string()
}

pub(super) fn parse_voice_session_description(
    value: &Value,
) -> Result<VoiceSessionDescription, String> {
    let data = value
        .get("d")
        .ok_or_else(|| "voice session description missing data".to_owned())?;
    let mode = data
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| !mode.is_empty())
        .ok_or_else(|| "voice session description missing mode".to_owned())?
        .to_owned();
    let secret_key = data
        .get("secret_key")
        .and_then(Value::as_array)
        .ok_or_else(|| "voice session description missing secret key".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(|| "voice session description has invalid secret key byte".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if secret_key.len() != 32 {
        return Err("voice session description secret key is not 32 bytes".to_owned());
    }
    let dave_protocol_version = data.get("dave_protocol_version").and_then(Value::as_u64);
    let video_codec = data
        .get("video_codec")
        .and_then(Value::as_str)
        .filter(|codec| !codec.is_empty())
        .map(str::to_owned);
    Ok(VoiceSessionDescription {
        mode,
        secret_key,
        dave_protocol_version,
        video_codec,
    })
}

pub(super) fn parse_voice_binary_frame(payload: &[u8]) -> Result<VoiceBinaryFrame<'_>, String> {
    if payload.len() < 3 {
        return Err("voice binary frame is too short".to_owned());
    }
    let sequence = u16::from_be_bytes([payload[0], payload[1]]);
    Ok(VoiceBinaryFrame {
        sequence: i64::from(sequence),
        opcode: payload[2],
        payload: &payload[3..],
    })
}

pub(super) fn voice_playback_frame(
    media: &VoiceMediaPayload,
    header: &RtpHeader,
    remote_user_id: Option<Id<UserMarker>>,
) -> Option<VoicePlaybackFrame> {
    let (user_id, opus) = match media {
        VoiceMediaPayload::Plain(opus) => (remote_user_id, opus.clone()),
        VoiceMediaPayload::DaveDecrypted { user_id, opus } => {
            (Id::new_checked(*user_id), opus.clone())
        }
        VoiceMediaPayload::DaveUnexpectedPlain { .. }
        | VoiceMediaPayload::DaveMissingUser { .. }
        | VoiceMediaPayload::DaveNotReady { .. }
        | VoiceMediaPayload::DaveDecryptFailed { .. } => return None,
    };
    Some(VoicePlaybackFrame {
        ssrc: header.ssrc,
        user_id,
        sequence: header.sequence,
        timestamp: header.timestamp,
        opus,
    })
}

pub(super) fn voice_media_payload_counts_as_remote_activity(media: &VoiceMediaPayload) -> bool {
    let opus = match media {
        VoiceMediaPayload::Plain(opus) | VoiceMediaPayload::DaveDecrypted { opus, .. } => opus,
        VoiceMediaPayload::DaveUnexpectedPlain { .. }
        | VoiceMediaPayload::DaveMissingUser { .. }
        | VoiceMediaPayload::DaveNotReady { .. }
        | VoiceMediaPayload::DaveDecryptFailed { .. } => return false,
    };
    opus.as_slice() != DISCORD_OPUS_SILENCE_FRAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use socket2::SockRef;

    #[tokio::test]
    async fn voice_udp_socket_binds_after_receive_buffer_tuning() {
        let socket = bind_voice_udp_socket().expect("voice UDP socket should bind");
        let local_addr = socket
            .local_addr()
            .expect("voice UDP socket should have a local address");
        let applied_bytes = SockRef::from(&socket)
            .recv_buffer_size()
            .expect("voice UDP receive buffer should be readable");

        assert!(local_addr.is_ipv4());
        assert_ne!(local_addr.port(), 0);
        assert_ne!(applied_bytes, 0);
    }
}
