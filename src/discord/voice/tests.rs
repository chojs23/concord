use super::dave::VoiceDaveOutboundPayload;
use super::rtp::build_voice_rtp_packet;
use super::runtime::stop_voice_connection_task;
use super::*;
#[cfg(feature = "voice-playback")]
use crate::support::audio_output::{f32_sample_to_i16, f32_sample_to_u8, f32_sample_to_u16};

fn requested_voice() -> CurrentVoiceConnectionState {
    CurrentVoiceConnectionState {
        self_mute: true,
        ..CurrentVoiceConnectionState::test(Id::new(1), Id::new(10))
    }
}

fn voice_state(user_id: u64, channel_id: Option<Id<ChannelMarker>>) -> VoiceStateInfo {
    VoiceStateInfo {
        session_id: Some("voice-session".to_owned()),
        ..VoiceStateInfo::test(Id::new(1), channel_id, Id::new(user_id))
    }
}

fn voice_server() -> VoiceServerInfo {
    VoiceServerInfo {
        guild_id: Some(Id::new(1)),
        channel_id: None,
        endpoint: Some("voice.example.com".to_owned()),
        token: "secret-token".to_owned(),
    }
}

#[test]
fn voice_runtime_assembles_local_voice_session() {
    let mut state = VoiceRuntimeState::default();

    assert_eq!(
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10)))),
        None
    );
    assert_eq!(
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice()))),
        None
    );
    assert_eq!(
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10))
        ))),
        None
    );
    let action = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    match action {
        Some(VoiceRuntimeAction::Connect(session)) => {
            assert_eq!(session.scope, VoiceScope::Guild(Id::new(1)));
            assert_eq!(session.channel_id, Id::new(10));
            assert_eq!(session.user_id, Id::new(10));
            assert_eq!(session.endpoint, "voice.example.com");
        }
        other => panic!("expected connect action, got {other:?}"),
    }
}

#[test]
fn voice_runtime_capture_gate_requires_allowed_active_unmuted_voice() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));

    let mut requested = requested_voice();
    requested.allow_microphone_transmit = true;
    requested.self_mute = false;
    requested.microphone_volume = VoiceVolumePercent::new(40);
    requested.voice_output_volume = VoiceVolumePercent::new(65);
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(state.capture_gate(), None);

    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            enabled: true,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_mute = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            enabled: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_deaf = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            enabled: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: false,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_mute = false;
    requested.allow_microphone_transmit = false;
    requested.self_deaf = false;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            enabled: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    let mut other_channel = requested;
    other_channel.channel_id = Id::new(11);
    other_channel.allow_microphone_transmit = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(other_channel)));
    assert_eq!(state.capture_gate(), None);
    assert_eq!(state.playback_gate(), None);
}

#[test]
fn voice_runtime_ignores_other_user_voice_state() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    assert_eq!(
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            99,
            Some(Id::new(10))
        ))),
        None
    );
}

#[test]
fn voice_runtime_closes_on_leave() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    assert_eq!(
        state.apply(VoiceRuntimeEvent::Requested(None)),
        Some(VoiceRuntimeAction::Close)
    );
}

#[test]
fn voice_runtime_reconnects_after_matching_connection_end() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    let connected = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    let Some(VoiceRuntimeAction::Connect(session)) = connected else {
        panic!("expected initial voice connect action, got {connected:?}");
    };

    assert_eq!(
        state.apply(session.connection_ended_event()),
        Some(VoiceRuntimeAction::Connect(session))
    );
}

#[test]
fn voice_gateway_session_debug_redacts_secrets() {
    let session = VoiceGatewaySession {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "secret-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "secret-token".to_owned(),
    };

    let debug = format!("{session:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret-session"));
    assert!(!debug.contains("secret-token"));
}

#[test]
fn voice_state_debug_redacts_session_id() {
    let state = voice_state(10, Some(Id::new(10)));

    let debug = format!("{state:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("voice-session"));
}

#[test]
fn voice_dave_state_tracks_speaking_ssrc_mapping() {
    let session = VoiceGatewaySession {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    };
    let mut state = VoiceDaveState::new(&session);

    state.record_speaking_state(VoiceSpeakingState {
        user_id: Some(30),
        ssrc: Some(1234),
        speaking: Some(1),
    });

    assert_eq!(state.ssrc_user_ids.get(&1234), Some(&30));
    assert_eq!(state.user_id_for_ssrc(1234), Some(Id::new(30)));
    assert_eq!(state.user_id_for_ssrc(9999), None);
    assert!(state.known_user_ids.contains(&30));
}

#[test]
fn voice_dave_active_drops_non_dave_payloads() {
    let session = test_voice_gateway_session();
    let mut state = VoiceDaveState::new(&session);
    state.reinit(1).expect("DAVE session should initialize");

    assert_eq!(
        state.unwrap_media_payload_for_ssrc(1234, b"plain-opus"),
        VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 10 }
    );
}

#[test]
fn voice_speaking_uses_microphone_bit_only() {
    assert!(!voice_speaking_microphone_active(0));
    assert!(voice_speaking_microphone_active(1));
    assert!(!voice_speaking_microphone_active(2));
    assert!(voice_speaking_microphone_active(5));
}

#[test]
fn voice_speaking_tracker_expires_remote_speakers_and_tracks_local_edges() {
    let mut tracker = VoiceSpeakingTracker::default();
    let remote_user = Id::new(30);
    let local_user = Id::new(20);
    let now = Instant::now();

    assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
    assert_eq!(
        tracker.record_remote(remote_user, true, now + VOICE_REMOTE_SPEAKING_TTL / 2),
        None
    );
    assert!(
        tracker
            .expire_remote(now + VOICE_REMOTE_SPEAKING_TTL)
            .is_empty()
    );
    assert_eq!(
        tracker.expire_remote(now + VOICE_REMOTE_SPEAKING_TTL + VOICE_REMOTE_SPEAKING_TTL / 2),
        vec![remote_user]
    );
    assert_eq!(tracker.record_remote(remote_user, false, now), None);
    assert_eq!(tracker.record_remote(remote_user, true, now), Some(true));
    assert_eq!(tracker.record_remote(remote_user, false, now), Some(false));

    assert_eq!(tracker.record_local(true), Some(true));
    assert_eq!(tracker.record_local(true), None);
    assert_eq!(tracker.clear_all(local_user), vec![local_user]);
}

#[test]
fn voice_dave_outbound_opus_fails_closed_unless_ready() {
    let mut state = VoiceDaveState::new(&test_voice_gateway_session());

    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
    );

    state.protocol_version = NonZeroU16::new(1);
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Blocked(VoiceOutboundSendBlockReason::DaveOutboundMissingSession)
    );

    state.reinit(1).expect("DAVE session should initialize");
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Blocked(VoiceOutboundSendBlockReason::DaveOutboundNotReady)
    );

    state.reinit(0).expect("DAVE should disable cleanly");
    assert_eq!(
        state.prepare_outbound_opus(b"opus-frame"),
        VoiceDaveOutboundPayload::Plain(b"opus-frame".to_vec())
    );
}

