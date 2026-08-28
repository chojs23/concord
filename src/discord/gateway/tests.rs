use super::{
    ConnectionOutcome, GATEWAY_SEND_LIMIT, GATEWAY_SEND_WINDOW, GATEWAY_URL, GatewayCommand,
    GatewayHandshake, GatewayPresence, GatewaySendWindow, GatewaySender, GatewaySessionResources,
    GatewayZlibDecoder, GuildMemberRequestKind, GuildMemberRequestScheduler, HeartbeatAckState,
    MAX_PENDING_GUILD_MEMBER_REQUESTS, SessionState, SubscriptionDeduper,
    USER_ACCOUNT_CAPABILITIES, build_identify_payload, build_resume_payload, close_code_outcome,
    create_stream_payload, delete_stream_payload, direct_message_subscribe_payload,
    dispatch_command, gateway_guild_member_rate_limit, gateway_request,
    guild_channel_subscribe_payload, parse_gateway_frame, presence_update_payload,
    ready_installation_id, request_guild_members_by_ids_payload, search_guild_members_payload,
    voice_state_update_payload, watch_stream_payload,
};
use crate::discord::fingerprint::{
    CLIENT_BROWSER, CLIENT_BROWSER_VERSION, CLIENT_BUILD_NUMBER, ClientFingerprint,
    DISCORD_REFERRER_CURRENT, DISCORD_REFERRING_DOMAIN_CURRENT, accept_language,
};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::state::ClientCacheState;
use crate::discord::{ActivityEmoji, ActivityInfo, ActivityKind, PresenceStatus, VoiceScope};
use flate2::{Compression, write::ZlibEncoder};
use serde_json::json;
use std::{io::Write, time::Duration};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::http::header::{
    ACCEPT_LANGUAGE, CACHE_CONTROL, ORIGIN, PRAGMA, USER_AGENT,
};

#[test]
fn gateway_send_window_enforces_the_connection_budget() {
    let mut window = GatewaySendWindow::default();
    let started_at = Instant::now();

    for _ in 0..GATEWAY_SEND_LIMIT {
        assert_eq!(window.delay_at(started_at), None);
        window.record(started_at);
    }

    assert_eq!(window.delay_at(started_at), Some(GATEWAY_SEND_WINDOW));
    assert_eq!(
        window.delay_at(started_at + Duration::from_secs(59)),
        Some(Duration::from_secs(1))
    );
    assert_eq!(window.delay_at(started_at + GATEWAY_SEND_WINDOW), None);
}

#[test]
fn voice_state_dispatch_uses_the_urgent_queue_without_blocking() {
    let (urgent_tx, mut urgent_rx) = tokio::sync::mpsc::unbounded_channel();
    let (normal_tx, _normal_rx) = tokio::sync::mpsc::unbounded_channel();
    let sender = GatewaySender {
        urgent_tx,
        normal_tx,
    };

    dispatch_command(
        &sender,
        GatewayCommand::UpdateVoiceState {
            guild_id: Some(Id::new(10)),
            channel_id: None,
            self_mute: false,
            self_deaf: false,
        },
        &mut SubscriptionDeduper::default(),
        &mut GatewaySessionResources::default(),
    )
    .expect("voice state update should enter the urgent queue");

    let request = urgent_rx
        .try_recv()
        .expect("voice state update should reach the time-sensitive gateway writer queue");

    let payload: serde_json::Value =
        serde_json::from_str(&request.payload).expect("gateway payload should be valid json");
    assert_eq!(payload["op"].as_u64(), Some(4));
    assert!(payload["d"]["channel_id"].is_null());
    assert!(
        request.completion.is_none(),
        "ordinary voice state updates must not block the gateway read loop"
    );
}

#[tokio::test]
async fn urgent_gateway_send_waits_for_writer_completion() {
    let (urgent_tx, mut urgent_rx) = tokio::sync::mpsc::unbounded_channel();
    let (normal_tx, _normal_rx) = tokio::sync::mpsc::unbounded_channel();
    let sender = GatewaySender {
        urgent_tx,
        normal_tx,
    };

    let send_task = tokio::spawn(async move {
        sender
            .send_urgent("voice leave".to_owned())
            .await
            .expect("urgent send should complete");
    });
    let request = urgent_rx
        .recv()
        .await
        .expect("urgent send should reach the gateway writer queue");

    assert!(
        !send_task.is_finished(),
        "shutdown must not close the websocket before voice leave is sent"
    );
    request
        .completion
        .expect("urgent send should request writer completion")
        .send(Ok(()))
        .expect("urgent sender should still await completion");
    send_task.await.expect("urgent send task should finish");
}

#[test]
fn guild_member_rate_limit_delays_targeted_requests_until_retry_after() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let mut scheduler = GuildMemberRequestScheduler::default();
    scheduler.enqueue_by_ids(guild_id, vec![Id::new(10)], false, now);
    let nonce = scheduler.pending[0].request.nonce.clone();
    scheduler
        .start_due(now)
        .expect("the queued request should be due");
    scheduler.complete_send(now);
    scheduler.enqueue_by_ids(guild_id, vec![Id::new(20)], false, now);

    let rate_limited = json!({
        "op": 0,
        "t": "RATE_LIMITED",
        "d": {
            "opcode": 8,
            "retry_after": 45.0,
            "meta": {
                "guild_id": guild_id.to_string(),
                "nonce": nonce,
            }
        }
    });
    let rate_limit =
        gateway_guild_member_rate_limit(&rate_limited).expect("rate limit should parse");
    scheduler.apply_rate_limit(
        rate_limit.guild_id,
        rate_limit.nonce.as_deref(),
        rate_limit.retry_after,
        now,
    );

    let retry_at = now + Duration::from_secs(45);
    assert_eq!(scheduler.pending.len(), 2);
    assert!(
        scheduler
            .pending
            .iter()
            .all(|pending| pending.send_at == retry_at),
        "the server-provided retry time delays the guild without adding fixed spacing"
    );
    assert!(scheduler.awaiting_response.is_empty());
    assert!(
        scheduler
            .start_due(retry_at - Duration::from_millis(1))
            .is_none()
    );
    scheduler
        .start_due(retry_at)
        .expect("the correlated request should retry when Discord allows it");
    scheduler.complete_send(retry_at);
    scheduler
        .start_due(retry_at)
        .expect("other targeted requests should not gain a fixed delay");
}

