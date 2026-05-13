use serde_json::Value;

use crate::discord::{
    PresenceStatus,
    events::default_avatar_url,
    ids::{Id, marker::UserMarker},
};

pub(super) fn raw_user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
    let avatar = user
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    Some(match avatar {
        Some(hash) => {
            let extension = if hash.starts_with("a_") { "gif" } else { "png" };
            format!("https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}")
        }
        None => default_avatar_url(user_id, raw_discriminator(user).unwrap_or(0)),
    })
}

fn raw_discriminator(user: &Value) -> Option<u16> {
    user.get("discriminator").and_then(|value| {
        value
            .as_str()
            .and_then(|value| value.parse::<u16>().ok())
            .or_else(|| value.as_u64().and_then(|value| u16::try_from(value).ok()))
    })
}

pub(super) fn parse_status(value: &str) -> PresenceStatus {
    match value {
        "online" => PresenceStatus::Online,
        "idle" => PresenceStatus::Idle,
        "dnd" => PresenceStatus::DoNotDisturb,
        "offline" | "invisible" => PresenceStatus::Offline,
        _ => PresenceStatus::Unknown,
    }
}

pub(super) fn parse_id<M>(value: &Value) -> Option<Id<M>> {
    value
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
}
