use serde_json::Value;

use crate::discord::{FriendStatus, RelationshipInfo, events::AppEvent, ids::marker::UserMarker};

use super::shared::parse_id;

pub(super) fn parse_relationship_add(data: &Value) -> Option<AppEvent> {
    let relationship = parse_relationship_entry(data)?;
    Some(AppEvent::RelationshipUpsert { relationship })
}

pub(super) fn parse_relationship_update(data: &Value) -> Option<AppEvent> {
    let relationship = parse_relationship_entry(data)?;
    Some(AppEvent::RelationshipUpsert { relationship })
}

pub(super) fn parse_relationship_remove(data: &Value) -> Option<AppEvent> {
    let user_id = data
        .get("id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("user")
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    Some(AppEvent::RelationshipRemove { user_id })
}

pub(super) fn parse_relationship_entry(value: &Value) -> Option<RelationshipInfo> {
    // READY's `relationships` array uses ids on the entry itself for the
    // target user. Older shards may nest it under `user.id`, so check both.
    let user_id = value
        .get("id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            value
                .get("user")
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    let kind = value.get("type").and_then(Value::as_u64)?;
    let status = match kind {
        1 => FriendStatus::Friend,
        2 => FriendStatus::Blocked,
        3 => FriendStatus::IncomingRequest,
        4 => FriendStatus::OutgoingRequest,
        _ => return None,
    };
    let nickname = value
        .get("nickname")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let username = value
        .get("user")
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str);
    let display_name = value
        .get("user")
        .and_then(|user| {
            user.get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .or(username)
        })
        .map(str::to_owned);
    Some(RelationshipInfo {
        user_id,
        status,
        nickname,
        display_name,
        username: username.map(str::to_owned),
    })
}
