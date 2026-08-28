use serde_json::Value;

use crate::discord::{
    events::{AppEvent, ChannelUnreadInfo},
    ids::marker::{ChannelMarker, GuildMarker, MessageMarker},
};

use super::shared::{parse_id, parse_nonnegative_i64};

pub(super) fn parse_feature_read_state_ack(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::FeatureReadStateAck {
        read_state_type: u8::try_from(data.get("ack_type")?.as_u64()?).ok()?,
        resource_id: parse_snowflake(data.get("resource_id")?)?,
        entity_id: parse_snowflake(data.get("entity_id")?)?,
        version: parse_nonnegative_i64(data.get("version")?)?,
    })
}

pub(super) fn parse_channel_pins_ack(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::ChannelPinsAck {
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        timestamp: data.get("timestamp")?.as_str()?.to_owned(),
        version: parse_nonnegative_i64(data.get("version")?)?,
    })
}

pub(super) fn parse_channel_unread_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let channels = data
        .get("channel_unread_updates")?
        .as_array()?
        .iter()
        .filter_map(parse_channel_unread)
        .collect();
    Some(AppEvent::ChannelUnreadUpdate { guild_id, channels })
}

fn parse_channel_unread(value: &Value) -> Option<ChannelUnreadInfo> {
    Some(ChannelUnreadInfo {
        channel_id: parse_id::<ChannelMarker>(value.get("id")?)?,
        last_message_id: value.get("last_message_id").map(parse_id::<MessageMarker>),
        last_pin_timestamp: value
            .get("last_pin_timestamp")
            .map(|timestamp| timestamp.as_str().map(str::to_owned)),
    })
}

fn parse_snowflake(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|value| value.parse().ok())
        .or_else(|| value.as_u64())
        .filter(|value| *value != 0)
}
