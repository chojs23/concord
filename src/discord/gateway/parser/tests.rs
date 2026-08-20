use crate::discord::ids::Id;
use serde_json::{Value, json};

use super::{
    parse_channel_info, parse_guild_create, parse_guild_emojis_update, parse_guild_update,
    parse_message_create, parse_message_info, parse_message_update, parse_user_account_dispatch,
    parse_user_account_event,
};
use crate::discord::{
    ActivityKind, AppEvent, AttachmentUpdate, ChannelVisibilityStats, DiscordState, FriendStatus,
    GuildMemberListItem, GuildMemberListOperation, GuildOnboardingMode, GuildVerificationLevel,
    MentionInfo, MessageKind, NotificationLevel, PollAnswerInfo, PollInfo, PremiumTier,
    PresenceStatus, ReactionEmoji, ReplyInfo, StickerInfo,
};

#[test]
fn guild_parsers_preserve_feature_names_from_lazy_properties() {
    let create = parse_guild_create(&json!({
        "id": "10",
        "properties": {
            "name": "guild",
            "features": ["COMMUNITY", "FUTURE_FEATURE"]
        },
        "channels": [],
        "members": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate { features, .. } = create else {
        panic!("expected guild create event");
    };
    assert_eq!(
        features,
        Some(vec!["COMMUNITY".to_owned(), "FUTURE_FEATURE".to_owned()])
    );

    let update = parse_guild_update(&json!({
        "id": "10",
        "properties": {
            "name": "guild",
            "features": ["MEMBER_VERIFICATION_GATE_ENABLED"]
        }
    }))
    .expect("guild update should parse");

    let AppEvent::GuildUpdate { features, .. } = update else {
        panic!("expected guild update event");
    };
    assert_eq!(
        features,
        Some(vec!["MEMBER_VERIFICATION_GATE_ENABLED".to_owned()])
    );
}

#[test]
fn guild_create_parser_preserves_complete_onboarding_payload() {
    let raw_onboarding = json!({
        "guild_id": "10",
        "enabled": false,
        "mode": 1,
        "default_channel_ids": ["30", "40"],
        "prompts": [{
            "id": "50",
            "title": "Choose topics",
            "future_prompt_field": { "kept": true }
        }],
        "future_top_level_field": [1, 2, 3]
    });
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "guild_onboarding": raw_onboarding,
        "channels": [],
        "members": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate {
        onboarding: Some(onboarding),
        ..
    } = event
    else {
        panic!("expected guild onboarding");
    };
    assert_eq!(onboarding.guild_id, Id::new(10));
    assert_eq!(onboarding.enabled, Some(false));
    assert_eq!(onboarding.mode, Some(GuildOnboardingMode::Advanced));
    assert_eq!(
        onboarding.default_channel_ids,
        vec![Id::new(30), Id::new(40)]
    );
    assert_eq!(onboarding.prompts().len(), 1);
    assert_eq!(onboarding.raw["future_top_level_field"], json!([1, 2, 3]));
    assert_eq!(
        onboarding.raw["prompts"][0]["future_prompt_field"],
        json!({ "kept": true })
    );
}

#[test]
fn guild_create_parser_preserves_initial_presence_activities() {
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "channels": [],
        "members": [],
        "roles": [],
        "emojis": [],
        "presences": [{
            "user": { "id": "20" },
            "status": "online",
            "activities": [{
                "type": 2,
                "name": "Spotify",
                "details": "A song"
            }]
        }]
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate { presences, .. } = event else {
        panic!("expected guild create event");
    };
    assert_eq!(presences.len(), 1);
    assert_eq!(presences[0].user_id, Id::new(20));
    assert_eq!(presences[0].status, PresenceStatus::Online);
    assert_eq!(presences[0].activities.len(), 1);
    assert_eq!(presences[0].activities[0].kind, ActivityKind::Listening);
    assert_eq!(presences[0].activities[0].name, "Spotify");
}

#[test]
fn onboarding_dispatches_preserve_payload() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_ONBOARDING_UPDATE",
            "d": {
                "guild_id": "10",
                "enabled": true,
                "mode": 99,
                "default_channel_ids": [],
                "prompts": [],
                "future_field": "kept"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::GuildOnboardingUpdate { guild_id, onboarding }]
            if *guild_id == Id::new(10)
                && onboarding.enabled == Some(true)
                && onboarding.mode == Some(GuildOnboardingMode::Unknown(99))
                && onboarding.raw["future_field"] == json!("kept")
    ));

    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{
                    "id": "10",
                    "guild_onboarding": {
                        "enabled": true,
                        "mode": 0,
                        "default_channel_ids": [],
                        "prompts": [],
                        "supplemental_field": "kept"
                    }
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildOnboardingUpdate { guild_id, onboarding }
            if *guild_id == Id::new(10)
                && onboarding.enabled == Some(true)
                && onboarding.raw["supplemental_field"] == json!("kept")
    )));
}

#[test]
fn guild_parser_keeps_message_verification_inputs() {
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "verification_level": 3,
        "mfa_level": 1,
        "channels": [],
        "roles": [],
        "emojis": [],
        "members": [{
            "user": { "id": "20", "username": "neo" },
            "roles": [],
            "joined_at": "2026-07-14T23:51:00+00:00",
            "flags": 4,
            "pending": true,
            "communication_disabled_until": "2026-07-15T01:00:00+00:00"
        }]
    }))
    .expect("guild should parse");

    let AppEvent::GuildCreate {
        verification_level,
        mfa_level,
        members,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(verification_level, Some(GuildVerificationLevel::High));
    assert_eq!(mfa_level, Some(1));
    assert_eq!(members[0].flags, Some(4));
    assert_eq!(members[0].pending, Some(true));
    assert!(members[0].communication_disabled_until_present);
    assert_eq!(
        members[0]
            .communication_disabled_until
            .expect("communication_disabled_until should parse")
            .to_rfc3339(),
        "2026-07-15T01:00:00+00:00"
    );
    assert_eq!(
        members[0]
            .joined_at
            .expect("joined_at should parse")
            .to_rfc3339(),
        "2026-07-14T23:51:00+00:00"
    );
}

#[test]
fn guild_parser_preserves_missing_authorization_inputs() {
    let event = parse_guild_create(&json!({
        "id": "10",
        "name": "guild",
        "channels": [],
        "members": [],
        "emojis": []
    }))
    .expect("partial lazy guild should parse");

    let AppEvent::GuildCreate {
        verification_level,
        mfa_level,
        features,
        roles,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(verification_level, None);
    assert_eq!(mfa_level, None);
    assert_eq!(features, None);
    assert_eq!(roles, None);
}

#[test]
fn current_user_verification_status_is_loaded_and_refreshed() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "20",
                    "username": "neo",
                    "verified": true,
                    "phone": "+10000000000",
                    "mfa_enabled": true
                },
                "guilds": []
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserVerification {
            email_verified: Some(true),
            phone_verified: Some(true),
            mfa_enabled: Some(true),
        }
    )));

    let events = parse_user_account_event(
        &json!({
            "t": "USER_UPDATE",
            "d": {
                "id": "20",
                "username": "neo",
                "verified": true,
                "phone": null
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserVerification {
            email_verified: Some(true),
            phone_verified: Some(false),
            mfa_enabled: _,
        }
    )));
}

#[test]
fn raw_dispatch_parser_keeps_original_payload_for_future_fields() {
    let parsed = parse_user_account_dispatch(json!({
        "t": "MESSAGE_CREATE",
        "d": {
            "id": "101",
            "channel_id": "20",
            "author": { "id": "30", "username": "neo" },
            "type": 0,
            "pinned": false,
            "content": "hello",
            "mentions": [],
            "attachments": [],
            "embeds": [],
            "future_discord_field": { "value": true }
        }
    }))
    .expect("dispatch should parse");

    assert_eq!(parsed.dispatch.event_type, "MESSAGE_CREATE");
    assert_eq!(
        parsed.dispatch.payload["future_discord_field"]["value"],
        true
    );
    assert!(matches!(
        parsed.events.as_slice(),
        [AppEvent::MessageCreate { .. }]
    ));
}

#[test]
fn raw_member_list_update_preserves_operations_and_member_data() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "10",
                "groups": [{ "id": "admin", "count": 4 }],
                "ops": [
                    {
                        "op": "SYNC",
                        "range": [0, 99],
                        "items": [
                            { "group": { "id": "admin" } },
                            {
                                "member": {
                                    "user": {
                                        "id": "20",
                                        "username": "alice",
                                        "global_name": "Alice",
                                        "avatar": "global_hash"
                                    },
                                    "avatar": "guild_hash",
                                    "nick": "Alice Nick",
                                    "roles": ["30"]
                                },
                                "presence": { "status": "idle" }
                            }
                        ]
                    },
                    {
                        "op": "SYNC",
                        "range": [100, 199],
                        "items": [{
                            "member": {
                                "user": { "id": "21", "username": "bob" },
                                "roles": []
                            },
                            "presence": { "status": "idle" }
                        }]
                    },
                    {
                        "op": "INSERT",
                        "index": 200,
                        "item": {
                            "member": {
                                "user": { "id": "22", "username": "carol" },
                                "roles": []
                            },
                            "presence": { "status": "online" }
                        }
                    },
                    {
                        "op": "UPDATE",
                        "index": 201,
                        "item": {
                            "member": {
                                "user": { "id": "23", "username": "dave" },
                                "roles": []
                            },
                            "presence": { "status": "dnd" }
                        }
                    },
                    { "op": "DELETE", "index": 12 },
                    { "op": "INVALIDATE", "range": [200, 299] },
                    { "op": "FUTURE_OPERATION", "index": 4 }
                ]
            }
        })
        .to_string(),
    );

    let [AppEvent::GuildMemberListUpdate { update }] = events.as_slice() else {
        panic!("expected one GuildMemberListUpdate");
    };
    assert_eq!(update.guild_id, Id::new(10));
    assert_eq!(update.ops.len(), 7);

    let GuildMemberListOperation::Sync { range, items } = &update.ops[0] else {
        panic!("expected first sync operation");
    };
    assert_eq!(*range, (0, 99));
    assert!(matches!(
        &items[0],
        GuildMemberListItem::Group { id, count } if id == "admin" && *count == 4
    ));
    let GuildMemberListItem::Member { member, presence } = &items[1] else {
        panic!("expected member list item");
    };
    assert_eq!(member.user_id, Id::new(20));
    assert_eq!(member.display_name, "Alice Nick");
    assert_eq!(member.nickname.as_deref(), Some("Alice Nick"));
    assert!(member.nickname_present);
    assert_eq!(
        member.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/guilds/10/users/20/avatars/guild_hash.png")
    );
    assert_eq!(member.role_ids, vec![Id::new(30)]);
    assert_eq!(
        presence.as_ref().map(|value| (value.user_id, value.status)),
        Some((Id::new(20), PresenceStatus::Idle))
    );

    assert!(matches!(
        &update.ops[1],
        GuildMemberListOperation::Sync { range: (100, 199), items }
            if matches!(
                &items[0],
                GuildMemberListItem::Member { member, presence: Some(presence) }
                    if member.user_id == Id::new(21)
                        && presence.status == PresenceStatus::Idle
            )
    ));
    assert!(matches!(
        &update.ops[2],
        GuildMemberListOperation::Insert {
            index: 200,
            item: GuildMemberListItem::Member { presence: Some(presence), .. }
        } if presence.user_id == Id::new(22)
            && presence.status == PresenceStatus::Online
    ));
    assert!(matches!(
        &update.ops[3],
        GuildMemberListOperation::Update {
            index: 201,
            item: GuildMemberListItem::Member { presence: Some(presence), .. }
        } if presence.user_id == Id::new(23)
            && presence.status == PresenceStatus::DoNotDisturb
    ));
    assert!(matches!(
        &update.ops[4..],
        [
            GuildMemberListOperation::Delete { index: 12 },
            GuildMemberListOperation::Invalidate { range: (200, 299) },
            GuildMemberListOperation::Unknown { name: Some(name), raw }
        ] if name == "FUTURE_OPERATION" && raw["index"] == json!(4)
    ));
}

