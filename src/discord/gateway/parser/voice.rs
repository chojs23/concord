use serde_json::Value;

use crate::discord::{
    Id, VoiceServerInfo, VoiceStateInfo, events::AppEvent,
    ids::marker::{ChannelMarker, GuildMarker},
};

use super::{members::parse_member_info, shared::parse_id};

/// Parse a `CALL_CREATE` / `CALL_UPDATE` dispatch into per-participant voice
/// states. This is how a client learns who is already in an in-progress DM call
/// (guild participants instead arrive via GUILD_CREATE).
pub(super) fn parse_call(data: &Value) -> Vec<AppEvent> {
    let Some(channel_id) = data.get("channel_id").and_then(parse_id::<ChannelMarker>) else {
        return Vec::new();
    };
    data.get("voice_states")
        .and_then(Value::as_array)
        .map(|states| {
            states
                .iter()
                .filter_map(|state| parse_call_voice_state(state, channel_id))
                .map(|state| AppEvent::VoiceStateUpdate { state })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a `CALL_DELETE` dispatch: a DM or group-DM call ended, so its voice
/// states are cleared.
pub(super) fn parse_call_delete(data: &Value) -> Option<AppEvent> {
    data.get("channel_id")
        .and_then(parse_id::<ChannelMarker>)
        .map(|channel_id| AppEvent::CallDelete { channel_id })
}

/// A call participant has no guild, and may omit its channel, so it inherits the
/// call's channel to stay attached to the right private call.
fn parse_call_voice_state(
    value: &Value,
    call_channel_id: Id<ChannelMarker>,
) -> Option<VoiceStateInfo> {
    let mut state = parse_voice_state_info(value, None)?;
    if state.channel_id.is_none() {
        state.channel_id = Some(call_channel_id);
    }
    Some(state)
}

pub(super) fn parse_voice_state_update(data: &Value) -> Option<AppEvent> {
    parse_voice_state_info(data, None).map(|state| AppEvent::VoiceStateUpdate { state })
}

pub(super) fn parse_voice_server_update(data: &Value) -> Option<AppEvent> {
    // Guild voice carries a `guild_id`; a DM or group-DM call carries a
    // `channel_id` with a null guild instead. We need at least one to route the
    // server update to the matching connection.
    let guild_id = data.get("guild_id").and_then(parse_id);
    let channel_id = data.get("channel_id").and_then(parse_id);
    if guild_id.is_none() && channel_id.is_none() {
        return None;
    }
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
            channel_id,
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
    // A guild voice state carries a `guild_id` (or one is supplied by the
    // GUILD_CREATE override). DM and group-DM call voice states arrive with a
    // null guild, so the absence of one is no longer a parse failure.
    let guild_id = guild_id_override.or_else(|| value.get("guild_id").and_then(parse_id));
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
        // Member objects only accompany guild voice states; a DM call state has
        // no guild and therefore no member to attach.
        member: value.get("member").and_then(|member| {
            guild_id.and_then(|guild_id| parse_member_info(member, Some(guild_id)))
        }),
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
