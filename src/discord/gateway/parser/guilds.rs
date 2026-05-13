use serde_json::Value;

use crate::discord::{
    ChannelInfo, ChannelNotificationOverrideInfo, CustomEmojiInfo, GuildNotificationSettingsInfo,
    NotificationLevel, RoleInfo,
    events::AppEvent,
    ids::{
        Id,
        marker::{ChannelMarker, EmojiMarker, GuildMarker, RoleMarker, UserMarker},
    },
};

use super::{
    channels::parse_channel_info, members::parse_member_info, presence::parse_presence_entry,
    shared::parse_id,
};

pub(super) fn parse_guild_create(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    // With user-account `capabilities` containing LAZY_USER_NOTIFICATIONS
    // (bit 0), Discord nests the guild's name / icon / owner_id under a
    // `properties` sub-object instead of placing them at the root. Fall back
    // to that location so guilds don't all render as "unknown".
    let name = guild_field(data, "name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let mut channels: Vec<ChannelInfo> = data
        .get("channels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                .collect()
        })
        .unwrap_or_default();
    if let Some(threads) = data.get("threads").and_then(Value::as_array) {
        channels.extend(
            threads
                .iter()
                .filter_map(|channel| parse_channel_info(channel, Some(guild_id))),
        );
    }

    let members = data
        .get("members")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_member_info).collect())
        .unwrap_or_default();
    let member_count = data.get("member_count").and_then(Value::as_u64);

    // Activities reach state via PresenceUpdate events, not GuildCreate.
    let presences = data
        .get("presences")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_presence_entry)
                .map(|(user_id, status, _activities)| (user_id, status))
                .collect()
        })
        .unwrap_or_default();

    let roles = data
        .get("roles")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_role_info).collect())
        .unwrap_or_default();

    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect())
        .unwrap_or_default();

    let owner_id = guild_field(data, "owner_id").and_then(parse_id::<UserMarker>);

    Some(AppEvent::GuildCreate {
        guild_id,
        name,
        member_count,
        owner_id,
        channels,
        members,
        presences,
        roles,
        emojis,
    })
}

pub(super) fn parse_role_info(value: &Value) -> Option<RoleInfo> {
    let id = parse_id::<RoleMarker>(value.get("id")?)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_owned();
    let color = value
        .get("colors")
        .and_then(|colors| colors.get("primary_color"))
        .and_then(Value::as_u64)
        .or_else(|| value.get("color").and_then(Value::as_u64))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value != 0);
    let position = value.get("position").and_then(Value::as_i64).unwrap_or(0);
    let hoist = value.get("hoist").and_then(Value::as_bool).unwrap_or(false);
    // Discord serializes `permissions` as a string-encoded 64-bit bitfield.
    // Numeric form is also accepted as a fallback for older payloads / tests.
    let permissions = value
        .get("permissions")
        .and_then(|value| {
            value
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| value.as_u64())
        })
        .unwrap_or(0);

    Some(RoleInfo {
        id,
        name,
        color,
        position,
        hoist,
        permissions,
    })
}

fn parse_custom_emoji(value: &Value) -> Option<CustomEmojiInfo> {
    let id = parse_id::<EmojiMarker>(value.get("id")?)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_owned();
    let animated = value
        .get("animated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let available = value
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    Some(CustomEmojiInfo {
        id,
        name,
        animated,
        available,
    })
}

pub(super) fn parse_user_can_use_animated_custom_emojis(user: &Value) -> Option<bool> {
    // Discord exposes `premium_type` on the current user object. Any non-zero
    // value represents a Nitro tier that can use animated custom emojis.
    user.get("premium_type")
        .and_then(Value::as_u64)
        .map(|premium_type| premium_type != 0)
}

pub(super) fn parse_guild_emojis_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect())
        .unwrap_or_default();

    Some(AppEvent::GuildEmojisUpdate { guild_id, emojis })
}

pub(super) fn parse_guild_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    // Same lazy-mode caveat as `parse_guild_create`: with capabilities such
    // as LAZY_USER_NOTIFICATIONS enabled, name/owner_id can ride inside a
    // `properties` sub-object instead of at the root.
    let name = guild_field(data, "name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect());
    let roles = data
        .get("roles")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_role_info).collect());
    let owner_id = guild_field(data, "owner_id").and_then(parse_id::<UserMarker>);
    Some(AppEvent::GuildUpdate {
        guild_id,
        name,
        owner_id,
        roles,
        emojis,
    })
}

