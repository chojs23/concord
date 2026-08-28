use serde_json::Value;

use crate::discord::{
    FriendStatus, RelationshipInfo, RelationshipUpdateInfo, events::AppEvent,
    ids::marker::UserMarker,
};

use super::shared::{display_name_from_parts, parse_id};

pub(super) fn parse_relationship_add(data: &Value) -> Option<AppEvent> {
    let relationship = parse_relationship_entry(data)?;
    Some(AppEvent::RelationshipUpsert { relationship })
}

pub(super) fn parse_relationship_update(data: &Value) -> Option<AppEvent> {
    let user_id = data
        .get("id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("user")
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    let status = data
        .get("type")
        .and_then(Value::as_u64)
        .and_then(parse_relationship_status);
    let nickname = data.get("nickname").map(|nickname| {
        nickname
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    });
    let user = data.get("user");
    let username = user.and_then(|user| {
        user.get("username")
            .map(|username| username.as_str().map(str::to_owned))
    });
    let display_name = user.and_then(|user| {
        let carries_name = user.get("global_name").is_some() || user.get("username").is_some();
        carries_name.then(|| {
            let global_name = user
                .get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            let username = user
                .get("username")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            display_name_from_parts(None, global_name, username).map(str::to_owned)
        })
    });

    Some(AppEvent::RelationshipUpdate {
        update: RelationshipUpdateInfo {
            user_id,
            status,
            nickname,
            display_name,
            username,
            ignored: data.get("user_ignored").and_then(Value::as_bool),
        },
    })
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
    let status = data
        .get("type")
        .and_then(Value::as_u64)
        .and_then(parse_relationship_status);
    Some(AppEvent::RelationshipRemove { user_id, status })
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
    let status = parse_relationship_status(kind)?;
    let nickname = value
        .get("nickname")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let username = value
        .get("user")
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str);
    let global_name = value
        .get("user")
        .and_then(|user| user.get("global_name"))
        .and_then(Value::as_str);
    let display_name = display_name_from_parts(None, global_name, username).map(str::to_owned);
    Some(RelationshipInfo {
        user_id,
        status,
        nickname,
        display_name,
        username: username.map(str::to_owned),
        ignored: value
            .get("user_ignored")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_relationship_status(kind: u64) -> Option<FriendStatus> {
    match kind {
        0 => Some(FriendStatus::None),
        1 => Some(FriendStatus::Friend),
        2 => Some(FriendStatus::Blocked),
        3 => Some(FriendStatus::IncomingRequest),
        4 => Some(FriendStatus::OutgoingRequest),
        5 => Some(FriendStatus::Implicit),
        _ => None,
    }
}
