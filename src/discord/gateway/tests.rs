use super::{
    ConnectionOutcome, GATEWAY_WEBSOCKET_LIMIT, GatewayCommand, HeartbeatAckState, SessionState,
    SubscriptionDeduper, USER_ACCOUNT_CAPABILITIES, build_identify_payload, build_resume_payload,
    close_code_outcome, direct_message_subscribe_payload, gateway_request,
    gateway_websocket_config, guild_channel_subscribe_payload, presence_update_payload,
    request_guild_members_by_ids_payload, request_guild_members_payload,
    voice_state_update_payload,
};
use crate::discord::fingerprint::{
    CLIENT_BROWSER, CLIENT_BROWSER_VERSION, CLIENT_BUILD_NUMBER, ClientFingerprint, accept_language,
};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
};
use crate::discord::{ActivityEmoji, ActivityInfo, ActivityKind, PresenceStatus};
use serde_json::json;
use tokio_tungstenite::tungstenite::http::header::{
    ACCEPT_LANGUAGE, CACHE_CONTROL, ORIGIN, PRAGMA, USER_AGENT,
};

#[test]
fn gateway_websocket_config_allows_large_ready_payloads() {
    let config = gateway_websocket_config();

    assert_eq!(config.max_message_size, Some(GATEWAY_WEBSOCKET_LIMIT));
    assert_eq!(config.max_frame_size, Some(GATEWAY_WEBSOCKET_LIMIT));
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
fn identify_payload_carries_user_account_capabilities() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let payload: serde_json::Value =
        serde_json::from_str(&build_identify_payload("dummy-token", &fingerprint))
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
    assert_eq!(payload["d"]["compress"].as_bool(), Some(false));
    assert_eq!(payload["d"]["presence"]["status"].as_str(), Some("online"));
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
        timestamps: Some(crate::discord::ActivityTimestamps {
            start: Some(1_700_000_000_000),
            end: None,
        }),
        assets: Some(crate::discord::ActivityAssets {
            large_image: Some("cover".to_owned()),
            large_text: Some("On the main menu".to_owned()),
            small_image: None,
            small_text: None,
        }),
        party: Some(crate::discord::ActivityParty {
            id: Some("party-1".to_owned()),
            size: Some((2, 5)),
        }),
        buttons: vec![crate::discord::ActivityButton {
            label: "Join".to_owned(),
            url: "https://example.com/join".to_owned(),
        }],
        ..ActivityInfo::playing("Concord")
    };
    let payload: serde_json::Value = serde_json::from_str(&presence_update_payload(
        PresenceStatus::Online,
        &[activity],
    ))
    .expect("presence payload should be valid json");
    let entry = &payload["d"]["activities"][0];

    assert_eq!(
        entry["timestamps"]["start"].as_i64(),
        Some(1_700_000_000_000)
    );
    assert!(entry["timestamps"].get("end").is_none());
    assert_eq!(entry["assets"]["large_image"].as_str(), Some("cover"));
    assert_eq!(
        entry["assets"]["large_text"].as_str(),
        Some("On the main menu")
    );
    assert!(entry["assets"].get("small_image").is_none());
    assert_eq!(entry["party"]["id"].as_str(), Some("party-1"));
    assert_eq!(entry["party"]["size"], json!([2, 5]));
    assert_eq!(entry["buttons"], json!(["Join"]));
    assert_eq!(
        entry["metadata"]["button_urls"],
        json!(["https://example.com/join"])
    );
}

#[test]
fn fatal_gateway_close_codes_do_not_retry_identify() {
    for code in [4004, 4010, 4011, 4012, 4013, 4014] {
        assert_eq!(close_code_outcome(code), ConnectionOutcome::Fatal, "{code}");
    }
    assert_eq!(close_code_outcome(4007), ConnectionOutcome::Reidentify);
    assert_eq!(close_code_outcome(4009), ConnectionOutcome::Reidentify);
    assert_eq!(close_code_outcome(4000), ConnectionOutcome::Resume);
}

#[test]
fn resume_payload_uses_saved_session_id_and_seq() {
    let session = SessionState {
        session_id: Some("sess-123".to_owned()),
        last_sequence: Some(42),
        ..SessionState::default()
    };
    let payload: serde_json::Value =
        serde_json::from_str(&build_resume_payload("dummy-token", &session))
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
fn request_guild_members_payload_supports_full_load_and_search_shapes() {
    let search_payload: serde_json::Value = serde_json::from_str(&request_guild_members_payload(
        Id::<GuildMarker>::new(10),
        "alic",
        10,
        false,
        Some("mention-ac-10-alic"),
    ))
    .expect("payload should be valid json");

    assert_eq!(
        search_payload,
        json!({
            "op": 8,
            "d": {
                "guild_id": "10",
                "query": "alic",
                "limit": 10,
                "presences": false,
                "nonce": "mention-ac-10-alic"
            }
        })
    );

    let full_load_payload: serde_json::Value = serde_json::from_str(
        &request_guild_members_payload(Id::<GuildMarker>::new(10), "", 0, true, None),
    )
    .expect("payload should be valid json");

    assert_eq!(full_load_payload["op"].as_u64(), Some(8));
    assert_eq!(full_load_payload["d"]["guild_id"].as_str(), Some("10"));
    assert_eq!(full_load_payload["d"]["query"].as_str(), Some(""));
    assert_eq!(full_load_payload["d"]["limit"].as_u64(), Some(0));
    assert_eq!(full_load_payload["d"]["presences"].as_bool(), Some(true));
    assert!(full_load_payload["d"].get("nonce").is_none());
}

#[test]
fn request_guild_members_by_ids_payload_matches_web_shape() {
    let payload: serde_json::Value = serde_json::from_str(&request_guild_members_by_ids_payload(
        Id::<GuildMarker>::new(10),
        &[Id::<UserMarker>::new(20), Id::<UserMarker>::new(30)],
        false,
    ))
    .expect("payload should be valid json");

    assert_eq!(
        payload,
        json!({
            "op": 8,
            "d": {
                "guild_id": "10",
                "user_ids": ["20", "30"],
                "presences": false
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
fn subscription_deduper_skips_exact_duplicate_channel_subscriptions() {
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
    assert!(
        !deduper.should_send(&GatewayCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        })
    );

    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            ranges: vec![(0, 99), (100, 199)],
        })
    );
    assert!(
        !deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            ranges: vec![(0, 99), (100, 199)],
        })
    );
    assert!(
        deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
            ranges: vec![(0, 99)],
        })
    );
    assert!(
        !deduper.should_send(&GatewayCommand::UpdateMemberListSubscription {
            guild_id,
            channel_id,
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
