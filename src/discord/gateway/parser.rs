use std::time::Instant;

use serde_json::Value;

use crate::{discord::events::AppEvent, logging};

mod channels;
mod guilds;
mod members;
mod messages;
mod presence;
mod ready;
mod relationships;
mod shared;

pub(crate) use channels::parse_channel_info;
use channels::{parse_channel_delete, parse_channel_upsert, parse_thread_list_sync};
use guilds::{
    parse_guild_create, parse_guild_delete, parse_guild_emojis_update, parse_guild_update,
    parse_user_guild_settings_update,
};
use members::{
    parse_member_add, parse_member_chunk, parse_member_list_update, parse_member_remove,
    parse_member_upsert,
};
pub(crate) use messages::parse_message_info;
use messages::{
    parse_message_ack, parse_message_create, parse_message_delete, parse_message_reaction_add,
    parse_message_reaction_remove, parse_message_reaction_remove_all,
    parse_message_reaction_remove_emoji, parse_message_update,
};
use presence::{parse_presence_update, parse_typing_start};
use ready::{parse_ready, parse_ready_supplemental};
use relationships::{parse_relationship_add, parse_relationship_remove, parse_relationship_update};

/// Best-effort fallback that rebuilds the dashboard's domain events directly
/// from the raw gateway payload. We only extract the fields the UI consumes,
/// and skip anything we can't model. Returns an iterable so a single payload
/// (e.g. `GUILD_CREATE`) can produce multiple downstream events.
pub(super) fn parse_user_account_event(raw: &str) -> Vec<AppEvent> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(event_type) = value.get("t").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(data) = value.get("d") else {
        return Vec::new();
    };

    match event_type {
        "READY" => parse_ready(data),
        "READY_SUPPLEMENTAL" => parse_ready_supplemental(data),
        "GUILD_CREATE" => {
            let started = Instant::now();
            let result = parse_guild_create(data).into_iter().collect();
            logging::timing("gateway", "fallback guild_create parse", started.elapsed());
            result
        }
        "GUILD_UPDATE" => parse_guild_update(data).into_iter().collect(),
        "GUILD_EMOJIS_UPDATE" => parse_guild_emojis_update(data).into_iter().collect(),
        "GUILD_DELETE" => parse_guild_delete(data).into_iter().collect(),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            parse_channel_upsert(data).into_iter().collect()
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => parse_channel_delete(data).into_iter().collect(),
        "THREAD_LIST_SYNC" => parse_thread_list_sync(data),
        "MESSAGE_CREATE" => parse_message_create(data).into_iter().collect(),
        "MESSAGE_UPDATE" => parse_message_update(data).into_iter().collect(),
        "MESSAGE_DELETE" => parse_message_delete(data).into_iter().collect(),
        "MESSAGE_REACTION_ADD" => parse_message_reaction_add(data).into_iter().collect(),
        "MESSAGE_REACTION_REMOVE" => parse_message_reaction_remove(data).into_iter().collect(),
        "MESSAGE_REACTION_REMOVE_ALL" => parse_message_reaction_remove_all(data)
            .into_iter()
            .collect(),
        "MESSAGE_REACTION_REMOVE_EMOJI" => parse_message_reaction_remove_emoji(data)
            .into_iter()
            .collect(),
        "MESSAGE_ACK" => parse_message_ack(data).into_iter().collect(),
        "USER_GUILD_SETTINGS_UPDATE" => {
            parse_user_guild_settings_update(data).into_iter().collect()
        }
        "GUILD_MEMBER_ADD" => parse_member_add(data).into_iter().collect(),
        "GUILD_MEMBER_UPDATE" => parse_member_upsert(data).into_iter().collect(),
        "GUILD_MEMBER_LIST_UPDATE" => parse_member_list_update(data),
        "GUILD_MEMBERS_CHUNK" => parse_member_chunk(data),
        "RELATIONSHIP_ADD" => parse_relationship_add(data).into_iter().collect(),
        "RELATIONSHIP_UPDATE" => parse_relationship_update(data).into_iter().collect(),
        "RELATIONSHIP_REMOVE" => parse_relationship_remove(data).into_iter().collect(),
        "GUILD_MEMBER_REMOVE" => parse_member_remove(data).into_iter().collect(),
        "PRESENCE_UPDATE" => parse_presence_update(data),
        "TYPING_START" => parse_typing_start(data).into_iter().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;
    use serde_json::json;

    use super::{
        parse_channel_info, parse_guild_create, parse_guild_emojis_update, parse_guild_update,
        parse_message_create, parse_message_info, parse_message_update, parse_user_account_event,
    };
    use crate::discord::{
        ActivityKind, AppEvent, AttachmentUpdate, ChannelVisibilityStats, DiscordState,
        FriendStatus, MentionInfo, MessageKind, NotificationLevel, PollAnswerInfo, PollInfo,
        PresenceStatus, ReactionEmoji, ReplyInfo,
    };

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
                                    "global_name": "Alice"
                                },
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

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10)
                    && member.user_id == Id::new(20)
                    && member.display_name == "Alice Nick"
                    && member.role_ids == vec![Id::new(30)]
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status, .. }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Idle
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

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10) && member.user_id == Id::new(20)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(10) && member.user_id == Id::new(21)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { user_id, status, .. }
                if *user_id == Id::new(21) && *status == PresenceStatus::Idle
        )));
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

        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status, .. }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Online
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::PresenceUpdate { guild_id, user_id, status, .. }
                if *guild_id == Id::new(10)
                    && *user_id == Id::new(30)
                    && *status == PresenceStatus::DoNotDisturb
        )));
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
            AppEvent::UserPresenceUpdate { user_id, status, .. }
                if *user_id == Id::new(99) && *status == PresenceStatus::Idle
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
                can_use_animated_custom_emojis: false
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
            AppEvent::UserPresenceUpdate { user_id, status, .. }
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status, .. }
                if *user_id == Id::new(30) && *status == PresenceStatus::Idle
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

        let activities = events
            .iter()
            .find_map(|event| match event {
                AppEvent::PresenceUpdate { activities, .. } => Some(activities),
                _ => None,
            })
            .expect("PRESENCE_UPDATE should produce a PresenceUpdate event");

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
                AppEvent::UserPresenceUpdate { activities, .. } => Some(activities),
                _ => None,
            })
            .expect("PRESENCE_UPDATE without guild_id should produce a UserPresenceUpdate");

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
            [AppEvent::UserPresenceUpdate { user_id, status, .. }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
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
            [AppEvent::UserPresenceUpdate { user_id, status, .. }]
                if *user_id == Id::new(20) && *status == PresenceStatus::DoNotDisturb
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
            [AppEvent::UserPresenceUpdate { user_id, status, .. }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        ));
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
        assert_eq!(channel.thread_archived, Some(true));
        assert_eq!(channel.thread_locked, Some(false));
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
                    && channel.thread_archived == Some(false)
                    && channel.thread_locked == Some(false)
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

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(10) && channel.name == "release notes"
        ));
        assert!(matches!(
            &events[1],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(11) && channel.name == "bug reports"
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
            let AppEvent::MessageUpdate { attachments, .. } = event else {
                panic!("expected message update event");
            };
            if clears_attachments {
                assert!(
                    matches!(attachments, AttachmentUpdate::Replace(values) if values.is_empty())
                );
            } else {
                assert!(matches!(attachments, AttachmentUpdate::Unchanged));
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

        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.display_name == "Alice Nick"
                    && !member.is_bot
        ));
        assert!(matches!(
            &events[1],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(20)
                    && member.display_name == "bob"
                    && member.is_bot
        ));
        assert!(matches!(
            &events[2],
            AppEvent::PresenceUpdate { guild_id, user_id, status, .. }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(10)
                    && *status == PresenceStatus::Online
        ));
        assert!(matches!(
            &events[3],
            AppEvent::PresenceUpdate { guild_id, user_id, status, .. }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Idle
        ));
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

        let AppEvent::MessageUpdate { mentions, .. } = event else {
            panic!("expected message update event");
        };
        assert_eq!(mentions, Some(vec![mention_info(40, "alice")]));
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

        let AppEvent::MessageUpdate { poll, .. } = event else {
            panic!("expected message update event");
        };
        let poll = poll.expect("poll payload should be kept");
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].vote_count, Some(5));
        assert!(poll.answers[0].me_voted);
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

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "cat.png");
        assert_eq!(attachments[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(attachments[0].width, Some(640));
        assert_eq!(attachments[0].height, Some(480));
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

        let AppEvent::MessageCreate { embeds, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0].color, Some(16711680));
        assert_eq!(embeds[0].provider_name.as_deref(), Some("YouTube"));
        assert_eq!(embeds[0].title.as_deref(), Some("Example Video"));
        assert_eq!(
            embeds[0].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        assert_eq!(
            embeds[0].thumbnail_proxy_url.as_deref(),
            Some(
                "https://images-ext-1.discordapp.net/external/thumb/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
            )
        );
        assert_eq!(embeds[0].thumbnail_width, Some(480));
        assert_eq!(embeds[0].thumbnail_height, Some(360));
        assert_eq!(
            embeds[0].image_url.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg")
        );
        assert_eq!(
            embeds[0].image_proxy_url.as_deref(),
            Some(
                "https://images-ext-2.discordapp.net/external/image/hash/https/i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg"
            )
        );
        assert_eq!(embeds[0].image_width, Some(1280));
        assert_eq!(embeds[0].image_height, Some(720));
        assert_eq!(
            embeds[0].video_url.as_deref(),
            Some("https://www.youtube.com/embed/dQw4w9WgXcQ")
        );
    }

    #[test]
    fn message_create_parser_keeps_message_type() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { message_kind, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message_kind, MessageKind::new(19));
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

        let AppEvent::MessageCreate { author, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(author, "server alias");
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

        let AppEvent::MessageCreate {
            author_avatar_url, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(
            author_avatar_url.as_deref(),
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

        let AppEvent::MessageCreate {
            author, guild_id, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(guild_id, None);
        assert_eq!(author, "global alias");
    }

    #[test]
    fn message_create_parser_keeps_mention_display_names() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "hello <@40> <@41> <@42>",
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

        let AppEvent::MessageCreate { mentions, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            mentions,
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

        let AppEvent::MessageCreate { mentions, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(mentions, vec![mention_info(40, "alpha")]);
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

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply,
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

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply.and_then(|reply| reply.mentions.into_iter().next()),
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

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll,
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

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll.expect("poll result should map to poll info")
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

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].url, "https://media.discordapp.net/cat.png");
        assert_eq!(
            attachments[0].proxy_url,
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

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "clip.mp4");
        assert_eq!(attachments[0].content_type.as_deref(), Some("video/mp4"));
        assert_eq!(attachments[0].size, 78_364_758);
        assert_eq!(attachments[0].width, Some(1920));
        assert_eq!(attachments[0].height, Some(1080));
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
            let AppEvent::MessageCreate {
                content,
                sticker_names,
                ..
            } = event
            else {
                panic!("expected message create event");
            };
            assert_eq!(content.as_deref(), Some(raw_content));
            assert_eq!(
                sticker_names,
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

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 2);
        assert_eq!(
            forwarded_snapshots[0].content.as_deref(),
            Some("hello <@40>")
        );
        assert_eq!(forwarded_snapshots[0].source_channel_id, Some(Id::new(11)));
        assert_eq!(
            forwarded_snapshots[0].timestamp.as_deref(),
            Some("2026-04-30T12:34:56.000000+00:00")
        );
        assert_eq!(
            forwarded_snapshots[0].mentions,
            vec![mention_info(40, "alice")]
        );
        assert_eq!(
            forwarded_snapshots[0].sticker_names,
            vec!["Wave".to_owned()]
        );
        assert_eq!(forwarded_snapshots[0].attachments.len(), 1);
        assert_eq!(forwarded_snapshots[0].attachments[0].filename, "cat.png");
        assert_eq!(forwarded_snapshots[1].content.as_deref(), Some(""));
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: None,
            display_name: display_name.to_owned(),
        }
    }

    fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: Some(nick.to_owned()),
            display_name: nick.to_owned(),
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
            [AppEvent::TypingStart { channel_id, user_id }]
                if *channel_id == Id::new(12345) && *user_id == Id::new(99)
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
                        "user": { "id": "42" }
                    },
                    "timestamp": 1_700_000_000
                }
            })
            .to_string(),
        );
        assert!(matches!(
            events.as_slice(),
            [AppEvent::TypingStart { channel_id, user_id }]
                if *channel_id == Id::new(55) && *user_id == Id::new(42)
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
                AppEvent::UserGuildNotificationSettingsInit { settings } => Some(settings),
                _ => None,
            })
            .expect("READY should emit user guild notification settings");
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].guild_id, Some(Id::new(10)));
        assert_eq!(
            settings[0].message_notifications,
            Some(NotificationLevel::OnlyMentions)
        );
        assert!(settings[0].suppress_everyone);
        assert!(settings[0].suppress_roles);
        assert_eq!(settings[0].channel_overrides.len(), 1);
        assert_eq!(settings[0].channel_overrides[0].channel_id, Id::new(20));
        assert_eq!(
            settings[0].channel_overrides[0].message_notifications,
            Some(NotificationLevel::AllMessages)
        );
        assert!(settings[0].channel_overrides[0].muted);
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
            [AppEvent::UserGuildNotificationSettingsUpdate { settings }] => {
                assert_eq!(settings.guild_id, Some(Id::new(10)));
                assert_eq!(
                    settings.message_notifications,
                    Some(NotificationLevel::NoMessages)
                );
                assert!(settings.muted);
            }
            other => panic!("expected one UserGuildNotificationSettingsUpdate, got {other:?}"),
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
                AppEvent::UserGuildNotificationSettingsInit { settings } => Some(settings),
                _ => None,
            })
            .expect("READY should emit private channel notification settings");
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].guild_id, None);
        assert_eq!(
            settings[0].message_notifications,
            Some(NotificationLevel::OnlyMentions)
        );
        assert_eq!(settings[0].channel_overrides.len(), 1);
        assert_eq!(settings[0].channel_overrides[0].channel_id, Id::new(20));
        assert_eq!(
            settings[0].channel_overrides[0].message_notifications,
            Some(NotificationLevel::NoMessages)
        );
        assert!(settings[0].channel_overrides[0].muted);
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
}