#[test]
fn dave_media_detection_requires_magic_marker() {
    assert!(!looks_like_dave_media_frame(b"opus-frame"));

    let mut payload = vec![0u8; DAVE_MIN_SUPPLEMENTAL_BYTES];
    let marker_start = payload.len() - DAVE_MAGIC_MARKER.len();
    payload[marker_start..].copy_from_slice(&DAVE_MAGIC_MARKER);

    assert!(looks_like_dave_media_frame(&payload));
}

#[test]
fn voice_playback_frame_uses_only_playable_media_payloads() {
    let header = RtpHeader {
        payload_type: DISCORD_VOICE_PAYLOAD_TYPE,
        sequence: 7,
        timestamp: 8,
        ssrc: 9,
        authenticated_header_len: 12,
        encrypted_extension_body_len: 0,
        payload_offset: 12,
    };

    assert_eq!(
        voice_playback_frame(&VoiceMediaPayload::Plain(b"opus".to_vec()), &header),
        Some(VoicePlaybackFrame {
            ssrc: 9,
            user_id: None,
            sequence: 7,
            timestamp: 8,
            opus: b"opus".to_vec(),
        })
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveDecrypted {
                user_id: 42,
                opus: b"dave-opus".to_vec(),
            },
            &header,
        ),
        Some(VoicePlaybackFrame {
            ssrc: 9,
            user_id: Some(42),
            sequence: 7,
            timestamp: 8,
            opus: b"dave-opus".to_vec(),
        })
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 4 },
            &header,
        ),
        None
    );
    assert_eq!(
        voice_playback_frame(
            &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
            &header,
        ),
        None
    );
}

fn test_playback_frame(ssrc: u32, user_id: Option<u64>, sequence: u16) -> VoicePlaybackFrame {
    VoicePlaybackFrame {
        ssrc,
        user_id,
        sequence,
        timestamp: u32::from(sequence) * DISCORD_OPUS_TIMESTAMP_INCREMENT,
        opus: vec![sequence as u8],
    }
}

#[test]
fn voice_playout_buffer_reorders_nearby_packets() {
    let now = Instant::now();
    let mut buffer = VoicePlaybackPlayoutBuffer::default();

    assert!(buffer.push(test_playback_frame(9, Some(42), 12), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 10), now));
    assert_eq!(buffer.next_frame(now), None);
    assert!(buffer.push(test_playback_frame(9, Some(42), 11), now));

    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            10
        )))
    );
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            11
        )))
    );
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            12
        )))
    );
}

#[test]
fn voice_playout_buffer_emits_packet_loss_for_missing_sequence() {
    let now = Instant::now();
    let mut buffer = VoicePlaybackPlayoutBuffer::default();

    assert!(buffer.push(test_playback_frame(9, Some(42), 10), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 12), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 13), now));

    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            10
        )))
    );
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::PacketLoss {
            ssrc: 9,
            user_id: Some(42),
            sequence: 11,
        })
    );
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            12
        )))
    );
}

#[test]
fn voice_playout_buffer_drops_stale_packets_after_playout_advances() {
    let now = Instant::now();
    let mut buffer = VoicePlaybackPlayoutBuffer::default();

    assert!(buffer.push(test_playback_frame(9, Some(42), 7), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 8), now));
    assert!(buffer.push(test_playback_frame(9, Some(42), 9), now));
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            7
        )))
    );
    assert_eq!(
        buffer.next_frame(now),
        Some(VoicePlayoutFrame::Audio(test_playback_frame(
            9,
            Some(42),
            8
        )))
    );

    assert!(!buffer.push(test_playback_frame(9, Some(42), 7), now));
}

#[test]
fn voice_decoded_samples_mix_same_tick_frames() {
    let mixed =
        mix_voice_decoded_samples(&[vec![0.5, 0.25, -0.5, -0.25], vec![0.5, -0.25, 0.5, -0.75]])
            .expect("same-tick decoded frames should mix");
    let gain = 1.0 / 2.0f32.sqrt();

    assert_voice_sample_near(mixed[0], 1.0 * gain);
    assert_voice_sample_near(mixed[1], 0.0);
    assert_voice_sample_near(mixed[2], 0.0);
    assert_voice_sample_near(mixed[3], -gain);
}

#[test]
fn voice_decoded_samples_clamp_mixed_peaks() {
    let mixed = mix_voice_decoded_samples(&[vec![1.0, 1.0], vec![1.0, 1.0]])
        .expect("same-tick decoded frames should mix");

    assert_eq!(mixed, vec![1.0, 1.0]);
}

#[test]
fn voice_post_process_reduces_alternating_high_frequency_noise() {
    let mut post_process = VoicePlaybackPostProcess::default();
    let mut samples = vec![1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];

    post_process.process(&mut samples);

    assert!(samples[2].abs() < 1.0);
    assert!(samples[4].abs() < 1.0);
    assert!(samples[6].abs() < 1.0);
}

#[cfg(feature = "voice-playback")]
#[test]
fn extra_output_channels_use_converted_silence() {
    let mut u8_output = [0u8; 4];
    write_voice_output_frame(&mut u8_output, 1.0, -1.0, f32_sample_to_u8);
    assert_eq!(
        u8_output,
        [255, 0, f32_sample_to_u8(0.0), f32_sample_to_u8(0.0)]
    );

    let mut u16_output = [0u16; 4];
    write_voice_output_frame(&mut u16_output, 1.0, -1.0, f32_sample_to_u16);
    assert_eq!(
        u16_output,
        [u16::MAX, 0, f32_sample_to_u16(0.0), f32_sample_to_u16(0.0),]
    );

    let mut i16_output = [1i16; 4];
    write_voice_output_frame(&mut i16_output, 1.0, -1.0, f32_sample_to_i16);
    assert_eq!(i16_output, [i16::MAX, i16::MIN + 1, 0, 0]);
}

fn assert_voice_sample_near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {actual} to be close to {expected}"
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_audio_buffer_resamples_non_48khz_output_clock() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.try_send(vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0])
        .expect("decoded samples should queue");
    let mut buffer = VoiceAudioBuffer::new(rx, 24_000);

    assert_eq!(buffer.next_stereo_frame(), Some([0.0, 0.0]));
    assert_eq!(buffer.next_stereo_frame(), Some([2.0, 2.0]));
    let faded = buffer
        .next_stereo_frame()
        .expect("resampled underrun should fade from the last frame");
    assert!(faded[0] < 2.0 && faded[0] > 0.0);
    assert_eq!(faded[0], faded[1]);
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_audio_buffer_fades_short_underruns() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    tx.try_send(vec![1.0, -1.0])
        .expect("decoded samples should queue");
    let mut buffer = VoiceAudioBuffer::new(rx, DISCORD_VOICE_SAMPLE_RATE);

    assert_eq!(buffer.next_stereo_frame(), Some([1.0, -1.0]));
    let faded = buffer
        .next_stereo_frame()
        .expect("underrun should produce a short fade tail");

    assert!(faded[0] < 1.0 && faded[0] > 0.0);
    assert!(faded[1] > -1.0 && faded[1] < 0.0);
}

