use serde_json::Value;

use crate::discord::{
    ActivityAssets, ActivityButton, ActivityEmoji, ActivityInfo, ActivityKind, ActivityParty,
    ActivityTimestamps,
    events::{AppEvent, PresenceEventFields},
    ids::{
        Id,
        marker::{ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
    },
};

use super::shared::{display_name_from_parts, parse_id, parse_status};

pub(super) fn parse_presence_update(data: &Value) -> Vec<AppEvent> {
    let Some(presence) = parse_presence_entry(data) else {
        return Vec::new();
    };
    if let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) {
        vec![AppEvent::PresenceUpdate {
            guild_id: Some(guild_id),
            presence,
        }]
    } else {
        vec![AppEvent::PresenceUpdate {
            guild_id: None,
            presence,
        }]
    }
}

/// Discord's TYPING_START shape: `{ channel_id, guild_id?, user_id,
/// timestamp, member? }`. Guild channels carry the typer's user_id directly,
/// while DMs sometimes only embed it under `member.user.id`. We accept both
/// and ignore the timestamp (state stamps its own Instant on receive).
pub(super) fn parse_typing_start(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let user_id = data
        .get("user_id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("member")
                .and_then(|member| member.get("user"))
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    let display_name = data.get("member").and_then(typing_member_display_name);
    Some(AppEvent::TypingStart {
        channel_id,
        user_id,
        display_name,
    })
}

fn typing_member_display_name(member: &Value) -> Option<String> {
    let user = member.get("user");
    let nick = member.get("nick").and_then(Value::as_str);
    let global_name = user
        .and_then(|user| user.get("global_name"))
        .and_then(Value::as_str);
    let username = user
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str);
    display_name_from_parts(nick, global_name, username).map(str::to_owned)
}

pub(super) fn parse_presence_entry(value: &Value) -> Option<PresenceEventFields> {
    let user_id = presence_user_id(value)?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)?;
    let activities = parse_activities(value);
    Some(PresenceEventFields {
        user_id,
        status,
        activities,
    })
}

pub(super) fn parse_activities(value: &Value) -> Vec<ActivityInfo> {
    value
        .get("activities")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_activity).collect())
        .unwrap_or_default()
}

pub(in crate::discord) fn parse_activity(value: &Value) -> Option<ActivityInfo> {
    let kind = ActivityKind::from_code(value.get("type").and_then(Value::as_u64).unwrap_or(0));
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_default();

    let details = value
        .get("details")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let application_id = value
        .get("application_id")
        .and_then(|node| {
            node.as_str()
                .map(str::to_owned)
                .or_else(|| node.as_u64().map(|n| n.to_string()))
        })
        .filter(|s| !s.is_empty());
    let emoji = value.get("emoji").and_then(parse_activity_emoji);
    let timestamps = value.get("timestamps").and_then(parse_activity_timestamps);
    let assets = value.get("assets").and_then(parse_activity_assets);
    let party = value.get("party").and_then(parse_activity_party);
    let buttons = parse_activity_buttons(value);

    if kind == ActivityKind::Unknown && name.is_empty() && state.is_none() && emoji.is_none() {
        return None;
    }

    Some(ActivityInfo {
        kind,
        name,
        details,
        state,
        url,
        application_id,
        emoji,
        timestamps,
        assets,
        party,
        buttons,
    })
}

fn parse_activity_timestamps(value: &Value) -> Option<ActivityTimestamps> {
    let start = value.get("start").and_then(Value::as_i64);
    let end = value.get("end").and_then(Value::as_i64);
    if start.is_none() && end.is_none() {
        return None;
    }
    Some(ActivityTimestamps { start, end })
}

fn parse_activity_assets(value: &Value) -> Option<ActivityAssets> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let assets = ActivityAssets {
        large_image: text("large_image"),
        large_text: text("large_text"),
        small_image: text("small_image"),
        small_text: text("small_text"),
    };
    if assets.large_image.is_none()
        && assets.large_text.is_none()
        && assets.small_image.is_none()
        && assets.small_text.is_none()
    {
        return None;
    }
    Some(assets)
}

fn parse_activity_party(value: &Value) -> Option<ActivityParty> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let size = value
        .get("size")
        .and_then(Value::as_array)
        .and_then(|entries| {
            let current = entries.first()?.as_u64()? as u32;
            let max = entries.get(1)?.as_u64()? as u32;
            Some((current, max))
        });
    if id.is_none() && size.is_none() {
        return None;
    }
    Some(ActivityParty { id, size })
}

/// Received presences encode buttons as an array of label strings with URLs in
/// `metadata.button_urls`, whereas RPC `SET_ACTIVITY` sends `[{ label, url }]`.
/// We accept both so this parser is reusable for the RPC path.
fn parse_activity_buttons(value: &Value) -> Vec<ActivityButton> {
    let Some(entries) = value.get("buttons").and_then(Value::as_array) else {
        return Vec::new();
    };
    let metadata_urls = value
        .get("metadata")
        .and_then(|metadata| metadata.get("button_urls"))
        .and_then(Value::as_array);
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            if let Some(label) = entry.as_str() {
                let url = metadata_urls
                    .and_then(|urls| urls.get(index))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                return Some(ActivityButton {
                    label: label.to_owned(),
                    url,
                });
            }
            let label = entry.get("label").and_then(Value::as_str)?.to_owned();
            let url = entry
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(ActivityButton { label, url })
        })
        .collect()
}

fn parse_activity_emoji(value: &Value) -> Option<ActivityEmoji> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_owned();
    let id = value.get("id").and_then(parse_id::<EmojiMarker>);
    let animated = value
        .get("animated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(ActivityEmoji { name, id, animated })
}

fn presence_user_id(value: &Value) -> Option<Id<UserMarker>> {
    value
        .get("user")
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)
}
