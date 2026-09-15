use super::*;

fn gate() -> VoiceCaptureGate {
    VoiceCaptureGate {
        transmit_epoch: 0,
        capture_enabled: true,
        transmit_enabled: true,
        use_voice_activity: true,
        noise_suppression: false,
        microphone_buffer_ms: None,
        microphone_sensitivity: MicrophoneSensitivityDb::default(),
        microphone_volume: VoiceVolumePercent::default(),
    }
}

#[test]
fn microphone_expiry_retires_backlog_and_recovers_without_partial_audio() {
    let now = Instant::now();
    let (tx, mut rx) = mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames = VoiceMicrophonePcmFrames::new(tx, stats, 48_000);
    // Two completed frames and a half-frame must all retire together.
    frames.push_stereo_samples(
        &vec![1; DISCORD_OPUS_20MS_STEREO_SAMPLES * 5 / 2],
        now - Duration::from_millis(501),
    );
    let first = rx.try_recv().expect("old batch should queue");
    let generation = Arc::clone(&first.generation);
    let (selected, dropped) = select_fresh_voice_microphone_frame(first, &mut rx, now);
    assert!(selected.is_none());
    assert_eq!(dropped, 2);
    assert!(!generation.load(Ordering::Acquire));

    frames.push_stereo_samples(&vec![2; DISCORD_OPUS_20MS_STEREO_SAMPLES], now);
    let fresh = rx.try_recv().expect("new audio should resume");
    assert_eq!(fresh.captured_at, now);
    assert!(fresh.samples.iter().all(|sample| *sample == 2));
    assert!(fresh.generation.load(Ordering::Acquire));
    assert!(!Arc::ptr_eq(&generation, &fresh.generation));
}

#[test]
fn microphone_pcm_overflow_retires_queued_history_before_recovery() {
    let now = Instant::now();
    let (tx, mut rx) = mpsc::channel(2);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames = VoiceMicrophonePcmFrames::new(tx, Arc::clone(&stats), 48_000);
    frames.push_stereo_samples(&vec![1; DISCORD_OPUS_20MS_STEREO_SAMPLES * 3], now);
    let first = rx.try_recv().expect("first batch should fill queue");
    let (selected, dropped) = select_fresh_voice_microphone_frame(first, &mut rx, now);
    assert!(selected.is_none());
    assert_eq!(dropped, 2);
    assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 1);

    let resumed_at = now + Duration::from_millis(60);
    frames.push_stereo_samples(&vec![3; DISCORD_OPUS_20MS_STEREO_SAMPLES], resumed_at);
    let frame = rx
        .try_recv()
        .expect("overflow should not close the capture queue");
    assert_eq!(frame.captured_at, resumed_at);
    assert!(frame.samples.iter().all(|sample| *sample == 3));
    assert!(frame.generation.load(Ordering::Acquire));
}

#[test]
fn microphone_partial_frames_tolerate_jitter_but_reset_across_capture_gaps() {
    let now = Instant::now();
    let (tx, mut rx) = mpsc::channel(4);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames = VoiceMicrophonePcmFrames::new(tx, stats, 48_000);
    let half = DISCORD_OPUS_20MS_STEREO_SAMPLES / 2;
    frames.push_stereo_samples(&vec![1; half], now);
    frames.push_stereo_samples(&vec![2; half], now + Duration::from_millis(12));
    let first = rx
        .try_recv()
        .expect("timestamp jitter should preserve partial audio");
    assert_eq!(first.captured_at, now);
    assert!(first.samples[..half].iter().all(|sample| *sample == 1));
    assert!(first.samples[half..].iter().all(|sample| *sample == 2));

    frames.push_stereo_samples(&vec![3; half], now + Duration::from_millis(20));
    let resumed_at = now + Duration::from_millis(100);
    frames.push_stereo_samples(&vec![4; half * 2], resumed_at);
    let fresh = rx
        .try_recv()
        .expect("new capture should resume after a gap");
    assert_eq!(fresh.captured_at, resumed_at);
    assert!(fresh.samples.iter().all(|sample| *sample == 4));
    assert!(!first.generation.load(Ordering::Acquire));
}

#[test]
fn microphone_final_send_check_enforces_deadline_mute_and_capture_boundary() {
    let now = Instant::now();
    for (age_ms, allowed) in [(80, true), (150, true), (500, true), (501, false)] {
        let (tx, rx) = watch::channel(gate());
        let frame = VoiceMicrophoneFrame {
            samples: vec![1],
            captured_at: now - Duration::from_millis(age_ms),
            generation: Arc::new(AtomicBool::new(true)),
        };
        let cutoff = now - Duration::from_secs(1);
        assert_eq!(
            voice_microphone_frame_can_send(&frame, &rx, 0, cutoff, now),
            allowed
        );
        assert!(!voice_microphone_frame_can_send(&frame, &rx, 0, now, now));
        tx.send_modify(|gate| {
            gate.transmit_enabled = false;
            gate.transmit_epoch += 1;
        });
        assert!(!voice_microphone_frame_can_send(
            &frame, &rx, 0, cutoff, now
        ));
        tx.send_modify(|gate| {
            gate.transmit_enabled = true;
            gate.transmit_epoch += 1;
        });
        // An unmute update must reach the control branch before a concurrent tick sends.
        assert!(!voice_microphone_frame_can_send(
            &frame, &rx, 0, cutoff, now
        ));
        // Pending non-transmit settings must not retire healthy queued audio.
        tx.send_modify(|gate| {
            gate.noise_suppression = true;
            gate.use_voice_activity = false;
        });
        assert_eq!(
            voice_microphone_frame_can_send(&frame, &rx, 2, cutoff, now),
            allowed
        );
        frame.generation.store(false, Ordering::Release);
        assert!(!voice_microphone_frame_can_send(
            &frame, &rx, 2, cutoff, now
        ));
    }
}