#[test]
fn remote_speaking_activity_ignores_silence_and_unplayable_media() {
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec()),
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecrypted {
            user_id: 42,
            opus: DISCORD_OPUS_SILENCE_FRAME.to_vec(),
        },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveUnexpectedPlain { payload_len: 4 },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveMissingUser { payload_len: 4 },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveNotReady {
            user_id: 42,
            payload_len: 4,
        },
    ));
    assert!(!voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecryptFailed {
            user_id: 42,
            message: "failed".to_owned(),
        },
    ));
    assert!(voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::Plain(b"opus".to_vec()),
    ));
    assert!(voice_media_payload_counts_as_remote_activity(
        &VoiceMediaPayload::DaveDecrypted {
            user_id: 42,
            opus: b"opus".to_vec(),
        },
    ));
}

#[test]
fn microphone_sensitivity_filters_quiet_pcm_frames() {
    let quiet = vec![100i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let loud = vec![4000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

    assert!(voice_pcm_frame_reaches_sensitivity(
        &quiet,
        MicrophoneSensitivityDb::new(-60),
    ));
    assert!(!voice_pcm_frame_reaches_sensitivity(
        &quiet,
        MicrophoneSensitivityDb::new(-30),
    ));
    assert!(voice_pcm_frame_reaches_sensitivity(
        &normal,
        MicrophoneSensitivityDb::default(),
    ));
    assert!(voice_pcm_frame_reaches_sensitivity(
        &loud,
        MicrophoneSensitivityDb::new(-20),
    ));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_hangover_keeps_short_quiet_gaps_open() {
    let quiet = vec![100i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut gate = VoiceMicrophoneGateState::default();

    assert!(gate.allows_frame(&normal, MicrophoneSensitivityDb::default()));
    for _ in 0..VOICE_MIC_GATE_HANGOVER_FRAMES {
        assert!(gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));
    }
    assert!(!gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));

    gate.allows_frame(&normal, MicrophoneSensitivityDb::default());
    gate.reset();
    assert!(!gate.allows_frame(&quiet, MicrophoneSensitivityDb::default()));
}

#[test]
fn voice_volume_scales_i16_pcm_frame() {
    let mut frame = vec![1000, -1000, i16::MAX, i16::MIN];

    apply_voice_volume_to_i16_frame(&mut frame, VoiceVolumePercent::new(50));

    assert_eq!(frame, vec![500, -500, 16384, -16384]);
}

#[test]
fn voice_microphone_protection_soft_limits_extreme_samples() {
    let mut frame = vec![1000, -1000, i16::MAX, i16::MIN];

    let limited = protect_voice_microphone_frame(&mut frame);

    assert_eq!(frame[0], 1000);
    assert_eq!(frame[1], -1000);
    assert!(frame[2] < i16::MAX);
    assert!(frame[3] > i16::MIN);
    assert_eq!(frame[2], -frame[3]);
    assert_eq!(limited, 2);
}

#[test]
fn voice_microphone_overload_detects_dense_clipping_not_single_peaks() {
    let mut normal_loud = vec![8_000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    normal_loud[0] = i16::MAX;
    normal_loud[1] = i16::MIN + 1;
    assert!(!voice_microphone_frame_is_overloaded(&normal_loud));

    let mut below_threshold = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in below_threshold
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 1)
    {
        *sample = i16::MAX;
    }
    assert!(!voice_microphone_frame_is_overloaded(&below_threshold));

    let mut overloaded = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in overloaded
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }
    assert!(voice_microphone_frame_is_overloaded(&overloaded));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_blanks_handling_noise_envelope() {
    let mut gate = VoiceMicrophoneGateState::default();
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = i16::MAX;
    handling_noise[1] = i16::MIN + 1;
    for sample in handling_noise
        .iter_mut()
        .skip(2)
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 2)
    {
        *sample = i16::MAX;
    }

    let overload_decision = gate
        .overload_decision(&handling_noise)
        .expect("handling-noise frame should be blanked");
    assert_eq!(
        overload_decision.kind,
        VoiceMicrophoneOverloadKind::HandlingNoise
    );
    assert_eq!(overload_decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);

    for _ in 0..VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES {
        let recovery_decision = gate
            .overload_decision(&normal)
            .expect("handling-noise envelope should be blanked");
        assert_eq!(
            recovery_decision.kind,
            VoiceMicrophoneOverloadKind::Recovery
        );
        assert_eq!(recovery_decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    }
    assert!(gate.overload_decision(&normal).is_none());

    gate.overload_decision(&handling_noise);
    gate.reset();
    assert!(gate.overload_decision(&normal).is_none());
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_gate_ramps_after_non_handling_transient() {
    let mut gate = VoiceMicrophoneGateState::default();
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let mut transient = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in transient
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let overload_decision = gate
        .overload_decision(&transient)
        .expect("transient frame should be attenuated");
    assert_eq!(
        overload_decision.kind,
        VoiceMicrophoneOverloadKind::Transient
    );
    assert_eq!(overload_decision.gain, VOICE_MIC_OVERLOAD_TRANSIENT_GAIN);

    let mut previous_gain = overload_decision.gain;
    for frame_index in 0..VOICE_MIC_OVERLOAD_RECOVERY_FRAMES {
        let recovery_decision = gate
            .overload_decision(&normal)
            .expect("transient recovery should be ramped");
        assert_eq!(
            recovery_decision.kind,
            VoiceMicrophoneOverloadKind::Recovery
        );
        if frame_index == 0 {
            assert!(
                (recovery_decision.gain - VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN).abs()
                    < f32::EPSILON
            );
        }
        assert!(recovery_decision.gain > previous_gain);
        assert!(recovery_decision.gain <= 1.0);
        previous_gain = recovery_decision.gain;
    }
    assert!(gate.overload_decision(&normal).is_none());
}

#[test]
fn voice_microphone_overload_gain_keeps_shouted_frame_audible() {
    let mut shouted = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in shouted
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let gain = voice_microphone_overload_gain(&shouted)
        .expect("clipped shouted frame should be gain-reduced");
    apply_voice_microphone_gain(&mut shouted, gain);

    assert_eq!(gain, VOICE_MIC_OVERLOAD_ATTENUATION_GAIN);
    assert!(shouted.iter().any(|sample| *sample > 0));
    assert!(
        shouted
            .iter()
            .all(|sample| i32::from(*sample).abs() < i32::from(i16::MAX))
    );
}

#[test]
fn voice_microphone_blanks_sparse_clipped_unclassified_frame() {
    let mut sparse_clip = vec![2000i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in sparse_clip
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES - 2)
    {
        *sample = i16::MAX;
    }
    let raw_decision = voice_microphone_overload_decision(&sparse_clip);
    assert!(raw_decision.is_none());

    assert!(voice_microphone_clipped_frame_needs_blank(
        &sparse_clip,
        raw_decision,
    ));
}

#[test]
fn voice_microphone_blanks_clipped_non_handling_frame() {
    let mut attenuated = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in attenuated
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }
    let raw_decision = voice_microphone_overload_decision(&attenuated);
    assert_eq!(
        raw_decision.map(|decision| decision.kind),
        Some(VoiceMicrophoneOverloadKind::Attenuated)
    );

    assert!(voice_microphone_clipped_frame_needs_blank(
        &attenuated,
        raw_decision,
    ));
}

#[test]
fn voice_microphone_keeps_handling_noise_on_gate_path() {
    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = i16::MAX;
    handling_noise[1] = i16::MIN + 1;
    let raw_decision = voice_microphone_overload_decision(&handling_noise);
    assert_eq!(
        raw_decision.map(|decision| decision.kind),
        Some(VoiceMicrophoneOverloadKind::HandlingNoise)
    );

    assert!(!voice_microphone_clipped_frame_needs_blank(
        &handling_noise,
        raw_decision,
    ));
}

#[test]
fn voice_microphone_does_not_blank_unclipped_unclassified_frame() {
    let normal = vec![1500i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let raw_decision = voice_microphone_overload_decision(&normal);
    assert!(raw_decision.is_none());

    assert!(!voice_microphone_clipped_frame_needs_blank(
        &normal,
        raw_decision,
    ));
}

#[test]
fn voice_microphone_handling_noise_uses_adjacent_delta_without_dense_clipping() {
    let mut handling_noise = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    handling_noise[0] = 22_000;
    handling_noise[1] = -20_001;

    let decision = voice_microphone_overload_decision(&handling_noise)
        .expect("large adjacent delta should classify handling noise");

    assert_eq!(decision.kind, VoiceMicrophoneOverloadKind::HandlingNoise);
    assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    assert_eq!(voice_microphone_clipped_sample_count(&handling_noise), 0);
}

#[test]
fn voice_microphone_overload_promotes_sparse_clipped_impulse_to_handling_noise() {
    let mut impulse = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    impulse[0] = i16::MAX;
    impulse[1] = -3_233;

    let decision = voice_microphone_overload_decision(&impulse)
        .expect("clipped impulse should be gain-reduced");

    assert_eq!(decision.kind, VoiceMicrophoneOverloadKind::HandlingNoise);
    assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    let max_adjacent_delta = voice_microphone_max_adjacent_delta(&impulse);
    assert!(max_adjacent_delta >= VOICE_MIC_OVERLOAD_IMPULSE_DELTA);
    assert!(max_adjacent_delta < VOICE_MIC_HANDLING_NOISE_DELTA);
    assert!(
        voice_microphone_clipped_sample_count(&impulse) < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
    );
}

#[test]
fn voice_microphone_overload_promotes_sparse_clipped_step_to_handling_noise() {
    let mut impulse = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    impulse[0] = i16::MAX;
    impulse[1] = i16::MAX;

    let decision =
        voice_microphone_overload_decision(&impulse).expect("clipped step should be gain-reduced");

    assert_eq!(decision.kind, VoiceMicrophoneOverloadKind::HandlingNoise);
    assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    let max_adjacent_delta = voice_microphone_max_adjacent_delta(&impulse);
    assert!(max_adjacent_delta >= VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA);
    assert!(max_adjacent_delta < VOICE_MIC_OVERLOAD_IMPULSE_DELTA);
    assert!(
        voice_microphone_clipped_sample_count(&impulse) < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
    );
}

#[test]
fn voice_microphone_overload_gain_keeps_severe_same_polarity_clip_audible() {
    let mut clipped = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in clipped
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    assert_eq!(
        voice_microphone_overload_gain(&clipped),
        Some(VOICE_MIC_OVERLOAD_TRANSIENT_GAIN)
    );
}

#[test]
fn voice_microphone_overload_gain_keeps_sub_extreme_same_polarity_clip_audible() {
    let mut clipped = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in clipped
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES - 1)
    {
        *sample = i16::MAX;
    }

    assert_eq!(
        voice_microphone_overload_gain(&clipped),
        Some(VOICE_MIC_OVERLOAD_TRANSIENT_GAIN)
    );
}

#[test]
fn voice_microphone_overload_gain_blanks_extreme_same_polarity_clip() {
    let mut clipped = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    for sample in clipped
        .iter_mut()
        .take(VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES)
    {
        *sample = i16::MAX;
    }

    let decision = voice_microphone_overload_decision(&clipped)
        .expect("extreme clipped frame should be blanked");

    assert_eq!(decision.kind, VoiceMicrophoneOverloadKind::HandlingNoise);
    assert_eq!(decision.gain, VOICE_MIC_HANDLING_NOISE_GAIN);
    assert_eq!(
        voice_microphone_clipped_sample_count(&clipped),
        VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES
    );
}

#[test]
fn voice_identify_payload_matches_expected_shape() {
    let session = VoiceGatewaySession {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    };
    let payload: Value = serde_json::from_str(&voice_identify_payload(&session))
        .expect("voice identify payload is valid JSON");

    assert_eq!(payload["op"].as_u64(), Some(0));
    assert_eq!(payload["d"]["server_id"].as_str(), Some("1"));
    assert_eq!(payload["d"]["user_id"].as_str(), Some("20"));
    assert_eq!(payload["d"]["channel_id"].as_str(), Some("10"));
    assert_eq!(payload["d"]["session_id"].as_str(), Some("voice-session"));
    assert_eq!(payload["d"]["token"].as_str(), Some("voice-token"));
    assert_eq!(
        payload["d"]["max_dave_protocol_version"].as_u64(),
        Some(u64::from(davey::DAVE_PROTOCOL_VERSION))
    );
}

#[test]
fn voice_gateway_url_normalizes_endpoint() {
    assert_eq!(
        voice_gateway_url("voice.example.com:2048/").as_deref(),
        Ok("wss://voice.example.com:2048/?v=9")
    );
    assert_eq!(
        voice_gateway_url("wss://voice.example.com").as_deref(),
        Ok("wss://voice.example.com/?v=9")
    );
    assert_eq!(
        voice_gateway_url("https://voice.example.com").as_deref(),
        Ok("wss://voice.example.com/?v=9")
    );
    assert_eq!(
        voice_gateway_url("   /").expect_err("empty endpoint should be rejected"),
        "voice endpoint is empty"
    );
}

#[test]
fn voice_ready_payload_parses_udp_transport_fields() {
    let payload = json!({
        "op": 2,
        "d": {
            "ssrc": 0x01020304u32,
            "ip": "203.0.113.10",
            "port": 50000u64,
            "modes": [
                "aead_xchacha20_poly1305_rtpsize",
                "aead_aes256_gcm_rtpsize"
            ],
        },
    });

    let ready = parse_voice_ready_payload(&payload).expect("ready payload should parse");

    assert_eq!(ready.ssrc, 0x01020304);
    assert_eq!(ready.ip, "203.0.113.10");
    assert_eq!(ready.port, 50000);
    assert_eq!(
        choose_encryption_mode(&ready.modes).as_deref(),
        Ok(AEAD_AES256_GCM_RTPSIZE)
    );
}

#[test]
fn udp_discovery_and_select_protocol_match_expected_shapes() {
    let packet = udp_discovery_request(0x01020304);

    assert_eq!(packet.len(), UDP_DISCOVERY_PACKET_LEN);
    assert_eq!(
        &packet[..8],
        &[0x00, 0x01, 0x00, 0x46, 0x01, 0x02, 0x03, 0x04]
    );
    assert!(packet[8..].iter().all(|byte| *byte == 0));

    let mut response = [0u8; UDP_DISCOVERY_PACKET_LEN];
    response[0..2].copy_from_slice(&2u16.to_be_bytes());
    response[2..4].copy_from_slice(&70u16.to_be_bytes());
    response[4..8].copy_from_slice(&0x01020304u32.to_be_bytes());
    response[8..21].copy_from_slice(b"203.0.113.10\0");
    response[72..74].copy_from_slice(&50000u16.to_be_bytes());

    let discovered = parse_udp_discovery_response(&response, 0x01020304)
        .expect("discovery response should parse");

    assert_eq!(
        discovered,
        DiscoveredVoiceAddress {
            address: "203.0.113.10".to_owned(),
            port: 50000,
        }
    );
    let payload: Value = serde_json::from_str(&voice_select_protocol_payload(
        &discovered,
        AEAD_XCHACHA20_POLY1305_RTPSIZE,
    ))
    .expect("select protocol payload should be valid JSON");

    assert_eq!(payload["op"].as_u64(), Some(1));
    assert_eq!(payload["d"]["protocol"].as_str(), Some("udp"));
    assert_eq!(
        payload["d"]["data"]["address"].as_str(),
        Some("203.0.113.10")
    );
    assert_eq!(payload["d"]["data"]["port"].as_u64(), Some(50000));
    assert_eq!(
        payload["d"]["data"]["mode"].as_str(),
        Some(AEAD_XCHACHA20_POLY1305_RTPSIZE)
    );
}

#[test]
fn voice_session_description_parses_mode_and_redacts_secret() {
    let payload = json!({
        "op": 4,
        "d": {
            "mode": AEAD_XCHACHA20_POLY1305_RTPSIZE,
            "secret_key": (0u8..32).collect::<Vec<_>>(),
            "dave_protocol_version": 1,
        },
    });

    let description =
        parse_voice_session_description(&payload).expect("session description should parse");
    let debug = format!("{description:?}");

    assert_eq!(description.mode, AEAD_XCHACHA20_POLY1305_RTPSIZE);
    assert_eq!(description.secret_key.len(), 32);
    assert_eq!(description.dave_protocol_version, Some(1));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("31"));
}

#[test]
fn rtp_header_parses_minimal_and_extended_packets() {
    let packet = [
        0x80, 0x78, 0x12, 0x34, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    ];

    let header = parse_rtp_header(&packet).expect("RTP header should parse");

    assert_eq!(
        header,
        RtpHeader {
            payload_type: 0x78,
            sequence: 0x1234,
            timestamp: 0x01020304,
            ssrc: 0x05060708,
            authenticated_header_len: 12,
            encrypted_extension_body_len: 0,
            payload_offset: 12,
        }
    );

    let mut extended = vec![0x91, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
    extended.extend_from_slice(&0x11223344u32.to_be_bytes());
    extended.extend_from_slice(&0x1000u16.to_be_bytes());
    extended.extend_from_slice(&1u16.to_be_bytes());
    extended.extend_from_slice(&0x55667788u32.to_be_bytes());

    let header = parse_rtp_header(&extended).expect("extended RTP header should parse");

    assert_eq!(header.authenticated_header_len, 20);
    assert_eq!(header.encrypted_extension_body_len, 4);
    assert_eq!(header.payload_offset, 24);
}

#[test]
fn rtp_decrypts_aead_rtpsize_modes_and_strips_extension_body() {
    let key = [7u8; 32];
    let nonce_suffix = [1, 2, 3, 4];
    let mut header = vec![0x90, 0x78, 0, 7, 0, 0, 0, 8, 0, 0, 0, 9];
    header.extend_from_slice(&0x1000u16.to_be_bytes());
    header.extend_from_slice(&1u16.to_be_bytes());
    let plaintext = [b"ext!".as_slice(), b"opus-frame".as_slice()].concat();

    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let mut packet = header.clone();
        packet.extend(encrypt_test_rtp_payload(
            mode,
            &key,
            &header,
            &plaintext,
            nonce_suffix,
        ));
        packet.extend_from_slice(&nonce_suffix);
        let rtp_header = parse_rtp_header(&packet).expect("RTP header should parse");
        let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");

        let decrypted = decryptor
            .decrypt_packet(&packet, &rtp_header)
            .expect("RTP payload should decrypt");

        assert_eq!(decrypted.encrypted_extension_body_len, 4);
        assert_eq!(decrypted.media_payload, b"opus-frame");
    }
}

#[test]
fn outbound_rtp_packet_builder_sets_header_and_advances_state() {
    let mut state = VoiceOutboundRtpState {
        sequence: u16::MAX,
        timestamp: u32::MAX - 100,
        ssrc: 0x01020304,
    };

    let packet = state
        .packetize(&DISCORD_OPUS_SILENCE_FRAME)
        .expect("RTP packet should build");
    let header = parse_rtp_header(&packet).expect("RTP header should parse");

    assert_eq!(packet[0], 0x80);
    assert_eq!(header.payload_type, DISCORD_VOICE_PAYLOAD_TYPE);
    assert_eq!(header.sequence, u16::MAX);
    assert_eq!(header.timestamp, u32::MAX - 100);
    assert_eq!(header.ssrc, 0x01020304);
    assert_eq!(header.payload_offset, RTP_HEADER_MIN_LEN);
    assert_eq!(&packet[header.payload_offset..], DISCORD_OPUS_SILENCE_FRAME);
    assert_eq!(state.sequence, 0);
    assert_eq!(
        state.timestamp,
        (u32::MAX - 100).wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT)
    );

    assert_eq!(
        build_voice_rtp_packet(1, 2, 3, &[]).expect_err("empty payload should fail"),
        "voice RTP packet requires a non-empty Opus payload"
    );
}