#[test]
fn raw_voice_state_update_extracts_channel_and_member() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": "10",
                "channel_id": "30",
                "user_id": "20",
                "deaf": false,
                "mute": true,
                "self_deaf": false,
                "self_mute": true,
                "self_stream": true,
                "session_id": "voice-session-1",
                "member": {
                    "user": {
                        "id": "20",
                        "username": "alice",
                        "global_name": "Alice"
                    },
                    "nick": "Alice Nick",
                    "roles": ["40"]
                }
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
                && state.mute
                && state.self_mute
                && state.self_stream
                && state.session_id.as_deref() == Some("voice-session-1")
                && state.member.as_ref().is_some_and(|member|
                    member.display_name == "Alice Nick" && member.role_ids == vec![Id::new(40)]
                )
    )));
}

#[test]
fn dm_call_voice_states_parse_without_a_guild() {
    use crate::discord::VoiceScope;

    // A DM/group-DM voice state arrives with a null guild and the DM channel id.
    let dm_state = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": null,
                "channel_id": "30",
                "user_id": "20",
                "session_id": "dm-voice-session"
            }
        })
        .to_string(),
    );
    assert!(dm_state.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id.is_none()
                && state.channel_id == Some(Id::new(30))
                && state.scope() == Some(VoiceScope::Private(Id::new(30)))
    )));

    // CALL_CREATE describes an in-progress DM call and seeds its participants.
    let call = parse_user_account_event(
        &json!({
            "t": "CALL_CREATE",
            "d": {
                "channel_id": "30",
                "voice_states": [
                    { "user_id": "20", "channel_id": "30" },
                    { "user_id": "21" }
                ]
            }
        })
        .to_string(),
    );
    let call_users: Vec<_> = call
        .iter()
        .filter_map(|event| match event {
            AppEvent::VoiceStateUpdate { state } => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(call_users.len(), 2);
    // A participant whose state omits its channel inherits the call's channel.
    assert!(
        call_users
            .iter()
            .all(|state| state.channel_id == Some(Id::new(30)) && state.guild_id.is_none())
    );

    // CALL_DELETE ends the call and clears its channel.
    let deleted = parse_user_account_event(
        &json!({ "t": "CALL_DELETE", "d": { "channel_id": "30" } }).to_string(),
    );
    assert!(deleted.iter().any(|event| matches!(
        event,
        AppEvent::CallDelete { channel_id } if *channel_id == Id::new(30)
    )));
}

#[test]
fn raw_voice_server_update_extracts_endpoint_without_exposing_token_in_debug() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_SERVER_UPDATE",
            "d": {
                "guild_id": "10",
                "endpoint": "voice.example.com",
                "token": "secret-voice-token"
            }
        })
        .to_string(),
    );

    let server = events
        .iter()
        .find_map(|event| match event {
            AppEvent::VoiceServerUpdate { server } => Some(server),
            _ => None,
        })
        .expect("voice server update should parse");

    assert_eq!(server.guild_id, Some(Id::new(10)));
    assert_eq!(server.endpoint.as_deref(), Some("voice.example.com"));
    assert_eq!(server.token, "secret-voice-token");
    assert!(!format!("{server:?}").contains("secret-voice-token"));
}

#[test]
fn raw_stream_events_supply_the_separate_rtc_connection() {
    let created = parse_user_account_event(
        &json!({
            "t": "STREAM_CREATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "rtc_server_id": "400",
                "rtc_channel_id": "401",
                "viewer_ids": ["50", "60"],
                "paused": true
            }
        })
        .to_string(),
    );
    assert!(created.iter().any(|event| matches!(
        event,
        AppEvent::StreamCreate { stream }
            if stream.stream_key == "guild:10:20:30"
                && stream.rtc_server_id == "400"
                && stream.rtc_channel_id == Id::new(401)
                && stream.viewer_ids == vec![Id::new(50), Id::new(60)]
                && stream.paused
    )));

    let updated = parse_user_account_event(
        &json!({
            "t": "STREAM_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "viewer_ids": ["50", "70"],
                "paused": false
            }
        })
        .to_string(),
    );
    assert!(updated.iter().any(|event| matches!(
        event,
        AppEvent::StreamUpdate { stream }
            if stream.stream_key == "guild:10:20:30"
                && stream.viewer_ids == vec![Id::new(50), Id::new(70)]
                && !stream.paused
    )));

    let server = parse_user_account_event(
        &json!({
            "t": "STREAM_SERVER_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "endpoint": "stream.example.com",
                "token": "secret-stream-token"
            }
        })
        .to_string(),
    );
    let server = server
        .iter()
        .find_map(|event| match event {
            AppEvent::StreamServerUpdate { server } => Some(server),
            _ => None,
        })
        .expect("stream server update should parse");
    assert_eq!(server.endpoint.as_deref(), Some("stream.example.com"));
    assert!(!format!("{server:?}").contains("secret-stream-token"));

    let deleted = parse_user_account_event(
        &json!({
            "t": "STREAM_DELETE",
            "d": {
                "stream_key": "guild:10:20:30",
                "reason": "stream_ended",
                "unavailable": true
            }
        })
        .to_string(),
    );
    assert!(deleted.iter().any(|event| matches!(
        event,
        AppEvent::StreamDelete { stream }
            if stream.reason == "stream_ended" && stream.unavailable
    )));
}

#[test]
fn stream_update_without_viewer_ids_is_ignored() {
    let events = parse_user_account_event(
        &json!({
            "t": "STREAM_UPDATE",
            "d": {
                "stream_key": "guild:10:20:30",
                "paused": false
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn raw_voice_state_update_extracts_leave_payload() {
    let events = parse_user_account_event(
        &json!({
            "t": "VOICE_STATE_UPDATE",
            "d": {
                "guild_id": "10",
                "channel_id": null,
                "user_id": "20"
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id.is_none()
                && state.user_id == Id::new(20)
    )));
}

#[test]
fn raw_guild_create_emits_initial_voice_states() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_CREATE",
            "d": {
                "id": "10",
                "name": "guild",
                "channels": [],
                "voice_states": [{
                    "channel_id": "30",
                    "user_id": "20",
                    "self_stream": true
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildCreate { guild_id, .. } if *guild_id == Id::new(10)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
                && state.self_stream
    )));
}

#[test]
fn raw_ready_parser_emits_initial_voice_states_from_embedded_guilds() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "me" },
                "guilds": [{
                    "id": "10",
                    "name": "guild",
                    "channels": [],
                    "voice_states": [{
                        "channel_id": "30",
                        "user_id": "20"
                    }]
                }]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildCreate { guild_id, .. } if *guild_id == Id::new(10)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
    )));
}

#[test]
fn relationship_payloads_emit_upserts_and_authoritative_empty_lists() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_ADD",
            "d": {
                "id": "20",
                "type": 1,
                "nickname": "Bestie",
                "user_ignored": true,
                "user": {
                    "id": "20",
                    "global_name": "Alice Global",
                    "username": "alice"
                }
            }
        })
        .to_string(),
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::RelationshipUpsert { relationship }
            if relationship.user_id == Id::new(20)
                && relationship.status == FriendStatus::Friend
                && relationship.nickname.as_deref() == Some("Bestie")
                && relationship.display_name.as_deref() == Some("Alice Global")
                && relationship.username.as_deref() == Some("alice")
                && relationship.ignored
    ));

    let ready = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "relationships": []
            }
        })
        .to_string(),
    );
    assert!(ready.iter().any(|event| matches!(
        event,
        AppEvent::RelationshipsLoaded { relationships } if relationships.is_empty()
    )));
}

#[test]
fn relationship_update_accepts_a_partial_nickname_patch() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_UPDATE",
            "d": {
                "id": "20",
                "nickname": "New nickname"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::RelationshipUpdate { update }]
            if update.user_id == Id::new(20)
                && update.status.is_none()
                && update.nickname == Some(Some("New nickname".to_owned()))
                && update.display_name.is_none()
                && update.username.is_none()
                && update.ignored.is_none()
    ));
}

#[test]
fn relationship_remove_emits_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_REMOVE",
            "d": {"id": "20", "type": 3}
        })
        .to_string(),
    );
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::RelationshipRemove { user_id, status }
            if *user_id == Id::new(20) && *status == Some(FriendStatus::IncomingRequest)
    ));
}

#[test]
fn channel_parser_keeps_last_message_id() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 1,
            "last_message_id": "99",
            "recipients": [{ "username": "neo" }]
        }),
        None,
    )
    .expect("dm channel should parse");

    assert_eq!(channel.last_message_id.map(|id| id.get()), Some(99));
}

#[test]
fn channel_parser_reads_dm_message_request_and_spam_flags() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 1,
            "is_message_request": true,
            "is_spam": true,
            "recipients": [{ "username": "stranger" }]
        }),
        None,
    )
    .expect("dm channel should parse");

    assert_eq!(channel.is_message_request, Some(true));
    assert_eq!(channel.is_spam, Some(true));
}

#[test]
fn channel_parser_reads_forum_tags_and_media_type() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "type": 16,
            "name": "support",
            "flags": 16,
            "available_tags": [{
                "id": "101",
                "name": "Resolved",
                "moderated": true,
                "emoji_id": "201"
            }]
        }),
        None,
    )
    .expect("media channel should parse");

    assert_eq!(channel.kind, "media");
    assert!(channel.requires_forum_tag());
    assert_eq!(channel.available_tags.len(), 1);
    assert_eq!(channel.available_tags[0].id.get(), 101);
    assert_eq!(channel.available_tags[0].name, "Resolved");
    assert!(channel.available_tags[0].moderated);
    assert_eq!(
        channel.available_tags[0].emoji_id.map(|id| id.get()),
        Some(201)
    );
}

#[test]
fn channel_parser_reads_thread_applied_tags() {
    let channel = parse_channel_info(
        &json!({
            "id": "20",
            "type": 11,
            "name": "post",
            "parent_id": "10",
            "thread_metadata": {
                "archived": false,
                "locked": false
            },
            "applied_tags": ["101", "102"]
        }),
        None,
    )
    .expect("thread should parse");

    assert_eq!(
        channel
            .applied_tags
            .iter()
            .map(|tag_id| tag_id.get())
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
}

#[test]
fn raw_ready_parser_adds_current_user_to_group_dm_recipients() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "sessions": [{ "status": "idle" }],
                "guilds": [],
                "merged_presences": {
                    "friends": [
                        { "user": { "id": "20" }, "status": "online" },
                        { "user": { "id": "30" }, "status": "idle" }
                    ]
                },
                "private_channels": [{
                    "id": "10",
                    "type": 3,
                    "name": "project chat",
                    "recipients": [
                        {
                            "id": "20",
                            "username": "alice",
                            "global_name": "Alice",
                            "bot": false
                        },
                        {
                            "id": "30",
                            "username": "helper-bot",
                            "bot": true
                        }
                    ]
                }]
            }
        })
        .to_string(),
    );

    let channel = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(channel) => Some(channel),
            _ => None,
        })
        .expect("ready should emit a private channel upsert");
    let recipients = channel
        .recipients
        .as_ref()
        .expect("group dm should carry recipients");

    assert_eq!(channel.kind, "group-dm");
    assert_eq!(recipients.len(), 3);
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].display_name, "Alice");
    assert!(!recipients[0].is_bot);
    assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
    assert_eq!(recipients[1].display_name, "helper-bot");
    assert!(recipients[1].is_bot);
    assert_eq!(recipients[1].status, Some(PresenceStatus::Idle));
    assert_eq!(recipients[2].user_id, Id::new(99));
    assert_eq!(recipients[2].display_name, "neo");
    assert_eq!(recipients[2].status, Some(PresenceStatus::Idle));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(99) && presence.status == PresenceStatus::Idle
    )));
}

#[test]
fn ready_uses_the_overall_session_status_and_activities() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "99", "username": "neo" },
                "sessions": [
                    {
                        "session_id": "desktop",
                        "status": "idle",
                        "activities": [{ "type": 0, "name": "Wrong session" }]
                    },
                    {
                        "session_id": "all",
                        "status": "dnd",
                        "activities": [{ "type": 0, "name": "Concord" }]
                    }
                ],
                "guilds": []
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(99)
                && presence.status == PresenceStatus::DoNotDisturb
                && presence.activities.len() == 1
                && presence.activities[0].name == "Concord"
    )));
}