#[test]
fn targeted_member_requests_and_searches_do_not_gain_a_fixed_guild_cooldown() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let mut scheduler = GuildMemberRequestScheduler::default();
    assert!(scheduler.enqueue_search(
        guild_id,
        "neo".to_owned(),
        10,
        false,
        "search".to_owned(),
        now,
    ));
    scheduler.enqueue_by_ids(guild_id, (1..=101).map(Id::new).collect(), false, now);

    assert_eq!(scheduler.pending.len(), 3);
    assert!(
        scheduler
            .pending
            .iter()
            .all(|pending| pending.send_at == now)
    );
    for _ in 0..3 {
        scheduler
            .start_due(now)
            .expect("each targeted request should be ready without a guild cooldown");
        scheduler.complete_send(now);
    }
    assert!(scheduler.pending.is_empty());
}

#[test]
fn explicit_guild_member_rate_limit_survives_new_requests_and_reidentify() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let retry_at = now + Duration::from_secs(45);
    let mut scheduler = GuildMemberRequestScheduler::default();

    scheduler.apply_rate_limit(
        guild_id,
        Some("unknown-request"),
        Duration::from_secs(45),
        now,
    );
    scheduler.enqueue_by_ids(
        guild_id,
        vec![Id::new(10)],
        false,
        now + Duration::from_secs(1),
    );
    assert_eq!(scheduler.pending[0].send_at, retry_at);

    scheduler.start_new_session(now + Duration::from_secs(2));
    assert_eq!(
        scheduler.pending[0].send_at, retry_at,
        "reidentifying must not discard Discord's explicit retry_after"
    );
    assert!(
        scheduler
            .start_due(retry_at - Duration::from_millis(1))
            .is_none()
    );
    scheduler
        .start_due(retry_at)
        .expect("the request should resume at Discord's retry deadline");
}

#[test]
fn guild_member_requests_survive_resume_and_reidentify() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let mut scheduler = GuildMemberRequestScheduler::default();
    assert!(scheduler.enqueue_search(
        guild_id,
        "neo".to_owned(),
        10,
        false,
        "member-request".to_owned(),
        now,
    ));

    scheduler
        .start_due(now)
        .expect("the queued request should start sending");
    let disconnected_at = now + Duration::from_secs(1);
    scheduler.cancel_in_flight(disconnected_at);
    let resumed = scheduler
        .pending
        .front()
        .expect("an unfinished send should return to the session queue");
    assert_eq!(resumed.request.nonce, "member-request");
    assert_eq!(resumed.send_at, disconnected_at);
    let resumed_at = resumed.send_at;

    scheduler
        .start_due(resumed_at)
        .expect("the resumed request should send without a fixed cooldown");
    scheduler.complete_send(resumed_at);
    assert_eq!(scheduler.awaiting_response.len(), 1);

    let written_disconnect_at = resumed_at + Duration::from_secs(1);
    scheduler.prepare_reconnect(written_disconnect_at);
    assert!(scheduler.awaiting_response.is_empty());
    let written_retry = scheduler
        .pending
        .front()
        .expect("a written request without a response should retry after reconnect");
    assert_eq!(written_retry.request.nonce, "member-request");
    assert_eq!(written_retry.send_at, written_disconnect_at);

    scheduler.acknowledge("member-request");
    assert!(
        scheduler.pending.is_empty(),
        "a replayed chunk should cancel the scheduled retry"
    );

    assert!(scheduler.enqueue_search(
        guild_id,
        "next".to_owned(),
        10,
        false,
        "member-request-2".to_owned(),
        written_disconnect_at,
    ));
    let next_send_at = scheduler
        .pending
        .front()
        .expect("the next request should be queued")
        .send_at;
    scheduler
        .start_due(next_send_at)
        .expect("the next request should start sending");
    scheduler.complete_send(next_send_at);
    let reidentified_at = next_send_at + Duration::from_secs(1);
    scheduler.start_new_session(reidentified_at);
    assert!(scheduler.awaiting_response.is_empty());
    let reidentified = scheduler
        .pending
        .front()
        .expect("an unresolved response should retry in the new session");
    assert_eq!(reidentified.request.nonce, "member-request-2");
    assert_eq!(reidentified.send_at, reidentified_at);
}

