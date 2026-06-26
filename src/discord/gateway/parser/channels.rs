use serde_json::Value;

use crate::discord::{
    ChannelInfo, ChannelRecipientInfo, ForumTagInfo, PermissionOverwriteInfo,
    PermissionOverwriteKind, ThreadListSyncInfo, ThreadMemberUpdateInfo, ThreadMembersUpdateInfo,
    ThreadMetadataInfo,
    events::AppEvent,
    ids::{
        Id,
        marker::{
            ChannelMarker, EmojiMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker,
        },
    },
};

use super::shared::{
    display_name_from_parts, display_name_from_parts_or_unknown, extra_fields, parse_id,
    parse_status, raw_user_avatar_url,
};

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
    let owner_id = value.get("owner_id").and_then(parse_id::<UserMarker>);
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
        Some(16) => "media".to_owned(),
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
    let current_user_joined_thread = parse_current_user_thread_membership(value, &kind);

    Some(ChannelInfo {
        guild_id,
        channel_id,
        parent_id,
        owner_id,
        position,
        last_message_id,
        name,
        kind,
        message_count: value.get("message_count").and_then(Value::as_u64),
        member_count: value.get("member_count").and_then(Value::as_u64),
        total_message_sent: value.get("total_message_sent").and_then(Value::as_u64),
        thread_metadata: value.get("thread_metadata").and_then(parse_thread_metadata),
        flags: value.get("flags").and_then(Value::as_u64),
        rate_limit_per_user: value.get("rate_limit_per_user").and_then(Value::as_u64),
        available_tags: parse_forum_tags(value.get("available_tags")),
        applied_tags: parse_id_array(value.get("applied_tags")),
        current_user_joined_thread,
        recipients,
        permission_overwrites,
        is_message_request: value.get("is_message_request").and_then(Value::as_bool),
        is_spam: value.get("is_spam").and_then(Value::as_bool),
    })
}

fn parse_forum_tags(value: Option<&Value>) -> Vec<ForumTagInfo> {
    value
        .and_then(Value::as_array)
        .map(|tags| tags.iter().filter_map(parse_forum_tag).collect())
        .unwrap_or_default()
}

fn parse_forum_tag(value: &Value) -> Option<ForumTagInfo> {
    Some(ForumTagInfo {
        id: parse_id::<ForumTagMarker>(value.get("id")?)?,
        name: value.get("name")?.as_str()?.to_owned(),
        moderated: value
            .get("moderated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        emoji_id: value.get("emoji_id").and_then(parse_id::<EmojiMarker>),
        emoji_name: value
            .get("emoji_name")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn parse_current_user_thread_membership(value: &Value, kind: &str) -> Option<bool> {
    if !matches!(
        kind,
        "GuildNewsThread" | "GuildPublicThread" | "GuildPrivateThread"
    ) {
        return None;
    }
    if value.get("member").is_some() || value.get("thread_member").is_some() {
        Some(true)
    } else {
        None
    }
}

fn parse_thread_metadata(value: &Value) -> Option<ThreadMetadataInfo> {
    Some(ThreadMetadataInfo {
        archived: value.get("archived")?.as_bool()?,
        auto_archive_duration: value.get("auto_archive_duration").and_then(Value::as_u64),
        archive_timestamp: value
            .get("archive_timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned),
        locked: value.get("locked")?.as_bool()?,
        invitable: value.get("invitable").and_then(Value::as_bool),
        create_timestamp: value
            .get("create_timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned),
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
            let global_name = recipient
                .get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            let username = recipient.get("username").and_then(Value::as_str);
            display_name_from_parts(None, global_name, username).map(str::to_owned)
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(names.join(", "))
}

pub(super) fn parse_channel_recipient_info(value: &Value) -> Option<ChannelRecipientInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let global_name = value.get("global_name").and_then(Value::as_str);
    let username = value.get("username").and_then(Value::as_str);
    let display_name = display_name_from_parts_or_unknown(None, global_name, username);
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

pub(super) fn parse_channel_upsert(data: &Value) -> Option<AppEvent> {
    let info = parse_channel_info(data, None)?;
    Some(AppEvent::ChannelUpsert(info))
}

pub(super) fn parse_channel_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::ChannelDelete {
        guild_id,
        channel_id,
    })
}

pub(super) fn parse_thread_list_sync(data: &Value) -> Vec<AppEvent> {
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let threads: Vec<ChannelInfo> = data
        .get("threads")
        .and_then(Value::as_array)
        .map(|threads| {
            threads
                .iter()
                .filter_map(|thread| parse_channel_info(thread, guild_id))
                .collect()
        })
        .unwrap_or_default();
    if threads.is_empty() {
        return Vec::new();
    }
    vec![AppEvent::ThreadListSync {
        sync: ThreadListSyncInfo {
            guild_id,
            channel_ids: parse_id_array(data.get("channel_ids")),
            threads,
            thread_members: clone_array(data.get("members")),
            extra_fields: extra_fields(data, &["guild_id", "channel_ids", "threads", "members"]),
        },
    }]
}

pub(super) fn parse_thread_members_update(data: &Value) -> Vec<AppEvent> {
    let Some(channel_id) = data.get("id").and_then(parse_id::<ChannelMarker>) else {
        return Vec::new();
    };

    let added_members: Vec<_> = data
        .get("added_members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_thread_member_update_info)
        .collect();
    let added_user_ids = added_members.iter().map(|member| member.user_id).collect();
    let removed_user_ids: Vec<_> = data
        .get("removed_member_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_id::<UserMarker>)
        .collect();

    vec![AppEvent::ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo {
            guild_id: data.get("guild_id").and_then(parse_id::<GuildMarker>),
            channel_id,
            member_count: data.get("member_count").and_then(Value::as_u64),
            added_members,
            added_user_ids,
            removed_user_ids,
            extra_fields: extra_fields(
                data,
                &[
                    "id",
                    "guild_id",
                    "member_count",
                    "added_members",
                    "removed_member_ids",
                ],
            ),
        },
    }]
}

fn parse_thread_member_update_info(value: &Value) -> Option<ThreadMemberUpdateInfo> {
    Some(ThreadMemberUpdateInfo {
        user_id: value.get("user_id").and_then(parse_id::<UserMarker>)?,
        extra_fields: extra_fields(value, &["user_id"]),
    })
}

fn clone_array(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|values| values.to_vec())
        .unwrap_or_default()
}

fn parse_id_array<T>(value: Option<&Value>) -> Vec<Id<T>> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(parse_id::<T>).collect())
        .unwrap_or_default()
}