#[test]
fn outbound_rtp_encrypts_aead_rtpsize_modes_for_decrypt_round_trip() {
    let key = [9u8; 32];
    let nonce_suffix = [4, 3, 2, 1];
    let packet =
        build_voice_rtp_packet(7, 960, 42, b"opus-frame").expect("RTP packet should build");

    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let encryptor = VoiceRtpEncryptor::new(mode, &key).expect("encryptor should build");
        let encrypted = encryptor
            .encrypt_packet(&packet, nonce_suffix)
            .expect("RTP payload should encrypt");
        let header = parse_rtp_header(&encrypted).expect("encrypted RTP header should parse");
        let decryptor = VoiceRtpDecryptor::new(mode, &key).expect("decryptor should build");
        let decrypted = decryptor
            .decrypt_packet(&encrypted, &header)
            .expect("RTP payload should decrypt");

        assert_eq!(
            &encrypted[encrypted.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..],
            nonce_suffix
        );
        assert_eq!(header.sequence, 7);
        assert_eq!(header.timestamp, 960);
        assert_eq!(header.ssrc, 42);
        assert_eq!(decrypted.media_payload, b"opus-frame");
    }
}

#[test]
fn opus_encoder_encodes_decodable_20ms_stereo_frame() {
    let mut encoder = VoiceOpusEncode::new().expect("Opus encoder should build");
    let pcm = vec![0i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];

    let opus = encoder
        .encode_20ms_i16(&pcm)
        .expect("20 ms stereo frame should encode");

    assert!(!opus.is_empty());

    let mut decoder = OpusDecoder::new(DISCORD_VOICE_SAMPLE_RATE, Channels::Stereo)
        .expect("Opus decoder should build");
    let mut decoded = vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES];
    let samples_per_channel = decoder
        .decode_float(&opus, &mut decoded, false)
        .expect("encoded Opus should decode");

    assert_eq!(samples_per_channel, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL);
    assert_eq!(
        encoder
            .encode_20ms_i16(&pcm[..pcm.len() - 1])
            .expect_err("short frame should fail"),
        format!(
            "voice Opus encoder expected {} interleaved stereo samples, got {}",
            DISCORD_OPUS_20MS_STEREO_SAMPLES,
            DISCORD_OPUS_20MS_STEREO_SAMPLES - 1
        )
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_input_conversion_produces_20ms_stereo_frames() {
    let mono = vec![0.5f32; DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL];
    let stereo = voice_input_f32_to_stereo_i16(&mono, 1);
    assert_eq!(stereo.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
    assert_eq!(stereo[0], stereo[1]);
    assert!(stereo[0] > 0);

    let interleaved = voice_input_i16_to_stereo_i16(&[1, 2, 3, 4, 5, 6], 3);
    assert_eq!(interleaved, vec![1, 2, 4, 5]);

    let unsigned = voice_input_u8_to_stereo_i16(&[0, 255], 2);
    assert_eq!(unsigned, vec![i16::MIN, 32512]);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_frames_resample_44100_to_48000() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames = VoiceMicrophonePcmFrames::new(tx, Arc::clone(&stats), 44_100);
    let input_frames = 883;
    let mut samples = Vec::with_capacity(input_frames * DISCORD_VOICE_CHANNELS_USIZE);
    for index in 0..input_frames {
        samples.push(index as i16);
        samples.push(-(index as i16));
    }

    frames.push_stereo_samples(&samples);
    let frame = rx
        .try_recv()
        .expect("resampled 20 ms frame should be queued");

    assert_eq!(frame.len(), DISCORD_OPUS_20MS_STEREO_SAMPLES);
    assert_eq!(frame[0], 0);
    assert_eq!(frame[1], 0);
    assert!(frame[frame.len() - 2] > 870);
    assert!(frame[frame.len() - 1] < -870);
    assert!(rx.try_recv().is_err());
    assert_eq!(stats.queued_frames.load(Ordering::Relaxed), 1);
    assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 0);
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_frames_count_full_queue_drops() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
    let mut frames =
        VoiceMicrophonePcmFrames::new(tx, Arc::clone(&stats), DISCORD_VOICE_SAMPLE_RATE);
    let samples = vec![1i16; DISCORD_OPUS_20MS_STEREO_SAMPLES * 2];

    frames.push_stereo_samples(&samples);

    assert_eq!(
        rx.try_recv().expect("first frame should queue").len(),
        DISCORD_OPUS_20MS_STEREO_SAMPLES
    );
    assert_eq!(stats.queued_frames.load(Ordering::Relaxed), 1);
    assert_eq!(stats.dropped_frames.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "voice-playback")]
#[tokio::test]
async fn microphone_pcm_recv_returns_frames_in_fifo_order() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);

    tx.try_send(vec![1]).expect("first frame should queue");
    tx.try_send(vec![2]).expect("second frame should queue");
    tx.try_send(vec![3]).expect("third frame should queue");

    assert_eq!(rx.recv().await, Some(vec![1]));
    assert_eq!(rx.recv().await, Some(vec![2]));
    assert_eq!(rx.recv().await, Some(vec![3]));
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_transmit_pacer_delays_queued_frames_to_20ms_slots() {
    let mut pacer = VoiceTransmitPacer::default();
    let start = Instant::now();

    assert_eq!(pacer.delay_before_send(start), None);
    assert_eq!(
        pacer.delay_before_send(start),
        Some(VOICE_PLAYBACK_FRAME_DURATION)
    );
    assert_eq!(
        pacer.delay_before_send(start + VOICE_PLAYBACK_FRAME_DURATION),
        Some(VOICE_PLAYBACK_FRAME_DURATION)
    );
    assert_eq!(
        pacer.delay_before_send(start + VOICE_PLAYBACK_FRAME_DURATION * 4),
        None
    );
}

