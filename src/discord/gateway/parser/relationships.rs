use serde_json::Value;

use crate::discord::{
    FriendStatus,
    events::AppEvent,
    ids::{Id, marker::UserMarker},
};

use super::shared::parse_id;

pub(super) fn parse_relationship_add(data: &Value) -> Option<AppEvent> {
    let (user_id, status) = parse_relationship_entry(data)?;
    Some(AppEvent::RelationshipUpsert { user_id, status })
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

pub(super) fn parse_relationship_entry(value: &Value) -> Option<(Id<UserMarker>, FriendStatus)> {
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
    Some((user_id, status))
}
