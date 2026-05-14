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

pub(super) fn display_name_from_parts<'a>(
    nick: Option<&'a str>,
    global_name: Option<&'a str>,
    username: Option<&'a str>,
) -> Option<&'a str> {
    nick.and_then(non_empty)
        .or_else(|| global_name.and_then(non_empty))
        .or_else(|| username.and_then(non_empty))
}

pub(super) fn display_name_from_parts_or_unknown(
    nick: Option<&str>,
    global_name: Option<&str>,
    username: Option<&str>,
) -> String {
    display_name_from_parts(nick, global_name, username)
        .unwrap_or("unknown")
        .to_owned()
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

pub(super) fn parse_id<M>(value: &Value) -> Option<Id<M>> {
    value
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
}

#[cfg(test)]
mod tests {
    use super::{display_name_from_parts, display_name_from_parts_or_unknown};

    #[test]
    fn display_name_from_parts_prefers_nick_global_then_username() {
        assert_eq!(
            display_name_from_parts(Some("nick"), Some("global"), Some("user")),
            Some("nick")
        );
        assert_eq!(
            display_name_from_parts(None, Some("global"), Some("user")),
            Some("global")
        );
        assert_eq!(
            display_name_from_parts(None, None, Some("user")),
            Some("user")
        );
    }

    #[test]
    fn display_name_from_parts_ignores_empty_names() {
        assert_eq!(
            display_name_from_parts(Some(""), Some(""), Some("user")),
            Some("user")
        );
        assert_eq!(
            display_name_from_parts_or_unknown(Some(""), Some(""), None),
            "unknown"
        );
    }
}
