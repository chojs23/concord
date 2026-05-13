use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::{
    discord::{
        ActivityInfo, ChannelInfo, ChannelRecipientInfo, GuildFolder, PresenceStatus,
        ReadStateInfo, RelationshipInfo, RoleInfo,
        events::AppEvent,
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
        },
    },
    logging,
};

use super::{
    channels::{parse_channel_info, parse_channel_recipient_info},
    guilds::{
        parse_guild_create, parse_role_info, parse_user_can_use_animated_custom_emojis,
        parse_user_guild_settings_entries,
    },
    members::parse_member_info,
    presence::parse_presence_entry,
    relationships::parse_relationship_entry,
    shared::{parse_id, parse_status},
};

/// User-account READY embeds the full guild list under `d.guilds`. Bots get a
/// stub list of unavailable guilds and a separate `GUILD_CREATE` per guild,
/// but user accounts never send standalone GUILD_CREATEs, so we emit a
/// synthetic GuildCreate for each entry inline.
pub(super) fn parse_ready(data: &Value) -> Vec<AppEvent> {
    let total_started = Instant::now();
    let mut events = Vec::new();
    let mut stats = ReadyTimingStats::default();
    let mut current_user = None;
    let mut current_user_id = None;
    let mut current_user_status = None;

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
        if let Some(can_use_animated_custom_emojis) =
            parse_user_can_use_animated_custom_emojis(user)
        {
            events.push(AppEvent::CurrentUserCapabilities {
                can_use_animated_custom_emojis,
            });
        }
        current_user_id = user_id;
        current_user = parse_channel_recipient_info(user);
        current_user_status = parse_current_user_session_status(data);
        if let (Some(user), Some(status)) = (current_user.as_mut(), current_user_status) {
            user.status = Some(status);
        }
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
    events.extend(parse_merged_member_events(data));
    stats.guilds_duration = guilds_started.elapsed();

    let mut merged_presences = parse_merged_presences(data);
    if let Some(presences) = data.get("presences").and_then(Value::as_array) {
        merged_presences.extend(
            presences
                .iter()
                .filter_map(parse_presence_entry)
                .map(|(user_id, status, activities)| (user_id, (status, activities))),
        );
    }

    // With DEDUPE_USER_OBJECTS in capabilities (bit 4), Discord ships every
    // referenced user once at the top of READY's `users` array and replaces
    // each private channel's full `recipients` array with `recipient_ids`.
    // Index those users by id once so DM hydration below is O(1) per
    // recipient.
    let users_by_id: BTreeMap<Id<UserMarker>, &Value> = data
        .get("users")
        .and_then(Value::as_array)
        .map(|users| {
            users
                .iter()
                .filter_map(|user| {
                    let id = parse_id::<UserMarker>(user.get("id")?)?;
                    Some((id, user))
                })
                .collect()
        })
        .unwrap_or_default();

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    let private_channels_started = Instant::now();
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        stats.private_channels = privates.len();
        for channel in privates {
            if let Some(mut info) = parse_channel_info(channel, None) {
                hydrate_dm_recipients_from_ids(&mut info, channel, &users_by_id);
                apply_recipient_presences(&mut info, &merged_presences);
                add_current_user_to_group_dm(&mut info, current_user.as_ref());
                events.push(AppEvent::ChannelUpsert(info));
            }
        }
    }
    stats.private_channels_duration = private_channels_started.elapsed();

    if let (Some(user_id), Some(status)) = (current_user_id, current_user_status) {
        events.push(AppEvent::UserPresenceUpdate {
            user_id,
            status,
            activities: Vec::new(),
        });
    }

    // User-account READY ships the friend list as `relationships`. Capture
    // it as a single event so the profile popup can show friend / pending /
    // blocked badges without an extra REST round trip.
    if let Some(relationships) = data.get("relationships").and_then(Value::as_array) {
        let parsed: Vec<RelationshipInfo> = relationships
            .iter()
            .filter_map(parse_relationship_entry)
            .collect();
        if !parsed.is_empty() {
            events.push(AppEvent::RelationshipsLoaded {
                relationships: parsed,
            });
        }
    }

    // VERSIONED_READ_STATES wraps the array as `{ entries, version, partial }`.
    // older shards send a bare array. Accept both.
    if let Some(entries) = data
        .get("read_state")
        .and_then(|node| node.get("entries").or(Some(node)))
        .and_then(Value::as_array)
    {
        let parsed: Vec<ReadStateInfo> =
            entries.iter().filter_map(parse_read_state_entry).collect();
        if !parsed.is_empty() {
            events.push(AppEvent::ReadStateInit { entries: parsed });
        }
    }

    if let Some(settings) = parse_user_guild_settings_entries(data.get("user_guild_settings")) {
        events.push(AppEvent::UserGuildNotificationSettingsInit { settings });
    }

    // Guild folder ordering and grouping live in the legacy `user_settings`
    // payload. The modern `user_settings_proto` blob is base64+protobuf and is
    // skipped for now. When present, every guild appears in some folder, either
    // an explicit one or a single-guild "container" with `id == null`.
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