#[test]
fn microphone_recovery_resets_dsp_and_encoder_history() {
    let mut encoder = VoiceOpusEncode::new().expect("Opus encoder should initialize");
    let mut microphone_gate = VoiceMicrophoneGateState::default();
    let mut noise = VoiceNoiseSuppressor::new();
    let mut trailing = VoiceTrailingSilence::default();
    let samples = vec![2_000; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    encoder
        .encode_20ms_i16(&samples)
        .expect("frame should encode");
    microphone_gate.allows_frame(&samples, MicrophoneSensitivityDb::default());
    noise.process_20ms_stereo(&mut samples.clone());
    trailing.start(true);

    reset_voice_microphone_processing(
        &mut encoder,
        &mut microphone_gate,
        &mut noise,
        &mut trailing,
    )
    .expect("processing should reset");
    assert_eq!(microphone_gate.hangover_frames, 0);
    assert_eq!(microphone_gate.overload_recovery_frames, 0);
    assert!(trailing.take_frame().is_none());
    let mut fresh = VoiceOpusEncode::new().expect("reference encoder should initialize");
    assert_eq!(
        encoder
            .encode_20ms_i16(&samples)
            .expect("reset encoder should encode"),
        fresh
            .encode_20ms_i16(&samples)
            .expect("fresh encoder should encode"),
    );
}

#[tokio::test]
async fn microphone_send_waits_are_bounded_by_both_media_age_and_transport_timeout() {
    let pending = || std::future::pending::<Result<bool, String>>();
    let expired = Instant::now() - Duration::from_millis(501);
    assert!(
        !voice_microphone_send_before_deadline(expired, async { Ok(true) })
            .await
            .expect("expired media is a recoverable drop")
    );
    let nearly_expired = Instant::now() - Duration::from_millis(480);
    assert!(
        !voice_microphone_send_before_deadline(nearly_expired, pending())
            .await
            .expect("media expiry is a recoverable drop")
    );
    assert_eq!(
        voice_microphone_send_before_deadline(Instant::now(), pending()).await,
        Err("voice microphone send timed out".to_owned()),
    );
}

#[tokio::test]
async fn microphone_outbound_expiry_and_gateway_contention_do_not_send_stale_audio() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let (client, accepted) =
        tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
    let (_server, _) = accepted.expect("local connection should be accepted");
    let websocket = tokio_tungstenite::WebSocketStream::from_raw_socket(
        tokio_tungstenite::MaybeTlsStream::Plain(client.expect("local client should connect")),
        tokio_tungstenite::tungstenite::protocol::Role::Client,
        None,
    )
    .await;
    let writer: VoiceWriter = Arc::new(Mutex::new(websocket.split().0));
    let socket = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("sender should bind");
    let receiver = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("receiver should bind");
    socket
        .connect(
            receiver
                .local_addr()
                .expect("receiver should have an address"),
        )
        .await
        .expect("local UDP peer should connect");
    let mut sender = VoiceOutboundSendState::new(
        AEAD_AES256_GCM_RTPSIZE,
        &[9; 32],
        VoiceOutboundRtpState {
            sequence: 0,
            timestamp: 0,
            ssrc: 42,
        },
        0,
    )
    .expect("test encryptor should initialize");
    sender.set_capture_gate(true, false);
    let (_gate_tx, gate_rx) = watch::channel(gate());
    let cutoff = Instant::now() - Duration::from_secs(1);
    let mut stats = VoiceUdpTransmitStats::default();

    // A speaking notification blocked by gateway work must not release expired RTP later.
    let frame = VoiceMicrophoneFrame {
        samples: vec![],
        captured_at: Instant::now() - Duration::from_millis(480),
        generation: Arc::new(AtomicBool::new(true)),
    };
    let held_writer = writer.lock().await;
    let outcome = sender.send_opus_frame(&DISCORD_OPUS_SILENCE_FRAME);
    let sent = voice_microphone_send_before_deadline(
        frame.captured_at,
        flush_voice_outbound_events(
            &socket,
            &writer,
            outcome,
            &mut sender,
            &mut stats,
            Some((&frame, &gate_rx, 0, cutoff)),
        ),
    )
    .await
    .expect("media expiry should recover without a transport error");
    assert!(!sent);
    assert_eq!(stats.sent_packets, 0);
    let mut packet = [0; 1024];
    assert!(
        timeout(Duration::from_millis(10), receiver.recv(&mut packet))
            .await
            .is_err()
    );

    // The already-speaking sender state emits RTP without taking the gateway writer.
    let frame = VoiceMicrophoneFrame {
        samples: vec![],
        captured_at: Instant::now(),
        generation: Arc::new(AtomicBool::new(true)),
    };
    let outcome = sender.send_opus_frame(&DISCORD_OPUS_SILENCE_FRAME);
    let sent = voice_microphone_send_before_deadline(
        frame.captured_at,
        flush_voice_outbound_events(
            &socket,
            &writer,
            outcome,
            &mut sender,
            &mut stats,
            Some((&frame, &gate_rx, 0, cutoff)),
        ),
    )
    .await
    .expect("RTP should not wait for the held websocket writer");
    assert!(sent);
    assert_eq!(stats.sent_packets, 1);
    assert!(
        timeout(Duration::from_millis(100), receiver.recv(&mut packet))
            .await
            .expect("fresh RTP should arrive")
            .expect("local RTP should receive")
            > 0
    );
    drop(held_writer);
}
