use crate::discord::ids::Id;
use serde_json::json;

use super::{
    parse_channel_info, parse_guild_create, parse_guild_emojis_update, parse_guild_update,
    parse_message_create, parse_message_info, parse_message_update, parse_user_account_dispatch,
    parse_user_account_event,
};
use crate::discord::{
    ActivityKind, AppEvent, AttachmentUpdate, ChannelVisibilityStats, DiscordState, FriendStatus,
    GuildOnboardingMode, GuildVerificationLevel, MentionInfo, MessageKind, NotificationLevel,
    PollAnswerInfo, PollInfo, PremiumTier, PresenceStatus, ReactionEmoji, ReplyInfo,
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
    assert_eq!(features, vec!["COMMUNITY", "FUTURE_FEATURE"]);

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
fn guild_onboarding_update_dispatch_preserves_payload() {
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
}

#[test]
fn ready_supplemental_refreshes_guild_onboarding() {
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
    assert_eq!(verification_level, GuildVerificationLevel::High);
    assert_eq!(mfa_level, 1);
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
fn ready_parser_keeps_current_user_verification_status() {
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
}

#[test]
fn user_update_refreshes_current_user_verification_status() {
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
fn raw_member_list_update_populates_members_and_presence() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "10",
                "ops": [{
                    "op": "SYNC",
                    "range": [0, 99],
                    "items": [{
                        "member": {
                            "user": {
                                "id": "20",
                                "username": "alice",
                                "global_name": "Alice",
                                "avatar": "global_hash"
                            },
                            "avatar": "guild_hash",
                            "nick": "Alice Nick",
                            "roles": ["30"],
                            "presence": { "status": "idle" }
                        }
                    }]
                }]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::GuildMemberListUpdate { update }] => {
            assert_eq!(update.guild_id, Id::new(10));
            assert_eq!(update.members[0].user_id, Id::new(20));
            assert_eq!(update.members[0].display_name, "Alice Nick");
            assert_eq!(
                update.members[0].avatar_url.as_deref(),
                Some("https://cdn.discordapp.com/guilds/10/users/20/avatars/guild_hash.png")
            );
            assert_eq!(update.members[0].role_ids, vec![Id::new(30)]);
            assert_eq!(update.presences[0].user_id, Id::new(20));
            assert_eq!(update.presences[0].status, PresenceStatus::Idle);
        }
        other => panic!("expected one GuildMemberListUpdate, got {other:?}"),
    }
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
fn raw_ready_supplemental_emits_voice_states_from_embedded_guilds() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{
                    "id": "10",
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
        AppEvent::VoiceStateUpdate { state }
            if state.guild_id == Some(Id::new(10))
                && state.channel_id == Some(Id::new(30))
                && state.user_id == Id::new(20)
    )));
}

#[test]
fn raw_member_list_update_processes_all_sync_ranges() {
    // Discord can ship more than one SYNC chunk in a single
    // GUILD_MEMBER_LIST_UPDATE, such as ranges [0,99] and [100,199]. We
    // need members from every chunk, not just the first.
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "10",
                "ops": [
                    {
                        "op": "SYNC",
                        "range": [0, 99],
                        "items": [{
                            "member": {
                                "user": { "id": "20", "username": "alice" },
                                "roles": [],
                                "presence": { "status": "online" }
                            }
                        }]
                    },
                    {
                        "op": "SYNC",
                        "range": [100, 199],
                        "items": [{
                            "member": {
                                "user": { "id": "21", "username": "bob" },
                                "roles": [],
                                "presence": { "status": "idle" }
                            }
                        }]
                    }
                ]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::GuildMemberListUpdate { update }] => {
            assert_eq!(update.guild_id, Id::new(10));
            assert!(
                update
                    .members
                    .iter()
                    .any(|member| member.user_id == Id::new(20))
            );
            assert!(
                update
                    .members
                    .iter()
                    .any(|member| member.user_id == Id::new(21))
            );
            assert!(update.presences.iter().any(|presence| {
                presence.user_id == Id::new(21) && presence.status == PresenceStatus::Idle
            }));
        }
        other => panic!("expected one GuildMemberListUpdate, got {other:?}"),
    }
}

