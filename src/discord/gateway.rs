use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use twilight_gateway::{
    EventTypeFlags, MessageSender, Shard, StreamExt, error::ReceiveMessageErrorType,
};
use twilight_model::{
    gateway::{Intents, ShardId, payload::outgoing::RequestGuildMembers},
    id::{
        Id,
        marker::{
            AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker,
            UserMarker,
        },
    },
};

use super::{
    AttachmentInfo, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, GuildFolder, MemberInfo,
    MentionInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PollAnswerInfo, PollInfo,
    PresenceStatus, ReplyInfo, RoleInfo,
    events::default_avatar_url,
    events::{AppEvent, AttachmentUpdate, map_event},
};
use crate::logging;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GatewayCommand {
    RequestGuildMembers { guild_id: Id<GuildMarker> },
}

pub async fn run_gateway(
    token: String,
    tx: broadcast::Sender<AppEvent>,
    mut commands: mpsc::UnboundedReceiver<GatewayCommand>,
) {
    let intents = gateway_intents();

    let mut shard = Shard::new(ShardId::ONE, token, intents);
    let sender = shard.sender();
    let mut commands_closed = false;

    loop {
        tokio::select! {
            maybe_command = commands.recv(), if !commands_closed => {
                match maybe_command {
                    Some(command) => handle_gateway_command(&sender, command, &tx),
                    None => commands_closed = true,
                }
            }
            item = shard.next_event(EventTypeFlags::all()) => {
                let Some(item) = item else {
                    break;
                };

                match item {
                    Ok(event) => {
                        logging::debug("gateway", format!("ok: {:?}", event.kind()));
                        for event in map_event(event) {
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
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}

fn handle_gateway_command(
    sender: &MessageSender,
    command: GatewayCommand,
    tx: &broadcast::Sender<AppEvent>,
) {
    match command {
        GatewayCommand::RequestGuildMembers { guild_id } => {
            let request = RequestGuildMembers::builder(guild_id).query("", None);
            match sender.command(&request) {
                Ok(()) => logging::debug(
                    "gateway",
                    format!("requested guild members: guild={}", guild_id.get()),
                ),
                Err(error) => {
                    let message = format!("request guild members failed: {error}");
                    logging::error("gateway", &message);
                    let _ = tx.send(AppEvent::GatewayError { message });
                }
            }
        }
    }
}

fn gateway_intents() -> Intents {
    Intents::GUILDS
        | Intents::GUILD_EMOJIS_AND_STICKERS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::GUILD_MEMBERS
        | Intents::MESSAGE_CONTENT
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
        "READY_SUPPLEMENTAL" => parse_ready_supplemental(data),
        "GUILD_CREATE" => {
            let started = Instant::now();
            let result = parse_guild_create(data).into_iter().collect();
            logging::timing("gateway", "fallback guild_create parse", started.elapsed());
            result
        }
        "GUILD_UPDATE" => parse_guild_update(data).into_iter().collect(),
        "GUILD_EMOJIS_UPDATE" => parse_guild_emojis_update(data).into_iter().collect(),
        "GUILD_DELETE" => parse_guild_delete(data).into_iter().collect(),
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
            parse_channel_upsert(data).into_iter().collect()
        }
        "CHANNEL_DELETE" | "THREAD_DELETE" => parse_channel_delete(data).into_iter().collect(),
        "THREAD_LIST_SYNC" => parse_thread_list_sync(data),
        "MESSAGE_CREATE" => parse_message_create(data).into_iter().collect(),
        "MESSAGE_UPDATE" => parse_message_update(data).into_iter().collect(),
        "MESSAGE_DELETE" => parse_message_delete(data).into_iter().collect(),
        "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
            parse_member_upsert(data).into_iter().collect()
        }
        "GUILD_MEMBERS_CHUNK" => parse_member_chunk(data),
        "GUILD_MEMBER_REMOVE" => parse_member_remove(data).into_iter().collect(),
        "PRESENCE_UPDATE" => parse_presence_update(data),
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
        let user_id = user.get("id").and_then(parse_id::<UserMarker>);
        let name = user
            .get("global_name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| user.get("username").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_owned();
        events.push(AppEvent::Ready {
            user: name,
            user_id,
        });
    }
    stats.user = user_started.elapsed();

    let guilds_started = Instant::now();
    if let Some(guilds) = data.get("guilds").and_then(Value::as_array) {
        stats.guilds = guilds.len();
        for guild in guilds {
            stats.guild_channels += channel_array_len(guild, "channels");
            stats.guild_channels += channel_array_len(guild, "threads");
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

    let merged_presences = parse_merged_presences(data);

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    let private_channels_started = Instant::now();
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        stats.private_channels = privates.len();
        for channel in privates {
            if let Some(mut info) = parse_channel_info(channel, None) {
                apply_recipient_presences(&mut info, &merged_presences);
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

fn parse_ready_supplemental(data: &Value) -> Vec<AppEvent> {
    parse_merged_presences(data)
        .into_iter()
        .map(|(user_id, status)| AppEvent::UserPresenceUpdate { user_id, status })
        .collect()
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

fn parse_merged_presences(data: &Value) -> BTreeMap<Id<UserMarker>, PresenceStatus> {
    let mut presences = BTreeMap::new();
    if let Some(merged) = data.get("merged_presences") {
        collect_presence_entries(merged, &mut presences);
    }
    presences
}

fn collect_presence_entries(
    value: &Value,
    presences: &mut BTreeMap<Id<UserMarker>, PresenceStatus>,
) {
    if let Some((user_id, status)) = parse_presence_entry(value) {
        presences.insert(user_id, status);
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_presence_entries(item, presences);
        }
    } else if let Some(object) = value.as_object() {
        for item in object.values() {
            collect_presence_entries(item, presences);
        }
    }
}

fn apply_recipient_presences(
    channel: &mut ChannelInfo,
    presences: &BTreeMap<Id<UserMarker>, PresenceStatus>,
) {
    let Some(recipients) = channel.recipients.as_mut() else {
        return;
    };
    for recipient in recipients {
        if let Some(status) = presences.get(&recipient.user_id) {
            recipient.status = Some(*status);
        }
    }
}

fn parse_guild_create(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    let name = data
        .get("name")
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

    let presences = data
        .get("presences")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_presence_entry).collect())
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

    Some(AppEvent::GuildCreate {
        guild_id,
        name,
        channels,
        members,
        presences,
        roles,
        emojis,
    })
}

fn parse_role_info(value: &Value) -> Option<RoleInfo> {
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

    Some(RoleInfo {
        id,
        name,
        color,
        position,
        hoist,
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

fn parse_guild_emojis_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let emojis = data
        .get("emojis")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_custom_emoji).collect())
        .unwrap_or_default();

    Some(AppEvent::GuildEmojisUpdate { guild_id, emojis })
}

fn parse_guild_update(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("id")?)?;
    let name = data
        .get("name")
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
    Some(AppEvent::GuildUpdate {
        guild_id,
        name,
        roles,
        emojis,
    })
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

fn parse_thread_list_sync(data: &Value) -> Vec<AppEvent> {
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    data.get("threads")
        .and_then(Value::as_array)
        .map(|threads| {
            threads
                .iter()
                .filter_map(|thread| parse_channel_info(thread, guild_id))
                .map(AppEvent::ChannelUpsert)
                .collect()
        })
        .unwrap_or_default()
}

fn channel_array_len(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

fn parse_message_create(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let author = data.get("author")?;
    let author_id = parse_id::<UserMarker>(author.get("id")?)?;
    let author_name = message_author_display_name(data, author);
    let author_avatar_url = raw_user_avatar_url(author_id, author);
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
    let mentions = parse_mentions(data.get("mentions"));
    let attachments = parse_attachments(data.get("attachments"));
    let reply = data.get("referenced_message").and_then(parse_reply_info);
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let reference = data
        .get("message_reference")
        .map(parse_message_reference_info);
    let source_channel_id = reference
        .as_ref()
        .and_then(|reference| reference.channel_id);
    let forwarded_snapshots =
        parse_message_snapshots(data.get("message_snapshots"), source_channel_id);

    Some(AppEvent::MessageCreate {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: author_name,
        author_avatar_url,
        message_kind,
        reference,
        reply,
        poll,
        content,
        mentions,
        attachments,
        forwarded_snapshots,
    })
}

fn parse_message_reference_info(value: &Value) -> MessageReferenceInfo {
    MessageReferenceInfo {
        guild_id: value.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: value.get("channel_id").and_then(parse_id::<ChannelMarker>),
        message_id: value.get("message_id").and_then(parse_id::<MessageMarker>),
    }
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
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let mentions = data
        .get("mentions")
        .map(|value| parse_mentions(Some(value)));
    Some(AppEvent::MessageUpdate {
        guild_id,
        channel_id,
        message_id,
        poll,
        content,
        mentions,
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

fn parse_member_chunk(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };

    let mut events: Vec<AppEvent> = data
        .get("members")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(parse_member_info)
                .map(|member| AppEvent::GuildMemberUpsert { guild_id, member })
                .collect()
        })
        .unwrap_or_default();

    if let Some(presences) = data.get("presences").and_then(Value::as_array) {
        events.extend(presences.iter().filter_map(parse_presence_entry).map(
            |(user_id, status)| AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
            },
        ));
    }

    events
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
    let author_name = message_author_display_name(value, author);
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mentions = parse_mentions(value.get("mentions"));

    Some(ReplyInfo {
        author: author_name,
        content,
        mentions,
    })
}

fn parse_mentions(value: Option<&Value>) -> Vec<MentionInfo> {
    value
        .and_then(Value::as_array)
        .map(|mentions| mentions.iter().filter_map(parse_mention_info).collect())
        .unwrap_or_default()
}

fn parse_mention_info(value: &Value) -> Option<MentionInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let nick = value
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = value
        .get("username")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    Some(MentionInfo {
        user_id,
        display_name: nick.or(global_name).or(username)?.to_owned(),
    })
}

fn message_author_display_name(message: &Value, author: &Value) -> String {
    let nick = message
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let global_name = author
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let username = author.get("username").and_then(Value::as_str);
    nick.or(global_name)
        .or(username)
        .unwrap_or("unknown")
        .to_owned()
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
    let total_votes = results
        .and_then(|results| results.get("answer_counts"))
        .and_then(Value::as_array)
        .map(|counts| {
            counts
                .iter()
                .filter_map(|count| count.get("count").and_then(Value::as_u64))
                .sum()
        });

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
        total_votes,
    })
}

fn parse_poll_result_embed(value: Option<&Value>) -> Option<PollInfo> {
    let embed = value?
        .as_array()?
        .iter()
        .find(|embed| embed.get("type").and_then(Value::as_str) == Some("poll_result"))?;
    let fields = embed.get("fields")?.as_array()?;
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;

    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(value) = field.get("value").and_then(Value::as_str) else {
            continue;
        };
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();
    Some(PollInfo {
        question: question.unwrap_or_else(|| "Poll results".to_owned()),
        answers,
        allow_multiselect: false,
        results_finalized: Some(true),
        total_votes,
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
    let mentions = parse_mentions(message.get("mentions"));
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
            mentions,
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

fn parse_presence_update(data: &Value) -> Vec<AppEvent> {
    let Some((user_id, status)) = parse_presence_entry(data) else {
        return Vec::new();
    };
    if let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) {
        vec![AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status,
        }]
    } else {
        vec![AppEvent::UserPresenceUpdate { user_id, status }]
    }
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
        recipients,
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

fn parse_channel_recipient_info(value: &Value) -> Option<ChannelRecipientInfo> {
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
        is_bot,
        avatar_url: raw_user_avatar_url(user_id, value),
        status,
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
    let is_bot = user.get("bot").and_then(Value::as_bool).unwrap_or(false);
    Some(MemberInfo {
        user_id,
        display_name,
        is_bot,
        avatar_url: raw_user_avatar_url(user_id, user),
        role_ids: value
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
            .unwrap_or_default(),
    })
}

fn raw_user_avatar_url(user_id: Id<UserMarker>, user: &Value) -> Option<String> {
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

fn parse_presence_entry(value: &Value) -> Option<(Id<UserMarker>, PresenceStatus)> {
    let user_id = presence_user_id(value)?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)?;
    Some((user_id, status))
}

fn presence_user_id(value: &Value) -> Option<Id<UserMarker>> {
    value
        .get("user")
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)
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

    use super::{
        gateway_intents, parse_channel_info, parse_guild_create, parse_guild_emojis_update,
        parse_guild_update, parse_message_create, parse_message_update, parse_user_account_event,
    };
    use crate::discord::{
        AppEvent, AttachmentUpdate, MentionInfo, MessageKind, PollAnswerInfo, PollInfo,
        PresenceStatus, ReplyInfo,
    };

    #[test]
    fn startup_intents_include_message_content() {
        let intents = gateway_intents();

        assert!(intents.contains(Intents::GUILDS));
        assert!(intents.contains(Intents::GUILD_MESSAGES));
        assert!(intents.contains(Intents::GUILD_EMOJIS_AND_STICKERS));
        assert!(intents.contains(Intents::DIRECT_MESSAGES));
        assert!(intents.contains(Intents::GUILD_MEMBERS));
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
    fn raw_ready_parser_keeps_group_dm_recipients() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [],
                    "merged_presences": {
                        "friends": [
                            { "user": { "id": "20" }, "status": "online" },
                            { "user": { "id": "30" }, "status": "idle" }
                        ]
                    },
                    "private_channels": [{
                        "id": "10",
                        "type": 3,
                        "name": "project chat",
                        "recipients": [
                            {
                                "id": "20",
                                "username": "alice",
                                "global_name": "Alice",
                                "bot": false
                            },
                            {
                                "id": "30",
                                "username": "helper-bot",
                                "bot": true
                            }
                        ]
                    }]
                }
            })
            .to_string(),
        );

        let channel = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(channel) => Some(channel),
                _ => None,
            })
            .expect("ready should emit a private channel upsert");
        let recipients = channel
            .recipients
            .as_ref()
            .expect("group dm should carry recipients");

        assert_eq!(channel.kind, "group-dm");
        assert_eq!(recipients.len(), 2);
        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].display_name, "Alice");
        assert!(!recipients[0].is_bot);
        assert_eq!(recipients[0].status, Some(PresenceStatus::Online));
        assert_eq!(recipients[1].display_name, "helper-bot");
        assert!(recipients[1].is_bot);
        assert_eq!(recipients[1].status, Some(PresenceStatus::Idle));
    }

    #[test]
    fn raw_ready_parser_applies_guild_merged_presence_to_dm_recipient() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [],
                    "merged_presences": {
                        "friends": [],
                        "guilds": [[
                            { "user_id": "20", "status": "idle" }
                        ]]
                    },
                    "private_channels": [{
                        "id": "10",
                        "type": 1,
                        "recipients": [{
                            "id": "20",
                            "username": "alice"
                        }]
                    }]
                }
            })
            .to_string(),
        );

        let channel = events
            .iter()
            .find_map(|event| match event {
                AppEvent::ChannelUpsert(channel) => Some(channel),
                _ => None,
            })
            .expect("ready should emit a private channel upsert");
        let recipients = channel
            .recipients
            .as_ref()
            .expect("dm should carry recipients");

        assert_eq!(channel.kind, "dm");
        assert_eq!(recipients[0].user_id, Id::new(20));
        assert_eq!(recipients[0].status, Some(PresenceStatus::Idle));
    }

    #[test]
    fn raw_ready_supplemental_updates_user_presences() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [
                            { "user_id": "20", "status": "online" }
                        ],
                        "guilds": [[
                            { "user_id": "30", "status": "idle" }
                        ]]
                    }
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status }
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AppEvent::UserPresenceUpdate { user_id, status }
                if *user_id == Id::new(30) && *status == PresenceStatus::Idle
        )));
    }

    #[test]
    fn raw_ready_supplemental_accepts_bare_id_presence_entries() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [
                            { "id": "20", "status": "online" }
                        ]
                    }
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        ));
    }

    #[test]
    fn raw_ready_supplemental_ignores_non_presence_ids() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY_SUPPLEMENTAL",
                "d": {
                    "merged_presences": {
                        "friends": [],
                        "metadata": { "id": "20" }
                    }
                }
            })
            .to_string(),
        );

        assert!(events.is_empty());
    }

    #[test]
    fn raw_presence_update_without_guild_updates_user_presence() {
        let events = parse_user_account_event(
            &json!({
                "t": "PRESENCE_UPDATE",
                "d": {
                    "user": { "id": "20" },
                    "status": "dnd"
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::DoNotDisturb
        ));
    }

    #[test]
    fn raw_presence_update_accepts_user_id_field() {
        let events = parse_user_account_event(
            &json!({
                "t": "PRESENCE_UPDATE",
                "d": {
                    "user_id": "20",
                    "status": "online"
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::UserPresenceUpdate { user_id, status }]
                if *user_id == Id::new(20) && *status == PresenceStatus::Online
        ));
    }

    #[test]
    fn thread_channel_parser_keeps_counts_and_status() {
        let channel = parse_channel_info(
            &json!({
                "id": "10",
                "guild_id": "1",
                "parent_id": "2",
                "type": 11,
                "name": "release notes",
                "message_count": 12,
                "total_message_sent": 14,
                "thread_metadata": { "archived": true, "locked": false }
            }),
            None,
        )
        .expect("thread channel should parse");

        assert_eq!(channel.kind, "thread");
        assert_eq!(channel.message_count, Some(12));
        assert_eq!(channel.total_message_sent, Some(14));
        assert_eq!(channel.thread_archived, Some(true));
        assert_eq!(channel.thread_locked, Some(false));
    }

    #[test]
    fn raw_thread_create_upserts_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_CREATE",
                "d": thread_payload(10, "release notes")
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelUpsert(channel)]
                if channel.channel_id == Id::new(10)
                    && channel.guild_id == Some(Id::new(1))
                    && channel.parent_id == Some(Id::new(2))
                    && channel.name == "release notes"
                    && channel.kind == "thread"
                    && channel.message_count == Some(12)
                    && channel.total_message_sent == Some(14)
                    && channel.thread_archived == Some(false)
                    && channel.thread_locked == Some(false)
        ));
    }

    #[test]
    fn raw_thread_update_upserts_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_UPDATE",
                "d": thread_payload(10, "renamed thread")
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelUpsert(channel)]
                if channel.channel_id == Id::new(10)
                    && channel.name == "renamed thread"
                    && channel.kind == "thread"
        ));
    }

    #[test]
    fn raw_thread_delete_removes_thread_channel() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_DELETE",
                "d": {
                    "id": "10",
                    "guild_id": "1",
                    "parent_id": "2",
                    "type": 11
                }
            })
            .to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [AppEvent::ChannelDelete { guild_id, channel_id }]
                if *guild_id == Some(Id::new(1)) && *channel_id == Id::new(10)
        ));
    }

    #[test]
    fn raw_thread_list_sync_upserts_all_threads() {
        let events = parse_user_account_event(
            &json!({
                "t": "THREAD_LIST_SYNC",
                "d": {
                    "guild_id": "1",
                    "channel_ids": ["2"],
                    "threads": [
                        thread_payload(10, "release notes"),
                        thread_payload(11, "bug reports")
                    ],
                    "members": []
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(10) && channel.name == "release notes"
        ));
        assert!(matches!(
            &events[1],
            AppEvent::ChannelUpsert(channel)
                if channel.channel_id == Id::new(11) && channel.name == "bug reports"
        ));
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
    fn guild_create_parser_keeps_custom_emojis() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "members": [],
            "presences": [],
            "emojis": [
                {
                    "id": "50",
                    "name": "party",
                    "animated": true,
                    "available": true
                },
                {
                    "id": "51",
                    "name": "sleep",
                    "available": false
                }
            ]
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { emojis, .. } = event else {
            panic!("expected guild create event");
        };
        assert_eq!(emojis.len(), 2);
        assert_eq!(emojis[0].id, Id::new(50));
        assert_eq!(emojis[0].name, "party");
        assert!(emojis[0].animated);
        assert!(emojis[0].available);
        assert!(!emojis[1].available);
    }

    #[test]
    fn guild_create_parser_keeps_roles() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "members": [],
            "presences": [],
            "roles": [{
                "id": "90",
                "name": "Admin",
                "color": 16755200,
                "position": 10,
                "hoist": true
            }],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { roles, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].id, Id::new(90));
        assert_eq!(roles[0].name, "Admin");
        assert_eq!(roles[0].color, Some(16755200));
        assert_eq!(roles[0].position, 10);
        assert!(roles[0].hoist);
    }

    #[test]
    fn guild_create_parser_keeps_active_threads() {
        let event = parse_guild_create(&json!({
            "id": "1",
            "name": "guild",
            "channels": [],
            "threads": [thread_payload(10, "release notes")],
            "members": [],
            "presences": [],
            "emojis": []
        }))
        .expect("guild create should parse");

        let AppEvent::GuildCreate { channels, .. } = event else {
            panic!("expected guild create event");
        };

        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].channel_id, Id::new(10));
        assert_eq!(channels[0].kind, "thread");
        assert_eq!(channels[0].name, "release notes");
    }

    #[test]
    fn raw_member_chunk_upserts_members_and_presences() {
        let events = parse_user_account_event(
            &json!({
                "t": "GUILD_MEMBERS_CHUNK",
                "d": {
                    "guild_id": "1",
                    "chunk_index": 0,
                    "chunk_count": 1,
                    "members": [
                        {
                            "nick": "Alice Nick",
                            "user": {
                                "id": "10",
                                "username": "alice",
                                "global_name": "Alice Global",
                                "avatar": "avatarhash"
                            }
                        },
                        {
                            "user": {
                                "id": "20",
                                "username": "bob",
                                "bot": true
                            }
                        }
                    ],
                    "presences": [
                        { "user": { "id": "10" }, "status": "online" },
                        { "user": { "id": "20" }, "status": "idle" }
                    ]
                }
            })
            .to_string(),
        );

        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(10)
                    && member.display_name == "Alice Nick"
                    && !member.is_bot
        ));
        assert!(matches!(
            &events[1],
            AppEvent::GuildMemberUpsert { guild_id, member }
                if *guild_id == Id::new(1)
                    && member.user_id == Id::new(20)
                    && member.display_name == "bob"
                    && member.is_bot
        ));
        assert!(matches!(
            &events[2],
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(10)
                    && *status == PresenceStatus::Online
        ));
        assert!(matches!(
            &events[3],
            AppEvent::PresenceUpdate { guild_id, user_id, status }
                if *guild_id == Id::new(1)
                    && *user_id == Id::new(20)
                    && *status == PresenceStatus::Idle
        ));
    }

    #[test]
    fn raw_ready_parser_keeps_guild_custom_emojis() {
        let events = parse_user_account_event(
            &json!({
                "t": "READY",
                "d": {
                    "user": {
                        "id": "99",
                        "username": "neo"
                    },
                    "guilds": [{
                        "id": "1",
                        "name": "guild",
                        "channels": [],
                        "members": [],
                        "presences": [],
                        "emojis": [{
                            "id": "50",
                            "name": "party_time",
                            "animated": true,
                            "available": true
                        }]
                    }],
                    "private_channels": []
                }
            })
            .to_string(),
        );

        let guild_create = events
            .iter()
            .find_map(|event| match event {
                AppEvent::GuildCreate { emojis, .. } => Some(emojis),
                _ => None,
            })
            .expect("ready should emit a guild create event");

        assert_eq!(guild_create.len(), 1);
        assert_eq!(guild_create[0].id, Id::new(50));
        assert_eq!(guild_create[0].name, "party_time");
        assert!(guild_create[0].animated);
        assert!(guild_create[0].available);
    }

    #[test]
    fn guild_emojis_update_parser_replaces_custom_emojis() {
        let event = parse_guild_emojis_update(&json!({
            "guild_id": "1",
            "emojis": [
                {
                    "id": "60",
                    "name": "wave",
                    "animated": false,
                    "available": true
                }
            ]
        }))
        .expect("guild emojis update should parse");

        let AppEvent::GuildEmojisUpdate { guild_id, emojis } = event else {
            panic!("expected guild emojis update event");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(60));
        assert_eq!(emojis[0].name, "wave");
        assert!(emojis[0].available);
    }

    #[test]
    fn guild_update_parser_keeps_custom_emojis_when_present() {
        let event = parse_guild_update(&json!({
            "id": "1",
            "name": "guild renamed",
            "emojis": [{
                "id": "70",
                "name": "dance",
                "animated": true,
                "available": true
            }]
        }))
        .expect("guild update should parse");

        let AppEvent::GuildUpdate {
            guild_id,
            name,
            roles,
            emojis,
        } = event
        else {
            panic!("expected guild update event");
        };
        assert_eq!(guild_id, Id::new(1));
        assert_eq!(name, "guild renamed");
        assert_eq!(roles, None);
        let emojis = emojis.expect("emoji field should be preserved when present");
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(70));
        assert_eq!(emojis[0].name, "dance");
        assert!(emojis[0].animated);
    }

    #[test]
    fn guild_update_parser_distinguishes_missing_custom_emojis() {
        let event = parse_guild_update(&json!({
            "id": "1",
            "name": "guild renamed"
        }))
        .expect("guild update should parse");

        let AppEvent::GuildUpdate { roles, emojis, .. } = event else {
            panic!("expected guild update event");
        };
        assert_eq!(roles, None);
        assert_eq!(emojis, None);
    }

    #[test]
    fn message_update_parser_keeps_mentions_when_present() {
        let event = parse_message_update(&json!({
            "id": "20",
            "channel_id": "10",
            "content": "edited <@40>",
            "mentions": [{ "id": "40", "username": "alice" }]
        }))
        .expect("message update should parse");

        let AppEvent::MessageUpdate { mentions, .. } = event else {
            panic!("expected message update event");
        };
        assert_eq!(mentions, Some(vec![mention_info(40, "alice")]));
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
    fn message_create_parser_prefers_member_nick_for_author() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "guild_id": "1",
            "author": { "id": "30", "global_name": "global", "username": "neo" },
            "member": { "nick": "server alias" },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { author, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(author, "server alias");
    }

    #[test]
    fn message_create_parser_builds_author_avatar_url() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": {
                "id": "30",
                "username": "neo",
                "avatar": "a_avatarhash"
            },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            author_avatar_url, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(
            author_avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/avatars/30/a_avatarhash.gif")
        );
    }

    #[test]
    fn message_create_parser_falls_back_to_global_name_without_member() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "global_name": "global alias", "username": "neo" },
            "content": "hello",
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate {
            author, guild_id, ..
        } = event
        else {
            panic!("expected message create event");
        };
        assert_eq!(guild_id, None);
        assert_eq!(author, "global alias");
    }

    #[test]
    fn message_create_parser_keeps_mention_display_names() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "hello <@40> <@41> <@42>",
            "mentions": [
                {
                    "id": "40",
                    "username": "alpha",
                    "global_name": "Alpha Global",
                    "member": { "nick": "Alpha Nick" }
                },
                {
                    "id": "41",
                    "username": "beta",
                    "global_name": "Beta Global"
                },
                {
                    "id": "42",
                    "username": "gamma"
                }
            ],
            "attachments": []
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { mentions, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            mentions,
            vec![
                mention_info(40, "Alpha Nick"),
                mention_info(41, "Beta Global"),
                mention_info(42, "gamma"),
            ]
        );
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
                mentions: Vec::new(),
            })
        );
    }

    #[test]
    fn message_create_parser_keeps_reply_mentions() {
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
                "author": { "id": "31", "username": "alex" },
                "content": "hello <@40>",
                "mentions": [{ "id": "40", "username": "alice" }],
                "attachments": []
            }
        }))
        .expect("message create should parse");

        let AppEvent::MessageCreate { reply, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            reply.and_then(|reply| reply.mentions.into_iter().next()),
            Some(mention_info(40, "alice"))
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
                total_votes: Some(3),
            })
        );
    }

    #[test]
    fn message_create_parser_keeps_poll_result_embed() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "type": 46,
            "content": "",
            "attachments": [],
            "embeds": [{
                "type": "poll_result",
                "fields": [
                    { "name": "poll_question_text", "value": "오늘 뭐 먹지?" },
                    { "name": "victor_answer_id", "value": "1" },
                    { "name": "victor_answer_text", "value": "김치찌개" },
                    { "name": "victor_answer_votes", "value": "5" },
                    { "name": "total_votes", "value": "7" }
                ]
            }]
        }))
        .expect("poll result message should parse");

        let AppEvent::MessageCreate { poll, .. } = event else {
            panic!("expected message create event");
        };
        assert_eq!(
            poll.expect("poll result should map to poll info")
                .total_votes,
            Some(7)
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
    fn message_create_parser_keeps_forwarded_snapshot_mentions() {
        let event = parse_message_create(&json!({
            "id": "20",
            "channel_id": "10",
            "author": { "id": "30", "username": "neo" },
            "content": "",
            "attachments": [],
            "message_snapshots": [{
                "message": {
                    "content": "hello <@40>",
                    "mentions": [{ "id": "40", "username": "alice" }],
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
            forwarded_snapshots[0].mentions,
            vec![mention_info(40, "alice")]
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

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
        }
    }

    fn thread_payload(id: u64, name: &str) -> serde_json::Value {
        json!({
            "id": id.to_string(),
            "guild_id": "1",
            "parent_id": "2",
            "type": 11,
            "name": name,
            "message_count": 12,
            "total_message_sent": 14,
            "thread_metadata": { "archived": false, "locked": false }
        })
    }
}