#[test]
fn ready_parsers_emit_authoritative_snapshot_boundaries() {
    let ready = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "99", "username": "neo" },
                "guilds": [{
                    "id": "10",
                    "name": "guild",
                    "channels": [{ "id": "20", "type": 0, "name": "general" }],
                    "threads": [{
                        "id": "21",
                        "type": 11,
                        "name": "thread",
                        "parent_id": "20",
                        "thread_metadata": {
                            "archived": false,
                            "archive_timestamp": "2026-08-10T00:00:00.000000+00:00",
                            "auto_archive_duration": 1440,
                            "locked": false
                        }
                    }]
                }],
                "private_channels": [{ "id": "30", "type": 1 }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        ready.last(),
        Some(AppEvent::ReadySnapshotComplete { snapshot })
            if snapshot.guild_ids.as_deref() == Some(&[Id::new(10)])
                && snapshot.guild_channel_ids.get(&Id::new(10)).map(Vec::as_slice)
                    == Some(&[Id::new(20), Id::new(21)])
                && snapshot.private_channel_ids.as_deref() == Some(&[Id::new(30)])
    ));

    let supplemental = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "lazy_private_channels": [{ "id": "31", "type": 1 }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        supplemental.last(),
        Some(AppEvent::ReadySupplementalComplete { private_channel_ids })
            if private_channel_ids == &[Id::new(31)]
    ));
}

#[test]
fn raw_ready_parser_exposes_current_user_premium_capability() {
    for (premium_type, expected) in [(0, PremiumTier::None), (2, PremiumTier::Nitro)] {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo",
                        "premium_type": premium_type
                    },
                    "guilds": []
                }
            })
            .to_string(),
        );

        assert!(
            events.iter().any(|event| matches!(
                event,
                AppEvent::CurrentUserCapabilities { premium_tier } if *premium_tier == expected
            )),
            "premium_type {premium_type}"
        );
    }
}

#[test]
fn raw_ready_parser_applies_guild_merged_presence_to_dm_recipient() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "guilds": [],
                "merged_presences": {
                    "friends": [],
                    "guilds": [[
                        { "user_id": "20", "status": "idle" }
                    ]]
                },
                "private_channels": [{
                    "id": "10",
                    "type": 1,
                    "recipients": [{
                        "id": "20",
                        "username": "alice"
                    }]
                }]
            }
        })
        .to_string(),
    );

    let channel = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(channel) => Some(channel),
            _ => None,
        })
        .expect("ready should emit a private channel upsert");
    let recipients = channel
        .recipients
        .as_ref()
        .expect("dm should carry recipients");

    assert_eq!(channel.kind, "dm");
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].status, Some(PresenceStatus::Idle));
}

#[test]
fn raw_ready_supplemental_updates_user_presences() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [
                        { "user_id": "20", "status": "online" }
                    ],
                    "guilds": [[
                        { "user_id": "30", "status": "idle" }
                    ]]
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::PresenceUpdate { guild_id: None, presence }
            if presence.user_id == Id::new(30) && presence.status == PresenceStatus::Idle
    )));
}

#[test]
fn raw_presence_update_extracts_activities() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "guild_id": "10",
                "user": { "id": "20" },
                "status": "online",
                "activities": [
                    {
                        "type": 4,
                        "name": "Custom Status",
                        "state": "Coding hard",
                        "emoji": { "name": "🦀" }
                    },
                    {
                        "type": 2,
                        "name": "Spotify",
                        "details": "Bohemian Rhapsody",
                        "state": "Queen"
                    },
                    {
                        "type": 0,
                        "name": "Concord"
                    }
                ]
            }
        })
        .to_string(),
    );

    let (guild_id, activities) = events
        .iter()
        .find_map(|event| match event {
            AppEvent::PresenceUpdate { guild_id, presence } => {
                Some((*guild_id, &presence.activities))
            }
            _ => None,
        })
        .expect("PRESENCE_UPDATE should produce a PresenceUpdate event");

    assert_eq!(guild_id, Some(Id::new(10)));
    assert_eq!(activities.len(), 3);
    assert_eq!(activities[0].kind, ActivityKind::Custom);
    assert_eq!(activities[0].state.as_deref(), Some("Coding hard"));
    assert_eq!(
        activities[0].emoji.as_ref().map(|e| e.name.as_str()),
        Some("🦀")
    );
    assert_eq!(activities[1].kind, ActivityKind::Listening);
    assert_eq!(activities[1].name, "Spotify");
    assert_eq!(activities[1].details.as_deref(), Some("Bohemian Rhapsody"));
    assert_eq!(activities[1].state.as_deref(), Some("Queen"));
    assert_eq!(activities[2].kind, ActivityKind::Playing);
    assert_eq!(activities[2].name, "Concord");
}

#[test]
fn raw_presence_update_without_guild_id_emits_user_event_with_activities() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user": { "id": "20" },
                "status": "dnd",
                "activities": [
                    { "type": 1, "name": "Twitch", "url": "https://twitch.tv/foo" }
                ]
            }
        })
        .to_string(),
    );

    let activities = events
        .iter()
        .find_map(|event| match event {
            AppEvent::PresenceUpdate {
                guild_id: None,
                presence,
            } => Some(&presence.activities),
            _ => None,
        })
        .expect("PRESENCE_UPDATE without guild_id should produce a PresenceUpdate without guild");

    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].kind, ActivityKind::Streaming);
    assert_eq!(activities[0].name, "Twitch");
    assert_eq!(activities[0].url.as_deref(), Some("https://twitch.tv/foo"));
}

#[test]
fn raw_ready_supplemental_aligns_merged_members_by_guild_index() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{ "id": "1" }, { "id": "2" }],
                "merged_members": [[{
                    "user_id": "10",
                    "roles": ["20"]
                }], [{
                    "user_id": "10",
                    "roles": ["30"]
                }]]
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildMemberUpsert { guild_id, member }
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.role_ids == vec![Id::new(20)]
                && member.role_ids_present
                && !member.is_bot_present
                && !member.avatar_url_present
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::GuildMemberUpsert { guild_id, member }
            if *guild_id == Id::new(2)
                && member.user_id == Id::new(10)
                && member.role_ids == vec![Id::new(30)]
    )));
}

#[test]
fn partial_member_fields_only_replace_cached_data_when_their_types_are_valid() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_UPDATE",
            "d": {
                "guild_id": "1",
                "user_id": "10",
                "avatar": null,
                "roles": null,
                "user": { "id": "10", "bot": null }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::GuildMemberUpsert { guild_id, member }]
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.avatar_url.as_deref()
                    == Some("https://cdn.discordapp.com/embed/avatars/0.png")
                && member.avatar_url_present
                && !member.role_ids_present
                && !member.is_bot_present
    ));
}

#[test]
fn raw_ready_supplemental_member_roles_hide_role_denied_channel() {
    let ready_events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "guilds": [{
                    "id": "1",
                    "name": "guild",
                    "owner_id": "11",
                    "channels": [{
                        "id": "2",
                        "type": 0,
                        "name": "staff-hidden",
                        "permission_overwrites": [{
                            "id": "20",
                            "type": 0,
                            "allow": "0",
                            "deny": "1024"
                        }]
                    }],
                    "members": [],
                    "presences": [],
                    "roles": [],
                    "emojis": []
                }],
                "private_channels": []
            }
        })
        .to_string(),
    );
    let supplemental_events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{
                    "id": "1",
                    "roles": [{
                        "id": "1",
                        "name": "@everyone",
                        "permissions": "1024",
                        "position": 0,
                        "hoist": false
                    }, {
                        "id": "20",
                        "name": "Staff",
                        "permissions": "0",
                        "position": 1,
                        "hoist": false
                    }]
                }],
                "merged_members": [[{
                    "user_id": "10",
                    "roles": ["20"]
                }]]
            }
        })
        .to_string(),
    );
    let mut state = DiscordState::default();
    for event in ready_events.iter().chain(supplemental_events.iter()) {
        state.apply_event(event);
    }

    assert_eq!(
        state.channel_visibility_stats(Some(Id::new(1))),
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
    assert!(
        state
            .viewable_channels_for_guild(Some(Id::new(1)))
            .is_empty()
    );
}

#[test]
fn raw_ready_supplemental_accepts_bare_id_presence_entries() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [
                        { "id": "20", "status": "online" }
                    ]
                }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::PresenceUpdate { guild_id: None, presence }]
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    ));
}

#[test]
fn raw_ready_supplemental_ignores_non_presence_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "merged_presences": {
                    "friends": [],
                    "metadata": { "id": "20" }
                }
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn raw_presence_update_accepts_user_id_field() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user_id": "20",
                "status": "online"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::PresenceUpdate { guild_id: None, presence }]
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
    ));
}

#[test]
fn raw_presence_update_parses_rich_activity_fields() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user": { "id": "20" },
                "status": "online",
                "activities": [{
                    "id": "activity-1",
                    "type": 6,
                    "name": "Hang Status",
                    "created_at": "1700000000000",
                    "session_id": "session-1",
                    "platform": "xbox",
                    "supported_platforms": ["xbox", "desktop"],
                    "application_id": "12345",
                    "parent_application_id": "54321",
                    "status_display_type": 2,
                    "details": "Building Concord",
                    "details_url": "https://example.com/details",
                    "state": "custom",
                    "state_url": "https://example.com/state",
                    "sync_id": "sync-1",
                    "flags": 17,
                    "timestamps": {
                        "start": "1700000000000",
                        "end": 1_700_000_100_000i64
                    },
                    "assets": {
                        "large_image": "cover",
                        "large_text": "Main menu",
                        "large_url": "https://example.com/large",
                        "small_image": "small",
                        "small_text": "Small",
                        "small_url": "https://example.com/small",
                        "invite_cover_image": "invite",
                        "future_asset": true
                    },
                    "party": {
                        "id": "party-1",
                        "size": [2, 5],
                        "privacy": 1,
                        "future_party": "kept"
                    },
                    "secrets": {
                        "join": "join-secret",
                        "spectate": "spectate-secret",
                        "future_secret": 7
                    },
                    "buttons": ["Join"],
                    "metadata": {
                        "button_urls": ["https://example.com/join"],
                        "artist_ids": ["artist-1"]
                    },
                    "future_activity": { "value": 1 }
                }]
            }
        })
        .to_string(),
    );

    let [AppEvent::PresenceUpdate { presence, .. }] = events.as_slice() else {
        panic!("expected a single presence update, got {events:?}");
    };
    let activity = &presence.activities[0];
    assert_eq!(activity.id.as_deref(), Some("activity-1"));
    assert_eq!(activity.kind, ActivityKind::Hang);
    assert_eq!(activity.created_at, Some(1_700_000_000_000));
    assert_eq!(activity.session_id.as_deref(), Some("session-1"));
    assert_eq!(activity.platform.as_deref(), Some("xbox"));
    assert_eq!(activity.supported_platforms, ["xbox", "desktop"]);
    assert_eq!(activity.application_id.as_deref(), Some("12345"));
    assert_eq!(activity.parent_application_id.as_deref(), Some("54321"));
    assert_eq!(activity.status_display_type, Some(2));
    assert_eq!(activity.details.as_deref(), Some("Building Concord"));
    assert_eq!(
        activity.details_url.as_deref(),
        Some("https://example.com/details")
    );
    assert_eq!(activity.state.as_deref(), Some("custom"));
    assert_eq!(
        activity.state_url.as_deref(),
        Some("https://example.com/state")
    );
    assert_eq!(activity.sync_id.as_deref(), Some("sync-1"));
    assert_eq!(activity.flags, Some(17));
    assert_eq!(
        activity.timestamps.and_then(|t| t.start),
        Some(1_700_000_000_000)
    );
    assert_eq!(
        activity.timestamps.and_then(|t| t.end),
        Some(1_700_000_100_000)
    );
    let assets = activity.assets.as_ref().expect("assets parsed");
    assert_eq!(assets.large_image.as_deref(), Some("cover"));
    assert_eq!(assets.large_text.as_deref(), Some("Main menu"));
    assert_eq!(
        assets.large_url.as_deref(),
        Some("https://example.com/large")
    );
    assert_eq!(assets.small_image.as_deref(), Some("small"));
    assert_eq!(assets.small_text.as_deref(), Some("Small"));
    assert_eq!(
        assets.small_url.as_deref(),
        Some("https://example.com/small")
    );
    assert_eq!(assets.invite_cover_image.as_deref(), Some("invite"));
    assert_eq!(assets.extra_fields["future_asset"], json!(true));
    let party = activity.party.as_ref().expect("party parsed");
    assert_eq!(party.size, Some((2, 5)));
    assert_eq!(party.privacy, Some(1));
    assert_eq!(party.extra_fields["future_party"], json!("kept"));
    let secrets = activity.secrets.as_ref().expect("secrets parsed");
    assert_eq!(secrets.join.as_deref(), Some("join-secret"));
    assert_eq!(secrets.spectate.as_deref(), Some("spectate-secret"));
    assert_eq!(secrets.extra_fields["future_secret"], json!(7));
    assert_eq!(activity.buttons.len(), 1);
    assert_eq!(activity.buttons[0].label, "Join");
    assert_eq!(activity.buttons[0].url, "https://example.com/join");
    assert_eq!(activity.metadata["artist_ids"], json!(["artist-1"]));
    assert_eq!(
        activity.extra_fields["future_activity"],
        json!({ "value": 1 })
    );
}