#[test]
fn raw_member_list_update_handles_insert_and_update_items() {
    let events = parse_user_account_event(
        &json!({
            "t": "GUILD_MEMBER_LIST_UPDATE",
            "d": {
                "guild_id": "10",
                "ops": [
                    {
                        "op": "INSERT",
                        "item": {
                            "member": {
                                "user": {
                                    "id": "20",
                                    "username": "alice"
                                },
                                "roles": [],
                                "presence": { "status": "online" }
                            }
                        }
                    },
                    {
                        "op": "UPDATE",
                        "item": {
                            "member": {
                                "user": {
                                    "id": "30",
                                    "username": "bob"
                                },
                                "roles": [],
                                "presence": { "status": "dnd" }
                            }
                        }
                    }
                ]
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::GuildMemberListUpdate { update }] => {
            assert!(update.presences.iter().any(|presence| {
                presence.user_id == Id::new(20) && presence.status == PresenceStatus::Online
            }));
            assert!(update.presences.iter().any(|presence| {
                presence.user_id == Id::new(30) && presence.status == PresenceStatus::DoNotDisturb
            }));
        }
        other => panic!("expected one GuildMemberListUpdate, got {other:?}"),
    }
}

#[test]
fn relationship_add_emits_friend_upsert() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_ADD",
            "d": {
                "id": "20",
                "type": 1,
                "nickname": "Bestie",
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
    ));
}

#[test]
fn relationship_update_emits_friend_upsert() {
    let events = parse_user_account_event(
        &json!({
            "t": "RELATIONSHIP_UPDATE",
            "d": {
                "id": "20",
                "type": 1,
                "nickname": "Bestie"
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
                && relationship.display_name.is_none()
                && relationship.username.is_none()
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
        AppEvent::RelationshipRemove { user_id } if *user_id == Id::new(20)
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
fn raw_ready_parser_exposes_current_user_premium_capability() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo",
                    "premium_type": 0
                },
                "guilds": []
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserCapabilities {
            premium_tier: PremiumTier::None
        }
    )));
}

#[test]
fn raw_ready_parser_exposes_current_user_nitro_capabilities() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo",
                    "premium_type": 2
                },
                "guilds": []
            }
        })
        .to_string(),
    );

    assert!(events.iter().any(|event| matches!(
        event,
        AppEvent::CurrentUserCapabilities {
            premium_tier: PremiumTier::Nitro
        }
    )));
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
fn raw_ready_parser_applies_top_level_presence_to_dm_recipient() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "guilds": [],
                "presences": [{
                    "user": { "id": "20" },
                    "status": "online"
                }],
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

    assert_eq!(recipients[0].user_id, Id::new(20));
    assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
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
fn raw_ready_supplemental_updates_merged_member_roles() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY_SUPPLEMENTAL",
            "d": {
                "guilds": [{ "id": "1" }],
                "merged_members": [[{
                    "user_id": "10",
                    "roles": ["20"]
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
    )));
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
fn raw_presence_update_without_guild_updates_user_presence() {
    let events = parse_user_account_event(
        &json!({
            "t": "PRESENCE_UPDATE",
            "d": {
                "user": { "id": "20" },
                "status": "dnd"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::PresenceUpdate { guild_id: None, presence }]
            if presence.user_id == Id::new(20) && presence.status == PresenceStatus::DoNotDisturb
    ));
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
                    "type": 0,
                    "name": "Concord",
                    "application_id": "12345",
                    "timestamps": { "start": 1_700_000_000_000i64 },
                    "assets": { "large_image": "cover", "large_text": "Main menu" },
                    "party": { "id": "party-1", "size": [2, 5] },
                    "buttons": ["Join"],
                    "metadata": { "button_urls": ["https://example.com/join"] }
                }]
            }
        })
        .to_string(),
    );

    let [AppEvent::PresenceUpdate { presence, .. }] = events.as_slice() else {
        panic!("expected a single presence update, got {events:?}");
    };
    let activity = &presence.activities[0];
    assert_eq!(
        activity.timestamps.and_then(|t| t.start),
        Some(1_700_000_000_000)
    );
    let assets = activity.assets.as_ref().expect("assets parsed");
    assert_eq!(assets.large_image.as_deref(), Some("cover"));
    assert_eq!(assets.large_text.as_deref(), Some("Main menu"));
    let party = activity.party.as_ref().expect("party parsed");
    assert_eq!(party.size, Some((2, 5)));
    assert_eq!(activity.buttons.len(), 1);
    assert_eq!(activity.buttons[0].label, "Join");
    assert_eq!(activity.buttons[0].url, "https://example.com/join");
}

