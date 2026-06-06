use serde_json::Value;

use crate::discord::{
    PresenceStatus,
    events::{avatar_hash_extension, default_avatar_url},
    ids::{
        Id,
        marker::{GuildMarker, UserMarker},
    },
};

pub(super) use crate::discord::display_name::{
    display_name_from_parts, display_name_from_parts_or_unknown,
};

pub(super) fn raw_user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
    let avatar = user
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    Some(match avatar {
        Some(hash) => {
            let extension = avatar_hash_extension(hash);
            format!("https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}")
        }
        None => default_avatar_url(user_id, raw_discriminator(user).unwrap_or(0)),
    })
}

pub(super) fn raw_member_avatar_url(
    guild_id: Option<Id<GuildMarker>>,
    user_id: Id<UserMarker>,
    member: &Value,
    user: Option<&Value>,
) -> Option<String> {
    if let Some(guild_id) = guild_id
        && let Some(hash) = member
            .get("avatar")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    {
        let extension = avatar_hash_extension(hash);
        return Some(format!(
            "https://cdn.discordapp.com/guilds/{guild_id}/users/{user_id}/avatars/{hash}.{extension}"
        ));
    }

    user.and_then(|user| raw_user_avatar_url(user_id, user))
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