#[test]
fn thread_gateway_events_keep_active_metadata_and_membership_separate() {
    let mut joined = thread_payload(10, "joined");
    joined["member"] = json!({
        "id": "10",
        "user_id": "99",
        "flags": 4,
        "muted": true,
        "mute_config": {
            "end_time": "2099-01-01T00:00:00.000Z",
            "selected_time_window": 3600
        }
    });
    for (payload, expected_member) in [(joined, true), (thread_payload(11, "not joined"), false)] {
        let events =
            parse_user_account_event(&json!({ "t": "THREAD_CREATE", "d": payload }).to_string());
        let [AppEvent::ThreadUpsert { thread, created }] = events.as_slice() else {
            panic!("expected one thread upsert, got {events:?}");
        };
        assert!(*created);
        assert_eq!(thread.current_user_member.is_some(), expected_member);
        if let Some(member) = &thread.current_user_member {
            assert_eq!(member.thread_id, Some(thread.channel.channel_id));
            assert_eq!(member.flags, Some(4));
            assert_eq!(member.muted, Some(true));
            assert_eq!(
                member.mute_end_time.as_deref(),
                Some("2099-01-01T00:00:00.000Z")
            );
            assert_eq!(member.selected_time_window, Some(3600));
        }
    }
}

#[test]
fn thread_list_sync_keeps_all_active_threads_and_only_explicit_members() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_LIST_SYNC",
            "d": {
                "guild_id": "1",
                "channel_ids": ["2"],
                "threads": [
                    thread_payload(10, "joined"),
                    thread_payload(11, "not joined")
                ],
                "members": [{
                    "id": "10",
                    "user_id": "99",
                    "flags": 2,
                    "muted": false
                }]
            }
        })
        .to_string(),
    );

    let [AppEvent::ThreadListSync { sync }] = events.as_slice() else {
        panic!("expected one thread list sync, got {events:?}");
    };
    assert_eq!(
        sync.threads
            .iter()
            .map(|thread| thread.channel_id)
            .collect::<Vec<_>>(),
        vec![Id::new(10), Id::new(11)]
    );
    let members = sync
        .current_user_members
        .as_ref()
        .expect("present member array should be preserved");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].thread_id, Some(Id::new(10)));
    assert_eq!(members[0].flags, Some(2));
}

#[test]
fn thread_list_sync_distinguishes_omitted_and_empty_member_snapshots() {
    for (members, expected) in [(None, None), (Some(json!([])), Some(0))] {
        let mut data = json!({
            "guild_id": "1",
            "threads": [thread_payload(10, "thread")]
        });
        if let Some(members) = members {
            data["members"] = members;
        }
        let events =
            parse_user_account_event(&json!({ "t": "THREAD_LIST_SYNC", "d": data }).to_string());
        let [AppEvent::ThreadListSync { sync }] = events.as_slice() else {
            panic!("expected one thread list sync, got {events:?}");
        };
        assert_eq!(sync.current_user_members.as_ref().map(Vec::len), expected);
    }
}

#[test]
fn thread_participant_snapshot_parses_profiles_without_implying_current_membership() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "1",
                "thread_id": "10",
                "members": [{
                    "user_id": "20",
                    "member": {
                        "user": { "id": "20", "username": "alice" },
                        "roles": []
                    },
                    "presence": { "status": "online" }
                }]
            }
        })
        .to_string(),
    );

    let [AppEvent::ThreadMemberListUpdate { update }] = events.as_slice() else {
        panic!("expected one participant snapshot, got {events:?}");
    };
    assert_eq!(update.channel_id, Id::new(10));
    assert_eq!(update.members.len(), 1);
    assert_eq!(update.members[0].user_id, Some(Id::new(20)));
    assert_eq!(
        update.members[0]
            .member
            .as_ref()
            .map(|member| member.display_name.as_str()),
        Some("alice")
    );
    assert_eq!(
        update.members[0]
            .presence
            .as_ref()
            .map(|presence| presence.status),
        Some(PresenceStatus::Online)
    );
}

#[test]
fn guild_create_marks_every_user_snapshot_thread_as_joined() {
    let mut joined = thread_payload(10, "joined");
    joined["member"] = json!({ "id": "10", "user_id": "99", "flags": 4 });
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "threads": [joined, thread_payload(11, "not joined")],
        "members": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate {
        channels,
        current_user_thread_members,
        ..
    } = event
    else {
        panic!("expected guild create");
    };
    assert_eq!(channels.len(), 2);
    assert_eq!(current_user_thread_members.len(), 2);
    assert_eq!(current_user_thread_members[0].thread_id, Some(Id::new(10)));
    assert_eq!(current_user_thread_members[0].flags, Some(4));
    assert_eq!(current_user_thread_members[1].thread_id, Some(Id::new(11)));
    assert_eq!(current_user_thread_members[1].flags, None);
}

#[test]
fn ready_supplemental_marks_user_snapshot_threads_as_joined() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{ "id": "1", "threads": [thread_payload(10, "joined")] }]
            }
        })
        .to_string(),
    );

    let thread = events.iter().find_map(|event| match event {
        AppEvent::ThreadUpsert { thread, .. } => Some(thread),
        _ => None,
    });
    let thread = thread.expect("supplemental thread should use the thread event path");
    assert_eq!(thread.channel.guild_id, Some(Id::new(1)));
    assert_eq!(
        thread
            .current_user_member
            .as_ref()
            .and_then(|member| member.thread_id),
        Some(Id::new(10))
    );
}

#[test]
fn raw_group_dm_recipient_events_preserve_the_user_delta() {
    let added = parse_user_account_event(
        &json!({
            "t": "CHANNEL_RECIPIENT_ADD",
            "d": {
                "channel_id": "10",
                "user": {
                    "id": "20",
                    "username": "alice",
                    "global_name": "Alice"
                }
            }
        })
        .to_string(),
    );
    let removed = parse_user_account_event(
        &json!({
            "t": "CHANNEL_RECIPIENT_REMOVE",
            "d": {
                "channel_id": "10",
                "user": { "id": "20" }
            }
        })
        .to_string(),
    );

    assert!(matches!(
        added.as_slice(),
        [AppEvent::ChannelRecipientAdd { channel_id, recipient }]
            if *channel_id == Id::new(10)
                && recipient.user_id == Id::new(20)
                && recipient.display_name == "Alice"
    ));
    assert!(matches!(
        removed.as_slice(),
        [AppEvent::ChannelRecipientRemove { channel_id, user_id }]
            if *channel_id == Id::new(10) && *user_id == Id::new(20)
    ));
}

#[test]
fn message_update_parser_distinguishes_absent_and_empty_attachments() {
    let cases = [
        (
            json!({
                "id": "20",
                "channel_id": "10",
                "content": "edited"
            }),
            false,
        ),
        (
            json!({
                "id": "20",
                "channel_id": "10",
                "content": "edited",
                "attachments": []
            }),
            true,
        ),
    ];

    for (payload, clears_attachments) in cases {
        let event = parse_message_update(&payload).expect("message update should parse");
        let AppEvent::MessageUpdateDispatch { update } = event else {
            panic!("expected message update event");
        };
        if clears_attachments {
            assert!(
                matches!(update.fields.attachments, AttachmentUpdate::Replace(values) if values.is_empty())
            );
        } else {
            assert!(matches!(
                update.fields.attachments,
                AttachmentUpdate::Unchanged
            ));
        }
    }
}

#[test]
fn message_update_parser_preserves_pin_state() {
    let event = parse_message_update(&json!({
        "id": "20",
        "channel_id": "10",
        "pinned": true
    }))
    .expect("message update should parse");
    let AppEvent::MessageUpdateDispatch { update } = event else {
        panic!("expected message update event");
    };

    assert_eq!(update.fields.pinned, Some(true));
}

#[test]
fn guild_create_parser_keeps_custom_emojis() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "member_count": 123,
        "channels": [],
        "members": [],
        "presences": [],
        "emojis": [
            {
                "id": "50",
                "name": "party",
                "animated": true,
                "available": true
            },
            {
                "id": "51",
                "name": "sleep",
                "available": false
            }
        ]
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate {
        member_count,
        emojis,
        ..
    } = event
    else {
        panic!("expected guild create event");
    };
    assert_eq!(member_count, Some(123));
    assert_eq!(emojis.len(), 2);
    assert_eq!(emojis[0].id, Id::new(50));
    assert_eq!(emojis[0].name, "party");
    assert!(emojis[0].animated);
    assert!(emojis[0].available);
    assert!(!emojis[1].available);
}

#[test]
fn guild_create_parser_keeps_roles() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "members": [],
        "presences": [],
        "roles": [{
            "id": "90",
            "name": "Admin",
            "color": 16755200,
            "position": 10,
            "hoist": true
        }],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { roles, .. } = event else {
        panic!("expected guild create event");
    };

    let roles = roles.expect("guild roles should be present");
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].id, Id::new(90));
    assert_eq!(roles[0].name, "Admin");
    assert_eq!(roles[0].color, Some(16755200));
    assert_eq!(roles[0].position, 10);
    assert!(roles[0].hoist);
}

#[test]
fn raw_guild_role_events_patch_single_roles() {
    let created = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_CREATE",
            "d": {
                "guild_id": "1",
                "role": {
                    "id": "90",
                    "name": "Admin",
                    "color": 16755200,
                    "position": 10,
                    "hoist": true,
                    "permissions": "1024"
                }
            }
        })
        .to_string(),
    );
    let updated = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_UPDATE",
            "d": {
                "guild_id": "1",
                "role": {
                    "id": "90",
                    "name": "Owner",
                    "color": 0,
                    "position": 11,
                    "hoist": false,
                    "permissions": "2048"
                }
            }
        })
        .to_string(),
    );
    let deleted = parse_user_account_event(
        &json!({
            "t": "GUILD_ROLE_DELETE",
            "d": {
                "guild_id": "1",
                "role_id": "90"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        created.as_slice(),
        [AppEvent::GuildRoleUpsert { guild_id, role }]
            if *guild_id == Id::new(1)
                && role.id == Id::new(90)
                && role.name == "Admin"
                && role.color == Some(16755200)
                && role.position == 10
                && role.hoist
                && role.permissions == 1024
    ));
    assert!(matches!(
        updated.as_slice(),
        [AppEvent::GuildRoleUpsert { guild_id, role }]
            if *guild_id == Id::new(1)
                && role.id == Id::new(90)
                && role.name == "Owner"
                && role.color.is_none()
                && role.position == 11
                && !role.hoist
                && role.permissions == 2048
    ));
    assert!(matches!(
        deleted.as_slice(),
        [AppEvent::GuildRoleDelete { guild_id, role_id }]
            if *guild_id == Id::new(1) && *role_id == Id::new(90)
    ));
}

#[test]
fn raw_channel_pins_update_invalidates_channel_pins() {
    let full = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "last_pin_timestamp": "2026-05-25T12:34:56.000000+00:00"
            }
        })
        .to_string(),
    );
    assert!(matches!(
        full.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if *guild_id == Some(Id::new(1))
                && *channel_id == Id::new(10)
                && last_pin_timestamp.as_deref() == Some("2026-05-25T12:34:56.000000+00:00")
    ));

    let minimal = parse_user_account_event(
        &json!({ "t": "CHANNEL_PINS_UPDATE", "d": { "channel_id": "10" } }).to_string(),
    );
    assert!(matches!(
        minimal.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if guild_id.is_none() && *channel_id == Id::new(10) && last_pin_timestamp.is_none()
    ));

    // Without a channel there is nothing to invalidate, so no event at all.
    let channelless = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": { "guild_id": "1", "last_pin_timestamp": null }
        })
        .to_string(),
    );
    assert!(channelless.is_empty());
}