#[test]
fn thread_channel_parser_keeps_counts_and_status() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": "release notes",
            "message_count": 12,
            "total_message_sent": 14,
            "thread_metadata": { "archived": true, "locked": false }
        }),
        None,
    )
    .expect("thread channel should parse");

    assert_eq!(channel.kind, "GuildPublicThread");
    assert_eq!(channel.message_count, Some(12));
    assert_eq!(channel.total_message_sent, Some(14));
    assert_eq!(channel.thread_archived(), Some(true));
    assert_eq!(channel.thread_locked(), Some(false));
}

#[test]
fn thread_channel_parser_marks_current_user_joined_when_member_is_present() {
    let channel = parse_channel_info(
        &json!({
            "id": "10",
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": "release notes",
            "member": { "id": "10", "user_id": "99" },
            "thread_metadata": { "archived": false, "locked": false }
        }),
        None,
    )
    .expect("thread channel should parse");

    assert_eq!(channel.current_user_joined_thread, Some(true));
}

#[test]
fn raw_thread_members_update_carries_member_delta_ids() {
    let joined = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBERS_UPDATE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "added_members": [{ "user_id": "99" }]
            }
        })
        .to_string(),
    );
    let left = parse_user_account_event(
        &json!({
            "t": "THREAD_MEMBERS_UPDATE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "removed_member_ids": ["99"]
            }
        })
        .to_string(),
    );

    assert!(matches!(
        joined.as_slice(),
        [AppEvent::ThreadMembersUpdateDispatch { update }]
            if update.channel_id == Id::new(10)
                && update.guild_id == Some(Id::new(1))
                && update.added_user_ids == vec![Id::new(99)]
                && update.removed_user_ids.is_empty()
    ));
    assert!(matches!(
        left.as_slice(),
        [AppEvent::ThreadMembersUpdateDispatch { update }]
            if update.channel_id == Id::new(10)
                && update.added_user_ids.is_empty()
                && update.removed_user_ids == vec![Id::new(99)]
    ));
}

#[test]
fn raw_thread_create_upserts_thread_channel() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_CREATE",
            "d": thread_payload(10, "release notes")
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelUpsert(channel)]
            if channel.channel_id == Id::new(10)
                && channel.guild_id == Some(Id::new(1))
                && channel.parent_id == Some(Id::new(2))
                && channel.name == "release notes"
                && channel.kind == "GuildPublicThread"
                && channel.message_count == Some(12)
                && channel.total_message_sent == Some(14)
                && channel.thread_archived() == Some(false)
                && channel.thread_locked() == Some(false)
    ));
}

#[test]
fn raw_thread_update_upserts_thread_channel() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_UPDATE",
            "d": thread_payload(10, "renamed thread")
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelUpsert(channel)]
            if channel.channel_id == Id::new(10)
                && channel.name == "renamed thread"
                && channel.kind == "GuildPublicThread"
    ));
}