pub(super) fn parse_ready_supplemental(data: &Value) -> Vec<AppEvent> {
    let mut events = parse_supplemental_guild_events(data);
    events.extend(parse_merged_member_events(data));
    events.extend(parse_merged_presences(data).into_iter().map(
        |(user_id, (status, activities))| AppEvent::UserPresenceUpdate {
            user_id,
            status,
            activities,
        },
    ));
    events
}

fn parse_merged_member_events(data: &Value) -> Vec<AppEvent> {
    let Some(guilds) = data.get("guilds").and_then(Value::as_array) else {
        return Vec::new();
    };
    let Some(merged_members) = data.get("merged_members").and_then(Value::as_array) else {
        return Vec::new();
    };

    guilds
        .iter()
        .zip(merged_members)
        .flat_map(|(guild, members)| {
            let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) else {
                return Vec::new();
            };
            members
                .as_array()
                .map(|members| guild_member_upsert_events(guild_id, members))
                .unwrap_or_default()
        })
        .collect()
}

fn parse_supplemental_guild_events(data: &Value) -> Vec<AppEvent> {
    let Some(guilds) = data.get("guilds").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut events = Vec::new();
    for guild in guilds {
        let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) else {
            continue;
        };
        if let Some(roles) = guild.get("roles").and_then(Value::as_array) {
            let roles: Vec<RoleInfo> = roles.iter().filter_map(parse_role_info).collect();
            if !roles.is_empty() {
                events.push(AppEvent::GuildRolesUpdate { guild_id, roles });
            }
        }
        if let Some(channels) = guild.get("channels").and_then(Value::as_array) {
            events.extend(
                channels
                    .iter()
                    .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                    .map(AppEvent::ChannelUpsert),
            );
        }
        if let Some(threads) = guild.get("threads").and_then(Value::as_array) {
            events.extend(
                threads
                    .iter()
                    .filter_map(|channel| parse_channel_info(channel, Some(guild_id)))
                    .map(AppEvent::ChannelUpsert),
            );
        }
        if let Some(members) = guild.get("members").and_then(Value::as_array) {
            events.extend(guild_member_upsert_events(guild_id, members));
        }
        if let Some(member) = guild.get("member").and_then(parse_member_info) {
            events.push(AppEvent::GuildMemberUpsert { guild_id, member });
        }
        if let Some(presences) = guild.get("presences").and_then(Value::as_array) {
            events.extend(presences.iter().filter_map(parse_presence_entry).map(
                |(user_id, status, activities)| AppEvent::PresenceUpdate {
                    guild_id,
                    user_id,
                    status,
                    activities,
                },
            ));
        }
    }
    events
}

