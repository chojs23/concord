use super::*;

pub(super) async fn run_voice_gateway_session(
    session: VoiceGatewaySession,
    events_tx: mpsc::UnboundedSender<VoiceRuntimeEvent>,
    status_publisher: VoiceStatusPublisher,
    initial_capture_gate: VoiceCaptureGate,
    capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    initial_playback_gate: VoicePlaybackGate,
    playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
) {
    match connect_voice_gateway(
        &session,
        &status_publisher,
        initial_capture_gate,
        capture_gate_rx,
        initial_playback_gate,
        playback_gate_rx,
    )
    .await
    {
        Ok(()) => {
            status_publisher
                .publish(
                    &session,
                    VoiceConnectionStatus::Disconnected,
                    "Voice gateway disconnected",
                )
                .await;
        }
        Err(error) => {
            logging::error("voice", &error);
            status_publisher
                .publish(&session, VoiceConnectionStatus::Failed, error)
                .await;
        }
    }
    let _ = events_tx.send(session.connection_ended_event());
}

pub(super) async fn connect_voice_gateway(
    session: &VoiceGatewaySession,
    status_publisher: &VoiceStatusPublisher,
    initial_capture_gate: VoiceCaptureGate,
    mut capture_gate_rx: mpsc::UnboundedReceiver<VoiceCaptureGate>,
    initial_playback_gate: VoicePlaybackGate,
    mut playback_gate_rx: mpsc::UnboundedReceiver<VoicePlaybackGate>,
) -> Result<(), String> {
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
    let audio_runtime = VoiceAudioRuntime::start()?;
    let audio_handle = audio_runtime.handle().clone();
    child_tasks.audio_runtime = Some(audio_runtime);
    let mut speaking_tracker = VoiceSpeakingTracker::default();
    let mut speaking_sweep = tokio::time::interval(VOICE_REMOTE_SPEAKING_SWEEP_INTERVAL);
    #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
    let (local_speaking_tx, mut local_speaking_rx) = mpsc::unbounded_channel();
    let (remote_speaking_tx, mut remote_speaking_rx) = mpsc::unbounded_channel();
    #[cfg_attr(
        not(feature = "voice-playback"),
        allow(unused_mut, unused_variables, unused_assignments)
    )]
    let mut current_capture_gate = initial_capture_gate;
    let mut current_playback_gate = initial_playback_gate;
    let mut udp_socket: Option<Arc<UdpSocket>> = None;
    #[cfg_attr(
        not(feature = "voice-playback"),
        allow(unused_mut, unused_variables, unused_assignments)
    )]
    let mut voice_ready: Option<VoiceTransportSession> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let dave_state = Arc::new(Mutex::new(VoiceDaveState::new(session)));

    let result: Result<(), String> = async {
    send_voice_text(&writer, voice_identify_payload(session)).await?;
    logging::debug("voice", "voice identify sent");
    logging::debug("voice", "voice websocket read loop started");

    loop {
        let frame = tokio::select! {
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
                            enabled: false,
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
            _ = speaking_sweep.tick() => {
                for user_id in speaking_tracker.expire_remote(Instant::now()) {
                    status_publisher.publish_speaking(session, user_id, false).await;
                }
                continue;
            }
            frame = reader.next() => frame,
        };
        let Some(frame) = frame else {
            break;
        };
        let frame = frame.map_err(|error| format!("voice websocket read failed: {error}"))?;
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
                        let ready = parse_voice_ready_payload(&value)?;
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
                        // Bind the UDP socket on the audio runtime so its tokio
                        // reactor owns the I/O; subsequent send/recv stay on the
                        // dedicated thread instead of competing with the TUI.
                        let ready_for_discover = ready.clone();
                        let (socket, discovered) =
                            audio_handle
                                .spawn(async move {
                                    discover_voice_udp_address(&ready_for_discover).await
                                })
                                .await
                                .map_err(|error| {
                                    format!("voice UDP discovery task join failed: {error}")
                                })??;
                        send_voice_text(&writer, voice_select_protocol_payload(&discovered, &mode))
                            .await?;
                        logging::debug(
                            "voice",
                            format!(
                                "voice select protocol sent: address={} port={} mode={}",
                                discovered.address, discovered.port, mode
                            ),
                        );
                        udp_socket = Some(socket);
                        #[cfg(feature = "voice-playback")]
                        {
                            voice_ready = Some(ready);
                        }
                        logging::debug("voice", "voice UDP discovery completed");
                    }
                    VOICE_OP_SESSION_DESCRIPTION => {
                        let description = parse_voice_session_description(&value)?;
                        logging::debug(
                            "voice",
                            format!("voice session description received: {description:?}"),
                        );
                        if let Some(dave_protocol_version) = description.dave_protocol_version {
                            let dave_protocol_version = u16::try_from(dave_protocol_version)
                                .map_err(|_| "DAVE protocol version does not fit u16".to_owned())?;
                            dave_state.lock().await.reinit(dave_protocol_version)?;
                        }
                        if let Some(socket) = udp_socket.as_ref() {
                            logging::debug("voice", "starting voice UDP receive task");
                            let opus_decode =
                                VoiceOpusDecode::start(current_playback_gate, &audio_handle);
                            let playback_tx = Some(opus_decode.frames_tx.clone());
                            child_tasks.replace_opus_decode(opus_decode);
                            child_tasks.set_voice_playback_gate(current_playback_gate);
                            #[cfg_attr(not(feature = "voice-playback"), allow(unused_variables))]
                            let transmit_description = description.clone();
                            child_tasks.replace_udp_receive(audio_handle.spawn(
                                run_voice_udp_receive(
                                    Arc::clone(socket),
                                    description,
                                    Arc::clone(&dave_state),
                                    playback_tx,
                                    remote_speaking_tx.clone(),
                                ),
                            ));
                            #[cfg(feature = "voice-playback")]
                            if let Some(ready) = voice_ready.as_ref() {
                                let (pcm_tx, pcm_rx) = mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);
                                let (gate_tx, gate_rx) = watch::channel(current_capture_gate);
                                child_tasks
                                    .replace_udp_transmit(
                                        audio_handle.spawn(run_voice_udp_transmit(
                                            pcm_rx,
                                            gate_rx,
                                            VoiceUdpTransmitContext {
                                                udp_socket: Arc::clone(socket),
                                                writer: Arc::clone(&writer),
                                                description: transmit_description,
                                                ssrc: ready.ssrc,
                                                dave_state: Arc::clone(&dave_state),
                                                local_speaking_tx: local_speaking_tx.clone(),
                                            },
                                        )),
                                        gate_tx,
                                        pcm_tx,
                                    )
                                    .await;
                                child_tasks.set_voice_transmit_gate(current_capture_gate);
                            }
                        }
                    }
                    VOICE_OP_HEARTBEAT_ACK => {}
                    VOICE_OP_HELLO => {
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
                        child_tasks.replace_heartbeat(tokio::spawn(run_voice_heartbeat(
                            Arc::clone(&writer),
                            interval,
                            Arc::clone(&last_sequence),
                        )));
                        logging::debug("voice", "voice heartbeat task started");
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
                        let speaking = dave_state.lock().await.handle_speaking_op(&value);
                        if let (Some(user_id), Some(speaking)) = (
                            speaking.user_id.and_then(Id::<UserMarker>::new_checked),
                            speaking.speaking,
                        ) {
                            if let Some(speaking) = speaking_tracker.record_remote(
                                user_id,
                                voice_speaking_microphone_active(speaking),
                                Instant::now(),
                            ) {
                                status_publisher
                                    .publish_speaking(session, user_id, speaking)
                                    .await;
                            }
                        }
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
                break;
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

    Ok(())
    }
    .await;

    child_tasks.shutdown_all().await;
    for user_id in speaking_tracker.clear_all(session.user_id) {
        status_publisher
            .publish_speaking(session, user_id, false)
            .await;
    }
    result
}

