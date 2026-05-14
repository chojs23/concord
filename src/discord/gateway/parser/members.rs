use serde_json::Value;

use crate::discord::{
    MemberInfo,
    events::AppEvent,
    ids::{
        Id,
        marker::{GuildMarker, RoleMarker, UserMarker},
    },
};

use super::{
    presence::{parse_activities, parse_presence_entry},
    shared::{display_name_from_parts_or_unknown, parse_id, parse_status, raw_user_avatar_url},
};

pub(super) fn parse_member_upsert(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data)?;
    Some(AppEvent::GuildMemberUpsert { guild_id, member })
}

pub(super) fn parse_member_add(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let member = parse_member_info(data)?;
    Some(AppEvent::GuildMemberAdd { guild_id, member })
}

pub(super) fn parse_member_chunk(data: &Value) -> Vec<AppEvent> {
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
            |(user_id, status, activities)| AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
                activities,
            },
        ));
    }

    events
}

pub(super) fn parse_member_list_update(data: &Value) -> Vec<AppEvent> {
    let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) else {
        return Vec::new();
    };
    let Some(ops) = data.get("ops").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut events = Vec::new();

    if let Some(groups) = data.get("groups").and_then(Value::as_array) {
        let online = groups
            .iter()
            .filter(|g| g.get("id").and_then(Value::as_str) != Some("offline"))
            .filter_map(|g| g.get("count").and_then(Value::as_u64))
            .map(|c| c as u32)
            .sum();
        events.push(AppEvent::GuildMemberListCounts { guild_id, online });
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
                        events.extend(parse_member_list_item(guild_id, item));
                    }
                }
            }
            Some("INSERT" | "UPDATE") => {
                if let Some(item) = op.get("item") {
                    events.extend(parse_member_list_item(guild_id, item));
                }
            }
            _ => {}
        }
    }

    events
}

fn parse_member_list_item(guild_id: Id<GuildMarker>, item: &Value) -> Vec<AppEvent> {
    let Some(member) = item
        .get("member")
        .or_else(|| item.get("user").map(|_| item))
    else {
        return Vec::new();
    };
    let Some(member_info) = parse_member_info(member) else {
        return Vec::new();
    };
    let user_id = member_info.user_id;
    let presence = member.get("presence");
    let status = presence
        .and_then(|presence| presence.get("status"))
        .and_then(Value::as_str)
        .map(parse_status);
    let activities = presence.map(parse_activities).unwrap_or_default();

    let mut events = vec![AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_info,
    }];
    if let Some(status) = status {
        events.push(AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status,
            activities,
        });
    }
    events
}

pub(super) fn parse_member_remove(data: &Value) -> Option<AppEvent> {
    let guild_id = parse_id::<GuildMarker>(data.get("guild_id")?)?;
    let user = data.get("user")?;
    let user_id = parse_id::<UserMarker>(user.get("id")?)?;
    Some(AppEvent::GuildMemberRemove { guild_id, user_id })
}

pub(super) fn parse_member_info(value: &Value) -> Option<MemberInfo> {
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
        avatar_url: user.and_then(|user| raw_user_avatar_url(user_id, user)),
        role_ids: value
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
            .unwrap_or_default(),
    })
}
