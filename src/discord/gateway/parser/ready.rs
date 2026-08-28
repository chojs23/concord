use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::{
    ChannelInfo, ChannelRecipientInfo, PresenceStatus, ReadStateInfo, RelationshipInfo, RoleInfo,
    ThreadMemberInfo,
    events::{AppEvent, PresenceEventFields, ReadySnapshotInfo},
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use super::{
    channels::{parse_channel_info, parse_channel_recipient_info},
    guilds::{
        parse_guild_create, parse_guild_onboarding_from_guild, parse_role_info,
        parse_user_guild_settings_entries, parse_user_premium_tier,
    },
    members::{parse_current_user_verification, parse_member_info},
    presence::{parse_activities, parse_presence_entry},
    relationships::parse_relationship_entry,
    shared::{display_name_from_parts_or_unknown, parse_id, parse_status},
    user_settings::parse_user_settings_info,
    voice::parse_guild_voice_states,
};

/// User-account READY embeds the full guild list under `d.guilds`. Bots get a
/// stub list of unavailable guilds and a separate `GUILD_CREATE` per guild,
/// but user accounts never send standalone GUILD_CREATEs, so we emit a
/// synthetic GuildCreate for each entry inline.
pub(super) fn parse_ready(data: &Value) -> Vec<AppEvent> {
    let mut events = Vec::new();
    let mut current_user = None;
    let mut current_user_id = None;
    let mut current_user_presence = None;

    if let Some(user) = data.get("user") {
        let user_id = user.get("id").and_then(parse_id::<UserMarker>);
        let name = display_name_from_parts_or_unknown(
            None,
            user.get("global_name").and_then(Value::as_str),
            user.get("username").and_then(Value::as_str),
        );
        events.push(AppEvent::Ready {
            user: name,
            user_id,
        });
        if let Some(premium_tier) = parse_user_premium_tier(user) {
            events.push(AppEvent::CurrentUserCapabilities { premium_tier });
        }
        if let Some(event) = parse_current_user_verification(user) {
            events.push(event);
        }
        current_user_id = user_id;
        current_user = parse_channel_recipient_info(user);
        current_user_presence = parse_current_user_session_presence(data);
        if let (Some(user), Some((status, _))) =
            (current_user.as_mut(), current_user_presence.as_ref())
        {
            user.status = Some(*status);
        }
    }

    if let Some(guilds) = data.get("guilds").and_then(Value::as_array) {
        for guild in guilds {
            if guild.get("unavailable").and_then(Value::as_bool) == Some(true) {
                if let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) {
                    events.push(AppEvent::GuildUnavailable { guild_id });
                }
                continue;
            }
            if let Some(event) = parse_guild_create(guild) {
                events.push(event);
            }
            events.extend(parse_guild_voice_states(guild));
        }
    }
    events.extend(parse_merged_member_events(data));

    let mut merged_presences = parse_merged_presences(data);
    if let Some(presences) = data.get("presences").and_then(Value::as_array) {
        merged_presences.extend(
            presences
                .iter()
                .filter_map(parse_presence_entry)
                .map(|presence| (presence.user_id, presence)),
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
    let mut ready_users = users_by_id
        .values()
        .filter_map(|user| parse_channel_recipient_info(user))
        .collect::<Vec<_>>();
    if let Some(current_user) = current_user.clone()
        && !ready_users
            .iter()
            .any(|user| user.user_id == current_user.user_id)
    {
        ready_users.push(current_user);
    }
    if !ready_users.is_empty() {
        events.push(AppEvent::ReadyUserDirectory { users: ready_users });
    }

    // User-account READY also lists DM and group-DM channels under
    // `private_channels`. They have no `guild_id` and never come through
    // `GUILD_CREATE`, so we surface them as standalone channel upserts.
    if let Some(privates) = data.get("private_channels").and_then(Value::as_array) {
        for channel in privates {
            if let Some(mut info) = parse_channel_info(channel, None) {
                hydrate_dm_recipients_from_ids(&mut info, channel, &users_by_id);
                apply_recipient_presences(&mut info, &merged_presences);
                add_current_user_to_group_dm(&mut info, current_user.as_ref());
                events.push(AppEvent::ChannelUpsert(info));
            }
        }
    }

    if let (Some(user_id), Some((status, activities))) = (current_user_id, current_user_presence) {
        events.push(AppEvent::PresenceUpdate {
            guild_id: None,
            presence: PresenceEventFields {
                user_id,
                status,
                activities,
            },
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
        // The READY list is authoritative. Emit an empty list too so a new
        // session can clear relationships that disappeared while offline.
        events.push(AppEvent::RelationshipsLoaded {
            relationships: parsed,
        });
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
        let (partial, version) = versioned_array_metadata(data.get("read_state"));
        events.push(AppEvent::ReadStateSync {
            entries: parsed,
            partial,
            version,
        });
    }

    if let Some(settings) = parse_user_guild_settings_entries(data.get("user_guild_settings")) {
        let (partial, version) = versioned_array_metadata(data.get("user_guild_settings"));
        events.push(AppEvent::UserGuildSettingsSync {
            settings,
            partial,
            version,
        });
    }
    if let Some(flags) = data
        .get("notification_settings")
        .and_then(|settings| settings.get("flags"))
        .and_then(Value::as_u64)
    {
        events.push(AppEvent::UserNotificationSettingsUpdate { flags });
    }

    // Guild folder ordering and grouping live in the legacy `user_settings`
    // payload. The modern `user_settings_proto` blob is base64+protobuf and is
    // skipped for now. When present, every guild appears in some folder, either
    // an explicit one or a single-guild "container" with `id == null`.
    if let Some(settings) = data.get("user_settings").and_then(parse_user_settings_info) {
        events.push(AppEvent::UserSettingsUpdate { settings });
    }

    if let Some(snapshot) = parse_ready_snapshot(data) {
        events.push(AppEvent::ReadySnapshotComplete { snapshot });
    }

    events
}

fn versioned_array_metadata(value: Option<&Value>) -> (bool, Option<i64>) {
    let Some(value) = value.filter(|value| value.is_object()) else {
        return (false, None);
    };
    let partial = value
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let version = value.get("version").and_then(|version| {
        version
            .as_i64()
            .or_else(|| version.as_u64().and_then(|value| i64::try_from(value).ok()))
    });
    (partial, version)
}

pub(super) fn parse_ready_supplemental(data: &Value) -> Vec<AppEvent> {
    let mut events = parse_supplemental_guild_events(data);
    events.extend(parse_merged_member_events(data));
    events.extend(parse_merged_presences(data).into_values().map(|presence| {
        AppEvent::PresenceUpdate {
            guild_id: None,
            presence,
        }
    }));
    if let Some(channels) = data.get("lazy_private_channels").and_then(Value::as_array) {
        for raw_channel in channels {
            let Some(channel) = parse_channel_info(raw_channel, None) else {
                continue;
            };
            let recipient_ids = raw_channel
                .get("recipient_ids")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(parse_id::<UserMarker>)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if recipient_ids.is_empty() {
                events.push(AppEvent::ChannelUpsert(channel));
            } else {
                events.push(AppEvent::LazyPrivateChannelUpsert {
                    channel,
                    recipient_ids,
                });
            }
        }
        events.push(AppEvent::ReadySupplementalComplete {
            private_channel_ids: channels
                .iter()
                .filter_map(|channel| channel.get("id").and_then(parse_id::<ChannelMarker>))
                .collect(),
        });
    }
    events
}

fn parse_ready_snapshot(data: &Value) -> Option<ReadySnapshotInfo> {
    let guilds = data.get("guilds").and_then(Value::as_array);
    let guild_ids = guilds.map(|guilds| {
        guilds
            .iter()
            .filter_map(|guild| guild.get("id").and_then(parse_id::<GuildMarker>))
            .collect()
    });
    let mut guild_channel_ids = BTreeMap::new();
    if let Some(guilds) = guilds {
        for guild in guilds {
            let Some(guild_id) = guild.get("id").and_then(parse_id::<GuildMarker>) else {
                continue;
            };
            // An unavailable guild omits `channels`. Its old channel state is
            // retained until Discord sends an available snapshot.
            let Some(channels) = guild.get("channels").and_then(Value::as_array) else {
                continue;
            };
            let mut channel_ids = channels
                .iter()
                .filter_map(|channel| channel.get("id").and_then(parse_id::<ChannelMarker>))
                .collect::<Vec<_>>();
            if let Some(threads) = guild.get("threads").and_then(Value::as_array) {
                channel_ids.extend(
                    threads
                        .iter()
                        .filter_map(|thread| thread.get("id").and_then(parse_id::<ChannelMarker>)),
                );
            }
            guild_channel_ids.insert(guild_id, channel_ids);
        }
    }
    let private_channel_ids =
        data.get("private_channels")
            .and_then(Value::as_array)
            .map(|channels| {
                channels
                    .iter()
                    .filter_map(|channel| channel.get("id").and_then(parse_id::<ChannelMarker>))
                    .collect()
            });

    (guild_ids.is_some() || private_channel_ids.is_some()).then_some(ReadySnapshotInfo {
        guild_ids,
        guild_channel_ids,
        private_channel_ids,
    })
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
        if let Some(onboarding) = parse_guild_onboarding_from_guild(guild, guild_id) {
            events.push(AppEvent::GuildOnboardingUpdate {
                guild_id,
                onboarding,
            });
        }
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
            events.extend(threads.iter().filter_map(|thread| {
                let mut thread =
                    super::channels::parse_thread_gateway_info(thread, Some(guild_id))?;
                if thread.current_user_member.is_none() {
                    thread.current_user_member =
                        Some(ThreadMemberInfo::joined_snapshot(thread.channel.channel_id));
                }
                Some(AppEvent::ThreadUpsert {
                    thread,
                    created: false,
                })
            }));
        }
        if let Some(members) = guild.get("members").and_then(Value::as_array) {
            events.extend(guild_member_upsert_events(guild_id, members));
        }
        if let Some(member) = guild
            .get("member")
            .and_then(|member| parse_member_info(member, Some(guild_id)))
        {
            events.push(AppEvent::GuildMemberUpsert { guild_id, member });
        }
        if let Some(presences) = guild.get("presences").and_then(Value::as_array) {
            events.extend(
                presences
                    .iter()
                    .filter_map(parse_presence_entry)
                    .map(|presence| AppEvent::PresenceUpdate {
                        guild_id: Some(guild_id),
                        presence,
                    }),
            );
        }
        events.extend(parse_guild_voice_states(guild));
    }
    events
}

fn guild_member_upsert_events(guild_id: Id<GuildMarker>, members: &[Value]) -> Vec<AppEvent> {
    members
        .iter()
        .filter_map(|member| parse_member_info(member, Some(guild_id)))
        .map(|member| AppEvent::GuildMemberUpsert { guild_id, member })
        .collect()
}

type MergedPresences = BTreeMap<Id<UserMarker>, PresenceEventFields>;

fn parse_merged_presences(data: &Value) -> MergedPresences {
    let mut presences = MergedPresences::new();
    if let Some(merged) = data.get("merged_presences") {
        collect_presence_entries(merged, &mut presences);
    }
    presences
}

fn parse_current_user_session_presence(
    data: &Value,
) -> Option<(PresenceStatus, Vec<crate::discord::ActivityInfo>)> {
    let sessions = data.get("sessions").and_then(Value::as_array)?;
    let parse = |session: &Value| {
        let status = session
            .get("status")
            .and_then(Value::as_str)
            .map(parse_status)?;
        Some((status, parse_activities(session)))
    };

    sessions
        .iter()
        .find(|session| session.get("session_id").and_then(Value::as_str) == Some("all"))
        .and_then(parse)
        .or_else(|| sessions.iter().find_map(parse))
}

fn collect_presence_entries(value: &Value, presences: &mut MergedPresences) {
    if let Some(presence) = parse_presence_entry(value) {
        presences.insert(presence.user_id, presence);
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
        if let Some(presence) = presences.get(&recipient.user_id) {
            recipient.status = Some(presence.status);
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

fn parse_read_state_entry(value: &Value) -> Option<ReadStateInfo> {
    let channel_id = parse_id::<ChannelMarker>(value.get("id")?)?;
    Some(ReadStateInfo {
        read_state_type: value
            .get("read_state_type")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(0),
        channel_id,
        last_acked_message_id: value
            .get("last_message_id")
            .or_else(|| value.get("last_acked_id"))
            .and_then(parse_id::<MessageMarker>),
        mention_count: value
            .get("mention_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        badge_count: value
            .get("badge_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        last_pin_timestamp: value
            .get("last_pin_timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned),
        flags: value.get("flags").and_then(Value::as_u64).unwrap_or(0),
        last_viewed: value.get("last_viewed").and_then(Value::as_u64),
    })
}
