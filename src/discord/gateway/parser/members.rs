use serde_json::Value;

use crate::discord::{
    GuildMemberListUpdateInfo, GuildMembersChunkInfo, MemberInfo,
    events::{AppEvent, PresenceEventFields},
    ids::{
        Id,
        marker::{GuildMarker, RoleMarker, UserMarker},
    },
};

use super::{
    presence::{parse_activities, parse_presence_entry},
    shared::{
        display_name_from_parts_or_unknown, extra_fields, parse_id, parse_status,
        raw_member_avatar_url, raw_user_avatar_url,
    },
};

pub(super) fn parse_member_upsert(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data, Some(guild_id))?;
    Some(AppEvent::GuildMemberUpsert { guild_id, member })
}

pub(super) fn parse_member_add(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data, Some(guild_id))?;
    Some(AppEvent::GuildMemberAdd { guild_id, member })
}

pub(super) fn parse_user_update(data: &Value) -> Option<AppEvent> {
    let user_id = parse_id::<UserMarker>(data.get("id")?)?;
    let username = data.get("username").and_then(Value::as_str)?.to_owned();
    let global_name = data
        .get("global_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(AppEvent::UserIdentityUpdate {
        user_id,
        username,
        global_name,
        avatar_url: raw_user_avatar_url(user_id, data),
        is_bot: data.get("bot").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub(super) fn parse_member_chunk(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };

    let members = data
        .get("members")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| parse_member_info(member, Some(guild_id)))
                .collect()
        })
        .unwrap_or_default();

    let presences = data
        .get("presences")
        .and_then(Value::as_array)
        .map(|presences| presences.iter().filter_map(parse_presence_entry).collect())
        .unwrap_or_default();

    vec![AppEvent::GuildMembersChunk {
        chunk: GuildMembersChunkInfo {
            guild_id,
            members,
            presences,
            chunk_index: data.get("chunk_index").and_then(Value::as_u64),
            chunk_count: data.get("chunk_count").and_then(Value::as_u64),
            nonce: data.get("nonce").and_then(Value::as_str).map(str::to_owned),
            not_found: parse_id_array(data.get("not_found")),
            extra_fields: extra_fields(
                data,
                &[
                    "guild_id",
                    "members",
                    "presences",
                    "chunk_index",
                    "chunk_count",
                    "nonce",
                    "not_found",
                ],
            ),
        },
    }]
}

pub(super) fn parse_member_list_update(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };
    let Some(ops) = data.get("ops").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut online_count = None;
    let mut members = Vec::new();
    let mut presences = Vec::new();

    if let Some(groups) = data.get("groups").and_then(Value::as_array) {
        online_count = Some(
            groups
                .iter()
                .filter(|g| g.get("id").and_then(Value::as_str) != Some("offline"))
                .filter_map(|g| g.get("count").and_then(Value::as_u64))
                .map(|c| c as u32)
                .sum(),
        );
    }

    // A single GUILD_MEMBER_LIST_UPDATE event can carry SYNC ops for several
    // ranges (e.g. `[0,99]` plus `[100,199]`). We previously dropped every
    // SYNC whose range did not start at zero, which left members past the
    // first chunk invisible in larger guilds.
    for op in ops {
        match op.get("op").and_then(Value::as_str) {
            Some("SYNC") => {
                if let Some(items) = op.get("items").and_then(Value::as_array) {
                    for item in items {
                        if let Some(item) = parse_member_list_item(guild_id, item) {
                            members.push(item.member);
                            if let Some(presence) = item.presence {
                                presences.push(presence);
                            }
                        }
                    }
                }
            }
            Some("INSERT" | "UPDATE") => {
                if let Some(item) = op.get("item") {
                    if let Some(item) = parse_member_list_item(guild_id, item) {
                        members.push(item.member);
                        if let Some(presence) = item.presence {
                            presences.push(presence);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    vec![AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: data.get("id").and_then(Value::as_str).map(str::to_owned),
            member_count: data.get("member_count").and_then(Value::as_u64),
            online_count,
            members,
            presences,
            groups: clone_array(data.get("groups")),
            ops: ops.to_vec(),
            extra_fields: extra_fields(
                data,
                &[
                    "guild_id",
                    "id",
                    "member_count",
                    "online_count",
                    "groups",
                    "ops",
                ],
            ),
        },
    }]
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

struct MemberListItemInfo {
    member: MemberInfo,
    presence: Option<PresenceEventFields>,
}

fn parse_member_list_item(guild_id: Id<GuildMarker>, item: &Value) -> Option<MemberListItemInfo> {
    let member = item
        .get("member")
        .or_else(|| item.get("user").map(|_| item))?;
    let member_info = parse_member_info(member, Some(guild_id))?;
    let user_id = member_info.user_id;
    let presence = member.get("presence");
    let status = presence
        .and_then(|presence| presence.get("status"))
        .and_then(Value::as_str)
        .map(parse_status);
    let activities = presence.map(parse_activities).unwrap_or_default();

    let presence = status.map(|status| PresenceEventFields {
        user_id,
        status,
        activities,
    });
    Some(MemberListItemInfo {
        member: member_info,
        presence,
    })
}

pub(super) fn parse_member_remove(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let user = data.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    Some(AppEvent::GuildMemberRemove { guild_id, user_id })
}

pub(super) fn parse_member_info(
    value: &Value,
    guild_id: Option<Id<GuildMarker>>,
) -> Option<MemberInfo> {
    let user = value.get("user");
    let user_id = user
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)?;
    let nick = value.get("nick").and_then(Value::as_str);
    let global_name = user
        .and_then(|user| user.get("global_name"))
        .and_then(Value::as_str);
    let username = user
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str);
    let display_name = display_name_from_parts_or_unknown(nick, global_name, username);
    let is_bot = user
        .and_then(|user| user.get("bot"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(MemberInfo {
        user_id,
        display_name,
        username: username.map(str::to_owned),
        is_bot,
        avatar_url: raw_member_avatar_url(guild_id, user_id, value, user),
        role_ids: value
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
            .unwrap_or_default(),
    })
}