#[test]
fn guild_create_parser_accepts_member_user_id_without_nested_user() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "members": [{
            "user_id": "10",
            "roles": [20]
        }],
        "presences": [],
        "roles": [],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { members, .. } = event else {
        panic!("expected guild create event");
    };

    assert_eq!(members.len(), 1);
    assert_eq!(members[0].user_id, Id::new(10));
    assert_eq!(members[0].role_ids, vec![Id::new(20)]);
}

#[test]
fn raw_guild_create_with_thin_current_member_hides_denied_channel() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "owner_id": "11",
        "channels": [{
            "id": "2",
            "type": 0,
            "name": "secret",
            "permission_overwrites": [{
                "id": "1",
                "type": 0,
                "allow": "0",
                "deny": "1024"
            }]
        }],
        "members": [{
            "user_id": "10",
            "roles": []
        }],
        "presences": [],
        "roles": [{
            "id": "1",
            "name": "@everyone",
            "permissions": "1024",
            "position": 0,
            "hoist": false
        }],
        "emojis": []
    }))
    .expect("guild create should parse");
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&event);

    assert_eq!(
        state.channel_visibility_stats(Some(Id::new(1))),
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
    assert!(
        state
            .viewable_channels_for_guild(Some(Id::new(1)))
            .is_empty()
    );
}

#[test]
fn raw_guild_create_with_thin_current_member_keeps_role_based_access() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "owner_id": "11",
        "channels": [{
            "id": "2",
            "type": 0,
            "name": "staff",
            "permission_overwrites": [{
                "id": "1",
                "type": 0,
                "allow": "0",
                "deny": "1024"
            }, {
                "id": "20",
                "type": 0,
                "allow": "1024",
                "deny": "0"
            }]
        }],
        "members": [{
            "user_id": "10",
            "roles": [20]
        }],
        "presences": [],
        "roles": [{
            "id": "1",
            "name": "@everyone",
            "permissions": "1024",
            "position": 0,
            "hoist": false
        }, {
            "id": "20",
            "name": "Staff",
            "permissions": "0",
            "position": 1,
            "hoist": false
        }],
        "emojis": []
    }))
    .expect("guild create should parse");
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.apply_event(&event);

    assert_eq!(
        state.channel_visibility_stats(Some(Id::new(1))),
        ChannelVisibilityStats {
            visible: 1,
            hidden: 0,
        }
    );
    assert_eq!(state.viewable_channels_for_guild(Some(Id::new(1))).len(), 1);
}

#[test]
fn raw_member_chunk_upserts_members_and_presences() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBERS_CHUNK",
            "d": {
                "guild_id": "1",
                "chunk_index": 0,
                "chunk_count": 1,
                "members": [
                    {
                        "nick": "Alice Nick",
                        "roles": ["30", "31"],
                        "user": {
                            "id": "10",
                            "username": "alice",
                            "global_name": "Alice Global",
                            "avatar": "avatarhash"
                        }
                    },
                    {
                        "user": {
                            "id": "20",
                            "username": "bob",
                            "bot": true
                        }
                    }
                ],
                "presences": [
                    { "user": { "id": "10" }, "status": "online" },
                    { "user": { "id": "20" }, "status": "idle" }
                ]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::GuildMembersChunk { chunk }] => {
            assert_eq!(chunk.guild_id, Id::new(1));
            assert_eq!(chunk.members.len(), 2);
            assert_eq!(chunk.members[0].user_id, Id::new(10));
            assert_eq!(chunk.members[0].display_name, "Alice Nick");
            assert_eq!(chunk.members[0].role_ids, vec![Id::new(30), Id::new(31)]);
            assert!(!chunk.members[0].is_bot);
            assert_eq!(chunk.members[1].user_id, Id::new(20));
            assert_eq!(chunk.members[1].display_name, "bob");
            assert!(chunk.members[1].is_bot);
            assert_eq!(chunk.presences[0].user_id, Id::new(10));
            assert_eq!(chunk.presences[0].status, PresenceStatus::Online);
            assert_eq!(chunk.presences[1].user_id, Id::new(20));
            assert_eq!(chunk.presences[1].status, PresenceStatus::Idle);
        }
        other => panic!("expected one GuildMembersChunk, got {other:?}"),
    }
}

#[test]
fn raw_member_add_keeps_real_join_semantics() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_ADD",
            "d": {
                "guild_id": "1",
                "nick": "Alice Nick",
                "user": {
                    "id": "10",
                    "username": "alice"
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        AppEvent::GuildMemberAdd { guild_id, member }
            if *guild_id == Id::new(1)
                && member.user_id == Id::new(10)
                && member.display_name == "Alice Nick"
    ));
}

#[test]
fn guild_emojis_update_parser_replaces_custom_emojis() {
    let event = parse_guild_emojis_update(&json!({
        "guild_id": "1",
        "emojis": [
            {
                "id": "60",
                "name": "wave",
                "animated": false,
                "available": true
            }
        ]
    }))
    .expect("guild emojis update should parse");

    let AppEvent::GuildEmojisUpdate { guild_id, emojis } = event else {
        panic!("expected guild emojis update event");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(60));
    assert_eq!(emojis[0].name, "wave");
    assert!(emojis[0].available);
}

#[test]
fn guild_update_parser_distinguishes_present_and_absent_custom_emojis() {
    let event = parse_guild_update(&json!({
        "id": "1",
        "name": "guild renamed",
        "emojis": [{
            "id": "70",
            "name": "dance",
            "animated": true,
            "available": true
        }]
    }))
    .expect("guild update should parse");

    let AppEvent::GuildUpdate {
        guild_id,
        name,
        roles,
        emojis,
        ..
    } = event
    else {
        panic!("expected guild update event");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(name, "guild renamed");
    assert_eq!(roles, None);
    let emojis = emojis.expect("emoji field should be preserved when present");
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(70));
    assert_eq!(emojis[0].name, "dance");
    assert!(emojis[0].animated);

    // An absent field must stay `None` rather than collapsing to an empty
    // list, or applying the update would wipe the guild's emojis.
    let event = parse_guild_update(&json!({ "id": "1", "name": "guild renamed" }))
        .expect("guild update should parse");
    let AppEvent::GuildUpdate { roles, emojis, .. } = event else {
        panic!("expected guild update event");
    };
    assert_eq!(roles, None);
    assert_eq!(emojis, None);
}

#[test]
fn message_update_parser_keeps_mentions_when_present() {
    let event = parse_message_update(&json!({
        "id": "20",
        "channel_id": "10",
        "content": "edited <@40>",
        "mentions": [{ "id": "40", "username": "alice" }]
    }))
    .expect("message update should parse");

    let AppEvent::MessageUpdateDispatch { update } = event else {
        panic!("expected message update event");
    };
    assert_eq!(
        update.fields.mentions,
        Some(vec![mention_info(40, "alice")])
    );
}

#[test]
fn message_update_parser_keeps_poll_results() {
    let event = parse_message_update(&json!({
        "id": "20",
        "channel_id": "10",
        "poll": {
            "question": { "text": "오늘 뭐 먹지?" },
            "answers": [
                { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                { "answer_id": 2, "poll_media": { "text": "라멘" } }
            ],
            "results": {
                "is_finalized": true,
                "answer_counts": [
                    { "id": 1, "count": 5, "me_voted": true },
                    { "id": 2, "count": 3, "me_voted": false }
                ]
            }
        }
    }))
    .expect("message update should parse");

    let AppEvent::MessageUpdateDispatch { update } = event else {
        panic!("expected message update event");
    };
    let poll = update.fields.poll.expect("poll payload should be kept");
    assert_eq!(poll.results_finalized, Some(true));
    assert_eq!(poll.answers[0].vote_count, Some(5));
    assert!(poll.answers[0].me_voted);
}

#[test]
fn message_delete_bulk_dispatch_parses_deleted_message_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_DELETE_BULK",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "ids": ["20", "30"]
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageDeleteBulk {
        guild_id,
        channel_id,
        message_ids,
    } = &events[0]
    else {
        panic!("expected message delete bulk event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(message_ids, &vec![Id::new(20), Id::new(30)]);
}

#[test]
fn message_delete_bulk_dispatch_ignores_empty_deleted_message_ids() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_DELETE_BULK",
            "d": {
                "channel_id": "10",
                "ids": []
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
}

#[test]
fn message_reaction_add_dispatch_parses_reaction_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_ADD",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "message_id": "20",
                "user_id": "30",
                "emoji": { "name": "👍" }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionAdd {
        guild_id,
        channel_id,
        message_id,
        user_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction add event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(*user_id, Id::new(30));
    assert_eq!(emoji, &ReactionEmoji::Unicode("👍".to_owned()));
}

#[test]
fn message_reaction_remove_dispatch_parses_custom_reaction_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE",
            "d": {
                "channel_id": "10",
                "message_id": "20",
                "user_id": "30",
                "emoji": {
                    "id": "40",
                    "name": "party",
                    "animated": true
                }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemove {
        guild_id,
        channel_id,
        message_id,
        user_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction remove event");
    };
    assert_eq!(*guild_id, None);
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(*user_id, Id::new(30));
    assert_eq!(
        emoji,
        &ReactionEmoji::Custom {
            id: Id::new(40),
            name: Some("party".to_owned()),
            animated: true,
        }
    );
}

#[test]
fn message_reaction_remove_all_dispatch_parses_clear_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE_ALL",
            "d": {
                "guild_id": "1",
                "channel_id": "10",
                "message_id": "20"
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemoveAll {
        guild_id,
        channel_id,
        message_id,
    } = &events[0]
    else {
        panic!("expected message reaction remove all event");
    };
    assert_eq!(*guild_id, Some(Id::new(1)));
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
}

#[test]
fn message_reaction_remove_emoji_dispatch_parses_clear_emoji_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_REACTION_REMOVE_EMOJI",
            "d": {
                "channel_id": "10",
                "message_id": "20",
                "emoji": { "name": "👍" }
            }
        })
        .to_string(),
    );

    assert_eq!(events.len(), 1);
    let AppEvent::MessageReactionRemoveEmoji {
        guild_id,
        channel_id,
        message_id,
        emoji,
    } = &events[0]
    else {
        panic!("expected message reaction remove emoji event");
    };
    assert_eq!(*guild_id, None);
    assert_eq!(*channel_id, Id::new(10));
    assert_eq!(*message_id, Id::new(20));
    assert_eq!(emoji, &ReactionEmoji::Unicode("👍".to_owned()));
}