pub(super) async fn discover_voice_udp_address(
    ready: &VoiceTransportSession,
) -> Result<(Arc<UdpSocket>, DiscoveredVoiceAddress), String> {
    logging::debug("voice", "binding voice UDP socket");
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|error| format!("voice UDP bind failed: {error}"))?;
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
    loop {
        match socket.recv(&mut packet).await {
            Ok(len) => {
                if looks_like_rtcp_packet(&packet[..len]) {
                    rtcp_packets = rtcp_packets.saturating_add(1);
                    if rtcp_packets == 1 || rtcp_packets % 100 == 0 {
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
                            if non_audio_packets == 1 || non_audio_packets % 100 == 0 {
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
                                            || dave_pending_packets % 100 == 0
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
                                        if decrypt_failures == 1 || decrypt_failures % 100 == 0 {
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
                                if dave_decrypted_packets == 1 || dave_decrypted_packets % 500 == 0
                                {
                                    if let VoiceMediaPayload::DaveDecrypted { user_id, .. } = &media
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
                                }
                                if let Some(frame) = voice_playback_frame(&media, &header)
                                    && let Some(tx) = playback_tx.as_ref()
                                {
                                    let _ = tx.try_send(frame);
                                }
                                if let Some(user_id) = remote_user_id
                                    && voice_media_payload_counts_as_remote_activity(&media)
                                {
                                    let _ = remote_speaking_tx.send(user_id);
                                }
                                if decrypted_packets == 1 || decrypted_packets % 500 == 0 {
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
                                if decrypt_failures == 1 || decrypt_failures % 100 == 0 {
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
                        if malformed_packets == 1 || malformed_packets % 100 == 0 {
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
) {
    loop {
        let sequence = last_sequence.lock().await.unwrap_or(-1);
        if let Err(error) = send_voice_text(&writer, voice_heartbeat_payload(sequence)).await {
            logging::error("voice", format!("voice heartbeat send failed: {error}"));
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
        "op": 3,
        "d": {
            "t": chrono::Utc::now().timestamp_millis(),
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
    Ok(VoiceSessionDescription {
        mode,
        secret_key,
        dave_protocol_version,
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
) -> Option<VoicePlaybackFrame> {
    let (user_id, opus) = match media {
        VoiceMediaPayload::Plain(opus) => (None, opus.clone()),
        VoiceMediaPayload::DaveDecrypted { user_id, opus } => (Some(*user_id), opus.clone()),
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
