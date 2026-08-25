use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};

pub(crate) fn user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
    match custom_avatar_hash(user) {
        Some(hash) => Some(format!(
            "https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}",
            extension = avatar_hash_extension(hash)
        )),
        None => Some(default_avatar_url(user_id, discriminator(user))),
    }
}

pub(crate) fn member_avatar_url(
    guild_id: Option<Id<GuildMarker>>,
    user_id: Id<UserMarker>,
    member: Option<&Value>,
    user: Option<&Value>,
) -> Option<String> {
    if let Some(guild_id) = guild_id
        && let Some(hash) = member.and_then(custom_avatar_hash)
    {
        let extension = avatar_hash_extension(hash);
        return Some(format!(
            "https://cdn.discordapp.com/guilds/{guild_id}/users/{user_id}/avatars/{hash}.{extension}"
        ));
    }

    user.and_then(|user| user_avatar_url(user_id, user))
}

pub(crate) fn default_avatar_url(user_id: Id<UserMarker>, discriminator: u16) -> String {
    let index = if discriminator == 0 {
        (user_id.get() >> 22) % 6
    } else {
        u64::from(discriminator % 5)
    };

    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

pub(crate) fn guild_icon_url(guild_id: Id<GuildMarker>, icon_hash: &str) -> String {
    let extension = avatar_hash_extension(icon_hash);
    format!("https://cdn.discordapp.com/icons/{guild_id}/{icon_hash}.{extension}?size=64")
}

pub(crate) fn avatar_hash_extension(hash: &str) -> &'static str {
    if hash.starts_with("a_") { "gif" } else { "png" }
}

fn custom_avatar_hash(value: &Value) -> Option<&str> {
    value
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|hash| !hash.is_empty())
}

fn discriminator(user: &Value) -> u16 {
    user.get("discriminator")
        .and_then(|value| {
            value
                .as_str()
                .and_then(|value| value.parse::<u16>().ok())
                .or_else(|| value.as_u64().and_then(|value| u16::try_from(value).ok()))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::guild_icon_url;
    use crate::discord::ids::Id;

    #[test]
    fn guild_icon_url_uses_thumbnail_size_and_hash_animation() {
        let guild_id = Id::new(42);
        assert_eq!(
            guild_icon_url(guild_id, "static_hash"),
            "https://cdn.discordapp.com/icons/42/static_hash.png?size=64"
        );
        assert_eq!(
            guild_icon_url(guild_id, "a_animated_hash"),
            "https://cdn.discordapp.com/icons/42/a_animated_hash.gif?size=64"
        );
    }
}
