use serde_json::Value;

use crate::discord::{
    ActivityEmoji, ActivityInfo, ActivityKind, PresenceStatus,
    events::AppEvent,
    ids::{
        Id,
        marker::{ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
    },
};

use super::shared::{parse_id, parse_status};

pub(super) fn parse_presence_update(data: &Value) -> Vec<AppEvent> {
    let Some((user_id, status, activities)) = parse_presence_entry(data) else {
        return Vec::new();
    };
    if let Some(guild_id) = data.get("guild_id").and_then(parse_id::<GuildMarker>) {
        vec![AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status,
            activities,
        }]
    } else {
        vec![AppEvent::UserPresenceUpdate {
            user_id,
            status,
            activities,
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
    Some(AppEvent::TypingStart {
        channel_id,
        user_id,
    })
}

pub(super) fn parse_presence_entry(
    value: &Value,
) -> Option<(Id<UserMarker>, PresenceStatus, Vec<ActivityInfo>)> {
    let user_id = presence_user_id(value)?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(parse_status)?;
    let activities = parse_activities(value);
    Some((user_id, status, activities))
}

pub(super) fn parse_activities(value: &Value) -> Vec<ActivityInfo> {
    value
        .get("activities")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_activity).collect())
        .unwrap_or_default()
}

fn parse_activity(value: &Value) -> Option<ActivityInfo> {
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
    })
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