#[cfg(feature = "voice-playback")]
#[tokio::test]
async fn voice_transmit_pacer_delay_stops_when_gate_disables() {
    let (gate_tx, mut gate_rx) = watch::channel(VoiceCaptureGate {
        enabled: true,
        microphone_sensitivity: MicrophoneSensitivityDb::default(),
        microphone_volume: VoiceVolumePercent::default(),
    });
    let wait = tokio::spawn(async move {
        wait_voice_transmit_pacer_delay(Duration::from_secs(60), &mut gate_rx).await
    });

    gate_tx
        .send(VoiceCaptureGate {
            enabled: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
        })
        .expect("gate receiver should still be alive");

    let outcome = timeout(Duration::from_millis(100), wait)
        .await
        .expect("gate disable should interrupt pacer delay")
        .expect("pacer wait task should finish");
    assert_eq!(outcome, VoiceTransmitPacerDelayOutcome::GateChanged);
}

#[cfg(feature = "voice-playback")]
#[tokio::test]
async fn voice_transmit_pacer_delay_drops_frame_on_disable_then_reenable() {
    let (gate_tx, mut gate_rx) = watch::channel(VoiceCaptureGate {
        enabled: true,
        microphone_sensitivity: MicrophoneSensitivityDb::default(),
        microphone_volume: VoiceVolumePercent::default(),
    });

    gate_tx
        .send(VoiceCaptureGate {
            enabled: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
        })
        .expect("gate receiver should still be alive");
    gate_tx
        .send(VoiceCaptureGate {
            enabled: true,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
        })
        .expect("gate receiver should still be alive");

    let outcome = timeout(
        Duration::from_millis(100),
        wait_voice_transmit_pacer_delay(Duration::from_secs(60), &mut gate_rx),
    )
    .await
    .expect("any gate change should interrupt pacer delay");
    assert_eq!(outcome, VoiceTransmitPacerDelayOutcome::GateChanged);
    assert!(gate_rx.borrow().enabled);
}