#[test]
fn guild_member_queue_coalesces_searches_and_merges_id_hydration() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let mut scheduler = GuildMemberRequestScheduler::default();

    assert!(scheduler.enqueue_search(
        guild_id,
        "ne".to_owned(),
        10,
        false,
        "search-1".to_owned(),
        now,
    ));
    assert!(scheduler.enqueue_search(
        guild_id,
        "neo".to_owned(),
        10,
        false,
        "search-2".to_owned(),
        now,
    ));
    assert_eq!(scheduler.pending.len(), 1);
    assert_eq!(scheduler.pending[0].request.nonce, "search-2");

    scheduler.enqueue_by_ids(guild_id, vec![Id::new(2), Id::new(1)], false, now);
    scheduler.enqueue_by_ids(guild_id, vec![Id::new(2), Id::new(3)], false, now);
    assert_eq!(scheduler.pending.len(), 2);
    let GuildMemberRequestKind::ByIds { user_ids, .. } = &scheduler.pending[1].request.kind else {
        panic!("second request should hydrate member ids");
    };
    assert_eq!(user_ids, &[Id::new(2), Id::new(1), Id::new(3)]);
}

#[test]
fn guild_member_id_hydration_displaces_searches_instead_of_dropping_ids() {
    let now = Instant::now();
    let guild_id = Id::new(99);
    let mut scheduler = GuildMemberRequestScheduler::default();
    scheduler.enqueue_by_ids(guild_id, (1..=99).map(Id::new).collect(), false, now);
    for index in 0..MAX_PENDING_GUILD_MEMBER_REQUESTS - 1 {
        assert!(scheduler.enqueue_search(
            Id::new(1_000 + index as u64),
            format!("member-{index}"),
            10,
            true,
            format!("search-{index}"),
            now,
        ));
    }

    scheduler.enqueue_by_ids(guild_id, vec![Id::new(1_000), Id::new(1_001)], false, now);

    let hydrated_user_ids = scheduler
        .pending
        .iter()
        .filter_map(|pending| match &pending.request.kind {
            GuildMemberRequestKind::ByIds { user_ids, .. } => Some(user_ids.as_slice()),
            GuildMemberRequestKind::Search { .. } => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(hydrated_user_ids.contains(&Id::new(1_000)));
    assert!(hydrated_user_ids.contains(&Id::new(1_001)));
    assert_eq!(scheduler.pending.len(), MAX_PENDING_GUILD_MEMBER_REQUESTS);
    assert_eq!(
        scheduler
            .pending
            .iter()
            .filter(|pending| matches!(pending.request.kind, GuildMemberRequestKind::Search { .. }))
            .count(),
        MAX_PENDING_GUILD_MEMBER_REQUESTS - 2
    );
}

#[test]
fn gateway_handshake_headers_match_shared_fingerprint() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let request =
        gateway_request(super::GATEWAY_URL, &fingerprint).expect("gateway request should be valid");
    let headers = request.headers();

    assert_eq!(
        headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some(fingerprint.user_agent.as_str())
    );
    assert_eq!(
        headers
            .get(ACCEPT_LANGUAGE)
            .and_then(|value| value.to_str().ok()),
        Some(accept_language(&fingerprint.system_locale).as_str())
    );
    assert_eq!(
        headers.get(ORIGIN).and_then(|value| value.to_str().ok()),
        Some("https://discord.com")
    );
    assert_eq!(
        headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers.get(PRAGMA).and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
}

#[test]
fn gateway_connection_plan_pairs_the_endpoint_with_the_required_handshake() {
    let mut initial = SessionState::default();
    let initial_plan = initial.next_connection();
    assert_eq!(initial_plan.url, GATEWAY_URL);
    assert_eq!(initial_plan.handshake, GatewayHandshake::Identify);
    assert!(initial_plan.recovery_warning.is_none());

    let mut resumable = SessionState {
        session_id: Some("session".to_owned()),
        resume_url: Some(
            "wss://gateway-us-east1-b.discord.gg/resume?region=us-east&encoding=etf".to_owned(),
        ),
        last_sequence: Some(42),
        ..SessionState::default()
    };
    let resume_plan = resumable.next_connection();
    assert_eq!(
        resume_plan.url,
        "wss://gateway-us-east1-b.discord.gg/resume?region=us-east&v=9&encoding=json&compress=zlib-stream"
    );
    assert_eq!(
        resume_plan.handshake,
        GatewayHandshake::Resume {
            session_id: "session".to_owned(),
            sequence: 42,
        }
    );
    assert!(resume_plan.recovery_warning.is_none());

    assert!(resumable.abandon_failed_resume(&resume_plan.handshake));
    assert!(!resumable.can_resume());
    assert_eq!(
        resumable.next_connection().handshake,
        GatewayHandshake::Identify
    );
}

#[test]
fn invalid_resume_endpoints_clear_the_session_and_reidentify() {
    for invalid_url in [
        "not a URL",
        "https://gateway.discord.gg",
        "ws://gateway.discord.gg",
        "ftp://gateway.discord.gg",
        "wss://",
        "wss://user:password@gateway.discord.gg",
        "wss://gateway.discord.gg/#fragment",
    ] {
        let mut session = SessionState {
            session_id: Some("session".to_owned()),
            resume_url: Some(invalid_url.to_owned()),
            last_sequence: Some(42),
            ..SessionState::default()
        };

        let plan = session.next_connection();

        assert_eq!(plan.url, GATEWAY_URL, "{invalid_url}");
        assert_eq!(plan.handshake, GatewayHandshake::Identify, "{invalid_url}");
        assert!(plan.recovery_warning.is_some(), "{invalid_url}");
        assert!(!session.can_resume(), "{invalid_url}");
        assert_eq!(session.session_id, None, "{invalid_url}");
        assert_eq!(session.resume_url, None, "{invalid_url}");
        assert_eq!(session.last_sequence, None, "{invalid_url}");
    }

    let mut incomplete = SessionState {
        session_id: Some("session".to_owned()),
        resume_url: Some("wss://gateway.discord.gg".to_owned()),
        last_sequence: None,
        ..SessionState::default()
    };
    let plan = incomplete.next_connection();
    assert_eq!(plan.handshake, GatewayHandshake::Identify);
    assert!(plan.recovery_warning.is_some());
    assert!(!incomplete.can_resume());
}

#[test]
fn gateway_zlib_decoder_keeps_stream_state_across_fragmented_payloads() {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let mut decoder = GatewayZlibDecoder::default();

    encoder
        .write_all(br#"{"op":10,"d":{"heartbeat_interval":41250}}"#)
        .expect("first gateway payload should be compressed");
    encoder
        .flush()
        .expect("first gateway payload should be sync-flushed");
    let first = encoder.get_ref().clone();
    let split_at = first.len() / 2;

    assert_eq!(
        decoder
            .decode(&first[..split_at])
            .expect("first compressed fragment should be accepted"),
        None
    );
    assert_eq!(
        decoder
            .decode(&first[split_at..])
            .expect("second compressed fragment should complete the payload")
            .as_deref(),
        Some(r#"{"op":10,"d":{"heartbeat_interval":41250}}"#)
    );

    let second_start = encoder.get_ref().len();
    encoder
        .write_all(br#"{"op":11}"#)
        .expect("second gateway payload should be compressed");
    encoder
        .flush()
        .expect("second gateway payload should be sync-flushed");
    assert_eq!(
        decoder
            .decode(&encoder.get_ref()[second_start..])
            .expect("the existing zlib stream should decode another payload")
            .as_deref(),
        Some(r#"{"op":11}"#)
    );
}

#[test]
fn repeated_malformed_gateway_frame_escalates_from_resume_to_reidentify() {
    let mut session = SessionState {
        session_id: Some("session".to_owned()),
        resume_url: Some("wss://gateway.discord.gg".to_owned()),
        last_sequence: Some(40),
        ..SessionState::default()
    };

    let first = parse_gateway_frame(r#"{"op":0,"s":41,"d":}"#, &mut session)
        .expect_err("the first malformed frame should require recovery");
    assert_eq!(first.outcome, super::FrameOutcome::Resume);
    assert_eq!(session.last_sequence, Some(40));

    let replay = parse_gateway_frame(r#"{"op":0,"s":41,"d":}"#, &mut session)
        .expect_err("a repeated malformed frame should abandon the replay buffer");
    assert_eq!(replay.outcome, super::FrameOutcome::Reidentify);
    assert_eq!(session.last_sequence, Some(40));

    let mut unidentified = SessionState::default();
    let without_session = parse_gateway_frame("not JSON", &mut unidentified)
        .expect_err("a malformed frame without a resumable session must re-identify");
    assert_eq!(without_session.outcome, super::FrameOutcome::Reidentify);
}

#[test]
fn identify_payload_carries_user_account_capabilities() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    fingerprint.set_installation_id_for_test("installation-id");
    let client_state = ClientCacheState {
        highest_guild_message_id: Some(Id::<MessageMarker>::new(123)),
        highest_private_message_id: Some(Id::<MessageMarker>::new(456)),
        read_state_version: Some(4),
        user_guild_settings_version: Some(5),
    };
    let payload: serde_json::Value = serde_json::from_str(&build_identify_payload(
        "dummy-token",
        &fingerprint,
        None,
        client_state,
    ))
    .expect("identify payload should be valid json");
    assert_eq!(payload["op"].as_u64(), Some(2));
    assert_eq!(
        payload["d"]["capabilities"].as_u64(),
        Some(USER_ACCOUNT_CAPABILITIES)
    );
    assert_eq!(
        payload["d"]["properties"]["os"].as_str(),
        Some(fingerprint.os)
    );
    assert_eq!(
        payload["d"]["properties"]["browser"].as_str(),
        Some(CLIENT_BROWSER)
    );
    assert_eq!(
        payload["d"]["properties"]["browser_user_agent"].as_str(),
        Some(fingerprint.user_agent.as_str())
    );
    assert_eq!(
        payload["d"]["properties"]["browser_version"].as_str(),
        Some(CLIENT_BROWSER_VERSION)
    );
    assert_eq!(
        payload["d"]["properties"]["os_version"].as_str(),
        Some(fingerprint.os_version.as_str())
    );
    assert_eq!(
        payload["d"]["properties"]["client_build_number"].as_u64(),
        Some(CLIENT_BUILD_NUMBER)
    );
    assert_eq!(
        payload["d"]["properties"]["system_locale"].as_str(),
        Some(fingerprint.system_locale.as_str())
    );
    assert_eq!(
        payload["d"]["properties"]["referrer_current"].as_str(),
        Some(DISCORD_REFERRER_CURRENT)
    );
    assert_eq!(
        payload["d"]["properties"]["referring_domain_current"].as_str(),
        Some(DISCORD_REFERRING_DOMAIN_CURRENT)
    );
    assert_eq!(
        payload["d"]["properties"]["installation_id"].as_str(),
        Some("installation-id")
    );
    assert_eq!(payload["d"]["compress"].as_bool(), Some(false));
    assert_eq!(
        payload["d"]["client_state"]["highest_last_message_id"].as_str(),
        Some("123")
    );
    assert_eq!(
        payload["d"]["client_state"]["read_state_version"].as_i64(),
        Some(4)
    );
    assert_eq!(
        payload["d"]["client_state"]["user_guild_settings_version"].as_i64(),
        Some(5)
    );
    assert_eq!(
        payload["d"]["client_state"]["private_channels_version"].as_str(),
        Some("456")
    );
    assert_eq!(payload["d"]["presence"]["status"].as_str(), Some("unknown"));
    assert_eq!(
        ready_installation_id(&json!({
            "apex_experiments": {
                "installation": "ready-installation-id"
            }
        })),
        Some("ready-installation-id")
    );
}

#[test]
fn reidentify_payload_uses_the_last_requested_presence() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let presence = GatewayPresence {
        status: PresenceStatus::Online,
        activities: vec![ActivityInfo::playing("Concord")],
    };

    let payload: serde_json::Value = serde_json::from_str(&build_identify_payload(
        "dummy-token",
        &fingerprint,
        Some(&presence),
        ClientCacheState::default(),
    ))
    .expect("reidentify payload should be valid json");

    assert_eq!(payload["d"]["presence"]["status"].as_str(), Some("online"));
    assert_eq!(
        payload["d"]["presence"]["activities"][0]["name"].as_str(),
        Some("Concord")
    );
}

#[test]
fn presence_update_payload_maps_statuses_for_gateway() {
    let online_payload: serde_json::Value =
        serde_json::from_str(&presence_update_payload(PresenceStatus::Online, &[]))
            .expect("presence payload should be valid json");
    assert_eq!(online_payload["op"].as_u64(), Some(3));
    assert_eq!(online_payload["d"]["status"].as_str(), Some("online"));
    assert_eq!(online_payload["d"]["since"].as_u64(), Some(0));
    assert_eq!(online_payload["d"]["activities"], json!([]));
    assert_eq!(online_payload["d"]["afk"].as_bool(), Some(false));

    let idle_payload: serde_json::Value =
        serde_json::from_str(&presence_update_payload(PresenceStatus::Idle, &[]))
            .expect("presence payload should be valid json");
    assert_eq!(idle_payload["d"]["status"].as_str(), Some("idle"));

    let dnd_payload: serde_json::Value =
        serde_json::from_str(&presence_update_payload(PresenceStatus::DoNotDisturb, &[]))
            .expect("presence payload should be valid json");
    assert_eq!(dnd_payload["d"]["status"].as_str(), Some("dnd"));

    let offline_payload: serde_json::Value =
        serde_json::from_str(&presence_update_payload(PresenceStatus::Offline, &[]))
            .expect("presence payload should be valid json");
    assert_eq!(offline_payload["d"]["status"].as_str(), Some("invisible"));
}

#[test]
fn presence_update_payload_carries_custom_status_emoji() {
    let mut activity = ActivityInfo::test(ActivityKind::Custom, "");
    activity.emoji = Some(ActivityEmoji {
        name: "wave".to_owned(),
        id: Some(Id::<EmojiMarker>::new(50)),
        animated: true,
    });
    let payload: serde_json::Value = serde_json::from_str(&presence_update_payload(
        PresenceStatus::Online,
        &[activity],
    ))
    .expect("presence payload should be valid json");
    let emoji = &payload["d"]["activities"][0]["emoji"];
    assert_eq!(emoji["name"].as_str(), Some("wave"));
    assert_eq!(emoji["id"].as_str(), Some("50"));
    assert_eq!(emoji["animated"].as_bool(), Some(true));
}

#[test]
fn presence_update_payload_includes_manual_activity() {
    let activity = ActivityInfo::playing("Concord");
    let payload: serde_json::Value = serde_json::from_str(&presence_update_payload(
        PresenceStatus::Online,
        &[activity],
    ))
    .expect("presence payload should be valid json");

    assert_eq!(
        payload["d"]["activities"][0]["name"].as_str(),
        Some("Concord")
    );
    assert_eq!(payload["d"]["activities"][0]["type"].as_u64(), Some(0));
}

#[test]
fn presence_update_payload_serializes_rich_activity_fields() {
    let activity = ActivityInfo {
        id: Some("receive-only-id".to_owned()),
        kind: ActivityKind::Hang,
        name: "Hang Status".to_owned(),
        created_at: Some(1_700_000_000_000),
        session_id: Some("receive-only-session".to_owned()),
        platform: Some("xbox".to_owned()),
        supported_platforms: vec!["xbox".to_owned(), "desktop".to_owned()],
        details: Some("Building Concord".to_owned()),
        details_url: Some("https://example.com/details".to_owned()),
        state: Some("custom".to_owned()),
        state_url: Some("https://example.com/state".to_owned()),
        application_id: Some("12345".to_owned()),
        parent_application_id: Some("54321".to_owned()),
        status_display_type: Some(2),
        sync_id: Some("sync-1".to_owned()),
        flags: Some(16),
        timestamps: Some(crate::discord::ActivityTimestamps {
            start: Some(1_700_000_000_000),
            end: Some(1_700_000_100_000),
        }),
        assets: Some(crate::discord::ActivityAssets {
            large_image: Some("cover".to_owned()),
            large_text: Some("On the main menu".to_owned()),
            large_url: Some("https://example.com/large".to_owned()),
            small_image: Some("small".to_owned()),
            small_text: Some("Small".to_owned()),
            small_url: Some("https://example.com/small".to_owned()),
            invite_cover_image: Some("invite".to_owned()),
            extra_fields: [("future_asset".to_owned(), json!(true))]
                .into_iter()
                .collect(),
        }),
        party: Some(crate::discord::ActivityParty {
            id: Some("party-1".to_owned()),
            size: Some((2, 5)),
            privacy: Some(1),
            extra_fields: [("future_party".to_owned(), json!("kept"))]
                .into_iter()
                .collect(),
        }),
        secrets: Some(crate::discord::ActivitySecrets {
            join: Some("join-secret".to_owned()),
            spectate: Some("spectate-secret".to_owned()),
            extra_fields: [("future_secret".to_owned(), json!(7))]
                .into_iter()
                .collect(),
        }),
        buttons: vec![crate::discord::ActivityButton {
            label: "Join".to_owned(),
            url: "https://example.com/join".to_owned(),
        }],
        instance: Some(true),
        metadata: [("artist_ids".to_owned(), json!(["artist-1"]))]
            .into_iter()
            .collect(),
        extra_fields: [("future_activity".to_owned(), json!({ "value": 1 }))]
            .into_iter()
            .collect(),
        ..ActivityInfo::playing("Concord")
    };
    let payload: serde_json::Value = serde_json::from_str(&presence_update_payload(
        PresenceStatus::Online,
        &[activity],
    ))
    .expect("presence payload should be valid json");
    let entry = &payload["d"]["activities"][0];

    assert_eq!(entry["type"].as_u64(), Some(6));
    assert_eq!(entry["name"].as_str(), Some("Hang Status"));
    assert!(entry.get("id").is_none());
    assert!(entry.get("created_at").is_none());
    assert!(entry.get("session_id").is_none());
    assert_eq!(entry["platform"].as_str(), Some("xbox"));
    assert_eq!(entry["supported_platforms"], json!(["xbox", "desktop"]));
    assert_eq!(entry["details"].as_str(), Some("Building Concord"));
    assert_eq!(
        entry["details_url"].as_str(),
        Some("https://example.com/details")
    );
    assert_eq!(entry["state"].as_str(), Some("custom"));
    assert_eq!(
        entry["state_url"].as_str(),
        Some("https://example.com/state")
    );
    assert_eq!(entry["application_id"].as_str(), Some("12345"));
    assert_eq!(entry["parent_application_id"].as_str(), Some("54321"));
    assert_eq!(entry["status_display_type"].as_u64(), Some(2));
    assert_eq!(entry["sync_id"].as_str(), Some("sync-1"));
    assert_eq!(entry["flags"].as_u64(), Some(17));
    assert_eq!(
        entry["timestamps"]["start"].as_i64(),
        Some(1_700_000_000_000)
    );
    assert_eq!(entry["timestamps"]["end"].as_i64(), Some(1_700_000_100_000));
    assert_eq!(entry["assets"]["large_image"].as_str(), Some("cover"));
    assert_eq!(
        entry["assets"]["large_text"].as_str(),
        Some("On the main menu")
    );
    assert_eq!(
        entry["assets"]["large_url"].as_str(),
        Some("https://example.com/large")
    );
    assert_eq!(entry["assets"]["small_image"].as_str(), Some("small"));
    assert_eq!(entry["assets"]["small_text"].as_str(), Some("Small"));
    assert_eq!(
        entry["assets"]["small_url"].as_str(),
        Some("https://example.com/small")
    );
    assert_eq!(
        entry["assets"]["invite_cover_image"].as_str(),
        Some("invite")
    );
    assert_eq!(entry["assets"]["future_asset"], json!(true));
    assert_eq!(entry["party"]["id"].as_str(), Some("party-1"));
    assert_eq!(entry["party"]["size"], json!([2, 5]));
    assert_eq!(entry["party"]["privacy"].as_u64(), Some(1));
    assert_eq!(entry["party"]["future_party"], json!("kept"));
    assert_eq!(entry["secrets"]["join"].as_str(), Some("join-secret"));
    assert_eq!(
        entry["secrets"]["spectate"].as_str(),
        Some("spectate-secret")
    );
    assert_eq!(entry["secrets"]["future_secret"], json!(7));
    assert_eq!(entry["buttons"], json!(["Join"]));
    assert_eq!(
        entry["metadata"]["button_urls"],
        json!(["https://example.com/join"])
    );
    assert_eq!(entry["metadata"]["artist_ids"], json!(["artist-1"]));
    assert_eq!(entry["future_activity"], json!({ "value": 1 }));
}

#[test]
fn presence_update_payload_preserves_unknown_activity_type() {
    let activity = ActivityInfo::test(ActivityKind::Unknown(99), "Future activity");
    let payload: serde_json::Value = serde_json::from_str(&presence_update_payload(
        PresenceStatus::Online,
        &[activity],
    ))
    .expect("presence payload should be valid json");

    assert_eq!(payload["d"]["activities"][0]["type"].as_u64(), Some(99));
}

#[test]
fn gateway_close_codes_choose_the_documented_recovery() {
    for code in [4004, 4010, 4011, 4012, 4013, 4014, 4015, 4016] {
        assert_eq!(close_code_outcome(code), ConnectionOutcome::Fatal, "{code}");
    }
    assert_eq!(close_code_outcome(4003), ConnectionOutcome::Reidentify);
    assert_eq!(close_code_outcome(4007), ConnectionOutcome::Reidentify);
    assert_eq!(close_code_outcome(4009), ConnectionOutcome::Reidentify);
    assert_eq!(close_code_outcome(4000), ConnectionOutcome::Resume);
}

#[test]
fn resume_payload_uses_saved_session_id_and_seq() {
    let payload: serde_json::Value =
        serde_json::from_str(&build_resume_payload("dummy-token", "sess-123", 42))
            .expect("resume payload should be valid json");
    assert_eq!(payload["op"].as_u64(), Some(6));
    assert_eq!(payload["d"]["session_id"].as_str(), Some("sess-123"));
    assert_eq!(payload["d"]["seq"].as_u64(), Some(42));
}

#[test]
fn heartbeat_ack_state_detects_missing_ack_before_next_heartbeat() {
    let mut state = HeartbeatAckState::default();

    assert!(state.mark_heartbeat_sent());
    assert!(!state.mark_heartbeat_sent());
    state.mark_ack_received();
    assert!(state.mark_heartbeat_sent());
}

#[test]
fn guild_member_search_payload_matches_the_user_gateway_shape() {
    let search_payload: serde_json::Value = serde_json::from_str(&search_guild_members_payload(
        Id::<GuildMarker>::new(10),
        "alic",
        10,
        false,
        "member-search-10-alic",
    ))
    .expect("payload should be valid json");

    assert_eq!(
        search_payload,
        json!({
            "op": 8,
            "d": {
                "guild_id": ["10"],
                "query": "alic",
                "limit": 10,
                "presences": false,
                "nonce": "member-search-10-alic"
            }
        })
    );
}

#[test]
fn request_guild_members_by_ids_payload_matches_web_shape() {
    let payload: serde_json::Value = serde_json::from_str(&request_guild_members_by_ids_payload(
        Id::<GuildMarker>::new(10),
        &[Id::<UserMarker>::new(20), Id::<UserMarker>::new(30)],
        false,
        "member-request",
    ))
    .expect("payload should be valid json");

    assert_eq!(
        payload,
        json!({
            "op": 8,
            "d": {
                "guild_id": ["10"],
                "user_ids": ["20", "30"],
                "presences": false,
                "nonce": "member-request"
            }
        })
    );
}

#[test]
fn direct_message_subscribe_payload_matches_expected_shape() {
    let payload: serde_json::Value =
        serde_json::from_str(&direct_message_subscribe_payload(Id::<ChannelMarker>::new(
            20,
        )))
        .expect("payload should be valid json");

    assert_eq!(
        payload,
        json!({
            "op": 13,
            "d": {
                "channel_id": "20"
            }
        })
    );
}

#[test]
fn guild_channel_subscribe_payload_matches_shape_and_member_ranges() {
    for (ranges, expected_ranges) in [
        (&[(0, 99)][..], json!([[0, 99]])),
        (
            &[(0, 99), (100, 199), (200, 299)][..],
            json!([[0, 99], [100, 199], [200, 299]]),
        ),
    ] {
        let payload: serde_json::Value = serde_json::from_str(&guild_channel_subscribe_payload(
            Id::<GuildMarker>::new(10),
            Id::<ChannelMarker>::new(20),
            ranges,
            None,
        ))
        .expect("payload should be valid json");

        assert_eq!(payload["op"].as_u64(), Some(37));
        assert_eq!(payload["d"]["subscriptions"]["10"]["typing"], json!(true));
        assert_eq!(
            payload["d"]["subscriptions"]["10"]["activities"],
            json!(true)
        );
        assert_eq!(payload["d"]["subscriptions"]["10"]["threads"], json!(true));
        assert_eq!(
            payload["d"]["subscriptions"]["10"]["member_updates"],
            json!(true)
        );
        assert_eq!(payload["d"]["subscriptions"]["10"]["members"], json!([]));
        assert!(
            payload["d"]["subscriptions"]["10"]
                .get("thread_member_lists")
                .is_none()
        );
        assert_eq!(
            payload["d"]["subscriptions"]["10"]["channels"]["20"],
            expected_ranges
        );
        if ranges == &[(0, 99)][..] {
            assert_eq!(
                payload,
                json!({
                    "op": 37,
                    "d": {
                        "subscriptions": {
                            "10": {
                                "typing": true,
                                "activities": true,
                                "threads": true,
                                "member_updates": true,
                                "members": [],
                                "channels": {
                                    "20": [[0, 99]]
                                }
                            }
                        }
                    }
                })
            );
        }
    }
}

#[test]
fn guild_channel_subscribe_payload_requests_the_selected_thread_member_list() {
    let thread_ids = [Id::<ChannelMarker>::new(30)];
    let payload: serde_json::Value = serde_json::from_str(&guild_channel_subscribe_payload(
        Id::<GuildMarker>::new(10),
        Id::<ChannelMarker>::new(20),
        &[(0, 99)],
        Some(&thread_ids),
    ))
    .expect("payload should be valid json");

    assert_eq!(
        payload["d"]["subscriptions"]["10"]["thread_member_lists"],
        json!(["30"])
    );

    let cleared: serde_json::Value = serde_json::from_str(&guild_channel_subscribe_payload(
        Id::<GuildMarker>::new(10),
        Id::<ChannelMarker>::new(20),
        &[(0, 99)],
        Some(&[]),
    ))
    .expect("payload should be valid json");
    assert_eq!(
        cleared["d"]["subscriptions"]["10"]["thread_member_lists"],
        json!([]),
        "an explicit empty list unsubscribes the previously selected thread"
    );
}

#[test]
fn guild_subscription_sends_only_the_requested_thread_enabled_payload() {
    let (urgent_tx, _urgent_rx) = tokio::sync::mpsc::unbounded_channel();
    let (normal_tx, mut normal_rx) = tokio::sync::mpsc::unbounded_channel();
    let sender = GatewaySender {
        urgent_tx,
        normal_tx,
    };
    let mut deduper = SubscriptionDeduper::default();
    let mut resources = GatewaySessionResources::default();
    let guild_id = Id::new(10);
    let channel_id = Id::new(20);

    dispatch_command(
        &sender,
        GatewayCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        },
        &mut deduper,
        &mut resources,
    )
    .expect("initial guild subscription should enter the gateway queue");

    let subscription: serde_json::Value = serde_json::from_str(
        &normal_rx
            .try_recv()
            .expect("initial subscription should then enable thread sync")
            .payload,
    )
    .expect("guild subscription payload should be valid json");

    assert_eq!(
        subscription["d"]["subscriptions"]["10"]["threads"],
        json!(true)
    );
    assert!(
        normal_rx.try_recv().is_err(),
        "a guild subscription must not inject a synthetic unsubscribe"
    );

    dispatch_command(
        &sender,
        GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id: None,
            ranges: vec![(0, 99)],
        },
        &mut deduper,
        &mut resources,
    )
    .expect("later range subscription should enter the gateway queue");

    let refresh: serde_json::Value = serde_json::from_str(
        &normal_rx
            .try_recv()
            .expect("later range subscription should send one payload")
            .payload,
    )
    .expect("range subscription payload should be valid json");
    assert_eq!(refresh["d"]["subscriptions"]["10"]["threads"], json!(true));
    assert!(
        normal_rx.try_recv().is_err(),
        "the range update should emit exactly one subscription payload"
    );
}

#[test]
fn subscription_deduper_allows_guild_range_refreshes() {
    let guild_id = Id::<GuildMarker>::new(10);
    let channel_id = Id::<ChannelMarker>::new(20);
    let other_channel_id = Id::<ChannelMarker>::new(30);
    let mut deduper = SubscriptionDeduper::default();

    assert!(deduper.should_send(&GatewayCommand::SubscribeDirectMessage { channel_id }));
    assert!(!deduper.should_send(&GatewayCommand::SubscribeDirectMessage { channel_id }));
    assert!(
        deduper.should_send(&GatewayCommand::SubscribeDirectMessage {
            channel_id: other_channel_id,
        })
    );

    assert!(deduper.should_send(&GatewayCommand::SubscribeGuildChannel {
        guild_id,
        channel_id,
    }));
    assert!(deduper.should_send(&GatewayCommand::SubscribeGuildChannel {
        guild_id,
        channel_id,
    }));

    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id: None,
            ranges: vec![(0, 99), (100, 199)],
        })
    );
    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id: None,
            ranges: vec![(0, 99), (100, 199)],
        })
    );
    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id: None,
            ranges: vec![(0, 99)],
        })
    );
    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            thread_id: None,
            ranges: vec![(0, 99)],
        })
    );
    assert!(
        deduper.should_send(&GatewayCommand::RequestGuildMembersByIds {
            guild_id,
            user_ids: vec![Id::new(40)],
            presences: false,
        })
    );
}

