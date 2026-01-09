use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::broadcast;
use twilight_gateway::{EventTypeFlags, Shard, StreamExt, error::ReceiveMessageErrorType};
use twilight_model::{
    gateway::{Intents, ShardId},
    id::{
        Id,
        marker::{AttachmentMarker, ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use super::{
    AttachmentInfo, ChannelInfo, GuildFolder, MemberInfo, MessageKind, MessageSnapshotInfo,
    PollAnswerInfo, PollInfo, PresenceStatus, ReplyInfo,
    events::{AppEvent, AttachmentUpdate, map_event},
};
use crate::logging;

pub async fn run_gateway(
    token: String,
    message_content_enabled: bool,
    tx: broadcast::Sender<AppEvent>,
) {
    let intents = gateway_intents(message_content_enabled);

    let mut shard = Shard::new(ShardId::ONE, token, intents);

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        match item {
            Ok(event) => {
                logging::debug("gateway", format!("ok: {:?}", event.kind()));
                if let Some(event) = map_event(event, message_content_enabled) {
                    let _ = tx.send(event);
                }
            }
            Err(error) => {
                // User-account payloads diverge from twilight's bot-shaped structs.
                // For events we can rebuild from raw JSON, do so. Other deserialize
                // failures are silently dropped instead of spamming the footer.
                if let ReceiveMessageErrorType::Deserializing { event } = error.kind() {
                    logging::debug("gateway", format!("deserialize fallback: {event}"));
                    let started = Instant::now();
                    let mut events = parse_user_account_event(event);
                    logging::timing("gateway", "fallback total", started.elapsed());
                    if events.is_empty() {
                        logging::debug("gateway", "fallback emitted no app events");
                    }
                    for app_event in events.drain(..) {
                        let _ = tx.send(app_event);
                    }
                    continue;
                }

                logging::error("gateway", error.to_string());
                let _ = tx.send(AppEvent::GatewayError {
                    message: error.to_string(),
                });
            }
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}

fn gateway_intents(message_content_enabled: bool) -> Intents {
    let mut intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::GUILD_MEMBERS;
    if message_content_enabled {
        intents |= Intents::MESSAGE_CONTENT;
    }

    intents
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
        "GUILD_CREATE" => {
            let started = Instant::now();
            let result = parse_guild_create(data).into_iter().collect();
            logging::timing("gateway", "fallback guild_create parse", started.elapsed());
            result
        }
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
    let total_started = Instant::now();
    let mut events = Vec::new();
    let mut stats = ReadyTimingStats::default();

    let user_started = Instant::now();
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
    stats.user = user_started.elapsed();

    let guilds_started = Instant::now();
    if let Some(guilds) = data.get("guilds").and_then(Value::as_array) {
        stats.guilds = guilds.len();
        for guild in guilds {
            stats.guild_channels += guild
                .get("channels")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            stats.guild_members += guild
                .get("members")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            stats.guild_presences += guild
                .get("presences")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if let Some(event) = parse_guild_create(guild) {
                events.push(event);
            }
        }
    }
    stats.guilds_duration = guilds_started.elapsed();

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    let private_channels_started = Instant::now();
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        stats.private_channels = privates.len();
        for channel in privates {
            if let Some(info) = parse_channel_info(channel, None) {
                events.push(AppEvent::ChannelUpsert(info));
            }
        }
    }
    stats.private_channels_duration = private_channels_started.elapsed();

    // Guild folder ordering and grouping live in the legacy `user_settings`
    // payload (the modern `user_settings_proto` blob is base64+protobuf and is
    // skipped for now). When present, every guild appears in some folder —
    // either an explicit one or a single-guild "container" with `id == null`.
    let folders_started = Instant::now();
    if let Some(folders) = data
        .get("user_settings")
        .and_then(|settings| settings.get("guild_folders"))
        .and_then(Value::as_array)
    {
        let folders: Vec<GuildFolder> = folders.iter().filter_map(parse_guild_folder).collect();
        stats.folders = folders.len();
        if !folders.is_empty() {
            events.push(AppEvent::GuildFoldersUpdate { folders });
        }
    }
    stats.folders_duration = folders_started.elapsed();
    stats.total = total_started.elapsed();

    log_ready_stats(&stats);

    events
}

#[derive(Default)]
struct ReadyTimingStats {
    guilds: usize,
    guild_channels: usize,
    guild_members: usize,
    guild_presences: usize,
    private_channels: usize,
    folders: usize,
    user: Duration,
    guilds_duration: Duration,
    private_channels_duration: Duration,
    folders_duration: Duration,
    total: Duration,
}

fn log_ready_stats(stats: &ReadyTimingStats) {
    logging::timing(
        "gateway",
        format!(
            "ready user={:.2}ms guilds={:.2}ms private_channels={:.2}ms folders={:.2}ms counts guilds={} channels={} members={} presences={} private_channels={} folders={}",
            ms(stats.user),
            ms(stats.guilds_duration),
            ms(stats.private_channels_duration),
            ms(stats.folders_duration),
            stats.guilds,
            stats.guild_channels,
            stats.guild_members,
            stats.guild_presences,
            stats.private_channels,
            stats.folders,
        ),
        stats.total,
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
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
    let message_kind = data
        .get("type")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .map(MessageKind::new)
        .unwrap_or_default();
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let attachments = parse_attachments(data.get("attachments"));
    let reply = data.get("referenced_message").and_then(parse_reply_info);
    let poll = data.get("poll").and_then(parse_poll_info);
    let source_channel_id = data
        .get("message_reference")
        .and_then(|reference| reference.get("channel_id"))
        .and_then(parse_id::<ChannelMarker>);
    let forwarded_snapshots =
        parse_message_snapshots(data.get("message_snapshots"), source_channel_id);

    Some(AppEvent::MessageCreate {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: author_name,
        message_kind,
        reply,
        poll,
        content,
        attachments,
        forwarded_snapshots,
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
    let attachments = if data.get("attachments").is_some() {
        AttachmentUpdate::Replace(parse_attachments(data.get("attachments")))
    } else {
        AttachmentUpdate::Unchanged
    };
    let poll = data.get("poll").and_then(parse_poll_info);
    Some(AppEvent::MessageUpdate {
        guild_id,
        channel_id,
        message_id,
        poll,
        content,
        attachments,
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

fn parse_attachments(value: Option<&Value>) -> Vec<AttachmentInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_attachment).collect())
        .unwrap_or_default()
}

fn parse_message_snapshots(
    value: Option<&Value>,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Vec<MessageSnapshotInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_message_snapshot(item, source_channel_id))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_reply_info(value: &Value) -> Option<ReplyInfo> {
    if value.is_null() {
        return None;
    }

    let author = value.get("author")?;
    let author_name = author
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| author.get("username").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_owned();
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    Some(ReplyInfo {
        author: author_name,
        content,
    })
}

fn parse_poll_info(value: &Value) -> Option<PollInfo> {
    if value.is_null() {
        return None;
    }

    let question = value
        .get("question")
        .and_then(|question| question.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no question text>")
        .to_owned();
    let answers: Vec<PollAnswerInfo> = value
        .get("answers")
        .and_then(Value::as_array)
        .map(|answers| answers.iter().filter_map(parse_poll_answer_info).collect())
        .unwrap_or_default();
    let allow_multiselect = value
        .get("allow_multiselect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let results = value.get("results");
    let results_finalized = results
        .and_then(|results| results.get("is_finalized"))
        .and_then(Value::as_bool);

    Some(PollInfo {
        question,
        answers: answers
            .into_iter()
            .map(|mut answer| {
                if let Some(count) = poll_answer_count(results, answer.answer_id) {
                    answer.vote_count = Some(count.0);
                    answer.me_voted = count.1;
                }
                answer
            })
            .collect(),
        allow_multiselect,
        results_finalized,
    })
}

fn parse_poll_answer_info(value: &Value) -> Option<PollAnswerInfo> {
    let answer_id = value
        .get("answer_id")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())?;
    let text = value
        .get("poll_media")
        .and_then(|media| media.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no answer text>")
        .to_owned();

    Some(PollAnswerInfo {
        answer_id,
        text,
        vote_count: None,
        me_voted: false,
    })
}

fn poll_answer_count(results: Option<&Value>, answer_id: u8) -> Option<(u64, bool)> {
    results?
        .get("answer_counts")?
        .as_array()?
        .iter()
        .find(|count| {
            count
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|id| id == u64::from(answer_id))
        })
        .map(|count| {
            (
                count.get("count").and_then(Value::as_u64).unwrap_or(0),
                count
                    .get("me_voted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
}

fn parse_message_snapshot(
    value: &Value,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Option<MessageSnapshotInfo> {
    let message = value.get("message")?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let attachments = parse_attachments(message.get("attachments"));
    let timestamp = message
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);

    if content.as_deref().is_some_and(|value| !value.is_empty())
        || !attachments.is_empty()
        || source_channel_id.is_some()
        || timestamp.is_some()
    {
        Some(MessageSnapshotInfo {
            content,
            attachments,
            source_channel_id,
            timestamp,
        })
    } else {
        None
    }
}

fn parse_attachment(value: &Value) -> Option<AttachmentInfo> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| value.get("proxy_url").and_then(Value::as_str))?
        .to_owned();
    let proxy_url = value
        .get("proxy_url")
        .and_then(Value::as_str)
        .unwrap_or(url.as_str())
        .to_owned();

    Some(AttachmentInfo {
        id: parse_id::<AttachmentMarker>(value.get("id")?)?,
        filename: value.get("filename")?.as_str()?.to_owned(),
        url,
        proxy_url,
        content_type: value
            .get("content_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        size: value
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        width: value.get("width").and_then(Value::as_u64),
        height: value.get("height").and_then(Value::as_u64),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
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

fn parse_channel_info(
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
        parent_id,
        position,
        last_message_id,
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
    let is_bot = user.get("bot").and_then(Value::as_bool).unwrap_or(false);
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use twilight_model::gateway::Intents;
    use twilight_model::id::Id;

    use super::{gateway_intents, parse_channel_info, parse_message_create, parse_message_update};
    use crate::discord::{
        AppEvent, AttachmentUpdate, MessageKind, PollAnswerInfo, PollInfo, ReplyInfo,
    };

    #[test]
    fn startup_intents_skip_presence_updates() {
        let intents = gateway_intents(false);

        assert!(intents.contains(Intents::GUILDS));
        assert!(intents.contains(Intents::GUILD_MESSAGES));
        assert!(intents.contains(Intents::DIRECT_MESSAGES));
        assert!(intents.contains(Intents::GUILD_MEMBERS));
        assert!(!intents.contains(Intents::GUILD_PRESENCES));
        assert!(!intents.contains(Intents::MESSAGE_CONTENT));
    }

    #[test]
    fn startup_intents_keep_message_content_optional() {
        let intents = gateway_intents(true);

        assert!(intents.contains(Intents::MESSAGE_CONTENT));
        assert!(!intents.contains(Intents::GUILD_PRESENCES));
    }

    #[test]
    fn channel_parser_keeps_last_message_id() {
        let channel = parse_channel_info(
            &json!({
                "id": "10",
                "type": 1,
                "last_message_id": "99",
                "recipients": [{ "username": "neo" }]
            }),
            None,
        )
        .expect("dm channel should parse");

        assert_eq!(channel.last_message_id.map(|id| id.get()), Some(99));
    }

    #[test]
    fn message_update_parser_without_attachments_does_not_clear_cached_attachments() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited"
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { attachments, .. } = event else {
            panic!("expected message update event");
        };
        assert!(matches!(attachments, AttachmentUpdate::Unchanged));
    }

    #[test]
    fn message_update_parser_empty_attachments_clears_cached_attachments() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited",
            "attachments": []
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { attachments, .. } = event else {
            panic!("expected message update event");
        };
        assert!(matches!(attachments, AttachmentUpdate::Replace(values) if values.is_empty()));
    }

    #[test]
    fn message_update_parser_keeps_poll_results() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "poll": {
                "question": { "text": "오늘 뭐 먹지?" },
                "answers": [
                    { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                    { "answer_id": 2, "poll_media": { "text": "라멘" } }
                ],
                "results": {
                    "is_finalized": true,
                    "answer_counts": [
                        { "id": 1, "count": 5, "me_voted": true },
                        { "id": 2, "count": 3, "me_voted": false }
                    ]
                }
            }
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { poll, .. } = event else {
            panic!("expected message update event");
        };
        let poll = poll.expect("poll payload should be kept");
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].vote_count, Some(5));
        assert!(poll.answers[0].me_voted);
    }

    #[test]
    fn message_create_parser_keeps_image_attachments() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "cat.png",
                "url": "https://cdn.discordapp.com/cat.png",
                "proxy_url": "https://media.discordapp.net/cat.png",
                "content_type": "image/png",
                "size": 2048,
                "width": 640,
                "height": 480,
                "description": "cat"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "cat.png");
        assert_eq!(attachments[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(attachments[0].width, Some(640));
        assert_eq!(attachments[0].height, Some(480));
    }

    #[test]
    fn message_create_parser_keeps_message_type() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { message_kind, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(message_kind, MessageKind::new(19));
    }

    #[test]
    fn message_create_parser_keeps_reply_preview() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 19,
            "content": "reply",
            "attachments": [],
            "referenced_message": {
                "id": "19",
                "channel_id": "10",
                "author": { "id": "31", "global_name": "Alex", "username": "alex" },
                "content": "잘되는군",
                "attachments": []
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply,
            Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
            })
        );
    }

    #[test]
    fn message_create_parser_keeps_poll_payload() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 0,
            "content": "",
            "attachments": [],
            "poll": {
                "question": { "text": "오늘 뭐 먹지?" },
                "answers": [
                    { "answer_id": 1, "poll_media": { "text": "김치찌개" } },
                    { "answer_id": 2, "poll_media": { "text": "라멘" } }
                ],
                "results": {
                    "is_finalized": false,
                    "answer_counts": [
                        { "id": 1, "count": 2, "me_voted": true },
                        { "id": 2, "count": 1, "me_voted": false }
                    ]
                },
                "allow_multiselect": true
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll,
            Some(PollInfo {
                question: "오늘 뭐 먹지?".to_owned(),
                answers: vec![
                    PollAnswerInfo {
                        answer_id: 1,
                        text: "김치찌개".to_owned(),
                        vote_count: Some(2),
                        me_voted: true,
                    },
                    PollAnswerInfo {
                        answer_id: 2,
                        text: "라멘".to_owned(),
                        vote_count: Some(1),
                        me_voted: false,
                    },
                ],
                allow_multiselect: true,
                results_finalized: Some(false),
            })
        );
    }

    #[test]
    fn message_create_parser_uses_proxy_url_when_url_is_missing() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "cat.png",
                "proxy_url": "https://media.discordapp.net/cat.png",
                "content_type": "image/png"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].url, "https://media.discordapp.net/cat.png");
        assert_eq!(
            attachments[0].proxy_url,
            "https://media.discordapp.net/cat.png"
        );
    }

    #[test]
    fn message_create_parser_keeps_video_attachment_metadata() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [{
                "id": "40",
                "filename": "clip.mp4",
                "url": "https://cdn.discordapp.com/clip.mp4",
                "proxy_url": "https://media.discordapp.net/clip.mp4",
                "content_type": "video/mp4",
                "size": 78364758,
                "width": 1920,
                "height": 1080,
                "description": "clip"
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { attachments, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename, "clip.mp4");
        assert_eq!(attachments[0].content_type.as_deref(), Some("video/mp4"));
        assert_eq!(attachments[0].size, 78_364_758);
        assert_eq!(attachments[0].width, Some(1920));
        assert_eq!(attachments[0].height, Some(1080));
    }

    #[test]
    fn message_create_parser_keeps_forwarded_snapshot_content() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_reference": { "channel_id": "11" },
            "message_snapshots": [{
                "message": {
                    "content": "forwarded text",
                    "timestamp": "2026-04-30T12:34:56.000000+00:00",
                    "attachments": []
                }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 1);
        assert_eq!(
            forwarded_snapshots[0].content.as_deref(),
            Some("forwarded text")
        );
        assert_eq!(forwarded_snapshots[0].source_channel_id, Some(Id::new(11)));
        assert_eq!(
            forwarded_snapshots[0].timestamp.as_deref(),
            Some("2026-04-30T12:34:56.000000+00:00")
        );
    }

    #[test]
    fn message_create_parser_keeps_forwarded_snapshot_attachments() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_snapshots": [{
                "message": {
                    "content": "",
                    "attachments": [{
                        "id": "40",
                        "filename": "cat.png",
                        "url": "https://cdn.discordapp.com/cat.png",
                        "proxy_url": "https://media.discordapp.net/cat.png",
                        "content_type": "image/png",
                        "size": 2048,
                        "width": 640,
                        "height": 480
                    }]
                }
            }]
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            forwarded_snapshots,
            ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(forwarded_snapshots.len(), 1);
        assert_eq!(forwarded_snapshots[0].attachments.len(), 1);
        assert_eq!(forwarded_snapshots[0].attachments[0].filename, "cat.png");
    }
}
