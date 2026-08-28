use serde_json::Value;

use crate::discord::{
    StreamCreateInfo, StreamDeleteInfo, StreamServerInfo, StreamUpdateInfo,
    events::AppEvent,
    ids::{
        Id,
        marker::{ChannelMarker, UserMarker},
    },
};

use super::shared::parse_id;

pub(super) fn parse_stream_create(data: &Value) -> Option<AppEvent> {
    let stream_key = required_string(data, "stream_key")?;
    let rtc_server_id = required_string(data, "rtc_server_id")?;
    let rtc_channel_id = data
        .get("rtc_channel_id")
        .and_then(parse_id::<ChannelMarker>)?;
    let viewer_ids = parse_viewer_ids(data).unwrap_or_default();
    let paused = data.get("paused").and_then(Value::as_bool).unwrap_or(false);

    Some(AppEvent::StreamCreate {
        stream: StreamCreateInfo {
            stream_key,
            rtc_server_id,
            rtc_channel_id,
            viewer_ids,
            paused,
        },
    })
}

pub(super) fn parse_stream_update(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::StreamUpdate {
        stream: StreamUpdateInfo {
            stream_key: required_string(data, "stream_key")?,
            viewer_ids: parse_viewer_ids(data)?,
            paused: data.get("paused").and_then(Value::as_bool).unwrap_or(false),
        },
    })
}

pub(super) fn parse_stream_server_update(data: &Value) -> Option<AppEvent> {
    let stream_key = required_string(data, "stream_key")?;
    let token = required_string(data, "token")?;
    let endpoint = data
        .get("endpoint")
        .filter(|endpoint| !endpoint.is_null())
        .and_then(Value::as_str)
        .filter(|endpoint| !endpoint.is_empty())
        .map(str::to_owned);

    Some(AppEvent::StreamServerUpdate {
        server: StreamServerInfo {
            stream_key,
            endpoint,
            token,
        },
    })
}

pub(super) fn parse_stream_delete(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::StreamDelete {
        stream: StreamDeleteInfo {
            stream_key: required_string(data, "stream_key")?,
            reason: data
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            unavailable: data
                .get("unavailable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
    })
}

fn required_string(data: &Value, field: &str) -> Option<String> {
    data.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_viewer_ids(data: &Value) -> Option<Vec<Id<UserMarker>>> {
    data.get("viewer_ids")
        .and_then(Value::as_array)
        .map(|viewer_ids| {
            viewer_ids
                .iter()
                .filter_map(parse_id::<UserMarker>)
                .collect()
        })
}