#[test]
fn message_create_parser_keeps_regular_embeds() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "embeds": [{
            "type": "video",
            "color": 16711680,
            "provider": { "name": "YouTube" },
            "title": "Example Video",
            "description": "A video description",
            "timestamp": "2026-05-13T15:22:03+00:00",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "thumbnail": {
                "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                "proxy_url": "https://images-ext-1.discordapp.net/external/thumb/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                "width": 480,
                "height": 360
            },
            "image": {
                "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
                "proxy_url": "https://images-ext-2.discordapp.net/external/image/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
                "width": 1280,
                "height": 720
            },
            "video": { "url": "https://www.youtube.com/embed/dQw4w9WgXcQ" }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.embeds.len(), 1);
    assert_eq!(message.embeds[0].color, Some(16711680));
    assert_eq!(message.embeds[0].provider_name.as_deref(), Some("YouTube"));
    assert_eq!(message.embeds[0].title.as_deref(), Some("Example Video"));
    assert_eq!(
        message.embeds[0].timestamp.as_deref(),
        Some("2026-05-13T15:22:03+00:00")
    );
    assert_eq!(
        message.embeds[0].thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
    );
    assert_eq!(
        message.embeds[0].thumbnail_proxy_url.as_deref(),
        Some(
            "https://images-ext-1.discordapp.net/external/thumb/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        )
    );
    assert_eq!(message.embeds[0].thumbnail_width, Some(480));
    assert_eq!(message.embeds[0].thumbnail_height, Some(360));
    assert_eq!(
        message.embeds[0].image_url.as_deref(),
        Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg")
    );
    assert_eq!(
        message.embeds[0].image_proxy_url.as_deref(),
        Some(
            "https://images-ext-2.discordapp.net/external/image/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg"
        )
    );
    assert_eq!(message.embeds[0].image_width, Some(1280));
    assert_eq!(message.embeds[0].image_height, Some(720));
    assert_eq!(
        message.embeds[0].video_url.as_deref(),
        Some("https://www.youtube.com/embed/dQw4w9WgXcQ")
    );
}

#[test]
fn message_create_parser_builds_giphy_animation_url_for_gifv() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "https://giphy.com/gifs/hvY8Ahy9r340SU8xLY",
        "embeds": [{
            "type": "gifv",
            "url": "https://giphy.com/gifs/hvY8Ahy9r340SU8xLY",
            "thumbnail": {
                "url": "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy_s.gif",
                "width": 500,
                "height": 599
            },
            "video": {
                "url": "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.mp4?cid=discord",
                "width": 500,
                "height": 599
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.embeds[0].gifv_image_url.as_deref(),
        Some("https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.webp?cid=discord")
    );
}

#[test]
fn message_create_parser_keeps_timestamp_only_embeds() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "embeds": [{
            "timestamp": "2026-05-13T15:22:03+00:00"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.embeds.len(), 1);
    assert_eq!(
        message.embeds[0].timestamp.as_deref(),
        Some("2026-05-13T15:22:03+00:00")
    );
}

#[test]
fn message_create_parser_keeps_message_type() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "mee6", "bot": true },
        "type": 20,
        "content": "",
        "attachments": [],
        "interaction": {
            "name": "anime search",
            "user": { "id": "40", "global_name": "Casey", "username": "casey" }
        },
        "interaction_metadata": {
            "user": { "id": "40", "global_name": "Casey", "username": "casey" }
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.message_kind, MessageKind::new(20));
    assert!(message.author_is_bot);
    let interaction = message
        .interaction
        .expect("interaction metadata should parse");
    assert_eq!(interaction.user_id, Some(Id::new(40)));
    assert_eq!(interaction.user, "Casey");
    assert_eq!(interaction.command_name.as_deref(), Some("anime search"));
}

#[test]
fn message_create_parser_resolves_author_name_by_precedence() {
    // Server nick beats global name beats username.
    let cases = [
        (
            json!({ "nick": "server alias" }),
            Some(Id::new(1)),
            "server alias",
        ),
        (json!(null), None, "global alias"),
    ];

    for (member, guild_id, expected_author) in cases {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": guild_id.map(|id: Id<_>| id.get().to_string()),
            "author": { "id": "30", "global_name": "global alias", "username": "neo" },
            "member": member,
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { message } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message.guild_id, guild_id);
        assert_eq!(message.author, expected_author);
    }
}

#[test]
fn message_info_parser_preserves_webhook_identity() {
    let message = parse_message_info(&json!({
        "id": "20",
        "channel_id": "10",
        "webhook_id": "40",
        "author": {
            "id": "30",
            "global_name": "cached bot name",
            "username": "Persona One",
            "avatar": "avatarhash",
            "bot": true
        },
        "content": "hello"
    }))
    .expect("webhook message should parse");

    assert_eq!(message.webhook_id, Some(Id::new(40)));
    assert_eq!(message.author, "Persona One");
    assert_eq!(
        message.author_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/30/avatarhash.png")
    );
}

#[test]
fn message_info_parser_tracks_author_role_payload_presence() {
    let cases = [
        (
            "roles present",
            json!({ "roles": ["90", "91"] }),
            vec![Id::new(90), Id::new(91)],
            true,
        ),
        (
            "roles explicitly empty",
            json!({ "roles": [] }),
            vec![],
            true,
        ),
        ("member omitted", Value::Null, vec![], false),
    ];

    for (label, member, expected_roles, expected_presence) in cases {
        let mut payload = json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": "1",
            "author": { "id": "30", "username": "neo" },
            "content": "hello",
            "attachments": []
        });
        if !member.is_null() {
            payload["member"] = member;
        }
        let message = parse_message_info(&payload).expect("message should parse");

        assert_eq!(message.author_role_ids, expected_roles, "{label}");
        assert_eq!(
            message.author_role_ids_present, expected_presence,
            "{label}"
        );
    }
}

#[test]
fn message_info_parser_keeps_outgoing_nonce() {
    let message = parse_message_info(&json!({
        "id": "20",
        "channel_id": "10",
        "nonce": "99",
        "author": { "id": "30", "username": "neo" },
        "content": "hello",
        "attachments": []
    }))
    .expect("message should parse");

    assert_eq!(message.nonce, Some(Id::new(99)));
}

#[test]
fn message_create_parser_builds_author_avatar_url() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": {
            "id": "30",
            "username": "neo",
            "avatar": "a_avatarhash"
        },
        "content": "hello",
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.author_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/30/a_avatarhash.gif")
    );
}

#[test]
fn message_create_parser_keeps_mention_display_names() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "hello <@40> <@41> <@42> <@43>",
        "mention_everyone": true,
        "mention_roles": ["50", "51"],
        "flags": 4096,
        "mentions": [
            {
                "id": "40",
                "username": "alpha",
                "global_name": "Alpha Global",
                "member": { "nick": "Alpha Nick" }
            },
            {
                "id": "41",
                "username": "beta",
                "global_name": "Beta Global"
            },
            {
                "id": "42",
                "username": "gamma"
            },
            { "id": "43" }
        ],
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert!(message.mention_everyone);
    assert_eq!(message.mention_roles, vec![Id::new(50), Id::new(51)]);
    assert_eq!(message.flags, 4096);
    assert_eq!(
        message.mentions,
        vec![
            mention_info_with_nick(40, "Alpha Nick"),
            mention_info(41, "Beta Global"),
            mention_info(42, "gamma"),
            mention_info(43, "unknown"),
        ]
    );
}

#[test]
fn message_create_parser_does_not_store_empty_mention_nick() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "hello <@40>",
        "mentions": [{
            "id": "40",
            "username": "alpha",
            "member": { "nick": "" }
        }],
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.mentions, vec![mention_info(40, "alpha")]);
}

#[test]
fn message_create_parser_keeps_reply_preview() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 19,
        "content": "reply",
        "attachments": [],
        "referenced_message": {
            "id": "19",
            "channel_id": "10",
            "author": { "id": "31", "global_name": "Alex", "username": "alex" },
            "content": "잘되는군",
            "attachments": []
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.reply,
        Some(ReplyInfo {
            author_id: Some(Id::new(31)),
            author: "Alex".to_owned(),
            content: Some("잘되는군".to_owned()),
            stickers: Vec::new(),
            mentions: Vec::new(),
        })
    );
}

#[test]
fn message_create_parser_keeps_reply_mentions() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 19,
        "content": "reply",
        "attachments": [],
        "referenced_message": {
            "id": "19",
            "channel_id": "10",
            "author": { "id": "31", "username": "alex" },
            "content": "hello <@40>",
            "mentions": [{ "id": "40", "username": "alice" }],
            "attachments": []
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message
            .reply
            .and_then(|reply| reply.mentions.into_iter().next()),
        Some(mention_info(40, "alice"))
    );
}

#[test]
fn message_create_parser_keeps_poll_payload() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 0,
        "content": "",
        "attachments": [],
        "poll": {
            "question": { "text": "오늘 뭐 먹지?" },
            "answers": [
                { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                { "answer_id": 2, "poll_media": { "text": "라멘" } }
            ],
            "results": {
                "is_finalized": false,
                "answer_counts": [
                    { "id": 1, "count": 2, "me_voted": true },
                    { "id": 2, "count": 1, "me_voted": false }
                ]
            },
            "allow_multiselect": true
        }
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message.poll,
        Some(PollInfo {
            question: "오늘 뭐 먹지?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "김치찌개".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "라멘".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect: true,
            results_finalized: Some(false),
            total_votes: Some(3),
        })
    );
}

#[test]
fn message_create_parser_keeps_poll_result_embed() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "type": 46,
        "content": "",
        "attachments": [],
        "embeds": [{
            "type": "poll_result",
            "fields": [
                { "name": "poll_question_text", "value": "오늘 뭐 먹지?" },
                { "name": "victor_answer_id", "value": "1" },
                { "name": "victor_answer_text", "value": "김치찌개" },
                { "name": "victor_answer_votes", "value": "5" },
                { "name": "total_votes", "value": "7" }
            ]
        }]
    }))
    .expect("poll result message should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(
        message
            .poll
            .expect("poll result should map to poll info")
            .total_votes,
        Some(7)
    );
}

#[test]
fn message_create_parser_uses_proxy_url_when_url_is_missing() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "cat.png",
            "proxy_url": "https://media.discordapp.net/cat.png",
            "content_type": "image/png"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(
        message.attachments[0].url,
        "https://media.discordapp.net/cat.png"
    );
    assert_eq!(
        message.attachments[0].proxy_url,
        "https://media.discordapp.net/cat.png"
    );
}

#[test]
fn message_create_parser_keeps_video_attachment_metadata() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "clip.mp4",
            "url": "https://cdn.discordapp.com/clip.mp4",
            "proxy_url": "https://media.discordapp.net/clip.mp4",
            "content_type": "video/mp4",
            "size": 78364758,
            "width": 1920,
            "height": 1080,
            "description": "clip"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].filename, "clip.mp4");
    assert_eq!(
        message.attachments[0].content_type.as_deref(),
        Some("video/mp4")
    );
    assert_eq!(message.attachments[0].size, 78_364_758);
    assert_eq!(message.attachments[0].width, Some(1920));
    assert_eq!(message.attachments[0].height, Some(1080));
}

#[test]
fn message_create_parser_keeps_animated_media_flags() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "dance.webp",
            "url": "https://cdn.discordapp.com/dance.webp",
            "proxy_url": "https://media.discordapp.net/dance.webp",
            "content_type": "image/webp",
            "flags": 32
        }],
        "embeds": [{
            "type": "image",
            "thumbnail": {
                "url": "https://example.com/thumb.webp",
                "flags": 32
            },
            "image": {
                "url": "https://example.com/image.webp",
                "flags": 32
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments[0].flags, 1 << 5);
    assert_eq!(message.embeds[0].thumbnail_flags, 1 << 5);
    assert_eq!(message.embeds[0].image_flags, 1 << 5);
}

#[test]
fn message_create_parser_preserves_content_and_sticker_items() {
    let cases = [
        (
            "",
            vec![json!({ "id": "11", "name": "Wave", "format_type": 1 })],
            vec![StickerInfo::test(11, "Wave")],
        ),
        (
            "hello",
            vec![
                json!({ "id": "11", "name": "Wave", "format_type": 1 }),
                json!({ "id": "12", "name": "Heart", "format_type": 1 }),
            ],
            vec![
                StickerInfo::test(11, "Wave"),
                StickerInfo::test(12, "Heart"),
            ],
        ),
    ];

    for (raw_content, sticker_items, expected_stickers) in cases {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": raw_content,
            "sticker_items": sticker_items
        }))
        .expect("message create should parse");
        let AppEvent::MessageCreate { message } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message.content.as_deref(), Some(raw_content));
        assert_eq!(message.stickers, expected_stickers);
    }
}