#[cfg(feature = "voice-playback")]
#[tokio::test]
async fn voice_child_tasks_waits_for_udp_transmit_shutdown() {
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let mut child_tasks = VoiceChildTasks::default();
    child_tasks.udp_transmit = Some(tokio::spawn(async move {
        sleep(Duration::from_millis(10)).await;
        let _ = done_tx.send(());
    }));

    child_tasks.shutdown_all().await;

    done_rx
        .await
        .expect("shutdown should await UDP transmit completion");
}

#[tokio::test]
async fn voice_runtime_stops_connection_task_by_closing_gate_channels() {
    let (capture_gate_tx, mut capture_gate_rx) = mpsc::unbounded_channel();
    let (playback_gate_tx, mut playback_gate_rx) = mpsc::unbounded_channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let mut connection_task = Some(tokio::spawn(async move {
        assert!(capture_gate_rx.recv().await.is_none());
        assert!(playback_gate_rx.recv().await.is_none());
        let _ = done_tx.send(());
    }));
    let mut capture_gate_tx = Some(capture_gate_tx);
    let mut playback_gate_tx = Some(playback_gate_tx);

    stop_voice_connection_task(
        &mut connection_task,
        &mut capture_gate_tx,
        &mut playback_gate_tx,
        "test voice connection stop",
    )
    .await;

    done_rx
        .await
        .expect("connection task should finish after gate channels close");
    assert!(connection_task.is_none());
    assert!(capture_gate_tx.is_none());
    assert!(playback_gate_tx.is_none());
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_pcm_drain_clears_backlog_before_reenable() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(VOICE_MIC_PCM_FRAME_QUEUE);

    tx.try_send(vec![10]).expect("first frame should queue");
    tx.try_send(vec![20]).expect("second frame should queue");

    drain_voice_microphone_pcm_queue(&mut rx);

    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[cfg(feature = "voice-playback")]
#[test]
fn microphone_capture_stats_track_callback_size_and_clipping() {
    let stats = VoiceMicrophoneCaptureStats::default();

    record_voice_input_chunk(960, 2, &stats);
    record_voice_input_chunk(480, 2, &stats);
    record_voice_input_pcm_stats(&[0, i16::MAX, i16::MIN + 1, 120], &stats);

    assert_eq!(stats.chunks.load(Ordering::Relaxed), 2);
    assert_eq!(stats.frames.load(Ordering::Relaxed), 720);
    assert_eq!(voice_microphone_min_callback_frames(&stats), 240);
    assert_eq!(stats.max_callback_frames.load(Ordering::Relaxed), 480);
    assert_eq!(stats.peak_sample.load(Ordering::Relaxed), 32767);
    assert_eq!(stats.clipped_samples.load(Ordering::Relaxed), 2);
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_input_config_prefers_mono_then_sample_format() {
    assert!(voice_input_channel_rank(1) < voice_input_channel_rank(2));
    assert!(
        voice_input_sample_format_rank(cpal::SampleFormat::F32)
            < voice_input_sample_format_rank(cpal::SampleFormat::I16)
    );
    assert!(
        voice_input_sample_format_rank(cpal::SampleFormat::I16)
            < voice_input_sample_format_rank(cpal::SampleFormat::U16)
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_input_buffer_size_requests_small_supported_fixed_buffer() {
    let supported = cpal::SupportedBufferSize::Range { min: 128, max: 960 };
    assert_eq!(
        voice_input_buffer_size(&supported),
        cpal::BufferSize::Fixed(VOICE_MIC_PREFERRED_BUFFER_FRAMES)
    );
    assert_eq!(
        voice_input_buffer_size(&cpal::SupportedBufferSize::Unknown),
        cpal::BufferSize::Default
    );
}

#[cfg(feature = "voice-playback")]
#[test]
fn voice_speaking_payload_matches_expected_shape() {
    let on: Value = serde_json::from_str(&voice_speaking_payload(1234, true))
        .expect("speaking-on payload should be JSON");
    assert_eq!(on["op"].as_u64(), Some(u64::from(VOICE_OP_SPEAKING)));
    assert_eq!(on["d"]["speaking"].as_u64(), Some(1));
    assert_eq!(on["d"]["delay"].as_u64(), Some(0));
    assert_eq!(on["d"]["ssrc"].as_u64(), Some(1234));

    let off: Value = serde_json::from_str(&voice_speaking_payload(1234, false))
        .expect("speaking-off payload should be JSON");
    assert_eq!(off["d"]["speaking"].as_u64(), Some(0));
}

#[test]
fn fake_outbound_noops_when_capture_gate_is_closed() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("send should no-op"),
        VoiceOutboundSendOutcome::Noop
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);

    state.set_capture_gate(true, true);
    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("muted send should no-op"),
        VoiceOutboundSendOutcome::Noop
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);
}

#[test]
fn fake_outbound_blocks_dave_active_plaintext_fallback() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 10);
    state.set_capture_gate(true, false);
    state.set_dave_active(true);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("DAVE block should be reported"),
        VoiceOutboundSendOutcome::Blocked(VoiceOutboundSendBlockReason::DaveOutboundUnsupported)
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 10);
}

