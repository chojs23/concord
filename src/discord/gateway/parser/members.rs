use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::discord::{
    GuildMemberListItem, GuildMemberListOperation, GuildMemberListUpdateInfo,
    GuildMembersChunkInfo, MemberInfo,
    avatar::{member_avatar_url, user_avatar_url},
    events::{AppEvent, PresenceEventFields},
    ids::{
        Id,
        marker::{GuildMarker, RoleMarker, UserMarker},
    },
};

use super::{
    presence::{parse_activities, parse_presence_entry},
    shared::{display_name_from_parts_or_unknown, extra_fields, parse_id, parse_status},
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
        avatar_url: user_avatar_url(user_id, data),
        is_bot: data.get("bot").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub(super) fn parse_current_user_verification(data: &Value) -> Option<AppEvent> {
    let email_verified = data.get("verified").and_then(Value::as_bool);
    let phone_verified = data
        .get("phone")
        .map(|phone| phone.as_str().is_some_and(|phone| !phone.is_empty()));
    let mfa_enabled = data.get("mfa_enabled").and_then(Value::as_bool);
    (email_verified.is_some() || phone_verified.is_some() || mfa_enabled.is_some()).then_some(
        AppEvent::CurrentUserVerification {
            email_verified,
            phone_verified,
            mfa_enabled,
        },
    )
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

    let mut online_count = data
        .get("online_count")
        .and_then(Value::as_u64)
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX));
    if online_count.is_none()
        && let Some(groups) = data.get("groups").and_then(Value::as_array)
    {
        online_count = Some(
            groups
                .iter()
                .filter(|g| g.get("id").and_then(Value::as_str) != Some("offline"))
                .filter_map(|g| g.get("count").and_then(Value::as_u64))
                .map(|c| c as u32)
                .sum(),
        );
    }

    let groups = clone_array(data.get("groups"));
    let parsed_ops = ops
        .iter()
        .map(|op| parse_member_list_operation(guild_id, op, &groups))
        .collect();

    vec![AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: data.get("id").and_then(Value::as_str).map(str::to_owned),
            member_count: data.get("member_count").and_then(Value::as_u64),
            online_count,
            groups,
            ops: parsed_ops,
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

fn parse_member_list_operation(
    guild_id: Id<GuildMarker>,
    op: &Value,
    groups: &[Value],
) -> GuildMemberListOperation {
    let name = op.get("op").and_then(Value::as_str);
    let parsed = (|| -> Option<GuildMemberListOperation> {
        match name {
            Some("SYNC") => Some(GuildMemberListOperation::Sync {
                range: parse_member_list_range(op.get("range")?)?,
                items: op
                    .get("items")?
                    .as_array()?
                    .iter()
                    .map(|item| parse_member_list_item(guild_id, item, groups))
                    .collect(),
            }),
            Some("INSERT") => Some(GuildMemberListOperation::Insert {
                index: parse_member_list_index(op.get("index")?)?,
                item: parse_member_list_item(guild_id, op.get("item")?, groups),
            }),
            Some("UPDATE") => Some(GuildMemberListOperation::Update {
                index: parse_member_list_index(op.get("index")?)?,
                item: parse_member_list_item(guild_id, op.get("item")?, groups),
            }),
            Some("DELETE") => Some(GuildMemberListOperation::Delete {
                index: parse_member_list_index(op.get("index")?)?,
            }),
            Some("INVALIDATE") => Some(GuildMemberListOperation::Invalidate {
                range: parse_member_list_range(op.get("range")?)?,
            }),
            _ => None,
        }
    })();
    parsed.unwrap_or_else(|| GuildMemberListOperation::Unknown {
        name: name.map(str::to_owned),
        raw: op.clone(),
    })
}

fn parse_member_list_index(value: &Value) -> Option<u32> {
    u32::try_from(value.as_u64()?).ok()
}

fn parse_member_list_range(value: &Value) -> Option<(u32, u32)> {
    let range = value.as_array()?;
    let start = parse_member_list_index(range.first()?)?;
    let end = parse_member_list_index(range.get(1)?)?;
    (start <= end).then_some((start, end))
}

fn parse_member_list_item(
    guild_id: Id<GuildMarker>,
    item: &Value,
    groups: &[Value],
) -> GuildMemberListItem {
    if let Some(group) = item.get("group") {
        if let Some(id) = group.get("id").and_then(Value::as_str)
            && let Some(count) = group
                .get("count")
                .and_then(Value::as_u64)
                .or_else(|| member_list_group_count(groups, id))
        {
            return GuildMemberListItem::Group {
                id: id.to_owned(),
                count,
            };
        }
        return GuildMemberListItem::Unknown { raw: item.clone() };
    }
    let Some(member) = item
        .get("member")
        .or_else(|| item.get("user").map(|_| item))
    else {
        return GuildMemberListItem::Unknown { raw: item.clone() };
    };
    let Some(member_info) = parse_member_info(member, Some(guild_id)) else {
        return GuildMemberListItem::Unknown { raw: item.clone() };
    };
    let user_id = member_info.user_id;
    let presence = item.get("presence").or_else(|| member.get("presence"));
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
    GuildMemberListItem::Member {
        member: member_info,
        presence,
    }
}

fn member_list_group_count(groups: &[Value], id: &str) -> Option<u64> {
    groups
        .iter()
        .find(|group| group.get("id").and_then(Value::as_str) == Some(id))
        .and_then(|group| group.get("count"))
        .and_then(Value::as_u64)
}

pub(super) fn parse_member_remove(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let user = data.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    Some(AppEvent::GuildMemberRemove { guild_id, user_id })
}

pub(crate) fn parse_member_info(
    value: &Value,
    guild_id: Option<Id<GuildMarker>>,
) -> Option<MemberInfo> {
    let communication_disabled_until_present = value.get("communication_disabled_until").is_some();
    let user = value.get("user");
    let is_bot_present = user
        .and_then(|user| user.get("bot"))
        .and_then(Value::as_bool)
        .is_some();
    let avatar_url_present = value.get("avatar").is_some()
        || user.is_some_and(|user| {
            user.get("avatar").is_some() || user.get("discriminator").is_some()
        });
    let role_ids_present = value.get("roles").and_then(Value::as_array).is_some();
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
        nickname: nick.filter(|nick| !nick.is_empty()).map(str::to_owned),
        nickname_present: value.get("nick").is_some(),
        is_bot,
        is_bot_present,
        avatar_url: member_avatar_url(guild_id, user_id, Some(value), user),
        avatar_url_present,
        role_ids: value
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
            .unwrap_or_default(),
        role_ids_present,
        joined_at: value
            .get("joined_at")
            .and_then(Value::as_str)
            .and_then(|joined_at| DateTime::parse_from_rfc3339(joined_at).ok())
            .map(|joined_at| joined_at.with_timezone(&Utc)),
        flags: value.get("flags").and_then(|flags| {
            flags
                .as_u64()
                .or_else(|| flags.as_str().and_then(|flags| flags.parse().ok()))
        }),
        pending: value.get("pending").and_then(Value::as_bool),
        communication_disabled_until: value
            .get("communication_disabled_until")
            .and_then(Value::as_str)
            .and_then(|until| DateTime::parse_from_rfc3339(until).ok())
            .map(|until| until.with_timezone(&Utc)),
        communication_disabled_until_present,
    })
}
