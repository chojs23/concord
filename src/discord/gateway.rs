use serde_json::Value;
use tokio::sync::broadcast;
use twilight_gateway::{EventTypeFlags, Shard, StreamExt, error::ReceiveMessageErrorType};
use twilight_model::{
    gateway::{Intents, ShardId},
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use super::{
    ChannelInfo, MemberInfo, PresenceStatus,
    events::{AppEvent, map_event},
};

pub async fn run_gateway(
    token: String,
    message_content_enabled: bool,
    tx: broadcast::Sender<AppEvent>,
) {
    let mut intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_PRESENCES;
    if message_content_enabled {
        intents |= Intents::MESSAGE_CONTENT;
    }

    let mut shard = Shard::new(ShardId::ONE, token, intents);

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        match item {
            Ok(event) => {
                if let Some(event) = map_event(event, message_content_enabled) {
                    let _ = tx.send(event);
                }
            }
            Err(error) => {
                // User-account payloads diverge from twilight's bot-shaped structs.
                // For events we can rebuild from raw JSON, do so. Other deserialize
                // failures are silently dropped instead of spamming the footer.
                if let ReceiveMessageErrorType::Deserializing { event } = error.kind() {
                    for app_event in parse_user_account_event(event) {
                        let _ = tx.send(app_event);
                    }
                    continue;
                }

                let _ = tx.send(AppEvent::GatewayError {
                    message: error.to_string(),
                });
            }
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}

/// Best-effort fallback that rebuilds the dashboard's domain events directly
/// from the raw gateway payload. We only extract the fields the UI consumes,
/// and skip anything we can't model. Returns an iterable so a single payload
/// (e.g. `GUILD_CREATE`) can produce multiple downstream events.
fn parse_user_account_event(raw: &str) -> Vec<AppEvent> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(event_type) = value.get("t").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(data) = value.get("d") else {
        return Vec::new();
    };

    match event_type {
        "READY" => parse_ready(data).into_iter().collect(),
        "GUILD_CREATE" => parse_guild_create(data).into_iter().collect(),
        "GUILD_UPDATE" => parse_guild_update(data).into_iter().collect(),
        "GUILD_DELETE" => parse_guild_delete(data).into_iter().collect(),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => parse_channel_upsert(data).into_iter().collect(),
        "CHANNEL_DELETE" => parse_channel_delete(data).into_iter().collect(),
        "MESSAGE_CREATE" => parse_message_create(data).into_iter().collect(),
        "MESSAGE_UPDATE" => parse_message_update(data).into_iter().collect(),
        "MESSAGE_DELETE" => parse_message_delete(data).into_iter().collect(),
        "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
            parse_member_upsert(data).into_iter().collect()
        }
        "GUILD_MEMBER_REMOVE" => parse_member_remove(data).into_iter().collect(),
        "PRESENCE_UPDATE" => parse_presence_update(data).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn parse_ready(data: &Value) -> Option<AppEvent> {
    let user = data.get("user")?;
    let name = user
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| user.get("username").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_owned();
    Some(AppEvent::Ready { user: name })
}

fn parse_guild_create(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let channels = data
        .get("channels")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                .collect()
        })
        .unwrap_or_default();

    let members = data
        .get("members")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_member_info).collect())
        .unwrap_or_default();

    let presences = data
        .get("presences")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_presence_entry).collect())
        .unwrap_or_default();

    Some(AppEvent::GuildCreate {
        guild_id,
        name,
        channels,
        members,
        presences,
    })
}

fn parse_guild_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    Some(AppEvent::GuildUpdate { guild_id, name })
}

fn parse_guild_delete(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    Some(AppEvent::GuildDelete { guild_id })
}

fn parse_channel_upsert(data: &Value) -> Option<AppEvent> {
    let info = parse_channel_info(data, None)?;
    Some(AppEvent::ChannelUpsert(info))
}

fn parse_channel_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::ChannelDelete {
        guild_id,
        channel_id,
    })
}

fn parse_message_create(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let author = data.get("author")?;
    let author_id = parse_id::<UserMarker>(author.get("id")?)?;
    let author_name = author
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| author.get("username").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_owned();
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(AppEvent::MessageCreate {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: author_name,
        content,
    })
}

fn parse_message_update(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(AppEvent::MessageUpdate {
        guild_id,
        channel_id,
        message_id,
        content,
    })
}

fn parse_message_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::MessageDelete {
        guild_id,
        channel_id,
        message_id,
    })
}

fn parse_member_upsert(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data)?;
    Some(AppEvent::GuildMemberUpsert { guild_id, member })
}

fn parse_member_remove(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let user = data.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    Some(AppEvent::GuildMemberRemove { guild_id, user_id })
}

fn parse_presence_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let (user_id, status) = parse_presence_entry(data)?;
    Some(AppEvent::PresenceUpdate {
        guild_id,
        user_id,
        status,
    })
}

fn parse_channel_info(value: &Value, default_guild: Option<Id<GuildMarker>>) -> Option<ChannelInfo> {
    let channel_id = parse_id::<ChannelMarker>(value.get("id")?)?;
    let guild_id = value
        .get("guild_id")
        .and_then(parse_id::<GuildMarker>)
        .or(default_guild);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("channel-{}", channel_id.get()));
    let kind = value
        .get("type")
        .and_then(Value::as_u64)
        .map(|value| format!("type-{value}"))
        .unwrap_or_else(|| "channel".to_owned());
    Some(ChannelInfo {
        guild_id,
        channel_id,
        name,
        kind,
    })
}

fn parse_member_info(value: &Value) -> Option<MemberInfo> {
    let user = value.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    let nick = value
        .get("nick")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = user
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = user.get("username").and_then(Value::as_str);
    let display_name = nick
        .or(global_name)
        .or(username)
        .unwrap_or("unknown")
        .to_owned();
    let is_bot = user
        .get("bot")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(MemberInfo {
        user_id,
        display_name,
        is_bot,
    })
}

fn parse_presence_entry(value: &Value) -> Option<(Id<UserMarker>, PresenceStatus)> {
    let user = value.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)
        .unwrap_or(PresenceStatus::Offline);
    Some((user_id, status))
}

fn parse_status(value: &str) -> PresenceStatus {
    match value {
        "online" => PresenceStatus::Online,
        "idle" => PresenceStatus::Idle,
        "dnd" => PresenceStatus::DoNotDisturb,
        _ => PresenceStatus::Offline,
    }
}

fn parse_id<M>(value: &Value) -> Option<Id<M>> {
    value.as_str()?.parse::<u64>().ok().map(Id::new)
}