#[test]
fn fake_outbound_uses_dave_outbound_policy_before_transport_encrypt() {
    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
    state.set_capture_gate(true, false);

    assert_eq!(
        state
            .send_opus_frame_with_dave(b"opus-frame", &mut dave)
            .expect("DAVE inactive frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_fake_packet(
        AEAD_AES256_GCM_RTPSIZE,
        &state.events()[1],
        7,
        960,
        42,
        b"opus-frame",
        30u32.to_be_bytes(),
    );

    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    dave.reinit(1).expect("DAVE session should initialize");
    let mut blocked = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 30);
    blocked.set_capture_gate(true, false);
    let rtp = blocked.rtp;

    assert_eq!(
        blocked
            .send_opus_frame_with_dave(b"opus-frame", &mut dave)
            .expect("DAVE not-ready frame should block"),
        VoiceOutboundSendOutcome::Blocked(VoiceOutboundSendBlockReason::DaveOutboundNotReady)
    );
    assert!(blocked.events().is_empty());
    assert_eq!(blocked.rtp, rtp);
    assert_eq!(blocked.nonce_suffix, 30);
}

#[test]
fn fake_outbound_sends_encrypted_packets_without_live_io() {
    for mode in [AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE] {
        let mut state = fake_outbound_state(mode, 0x01020304);
        state.set_capture_gate(true, false);

        assert_eq!(
            state
                .send_opus_frame(b"opus-frame")
                .expect("first frame should send"),
            VoiceOutboundSendOutcome::Sent
        );
        assert_eq!(state.events().len(), 2);
        assert_eq!(
            state.events()[0],
            VoiceOutboundSendEvent::Speaking {
                speaking: true,
                ssrc: 42,
            }
        );
        assert_fake_packet(
            mode,
            &state.events()[1],
            7,
            960,
            42,
            b"opus-frame",
            [1, 2, 3, 4],
        );
        assert_eq!(state.rtp.sequence, 8);
        assert_eq!(state.rtp.timestamp, 1920);
        assert_eq!(state.nonce_suffix, 0x01020305);

        assert_eq!(
            state
                .send_opus_frame(b"next-frame")
                .expect("second frame should send"),
            VoiceOutboundSendOutcome::Sent
        );
        assert_eq!(state.events().len(), 3);
        assert_fake_packet(
            mode,
            &state.events()[2],
            8,
            1920,
            42,
            b"next-frame",
            [1, 2, 3, 5],
        );
        assert_eq!(state.rtp.sequence, 9);
        assert_eq!(state.rtp.timestamp, 2880);
        assert_eq!(state.nonce_suffix, 0x01020306);
    }
}

