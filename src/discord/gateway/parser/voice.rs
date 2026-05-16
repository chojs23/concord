use serde_json::Value;

use crate::discord::{
    Id, VoiceServerInfo, VoiceStateInfo, events::AppEvent, ids::marker::GuildMarker,
};

use super::{members::parse_member_info, shared::parse_id};

pub(super) fn parse_voice_state_update(data: &Value) -> Option<AppEvent> {
    parse_voice_state_info(data, None).map(|state| AppEvent::VoiceStateUpdate { state })
}

pub(super) fn parse_voice_server_update(data: &Value) -> Option<AppEvent> {
    let guild_id = data.get("guild_id").and_then(parse_id)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())?
        .to_owned();
    let endpoint = data
        .get("endpoint")
        .filter(|endpoint| !endpoint.is_null())
        .and_then(Value::as_str)
        .filter(|endpoint| !endpoint.is_empty())
        .map(str::to_owned);

    Some(AppEvent::VoiceServerUpdate {
        server: VoiceServerInfo {
            guild_id,
            endpoint,
            token,
        },
    })
}

pub(super) fn parse_guild_voice_states(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };
    data.get("voice_states")
        .and_then(Value::as_array)
        .map(|states| {
            states
                .iter()
                .filter_map(|state| parse_voice_state_info(state, Some(guild_id)))
                .map(|state| AppEvent::VoiceStateUpdate { state })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_voice_state_info(
    value: &Value,
    guild_id_override: Option<Id<GuildMarker>>,
) -> Option<VoiceStateInfo> {
    let guild_id = guild_id_override.or_else(|| value.get("guild_id").and_then(parse_id))?;
    let user_id = value
        .get("user_id")
        .or_else(|| value.get("member").and_then(|member| member.get("user_id")))
        .or_else(|| {
            value
                .get("member")
                .and_then(|member| member.get("user"))
                .and_then(|user| user.get("id"))
        })
        .and_then(parse_id)?;
    let channel_id = value
        .get("channel_id")
        .filter(|channel_id| !channel_id.is_null())
        .and_then(parse_id);

    Some(VoiceStateInfo {
        guild_id,
        channel_id,
        user_id,
        session_id: value
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|session_id| !session_id.is_empty())
            .map(str::to_owned),
        member: value.get("member").and_then(parse_member_info),
        deaf: value.get("deaf").and_then(Value::as_bool).unwrap_or(false),
        mute: value.get("mute").and_then(Value::as_bool).unwrap_or(false),
        self_deaf: value
            .get("self_deaf")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        self_mute: value
            .get("self_mute")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        self_stream: value
            .get("self_stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}