pub(super) fn parse_guild_delete(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    Some(AppEvent::GuildDelete { guild_id })
}

pub(super) fn parse_user_guild_settings_update(data: &Value) -> Option<AppEvent> {
    parse_user_guild_notification_settings(data)
        .map(|settings| AppEvent::UserGuildNotificationSettingsUpdate { settings })
}

pub(super) fn parse_user_guild_settings_entries(
    value: Option<&Value>,
) -> Option<Vec<GuildNotificationSettingsInfo>> {
    let entries = value
        .and_then(|node| node.get("entries").or(Some(node)))
        .and_then(Value::as_array)?;
    let settings: Vec<GuildNotificationSettingsInfo> = entries
        .iter()
        .filter_map(parse_user_guild_notification_settings)
        .collect();
    (!settings.is_empty()).then_some(settings)
}

fn parse_user_guild_notification_settings(value: &Value) -> Option<GuildNotificationSettingsInfo> {
    let guild_id = parse_user_guild_settings_guild_id(value.get("guild_id"))?;
    let channel_overrides = parse_channel_notification_overrides(value.get("channel_overrides"));

    Some(GuildNotificationSettingsInfo {
        guild_id,
        message_notifications: parse_notification_level(value.get("message_notifications")),
        muted: value.get("muted").and_then(Value::as_bool).unwrap_or(false),
        mute_end_time: parse_mute_end_time(value),
        suppress_everyone: value
            .get("suppress_everyone")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        suppress_roles: value
            .get("suppress_roles")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        channel_overrides,
    })
}

fn parse_user_guild_settings_guild_id(value: Option<&Value>) -> Option<Option<Id<GuildMarker>>> {
    match value {
        Some(value) if value.is_null() => Some(None),
        Some(value) if value.as_str() == Some("@me") => Some(None),
        Some(value) => parse_id::<GuildMarker>(value).map(Some),
        None => Some(None),
    }
}

fn parse_channel_notification_overrides(
    value: Option<&Value>,
) -> Vec<ChannelNotificationOverrideInfo> {
    match value {
        Some(Value::Array(overrides)) => overrides
            .iter()
            .filter_map(parse_channel_notification_override)
            .collect(),
        Some(Value::Object(overrides)) => overrides
            .iter()
            .filter_map(|(channel_id, override_value)| {
                parse_channel_notification_override_with_key(channel_id, override_value)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_channel_notification_override(value: &Value) -> Option<ChannelNotificationOverrideInfo> {
    Some(ChannelNotificationOverrideInfo {
        channel_id: value
            .get("channel_id")
            .and_then(parse_id::<ChannelMarker>)?,
        message_notifications: parse_notification_level(value.get("message_notifications")),
        muted: value.get("muted").and_then(Value::as_bool).unwrap_or(false),
        mute_end_time: parse_mute_end_time(value),
    })
}

fn parse_channel_notification_override_with_key(
    channel_id: &str,
    value: &Value,
) -> Option<ChannelNotificationOverrideInfo> {
    Some(ChannelNotificationOverrideInfo {
        channel_id: channel_id.parse::<u64>().ok().and_then(Id::new_checked)?,
        message_notifications: parse_notification_level(value.get("message_notifications")),
        muted: value.get("muted").and_then(Value::as_bool).unwrap_or(false),
        mute_end_time: parse_mute_end_time(value),
    })
}

fn parse_notification_level(value: Option<&Value>) -> Option<NotificationLevel> {
    value
        .and_then(Value::as_u64)
        .and_then(NotificationLevel::from_code)
}

fn parse_mute_end_time(value: &Value) -> Option<String> {
    value
        .get("mute_config")
        .and_then(|config| config.get("end_time"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn guild_field<'a>(data: &'a Value, key: &str) -> Option<&'a Value> {
    data.get(key).or_else(|| {
        data.get("properties")
            .and_then(|properties| properties.get(key))
    })
}