#[test]
fn raw_thread_delete_removes_thread_channel() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_DELETE",
            "d": {
                "id": "10",
                "guild_id": "1",
                "parent_id": "2",
                "type": 11
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelDelete { guild_id, channel_id }]
            if *guild_id == Some(Id::new(1)) && *channel_id == Id::new(10)
    ));
}

#[test]
fn raw_thread_list_sync_upserts_all_threads() {
    let events = parse_user_account_event(
        &json!({
            "t": "THREAD_LIST_SYNC",
            "d": {
                "guild_id": "1",
                "channel_ids": ["2"],
                "threads": [
                    thread_payload(10, "release notes"),
                    thread_payload(11, "bug reports")
                ],
                "members": []
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [AppEvent::ThreadListSync { sync }] => {
            assert_eq!(sync.guild_id, Some(Id::new(1)));
            assert_eq!(sync.channel_ids, vec![Id::new(2)]);
            assert_eq!(sync.threads.len(), 2);
            assert_eq!(sync.threads[0].channel_id, Id::new(10));
            assert_eq!(sync.threads[0].name, "release notes");
            assert_eq!(sync.threads[1].channel_id, Id::new(11));
            assert_eq!(sync.threads[1].name, "bug reports");
        }
        other => panic!("expected one ThreadListSync, got {other:?}"),
    }
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

    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].id, Id::new(90));
    assert_eq!(roles[0].name, "Admin");
    assert_eq!(roles[0].color, Some(16755200));
    assert_eq!(roles[0].position, 10);
    assert!(roles[0].hoist);
}

#[test]
fn guild_create_parser_keeps_string_permission_bitfields() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "members": [],
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

    let AppEvent::GuildCreate { roles, .. } = event else {
        panic!("expected guild create event");
    };

    assert_eq!(roles[0].permissions, 0x400);
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
    let events = parse_user_account_event(
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
        events.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if *guild_id == Some(Id::new(1))
                && *channel_id == Id::new(10)
                && last_pin_timestamp.as_deref() == Some("2026-05-25T12:34:56.000000+00:00")
    ));
}

#[test]
fn raw_channel_pins_update_accepts_missing_timestamp() {
    let events = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": {
                "channel_id": "10"
            }
        })
        .to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AppEvent::ChannelPinsUpdate { guild_id, channel_id, last_pin_timestamp }]
            if guild_id.is_none() && *channel_id == Id::new(10) && last_pin_timestamp.is_none()
    ));
}

#[test]
fn raw_channel_pins_update_skips_missing_channel_id() {
    let events = parse_user_account_event(
        &json!({
            "t": "CHANNEL_PINS_UPDATE",
            "d": {
                "guild_id": "1",
                "last_pin_timestamp": null
            }
        })
        .to_string(),
    );

    assert!(events.is_empty());
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
fn guild_create_parser_keeps_active_threads() {
    let event = parse_guild_create(&json!({
        "id": "1",
        "name": "guild",
        "channels": [],
        "threads": [thread_payload(10, "release notes")],
        "members": [],
        "presences": [],
        "emojis": []
    }))
    .expect("guild create should parse");

    let AppEvent::GuildCreate { channels, .. } = event else {
        panic!("expected guild create event");
    };

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_id, Id::new(10));
    assert_eq!(channels[0].kind, "GuildPublicThread");
    assert_eq!(channels[0].name, "release notes");
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
fn raw_ready_parser_keeps_guild_custom_emojis() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": {
                    "id": "99",
                    "username": "neo"
                },
                "guilds": [{
                    "id": "1",
                    "name": "guild",
                    "channels": [],
                    "members": [],
                    "presences": [],
                    "emojis": [{
                        "id": "50",
                        "name": "party_time",
                        "animated": true,
                        "available": true
                    }]
                }],
                "private_channels": []
            }
        })
        .to_string(),
    );

    let guild_create = events
        .iter()
        .find_map(|event| match event {
            AppEvent::GuildCreate { emojis, .. } => Some(emojis),
            _ => None,
        })
        .expect("ready should emit a guild create event");

    assert_eq!(guild_create.len(), 1);
    assert_eq!(guild_create[0].id, Id::new(50));
    assert_eq!(guild_create[0].name, "party_time");
    assert!(guild_create[0].animated);
    assert!(guild_create[0].available);
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
fn guild_update_parser_keeps_custom_emojis_when_present() {
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
}