fn guild_member_upsert_events(guild_id: Id<GuildMarker>, members: &[Value]) -> Vec<AppEvent> {
    members
        .iter()
        .filter_map(parse_member_info)
        .map(|member| AppEvent::GuildMemberUpsert { guild_id, member })
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

type MergedPresences = BTreeMap<Id<UserMarker>, (PresenceStatus, Vec<ActivityInfo>)>;

fn parse_merged_presences(data: &Value) -> MergedPresences {
    let mut presences = MergedPresences::new();
    if let Some(merged) = data.get("merged_presences") {
        collect_presence_entries(merged, &mut presences);
    }
    presences
}

fn parse_current_user_session_status(data: &Value) -> Option<PresenceStatus> {
    data.get("sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| {
            sessions.iter().find_map(|session| {
                let status = session
                    .get("status")
                    .and_then(Value::as_str)
                    .map(parse_status)?;
                (status != PresenceStatus::Unknown).then_some(status)
            })
        })
}

fn collect_presence_entries(value: &Value, presences: &mut MergedPresences) {
    if let Some((user_id, status, activities)) = parse_presence_entry(value) {
        presences.insert(user_id, (status, activities));
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

fn apply_recipient_presences(channel: &mut ChannelInfo, presences: &MergedPresences) {
    let Some(recipients) = channel.recipients.as_mut() else {
        return;
    };
    for recipient in recipients {
        if let Some((status, _)) = presences.get(&recipient.user_id) {
            recipient.status = Some(*status);
        }
    }
}

/// Resolves a private channel's `recipient_ids` against READY's deduplicated
/// `users` array. With `DEDUPE_USER_OBJECTS` enabled Discord no longer
/// inlines the full recipient objects in private channels, so without this
/// step DM rows render as `dm-{channel_id}` and the recipient sidebar is
/// empty.
fn hydrate_dm_recipients_from_ids(
    channel: &mut ChannelInfo,
    raw: &Value,
    users_by_id: &BTreeMap<Id<UserMarker>, &Value>,
) {
    if !matches!(channel.kind.as_str(), "dm" | "group-dm") {
        return;
    }
    if channel
        .recipients
        .as_ref()
        .is_some_and(|recipients| !recipients.is_empty())
    {
        return;
    }
    let Some(ids) = raw.get("recipient_ids").and_then(Value::as_array) else {
        return;
    };
    let resolved: Vec<ChannelRecipientInfo> = ids
        .iter()
        .filter_map(parse_id::<UserMarker>)
        .filter_map(|user_id| {
            let user = users_by_id.get(&user_id)?;
            parse_channel_recipient_info(user)
        })
        .collect();
    if resolved.is_empty() {
        return;
    }
    // The previous `parse_channel_info` couldn't see the recipients, so its
    // name was a synthetic `dm-{channel_id}`. Rebuild the human-readable
    // label now using the same global_name → username preference the rest of
    // the parser uses.
    let synthetic_label = format!("dm-{}", channel.channel_id.get());
    if channel.name == synthetic_label {
        channel.name = resolved
            .iter()
            .map(|recipient| recipient.display_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    channel.recipients = Some(resolved);
}

fn add_current_user_to_group_dm(
    channel: &mut ChannelInfo,
    current_user: Option<&ChannelRecipientInfo>,
) {
    if channel.kind != "group-dm" {
        return;
    }
    let Some(current_user) = current_user else {
        return;
    };
    let Some(recipients) = channel.recipients.as_mut() else {
        return;
    };
    if recipients
        .iter()
        .any(|recipient| recipient.user_id == current_user.user_id)
    {
        return;
    }
    recipients.push(current_user.clone());
}

fn channel_array_len(value: &Value, field: &str) -> usize {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

fn parse_read_state_entry(value: &Value) -> Option<ReadStateInfo> {
    let channel_id = parse_id::<ChannelMarker>(value.get("id")?)?;
    Some(ReadStateInfo {
        channel_id,
        last_acked_message_id: value
            .get("last_message_id")
            .and_then(parse_id::<MessageMarker>),
        mention_count: value
            .get("mention_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    })
}