#[test]
fn message_create_parser_keeps_forwarded_snapshot_fields() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [],
        "message_reference": { "channel_id": "11" },
        "message_snapshots": [{
            "message": {
                "content": "hello <@40>",
                "timestamp": "2026-04-30T12:34:56.000000+00:00",
                "mentions": [{ "id": "40", "username": "alice" }],
                "attachments": [{
                    "id": "41",
                    "filename": "cat.png",
                    "url": "https://cdn.discordapp.com/cat.png",
                    "proxy_url": "https://media.discordapp.net/cat.png",
                    "content_type": "image/png",
                    "size": 2048,
                    "width": 640,
                    "height": 480
                }],
                "sticker_items": [
                    { "id": "42", "name": "Wave", "format_type": 1 }
                ]
            }
        }, {
            "message": {
                "content": ""
            }
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.forwarded_snapshots.len(), 2);
    assert_eq!(
        message.forwarded_snapshots[0].content.as_deref(),
        Some("hello <@40>")
    );
    assert_eq!(
        message.forwarded_snapshots[0].source_channel_id,
        Some(Id::new(11))
    );
    assert_eq!(
        message.forwarded_snapshots[0].timestamp.as_deref(),
        Some("2026-04-30T12:34:56.000000+00:00")
    );
    assert_eq!(
        message.forwarded_snapshots[0].mentions,
        vec![mention_info(40, "alice")]
    );
    assert_eq!(
        message.forwarded_snapshots[0].stickers,
        vec![StickerInfo::test(42, "Wave")]
    );
    assert_eq!(message.forwarded_snapshots[0].attachments.len(), 1);
    assert_eq!(
        message.forwarded_snapshots[0].attachments[0].filename,
        "cat.png"
    );
    assert_eq!(message.forwarded_snapshots[1].content.as_deref(), Some(""));
}

fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
    MentionInfo::test(Id::new(user_id), display_name.to_owned())
}

fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
    MentionInfo {
        guild_nick: Some(nick.to_owned()),
        ..MentionInfo::test(Id::new(user_id), nick.to_owned())
    }
}

fn thread_payload(id: u64, name: &str) -> serde_json::Value {
    json!({
        "id": id.to_string(),
        "guild_id": "1",
        "parent_id": "2",
        "type": 11,
        "name": name,
        "message_count": 12,
        "total_message_sent": 14,
        "thread_metadata": { "archived": false, "locked": false }
    })
}

#[test]
fn parse_guild_create_reads_name_from_properties_object() {
    // CLIENT_STATE_V2 nests guild metadata under `properties`. Concord looks
    // in both places so it can consume either documented gateway shape.
    let event = parse_guild_create(&json!({
        "id": "100",
        "member_count": 7,
        "channels": [],
        "roles": [],
        "emojis": [],
        "properties": {
            "name": "Lazy Server",
            "owner_id": "42",
        },
    }))
    .expect("guild_create payload should map");

    let AppEvent::GuildCreate {
        guild_id,
        name,
        owner_id,
        member_count,
        ..
    } = event
    else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(guild_id, Id::new(100));
    assert_eq!(name, "Lazy Server");
    assert_eq!(owner_id, Some(Id::new(42)));
    assert_eq!(member_count, Some(7));
}

#[test]
fn parse_guild_create_prefers_root_name_when_both_locations_set() {
    // Guard against future Discord shape drift: if both root-level and
    // nested name are present, the root wins (matches what the official
    // client does).
    let event = parse_guild_create(&json!({
        "id": "100",
        "name": "Root Name",
        "properties": {"name": "Properties Name"},
    }))
    .expect("guild_create payload should map");

    let AppEvent::GuildCreate { name, .. } = event else {
        panic!("expected GuildCreate event");
    };
    assert_eq!(name, "Root Name");
}

#[test]
fn typing_start_extracts_channel_and_user_from_dm_payload() {
    // DM TYPING_START omits guild_id and embeds user_id directly.
    let events = parse_user_account_event(
        &json!({
            "t": "TYPING_START",
            "d": {
                "channel_id": "12345",
                "user_id": "99",
                "timestamp": 1_700_000_000
            }
        })
        .to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [AppEvent::TypingStart { guild_id, channel_id, user_id, member }]
            if *channel_id == Id::new(12345)
                && *user_id == Id::new(99)
                && guild_id.is_none()
                && member.is_none()
    ));
}

#[test]
fn typing_start_falls_back_to_member_user_id_when_top_level_missing() {
    // Some guild TYPING_START payloads only embed the user id under
    // `member.user.id`. Make sure we still surface the typer.
    let events = parse_user_account_event(
        &json!({
            "t": "TYPING_START",
            "d": {
                "channel_id": "55",
                "guild_id": "77",
                "member": {
                    "nick": "Live Nick",
                    "roles": ["90"],
                    "user": {
                        "id": "42",
                        "username": "typing-user",
                        "global_name": "Typing Global",
                        "bot": true,
                        "avatar": "typing-avatar"
                    }
                },
                "timestamp": 1_700_000_000
            }
        })
        .to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [AppEvent::TypingStart { guild_id, channel_id, user_id, member }]
            if *channel_id == Id::new(55)
                && *user_id == Id::new(42)
                && *guild_id == Some(Id::new(77))
                && member.as_ref().is_some_and(|member|
                    member.display_name == "Live Nick"
                        && member.username.as_deref() == Some("typing-user")
                        && member.is_bot
                        && member.role_ids == vec![Id::new(90)]
                        && member.role_ids_present
                )
    ));
}

#[test]
fn ready_hydrates_dm_recipients_from_dedupe_user_ids() {
    // With DEDUPE_USER_OBJECTS in capabilities, READY puts users at the
    // top level once and each private channel only carries
    // `recipient_ids`. The dashboard must still show the peer's name
    // and not `dm-{channel_id}`.
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "10", "username": "me" },
                "users": [
                    {
                        "id": "20",
                        "username": "asdf",
                        "global_name": "global",
                        "discriminator": "0",
                    }
                ],
                "private_channels": [
                    {
                        "id": "12345",
                        "type": 1,
                        "recipient_ids": ["20"]
                    }
                ]
            }
        })
        .to_string(),
    );

    let dm = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ChannelUpsert(info) if info.kind == "dm" => Some(info),
            _ => None,
        })
        .expect("dm channel upsert should be emitted");
    assert_eq!(dm.name, "global");
    let recipients = dm.recipients.as_ref().expect("recipients hydrated");
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].display_name, "global");
    assert_eq!(recipients[0].username.as_deref(), Some("asdf"));

    let mut state = DiscordState::default();
    for event in &events {
        state.apply_event(event);
    }
    let supplemental = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "lazy_private_channels": [{
                    "id": "54321",
                    "type": 3,
                    "recipient_ids": ["20"]
                }]
            }
        })
        .to_string(),
    );
    for event in &supplemental {
        state.apply_event(event);
    }

    let group_dm = state
        .channel(Id::new(54321))
        .expect("supplemental group DM should be cached");
    assert_eq!(group_dm.name, "global, me");
    assert_eq!(
        group_dm
            .recipients
            .iter()
            .map(|recipient| recipient.user_id)
            .collect::<Vec<_>>(),
        vec![Id::new(20), Id::new(10)]
    );
}

#[test]
fn guild_delete_distinguishes_outages_from_membership_removal() {
    let unavailable = parse_user_account_event(
        &json!({
            "t": "GUILD_DELETE",
            "d": { "id": "10", "unavailable": true }
        })
        .to_string(),
    );
    let removed = parse_user_account_event(
        &json!({
            "t": "GUILD_DELETE",
            "d": { "id": "10" }
        })
        .to_string(),
    );

    assert!(matches!(
        unavailable.as_slice(),
        [AppEvent::GuildUnavailable { guild_id }] if *guild_id == Id::new(10)
    ));
    assert!(matches!(
        removed.as_slice(),
        [AppEvent::GuildDelete { guild_id }] if *guild_id == Id::new(10)
    ));
}

#[test]
fn message_ack_preserves_optional_read_state_fields() {
    let present = parse_user_account_event(
        &json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "99",
                "mention_count": 2,
                "flags": 5,
                "last_viewed": 20_000,
                "version": 6,
            }
        })
        .to_string(),
    );

    match present.as_slice() {
        [
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
                flags,
                last_viewed,
                version,
            },
        ] => {
            assert_eq!(*channel_id, Id::new(42));
            assert_eq!(*message_id, Id::new(99));
            assert_eq!(*mention_count, Some(2));
            assert_eq!(*flags, Some(5));
            assert_eq!(*last_viewed, Some(20_000));
            assert_eq!(*version, Some(6));
        }
        other => panic!("expected one MessageAck, got {other:?}"),
    }

    for payload in [
        json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "100",
                "version": 7
            }
        }),
        json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "101",
                "mention_count": null,
                "version": 8
            }
        }),
    ] {
        assert!(matches!(
            parse_user_account_event(&payload.to_string()).as_slice(),
            [AppEvent::MessageAck {
                mention_count: None,
                flags: None,
                last_viewed: None,
                version: Some(_),
                ..
            }]
        ));
    }

    let missing_version = json!({
        "t": "MESSAGE_ACK",
        "d": {
            "channel_id": "42",
            "message_id": "102"
        }
    });
    assert!(parse_user_account_event(&missing_version.to_string()).is_empty());
}

#[test]
fn read_state_dispatches_preserve_ack_and_unread_fields() {
    let feature_ack = parse_user_account_event(
        &json!({
            "t": "USER_NON_CHANNEL_ACK",
            "d": {
                "ack_type": 2,
                "resource_id": "10",
                "entity_id": "20",
                "version": 3
            }
        })
        .to_string(),
    );
    assert!(matches!(
        feature_ack.as_slice(),
        [AppEvent::FeatureReadStateAck {
            read_state_type: 2,
            resource_id: 10,
            entity_id: 20,
            version: 3,
        }]
    ));

    let pins_ack = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_ACK",
            "d": {
                "channel_id": "42",
                "timestamp": "2026-07-24T00:00:00+00:00",
                "version": 4
            }
        })
        .to_string(),
    );
    assert!(matches!(
        pins_ack.as_slice(),
        [AppEvent::ChannelPinsAck {
            channel_id,
            timestamp,
            version: 4,
        }] if *channel_id == Id::new(42) && timestamp == "2026-07-24T00:00:00+00:00"
    ));

    let unread = parse_user_account_event(
        &json!({
            "t": "CHANNEL_UNREAD_UPDATE",
            "d": {
                "guild_id": "1",
                "channel_unread_updates": [{
                    "id": "42",
                    "last_message_id": null,
                    "last_pin_timestamp": null
                }]
            }
        })
        .to_string(),
    );
    assert!(matches!(
        unread.as_slice(),
        [AppEvent::ChannelUnreadUpdate { guild_id, channels }]
            if *guild_id == Id::new(1)
                && channels.len() == 1
                && channels[0].channel_id == Id::new(42)
                && channels[0].last_message_id == Some(None)
                && channels[0].last_pin_timestamp == Some(None)
    ));
}

#[test]
fn application_command_gateway_events_keep_index_and_interaction_data() {
    let index = parse_user_account_event(
        &json!({
            "t": "GUILD_APPLICATION_COMMAND_INDEX_UPDATE",
            "d": { "guild_id": "10" }
        })
        .to_string(),
    );
    let success = parse_user_account_event(
        &json!({
            "t": "INTERACTION_SUCCESS",
            "d": { "id": "20", "nonce": "request-1" }
        })
        .to_string(),
    );
    let failure = parse_user_account_event(
        &json!({
            "t": "INTERACTION_FAILURE",
            "d": { "id": "21", "nonce": 22, "reason_code": 18 }
        })
        .to_string(),
    );
    let autocomplete = parse_user_account_event(
        &json!({
            "t": "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE",
            "d": {
                "nonce": "request-2",
                "choices": [{ "name": "first", "value": 1 }]
            }
        })
        .to_string(),
    );

    assert!(matches!(
        index.as_slice(),
        [AppEvent::ApplicationCommandIndexUpdated { guild_id }]
            if *guild_id == Id::new(10)
    ));
    assert!(matches!(
        success.as_slice(),
        [AppEvent::InteractionSucceeded {
            interaction_id: 20,
            nonce: Some(nonce),
            correlated: false,
        }] if nonce == "request-1"
    ));
    assert!(matches!(
        failure.as_slice(),
        [AppEvent::InteractionFailed {
            interaction_id: 21,
            nonce: Some(nonce),
            reason_code: 18,
            correlated: false,
        }] if nonce == "22"
    ));
    assert!(matches!(
        autocomplete.as_slice(),
        [AppEvent::ApplicationCommandAutocompleteResponse {
            nonce: Some(nonce),
            choices,
        }] if nonce == "request-2"
            && choices.len() == 1
            && choices[0].name == "first"
            && choices[0].value == json!(1)
    ));
}