#[test]
fn guild_update_parser_distinguishes_missing_custom_emojis() {
    let event = parse_guild_update(&json!({
        "id": "1",
        "name": "guild renamed"
    }))
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
fn message_create_parser_keeps_image_attachments() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "",
        "attachments": [{
            "id": "40",
            "filename": "cat.png",
            "url": "https://cdn.discordapp.com/cat.png",
            "proxy_url": "https://media.discordapp.net/cat.png",
            "content_type": "image/png",
            "size": 2048,
            "width": 640,
            "height": 480,
            "description": "cat"
        }]
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.attachments.len(), 1);
    assert_eq!(message.attachments[0].filename, "cat.png");
    assert_eq!(
        message.attachments[0].content_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(message.attachments[0].width, Some(640));
    assert_eq!(message.attachments[0].height, Some(480));
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
fn message_create_parser_prefers_member_nick_for_author() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "guild_id": "1",
        "author": { "id": "30", "global_name": "global", "username": "neo" },
        "member": { "nick": "server alias" },
        "content": "hello",
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.author, "server alias");
}

#[test]
fn message_info_parser_keeps_author_role_ids_from_member_payload() {
    let message = parse_message_info(&json!({
        "id": "20",
        "channel_id": "10",
        "guild_id": "1",
        "author": { "id": "30", "username": "neo" },
        "member": { "roles": ["90", "91"] },
        "content": "hello",
        "attachments": []
    }))
    .expect("message should parse");

    assert_eq!(message.author_role_ids, vec![Id::new(90), Id::new(91)]);
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
fn message_create_parser_falls_back_to_global_name_without_member() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "global_name": "global alias", "username": "neo" },
        "content": "hello",
        "attachments": []
    }))
    .expect("message create should parse");

    let AppEvent::MessageCreate { message } = event else {
        panic!("expected message create event");
    };
    assert_eq!(message.guild_id, None);
    assert_eq!(message.author, "global alias");
}

