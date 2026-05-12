use serde_json::Value;

use crate::discord::{
    ChannelInfo, ChannelRecipientInfo,
    events::{PermissionOverwriteInfo, PermissionOverwriteKind},
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use super::{parse_id, parse_status, raw_user_avatar_url};

pub(crate) fn parse_channel_info(
    value: &Value,
    default_guild: Option<Id<GuildMarker>>,
) -> Option<ChannelInfo> {
    let channel_id = parse_id::<ChannelMarker>(value.get("id")?)?;
    let guild_id = value
        .get("guild_id")
        .and_then(parse_id::<GuildMarker>)
        .or(default_guild);
    let parent_id = value.get("parent_id").and_then(parse_id::<ChannelMarker>);
    let position = value
        .get("position")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());
    let last_message_id = value
        .get("last_message_id")
        .and_then(parse_id::<MessageMarker>);

    // Map Discord channel type integers to friendlier strings. DMs and
    // group-DMs are special-cased so the dashboard can render them with
    // a dedicated prefix.
    let kind = match value.get("type").and_then(Value::as_u64) {
        Some(0) => "text".to_owned(),
        Some(1) => "dm".to_owned(),
        Some(2) => "voice".to_owned(),
        Some(3) => "group-dm".to_owned(),
        Some(4) => "category".to_owned(),
        Some(5) => "announcement".to_owned(),
        Some(10) => "GuildNewsThread".to_owned(),
        Some(11) => "GuildPublicThread".to_owned(),
        Some(12) => "GuildPrivateThread".to_owned(),
        Some(13) => "stage".to_owned(),
        Some(15) => "forum".to_owned(),
        Some(other) => format!("type-{other}"),
        None => "channel".to_owned(),
    };

    let explicit_name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let name = explicit_name.unwrap_or_else(|| {
        if matches!(kind.as_str(), "dm" | "group-dm") {
            recipient_label(value).unwrap_or_else(|| format!("dm-{}", channel_id.get()))
        } else {
            format!("channel-{}", channel_id.get())
        }
    });
    let recipients = if matches!(kind.as_str(), "dm" | "group-dm") {
        value.get("recipients").and_then(|recipients| {
            Some(
                recipients
                    .as_array()?
                    .iter()
                    .filter_map(parse_channel_recipient_info)
                    .collect(),
            )
        })
    } else {
        None
    };

    let permission_overwrites = value
        .get("permission_overwrites")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_permission_overwrite)
                .collect()
        })
        .unwrap_or_default();

    Some(ChannelInfo {
        guild_id,
        channel_id,
        parent_id,
        position,
        last_message_id,
        name,
        kind,
        message_count: value.get("message_count").and_then(Value::as_u64),
        total_message_sent: value.get("total_message_sent").and_then(Value::as_u64),
        thread_archived: value
            .get("thread_metadata")
            .and_then(|metadata| metadata.get("archived"))
            .and_then(Value::as_bool),
        thread_locked: value
            .get("thread_metadata")
            .and_then(|metadata| metadata.get("locked"))
            .and_then(Value::as_bool),
        // Discord's `flags` bitfield includes PINNED (1 << 1) for forum/media
        // threads. Surface it as `Some(true)` only when the bit is set, leaving
        // non-thread channels and unpinned threads as `None`/`Some(false)`.
        thread_pinned: value
            .get("flags")
            .and_then(Value::as_u64)
            .map(|flags| flags & (1 << 1) != 0),
        recipients,
        permission_overwrites,
    })
}

/// Parse one entry from a channel's `permission_overwrites` array. Discord
/// serializes the bitfields as decimal strings. The numeric fallback keeps
/// the parser tolerant of synthetic payloads (used in tests).
fn parse_permission_overwrite(value: &Value) -> Option<PermissionOverwriteInfo> {
    let id = value.get("id").and_then(|value| {
        value
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| value.as_u64())
    })?;
    let kind = match value.get("type").and_then(Value::as_u64)? {
        0 => PermissionOverwriteKind::Role,
        1 => PermissionOverwriteKind::Member,
        // Forward-compat: ignore unknown overwrite kinds so we neither grant
        // nor deny VIEW_CHANNEL based on a discriminant we can't interpret.
        _ => return None,
    };
    let parse_bits = |key: &str| -> u64 {
        value
            .get(key)
            .and_then(|value| {
                value
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| value.as_u64())
            })
            .unwrap_or(0)
    };
    Some(PermissionOverwriteInfo {
        id,
        kind,
        allow: parse_bits("allow"),
        deny: parse_bits("deny"),
    })
}

/// For DM channels, derive a display label from the recipients' names.
/// Skips the local user when present so 1-on-1 DMs read as just the peer.
fn recipient_label(value: &Value) -> Option<String> {
    let recipients = value.get("recipients")?.as_array()?;
    let names: Vec<String> = recipients
        .iter()
        .filter_map(|recipient| {
            let global = recipient
                .get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            let username = recipient.get("username").and_then(Value::as_str);
            global.or(username).map(str::to_owned)
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(names.join(", "))
}

pub(super) fn parse_channel_recipient_info(value: &Value) -> Option<ChannelRecipientInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let global_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = value.get("username").and_then(Value::as_str);
    let display_name = global_name.or(username).unwrap_or("unknown").to_owned();
    let is_bot = value.get("bot").and_then(Value::as_bool).unwrap_or(false);
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status);

    Some(ChannelRecipientInfo {
        user_id,
        display_name,
        username: username.map(str::to_owned),
        is_bot,
        avatar_url: raw_user_avatar_url(user_id, value),
        status,
    })
}