#[test]
fn voice_state_update_payload_joins_and_leaves_voice_channel() {
    let join_payload: serde_json::Value = serde_json::from_str(&voice_state_update_payload(
        Some(Id::<GuildMarker>::new(10)),
        Some(Id::<ChannelMarker>::new(20)),
        true,
        false,
    ))
    .expect("voice join payload should be valid json");
    assert_eq!(join_payload["op"].as_u64(), Some(4));
    assert_eq!(join_payload["d"]["guild_id"].as_str(), Some("10"));
    assert_eq!(join_payload["d"]["channel_id"].as_str(), Some("20"));
    assert_eq!(join_payload["d"]["self_mute"].as_bool(), Some(true));
    assert_eq!(join_payload["d"]["self_deaf"].as_bool(), Some(false));

    let leave_payload: serde_json::Value = serde_json::from_str(&voice_state_update_payload(
        Some(Id::<GuildMarker>::new(10)),
        None,
        true,
        false,
    ))
    .expect("voice leave payload should be valid json");
    assert!(leave_payload["d"]["channel_id"].is_null());

    // A DM or group-DM call joins with a null guild and the DM channel as
    // the voice target.
    let dm_call_payload: serde_json::Value = serde_json::from_str(&voice_state_update_payload(
        None,
        Some(Id::<ChannelMarker>::new(30)),
        false,
        false,
    ))
    .expect("dm call payload should be valid json");
    assert!(dm_call_payload["d"]["guild_id"].is_null());
    assert_eq!(dm_call_payload["d"]["channel_id"].as_str(), Some("30"));
}