#[test]
fn message_create_parser_keeps_mention_display_names() {
    let event = parse_message_create(&json!({
        "id": "20",
        "channel_id": "10",
        "author": { "id": "30", "username": "neo" },
        "content": "hello <@40> <@41> <@42>",
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
            }
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
            sticker_names: Vec::new(),
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
fn message_create_parser_preserves_content_and_sticker_names() {
    let cases = [
        (
            "",
            vec![json!({ "id": "11", "name": "Wave", "format_type": 1 })],
            vec!["Wave"],
        ),
        (
            "hello",
            vec![
                json!({ "id": "11", "name": "Wave", "format_type": 1 }),
                json!({ "id": "12", "name": "Heart", "format_type": 1 }),
            ],
            vec!["Wave", "Heart"],
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
        assert_eq!(
            message.sticker_names,
            expected_stickers
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
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
        message.forwarded_snapshots[0].sticker_names,
        vec!["Wave".to_owned()]
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
fn parse_guild_create_reads_name_from_lazy_properties_object() {
    // With user-account capabilities containing LAZY_USER_NOTIFICATIONS,
    // Discord nests guild metadata under `properties` instead of placing
    // `name` / `owner_id` at the root. Concord must look in both places
    // or every guild renders as "unknown".
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
        [AppEvent::TypingStart { channel_id, user_id, display_name }]
            if *channel_id == Id::new(12345)
                && *user_id == Id::new(99)
                && display_name.is_none()
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
                    "user": {
                        "id": "42",
                        "username": "typing-user",
                        "global_name": "Typing Global"
                    }
                },
                "timestamp": 1_700_000_000
            }
        })
        .to_string(),
    );
    assert!(matches!(
        events.as_slice(),
        [AppEvent::TypingStart { channel_id, user_id, display_name }]
            if *channel_id == Id::new(55)
                && *user_id == Id::new(42)
                && display_name.as_deref() == Some("Live Nick")
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
}

#[test]
fn message_ack_carries_channel_message_and_mention_count() {
    let events = parse_user_account_event(
        &json!({
            "t": "MESSAGE_ACK",
            "d": {
                "channel_id": "42",
                "message_id": "99",
                "mention_count": 2,
            }
        })
        .to_string(),
    );

    match events.as_slice() {
        [
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
            },
        ] => {
            assert_eq!(*channel_id, Id::new(42));
            assert_eq!(*message_id, Id::new(99));
            assert_eq!(*mention_count, 2);
        }
        other => panic!("expected one MessageAck, got {other:?}"),
    }
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
fn ready_payload_emits_read_state_init_with_ack_pointers() {
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
                        { "id": "11", "last_message_id": "20", "mention_count": 0 },
                        { "id": "12", "last_message_id": "30", "mention_count": 4 },
                    ]
                }
            }
        })
        .to_string(),
    );

    let entries = events
        .iter()
        .find_map(|event| match event {
            AppEvent::ReadStateInit { entries } => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a ReadStateInit");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, Some(Id::new(20)));
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].mention_count, 4);
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
            AppEvent::ReadStateInit { entries } => Some(entries.clone()),
            _ => None,
        })
        .expect("READY should emit a ReadStateInit");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].channel_id, Id::new(11));
    assert_eq!(entries[0].last_acked_message_id, None);
    assert_eq!(entries[0].mention_count, 0);
    assert_eq!(entries[1].channel_id, Id::new(12));
    assert_eq!(entries[1].last_acked_message_id, None);
    assert_eq!(entries[1].mention_count, 1);
}

#[test]
fn ready_payload_emits_user_guild_notification_settings() {
    let events = parse_user_account_event(
        &json!({
            "t": "READY",
            "d": {
                "user": { "id": "1", "username": "neo" },
                "guilds": [],
                "user_guild_settings": {
                    "entries": [{
                        "guild_id": "10",
                        "message_notifications": 1,
                        "muted": false,
                        "suppress_everyone": true,
                        "suppress_roles": true,
                        "channel_overrides": [{
                            "channel_id": "20",
                            "message_notifications": 0,
                            "muted": true,
                            "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" }
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
            AppEvent::UserGuildSettingsInit { settings } => Some(settings),
            _ => None,
        })
        .expect("READY should emit user guild settings");
    assert_eq!(settings.len(), 1);
    let notification_settings = &settings[0].notification_settings;
    assert_eq!(notification_settings.guild_id, Some(Id::new(10)));
    assert_eq!(
        notification_settings.message_notifications,
        Some(NotificationLevel::OnlyMentions)
    );
    assert!(notification_settings.suppress_everyone);
    assert!(notification_settings.suppress_roles);
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
                "mute_config": { "end_time": "2099-01-01T00:00:00.000Z" },
                "channel_overrides": []
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
        }
        other => panic!("expected one UserGuildSettingsUpdate, got {other:?}"),
    }
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
                                "mute_config": null
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
            AppEvent::UserGuildSettingsInit { settings } => Some(settings),
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
}

#[test]
fn parse_guild_update_reads_name_from_lazy_properties_object() {
    let event = parse_guild_update(&json!({
        "id": "100",
        "properties": {
            "name": "Renamed Lazy",
            "owner_id": "9",
        },
    }))
    .expect("guild_update payload should map");

    let AppEvent::GuildUpdate {
        guild_id,
        name,
        owner_id,
        ..
    } = event
    else {
        panic!("expected GuildUpdate event");
    };
    assert_eq!(guild_id, Id::new(100));
    assert_eq!(name, "Renamed Lazy");
    assert_eq!(owner_id, Some(Id::new(9)));
}