#[test]
fn fake_outbound_stop_sends_finite_silence_then_speaking_off() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        state.stop_speaking().expect("stop should send silence"),
        VoiceOutboundSendOutcome::Sent
    );

    assert_eq!(state.events().len(), DISCORD_TRAILING_SILENCE_FRAMES + 3);
    for index in 0..DISCORD_TRAILING_SILENCE_FRAMES {
        assert_fake_packet(
            AEAD_AES256_GCM_RTPSIZE,
            &state.events()[index + 2],
            8 + index as u16,
            1920 + index as u32 * DISCORD_OPUS_TIMESTAMP_INCREMENT,
            42,
            &DISCORD_OPUS_SILENCE_FRAME,
            (21 + index as u32).to_be_bytes(),
        );
    }
    assert_eq!(
        state.events()[DISCORD_TRAILING_SILENCE_FRAMES + 2],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp.sequence, 13);
    assert_eq!(state.rtp.timestamp, 6720);
    assert_eq!(state.nonce_suffix, 26);
}

#[test]
fn fake_outbound_stop_sends_speaking_off_when_capture_gate_closes() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);
    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect("frame should send"),
        VoiceOutboundSendOutcome::Sent
    );
    let event_count = state.events().len();
    let rtp = state.rtp;
    let nonce_suffix = state.nonce_suffix;

    state.set_capture_gate(true, true);
    assert_eq!(
        state
            .stop_speaking()
            .expect("muted stop should send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(state.events().len(), event_count + 1);
    assert_eq!(
        state.events()[event_count],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, nonce_suffix);

    state.speaking = true;
    state.set_capture_gate(false, false);
    assert_eq!(
        state
            .stop_speaking()
            .expect("disallowed stop should send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(state.events().len(), event_count + 2);
    assert_eq!(
        state.events()[event_count + 1],
        VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, nonce_suffix);
}

#[test]
fn fake_outbound_stop_uses_dave_policy_for_silence_frames() {
    let mut dave = VoiceDaveState::new(&test_voice_gateway_session());
    dave.reinit(1).expect("DAVE session should initialize");
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, 20);
    state.set_capture_gate(true, false);
    state.speaking = true;
    let rtp = state.rtp;

    assert_eq!(
        state
            .stop_speaking_with_dave(&mut dave)
            .expect("DAVE not-ready silence should still send speaking off"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        state.events(),
        &[VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }]
    );
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, 20);
    assert!(!state.speaking);
}

#[test]
fn fake_outbound_nonce_exhaustion_fails_without_state_change() {
    let mut state = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX);
    state.set_capture_gate(true, false);
    let rtp = state.rtp;

    assert_eq!(
        state
            .send_opus_frame(b"opus-frame")
            .expect_err("exhausted nonce should fail"),
        "voice RTP nonce suffix exhausted"
    );
    assert!(state.events().is_empty());
    assert_eq!(state.rtp, rtp);
    assert_eq!(state.nonce_suffix, u32::MAX);

    let mut stopping = fake_outbound_state(AEAD_AES256_GCM_RTPSIZE, u32::MAX - 2);
    stopping.set_capture_gate(true, false);
    stopping.speaking = true;
    let rtp = stopping.rtp;
    assert_eq!(
        stopping
            .stop_speaking()
            .expect("stop should still clear speaking"),
        VoiceOutboundSendOutcome::Sent
    );
    assert_eq!(
        stopping.events(),
        &[VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: 42,
        }]
    );
    assert_eq!(stopping.rtp, rtp);
    assert_eq!(stopping.nonce_suffix, u32::MAX - 2);
    assert!(!stopping.speaking);
}

#[test]
fn rtp_header_rejects_malformed_packets() {
    assert_eq!(
        parse_rtp_header(&[0; 11]).expect_err("short packet should fail"),
        "RTP packet is too short"
    );

    let packet = [0x40, 0x78, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

    assert_eq!(
        parse_rtp_header(&packet).expect_err("wrong version should fail"),
        "RTP packet has unsupported version"
    );
}

#[test]
fn rtp_header_rejects_rtcp_reports_before_payload_type_masking() {
    let local_ssrc = 0x0000_f5e7u32;
    let mut receiver_report = vec![0x80, 0xc9, 0, 7];
    receiver_report.extend_from_slice(&local_ssrc.to_be_bytes());
    receiver_report.extend_from_slice(&[0, 0, 0, 0]);

    assert!(looks_like_rtcp_packet(&receiver_report));
    assert_eq!(rtcp_sender_ssrc(&receiver_report), Some(local_ssrc));
    assert_eq!(
        parse_rtp_header(&receiver_report).expect_err("RTCP should not parse as RTP"),
        "RTP parser received RTCP packet"
    );

    let sender_report = [0x80, 0xc8, 0, 12, 0, 0, 0xf5, 0xe7, 0, 0, 0, 0];
    assert!(looks_like_rtcp_packet(&sender_report));
    assert_eq!(
        parse_rtp_header(&sender_report).expect_err("RTCP should not parse as RTP"),
        "RTP parser received RTCP packet"
    );
}

fn fake_outbound_state(mode: &str, nonce_suffix: u32) -> VoiceOutboundSendState {
    VoiceOutboundSendState::new(
        mode,
        &[9u8; 32],
        VoiceOutboundRtpState {
            sequence: 7,
            timestamp: 960,
            ssrc: 42,
        },
        nonce_suffix,
    )
    .expect("fake outbound state should build")
}

fn test_voice_gateway_session() -> VoiceGatewaySession {
    VoiceGatewaySession {
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "voice-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "voice-token".to_owned(),
    }
}

fn assert_fake_packet(
    mode: &str,
    event: &VoiceOutboundSendEvent,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    expected_payload: &[u8],
    nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
) {
    let VoiceOutboundSendEvent::Packet { bytes } = event else {
        panic!("expected fake packet event, got {event:?}");
    };
    let packet_bytes = bytes.as_slice();
    let header = parse_rtp_header(packet_bytes).expect("fake RTP header should parse");
    let decryptor = VoiceRtpDecryptor::new(mode, &[9u8; 32]).expect("decryptor should build");
    let decrypted = decryptor
        .decrypt_packet(packet_bytes, &header)
        .expect("fake RTP packet should decrypt");

    let actual_nonce_suffix = &packet_bytes[packet_bytes.len() - RTP_AEAD_NONCE_SUFFIX_BYTES..];
    assert_eq!(actual_nonce_suffix, nonce_suffix.as_slice());
    assert_eq!(header.sequence, sequence);
    assert_eq!(header.timestamp, timestamp);
    assert_eq!(header.ssrc, ssrc);
    assert_eq!(decrypted.media_payload, expected_payload);
}

fn encrypt_test_rtp_payload(
    mode: &str,
    key: &[u8],
    aad: &[u8],
    plaintext: &[u8],
    nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
) -> Vec<u8> {
    match mode {
        AEAD_AES256_GCM_RTPSIZE => {
            let cipher = Aes256Gcm::new_from_slice(key).expect("test key is valid");
            let mut nonce = [0u8; 12];
            nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
            cipher
                .encrypt(
                    AesGcmNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .expect("test payload encrypts")
        }
        AEAD_XCHACHA20_POLY1305_RTPSIZE => {
            let cipher = XChaCha20Poly1305::new_from_slice(key).expect("test key is valid");
            let mut nonce = [0u8; 24];
            nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
            cipher
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad,
                    },
                )
                .expect("test payload encrypts")
        }
        other => panic!("unsupported test mode: {other}"),
    }
}