#[test]
fn user_update_refreshes_global_identity() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_UPDATE",
            "d": {
                "id": "42",
                "username": "neo",
                "global_name": "Neo Global",
                "avatar": "avatar_hash",
                "discriminator": "0"
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [
            AppEvent::UserIdentityUpdate {
                user_id,
                username,
                global_name,
                avatar_url,
                is_bot,
            },
        ] => {
            assert_eq!(*user_id, Id::new(42));
            assert_eq!(username, "neo");
            assert_eq!(global_name.as_deref(), Some("Neo Global"));
            assert_eq!(
                avatar_url.as_deref(),
                Some("https://cdn.discordapp.com/avatars/42/avatar_hash.png"),
            );
            assert!(!is_bot);
        }
        other => panic!("expected one UserIdentityUpdate, got {other:?}"),
    }
}

#[test]
fn ready_payload_emits_read_state_sync_with_ack_pointers() {
    // Minimal READY: a `user`, an empty guild list (so the test stays
    // light), and a `read_state.entries[]` array with two channels.
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [
                        {
                            "id": "11",
                            "last_message_id": "20",
                            "mention_count": 0,
                            "last_pin_timestamp": "2026-07-24T00:00:00.000Z",
                            "flags": 3,
                            "last_viewed": 1234
                        },
                        { "id": "12", "last_message_id": "30", "mention_count": 4 },
                        {
                            "id": "11",
                            "read_state_type": 1,
                            "last_acked_id": "40",
                            "badge_count": 7
                        }
                    ]
                }
            }
        })
        .to_string(),
    );

    let entries = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ReadStateSync {
                entries,
                partial,
                version,
            } if !partial && version.is_none() => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a full ReadStateSync");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, Some(Id::new(20)));
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(
        entries[0].last_pin_timestamp.as_deref(),
        Some("2026-07-24T00:00:00.000Z")
    );
    assert_eq!(entries[0].flags, 3);
    assert_eq!(entries[0].last_viewed, Some(1234));
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].mention_count, 4);
    assert_eq!(entries[2].read_state_type, 1);
    assert_eq!(entries[2].channel_id, Id::new(11));
    assert_eq!(entries[2].last_acked_message_id, Some(Id::new(40)));
    assert_eq!(entries[2].badge_count, 7);
}

#[test]
fn ready_payload_treats_zero_read_state_ack_pointer_as_absent() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [
                        { "id": "11", "last_message_id": "0", "mention_count": 0 },
                        { "id": "12", "last_message_id": 0, "mention_count": 1 },
                    ]
                }
            }
        })
        .to_string(),
    );

    let entries = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ReadStateSync { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a ReadStateSync");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, None);
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].last_acked_message_id, None);
    assert_eq!(entries[1].mention_count, 1);
}

#[test]
fn ready_preserves_empty_and_partial_versioned_snapshots() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "read_state": {
                    "entries": [],
                    "partial": false,
                    "version": 12
                },
                "user_guild_settings": {
                    "entries": [],
                    "partial": true,
                    "version": 13
                }
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::ReadStateSync { entries, partial: false, version: Some(12) }
            if entries.is_empty()
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::UserGuildSettingsSync {
            settings,
            partial: true,
            version: Some(13),
        } if settings.is_empty()
    )));
}

#[test]
fn notification_settings_are_preserved_from_ready_and_updates() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "notification_settings": { "flags": 32 },
                "user_guild_settings": {
                    "entries": [{
                        "guild_id": "10",
                        "message_notifications": 1,
                        "muted": true,
                        "mute_config": {
                            "end_time": "2099-02-01T00:00:00.000Z",
                            "selected_time_window": 3600
                        },
                        "flags": 16384,
                        "hide_muted_channels": true,
                        "mobile_push": false,
                        "mute_scheduled_events": true,
                        "notify_highlights": 2,
                        "version": 9,
                        "suppress_everyone": true,
                        "suppress_roles": true,
                        "channel_overrides": [{
                            "channel_id": "20",
                            "message_notifications": 0,
                            "muted": true,
                            "collapsed": true,
                            "flags": 5120,
                            "mute_config": {
                                "end_time": "2099-01-01T00:00:00.000Z",
                                "selected_time_window": 900
                            }
                        }]
                    }]
                }
            }
        })
        .to_string(),
    );

    let settings = events
        .iter()
        .find_map(|event| match event {
            AppEvent::UserGuildSettingsSync { settings, .. } => Some(settings),
            _ => None,
        })
        .expect("READY should emit user guild settings");
    assert_eq!(settings.len(), 1);
    let notification_settings = &settings[0].notification_settings;
    assert_eq!(notification_settings.guild_id, Some(Id::new(10)));
    assert!(notification_settings.muted);
    assert_eq!(
        notification_settings.mute_end_time.as_deref(),
        Some("2099-02-01T00:00:00.000Z")
    );
    assert_eq!(notification_settings.selected_time_window, Some(3600));
    assert_eq!(
        notification_settings.message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
    assert!(notification_settings.suppress_everyone);
    assert!(notification_settings.suppress_roles);
    assert_eq!(notification_settings.flags, 16384);
    assert!(notification_settings.hide_muted_channels);
    assert!(!notification_settings.mobile_push);
    assert!(notification_settings.mute_scheduled_events);
    assert_eq!(notification_settings.notify_highlights, 2);
    assert_eq!(notification_settings.version, 9);
    assert_eq!(notification_settings.channel_overrides.len(), 1);
    assert_eq!(
        notification_settings.channel_overrides[0].channel_id,
        Id::new(20)
    );
    assert_eq!(
        notification_settings.channel_overrides[0].message_notifications,
        Some(NotificationLevel::AllMessages)
    );
    assert!(notification_settings.channel_overrides[0].muted);
    assert_eq!(
        notification_settings.channel_overrides[0]
            .mute_end_time
            .as_deref(),
        Some("2099-01-01T00:00:00.000Z")
    );
    assert_eq!(
        notification_settings.channel_overrides[0].selected_time_window,
        Some(900)
    );
    assert!(notification_settings.channel_overrides[0].collapsed);
    assert_eq!(notification_settings.channel_overrides[0].flags, 5120);
    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::UserNotificationSettingsUpdate { flags } if *flags == 32
    )));

    let update = parse_user_account_event(
        &json!({
            "t": "NOTIFICATION_SETTINGS_UPDATE",
            "d": { "flags": 64 }
        })
        .to_string(),
    );
    assert!(matches!(
        update.as_slice(),
        [AppEvent::UserNotificationSettingsUpdate { flags }] if *flags == 64
    ));
}

#[test]
fn user_guild_settings_update_emits_single_update_event() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_GUILD_SETTINGS_UPDATE",
            "d": {
                "guild_id": "10",
                "message_notifications": 2,
                "muted": true,
                "mute_config": {
                    "end_time": "2099-01-01T00:00:00.000Z",
                    "selected_time_window": 3600
                },
                "channel_overrides": [],
                "version": 11
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::UserGuildSettingsUpdate { settings }] => {
            let notification_settings = &settings.notification_settings;
            assert_eq!(notification_settings.guild_id, Some(Id::new(10)));
            assert_eq!(
                notification_settings.message_notifications,
                Some(NotificationLevel::NoMessages)
            );
            assert!(notification_settings.muted);
            assert_eq!(
                notification_settings.mute_end_time.as_deref(),
                Some("2099-01-01T00:00:00.000Z")
            );
            assert_eq!(notification_settings.selected_time_window, Some(3600));
            assert_eq!(notification_settings.version, 11);
        }
        other => panic!("expected one UserGuildSettingsUpdate, got {other:?}"),
    }

    let missing_version = json!({
        "t": "USER_GUILD_SETTINGS_UPDATE",
        "d": {
            "guild_id": "10",
            "message_notifications": 2,
            "channel_overrides": []
        }
    });
    assert!(parse_user_account_event(&missing_version.to_string()).is_empty());
}

#[test]
fn user_settings_update_emits_guild_folder_order() {
    let events = parse_user_account_event(
        &json!({
            "t": "USER_SETTINGS_UPDATE",
            "d": {
                "activity_restricted_guild_ids": ["40"],
                "custom_status": {
                    "text": "working",
                    "emoji_id": "50",
                    "expires_at": null
                },
                "friend_source_flags": {
                    "all": true,
                    "mutual_friends": false,
                    "mutual_guilds": true
                },
                "guild_folders": [
                    {
                        "id": null,
                        "name": null,
                        "color": null,
                        "guild_ids": ["20"]
                    },
                    {
                        "id": 42,
                        "name": "work",
                        "color": 16711680,
                        "guild_ids": ["10", "30"]
                    }
                ],
                "status": "online",
                "theme": "dark",
                "future_setting": { "preserved": true }
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::UserSettingsUpdate { settings }] => {
            assert_eq!(
                settings.activity_restricted_guild_ids,
                Some(vec![Id::new(40)])
            );
            assert_eq!(settings.status.as_deref(), Some("online"));
            assert_eq!(settings.theme.as_deref(), Some("dark"));
            assert_eq!(
                settings
                    .custom_status
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|status| status.text.as_deref()),
                Some("working")
            );
            assert_eq!(
                settings
                    .custom_status
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|status| status.emoji_id),
                Some(Id::new(50))
            );
            assert_eq!(
                settings
                    .friend_source_flags
                    .as_ref()
                    .and_then(|flags| flags.all),
                Some(true)
            );
            assert!(settings.extra_fields.contains_key("future_setting"));
            let folders = settings
                .guild_folders
                .as_ref()
                .expect("user settings update should keep guild folders");
            assert_eq!(folders.len(), 2);
            assert_eq!(folders[0].id, None);
            assert_eq!(folders[0].guild_ids, vec![Id::new(20)]);
            assert_eq!(folders[1].id, Some(42));
            assert_eq!(folders[1].name.as_deref(), Some("work"));
            assert_eq!(folders[1].color, Some(16_711_680));
            assert_eq!(folders[1].guild_ids, vec![Id::new(10), Id::new(30)]);
        }
        other => panic!("expected one UserSettingsUpdate, got {other:?}"),
    }
}

#[test]
fn ready_payload_parses_private_channel_notification_settings() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "user_guild_settings": {
                    "entries": [{
                        "guild_id": null,
                        "message_notifications": 1,
                        "channel_overrides": {
                            "20": {
                                "message_notifications": 2,
                                "muted": true,
                                "mute_config": {
                                    "end_time": null,
                                    "selected_time_window": -1
                                }
                            }
                        }
                    }]
                }
            }
        })
        .to_string(),
    );

    let settings = events
        .iter()
        .find_map(|event| match event {
            AppEvent::UserGuildSettingsSync { settings, .. } => Some(settings),
            _ => None,
        })
        .expect("READY should emit private channel guild settings");
    assert_eq!(settings.len(), 1);
    let notification_settings = &settings[0].notification_settings;
    assert_eq!(notification_settings.guild_id, None);
    assert_eq!(
        notification_settings.message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
    assert_eq!(notification_settings.channel_overrides.len(), 1);
    assert_eq!(
        notification_settings.channel_overrides[0].channel_id,
        Id::new(20)
    );
    assert_eq!(
        notification_settings.channel_overrides[0].message_notifications,
        Some(NotificationLevel::NoMessages)
    );
    assert!(notification_settings.channel_overrides[0].muted);
    assert_eq!(
        notification_settings.channel_overrides[0].selected_time_window,
        Some(-1)
    );
}
