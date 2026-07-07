use serde_json::Value;

use crate::discord::events::{AppEvent, GatewayDispatchInfo};

mod channels;
mod guilds;
mod members;
mod messages;
mod presence;
mod ready;
mod relationships;
mod shared;
mod user_settings;
mod voice;

pub(crate) use channels::parse_channel_info;
use channels::{
    parse_channel_delete, parse_channel_upsert, parse_thread_list_sync, parse_thread_members_update,
};
use guilds::{
    parse_guild_create, parse_guild_delete, parse_guild_emojis_update, parse_guild_role_delete,
    parse_guild_role_upsert, parse_guild_update, parse_user_guild_settings_update,
};
use members::{
    parse_member_add, parse_member_chunk, parse_member_list_update, parse_member_remove,
    parse_member_upsert, parse_user_update,
};
pub(crate) use messages::parse_message_info;
use messages::{
    parse_channel_pins_update, parse_message_ack, parse_message_create, parse_message_delete,
    parse_message_delete_bulk, parse_message_reaction_add, parse_message_reaction_remove,
    parse_message_reaction_remove_all, parse_message_reaction_remove_emoji, parse_message_update,
};
pub(in crate::discord) use presence::parse_activity;
use presence::{parse_presence_update, parse_typing_start};
use ready::{parse_ready, parse_ready_supplemental};
use relationships::{parse_relationship_add, parse_relationship_remove, parse_relationship_update};
use user_settings::parse_user_settings_update;
use voice::{
    parse_call, parse_call_delete, parse_guild_voice_states, parse_voice_server_update,
    parse_voice_state_update,
};

#[derive(Clone, Debug)]
pub(super) struct ParsedGatewayDispatch {
    pub dispatch: GatewayDispatchInfo,
    pub events: Vec<AppEvent>,
}

/// Takes the already-parsed gateway frame so the hot read loop never parses
/// the same JSON twice, and moves `d` out instead of cloning it.
pub(super) fn parse_user_account_dispatch(mut value: Value) -> Option<ParsedGatewayDispatch> {
    let event_type = value.get("t").and_then(Value::as_str)?.to_owned();
    let data = value.get_mut("d").map(Value::take)?;
    let events = parse_user_account_event_data(&event_type, &data);
    Some(ParsedGatewayDispatch {
        dispatch: GatewayDispatchInfo {
            event_type,
            payload: data,
        },
        events,
    })
}

#[cfg(test)]
pub(super) fn parse_user_account_event(raw: &str) -> Vec<AppEvent> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(parse_user_account_dispatch)
        .map(|dispatch| dispatch.events)
        .unwrap_or_default()
}

fn parse_user_account_event_data(event_type: &str, data: &Value) -> Vec<AppEvent> {
    match event_type {
        "READY" => parse_ready(data),
        "READY_SUPPLEMENTAL" => parse_ready_supplemental(data),
        "USER_UPDATE" => parse_user_update(data).into_iter().collect(),
        "GUILD_CREATE" => {
            let mut result: Vec<AppEvent> = parse_guild_create(data).into_iter().collect();
            result.extend(parse_guild_voice_states(data));
            result
        }
        "GUILD_UPDATE" => parse_guild_update(data).into_iter().collect(),
        "GUILD_EMOJIS_UPDATE" => parse_guild_emojis_update(data).into_iter().collect(),
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
            parse_guild_role_upsert(data).into_iter().collect()
        }
        "GUILD_ROLE_DELETE" => parse_guild_role_delete(data).into_iter().collect(),
        "GUILD_DELETE" => parse_guild_delete(data).into_iter().collect(),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            parse_channel_upsert(data).into_iter().collect()
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => parse_channel_delete(data).into_iter().collect(),
        "THREAD_LIST_SYNC" => parse_thread_list_sync(data),
        "THREAD_MEMBERS_UPDATE" => parse_thread_members_update(data),
        "MESSAGE_CREATE" => parse_message_create(data).into_iter().collect(),
        "MESSAGE_UPDATE" => parse_message_update(data).into_iter().collect(),
        "MESSAGE_DELETE" => parse_message_delete(data).into_iter().collect(),
        "MESSAGE_DELETE_BULK" => parse_message_delete_bulk(data).into_iter().collect(),
        "MESSAGE_REACTION_ADD" => parse_message_reaction_add(data).into_iter().collect(),
        "MESSAGE_REACTION_REMOVE" => parse_message_reaction_remove(data).into_iter().collect(),
        "MESSAGE_REACTION_REMOVE_ALL" => parse_message_reaction_remove_all(data)
            .into_iter()
            .collect(),
        "MESSAGE_REACTION_REMOVE_EMOJI" => parse_message_reaction_remove_emoji(data)
            .into_iter()
            .collect(),
        "CHANNEL_PINS_UPDATE" => parse_channel_pins_update(data).into_iter().collect(),
        "MESSAGE_ACK" => parse_message_ack(data).into_iter().collect(),
        "USER_GUILD_SETTINGS_UPDATE" => {
            parse_user_guild_settings_update(data).into_iter().collect()
        }
        "USER_SETTINGS_UPDATE" => parse_user_settings_update(data).into_iter().collect(),
        "GUILD_MEMBER_ADD" => parse_member_add(data).into_iter().collect(),
        "GUILD_MEMBER_UPDATE" => parse_member_upsert(data).into_iter().collect(),
        "GUILD_MEMBER_LIST_UPDATE" => parse_member_list_update(data),
        "GUILD_MEMBERS_CHUNK" => parse_member_chunk(data),
        "RELATIONSHIP_ADD" => parse_relationship_add(data).into_iter().collect(),
        "RELATIONSHIP_UPDATE" => parse_relationship_update(data).into_iter().collect(),
        "RELATIONSHIP_REMOVE" => parse_relationship_remove(data).into_iter().collect(),
        "GUILD_MEMBER_REMOVE" => parse_member_remove(data).into_iter().collect(),
        "PRESENCE_UPDATE" => parse_presence_update(data),
        "VOICE_STATE_UPDATE" => parse_voice_state_update(data).into_iter().collect(),
        "VOICE_SERVER_UPDATE" => parse_voice_server_update(data).into_iter().collect(),
        "CALL_CREATE" | "CALL_UPDATE" => parse_call(data),
        "CALL_DELETE" => parse_call_delete(data).into_iter().collect(),
        "TYPING_START" => parse_typing_start(data).into_iter().collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
