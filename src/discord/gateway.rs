use std::{
    env,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

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
    ChannelInfo, GuildFolder, MemberInfo, PresenceStatus,
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
    let logger = DebugLogger::from_env();

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        match item {
            Ok(event) => {
                logger.log(&format!("ok: {:?}\n", event.kind()));
                if let Some(event) = map_event(event, message_content_enabled) {
                    let _ = tx.send(event);
                }
            }
            Err(error) => {
                // User-account payloads diverge from twilight's bot-shaped structs.
                // For events we can rebuild from raw JSON, do so. Other deserialize
                // failures are silently dropped instead of spamming the footer.
                if let ReceiveMessageErrorType::Deserializing { event } = error.kind() {
                    logger.log(&format!("deserialize fallback: {event}\n"));
                    let mut events = parse_user_account_event(event);
                    if events.is_empty() {
                        logger.log("  -> no fallback events emitted\n");
                    }
                    for app_event in events.drain(..) {
                        let _ = tx.send(app_event);
                    }
                    continue;
                }

                logger.log(&format!("err: {error}\n"));
                let _ = tx.send(AppEvent::GatewayError {
                    message: error.to_string(),
                });
            }
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}

/// Optional debug log; activated by setting `DISCORD_DEBUG_GATEWAY=1`. Writes
/// raw deserialize-failed payloads and event kinds to
/// `~/.discord-rs/gateway-debug.log` so we can diagnose user-token format
/// surprises without polluting the TUI.
struct DebugLogger {
    file: Option<Mutex<File>>,
}

impl DebugLogger {
    fn from_env() -> Self {
        if env::var("DISCORD_DEBUG_GATEWAY").ok().as_deref() != Some("1") {
            return Self { file: None };
        }

        let Some(path) = log_path() else {
            return Self { file: None };
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
            .map(Mutex::new);
        Self { file }
    }

    fn log(&self, message: &str) {
        let Some(file) = self.file.as_ref() else {
            return;
        };
        if let Ok(mut guard) = file.lock() {
            let _ = guard.write_all(message.as_bytes());
        }
    }
}

fn log_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".discord-rs").join("gateway-debug.log"))
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
        "READY" => parse_ready(data),
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

/// User-account READY embeds the full guild list under `d.guilds`. Bots get a
/// stub list of unavailable guilds and a separate `GUILD_CREATE` per guild,
/// but user accounts never send standalone GUILD_CREATEs, so we emit a
/// synthetic GuildCreate for each entry inline.
fn parse_ready(data: &Value) -> Vec<AppEvent> {
    let mut events = Vec::new();

    if let Some(user) = data.get("user") {
        let name = user
            .get("global_name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| user.get("username").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_owned();
        events.push(AppEvent::Ready { user: name });
    }

    if let Some(guilds) = data.get("guilds").and_then(Value::as_array) {
        for guild in guilds {
            if let Some(event) = parse_guild_create(guild) {
                events.push(event);
            }
        }
    }

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        for channel in privates {
            if let Some(info) = parse_channel_info(channel, None) {
                events.push(AppEvent::ChannelUpsert(info));
            }
        }
    }

    // Guild folder ordering and grouping live in the legacy `user_settings`
    // payload (the modern `user_settings_proto` blob is base64+protobuf and is
    // skipped for now). When present, every guild appears in some folder —
    // either an explicit one or a single-guild "container" with `id == null`.
    if let Some(folders) = data
        .get("user_settings")
        .and_then(|settings| settings.get("guild_folders"))
        .and_then(Value::as_array)
    {
        let folders: Vec<GuildFolder> = folders.iter().filter_map(parse_guild_folder).collect();
        if !folders.is_empty() {
            events.push(AppEvent::GuildFoldersUpdate { folders });
        }
    }

    events
}

fn parse_guild_folder(value: &Value) -> Option<GuildFolder> {
    let guild_ids: Vec<Id<GuildMarker>> = value
        .get("guild_ids")?
        .as_array()?
        .iter()
        .filter_map(parse_id::<GuildMarker>)
        .collect();
    if guild_ids.is_empty() {
        return None;
    }

    let id = value.get("id").and_then(Value::as_u64);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let color = value.get("color").and_then(Value::as_u64).map(|c| c as u32);

    Some(GuildFolder {
        id,
        name,
        color,
        guild_ids,
    })
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
        Some(10..=12) => "thread".to_owned(),
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

    Some(ChannelInfo {
        guild_id,
        channel_id,
        name,
        kind,
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