#[test]
fn stream_watch_payloads_use_the_documented_gateway_opcodes() {
    let stream_key = "guild:10:20:30";
    let watch: serde_json::Value = serde_json::from_str(&watch_stream_payload(stream_key))
        .expect("watch stream payload should be valid json");
    assert_eq!(watch, json!({"op": 20, "d": {"stream_key": stream_key}}));

    let delete: serde_json::Value = serde_json::from_str(&delete_stream_payload(stream_key))
        .expect("delete stream payload should be valid json");
    assert_eq!(delete, json!({"op": 19, "d": {"stream_key": stream_key}}));
}

#[test]
fn stream_create_payload_uses_guild_and_private_call_shapes() {
    let guild: serde_json::Value = serde_json::from_str(&create_stream_payload(
        VoiceScope::Guild(Id::new(10)),
        Id::new(20),
    ))
    .expect("guild stream payload should be valid json");
    assert_eq!(
        guild,
        json!({
            "op": 18,
            "d": {
                "type": "guild",
                "guild_id": "10",
                "channel_id": "20",
                "preferred_region": null,
            }
        })
    );

    let call: serde_json::Value = serde_json::from_str(&create_stream_payload(
        VoiceScope::Private(Id::new(20)),
        Id::new(20),
    ))
    .expect("call stream payload should be valid json");
    assert_eq!(
        call,
        json!({
            "op": 18,
            "d": {
                "type": "call",
                "guild_id": null,
                "channel_id": "20",
                "preferred_region": null,
            }
        })
    );
}
