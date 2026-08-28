use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::{
    ActivityAssets, ActivityButton, ActivityEmoji, ActivityInfo, ActivityKind, ActivityParty,
    ActivitySecrets, ActivityTimestamps,
    events::{AppEvent, PresenceEventFields},
    ids::{
        Id,
        marker::{ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
    },
};

use super::{
    members::parse_member_info,
    shared::{extra_fields, parse_id, parse_status},
};

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
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let user_id = data
        .get("user_id")
        .and_then(parse_id::<UserMarker>)
        .or_else(|| {
            data.get("member")
                .and_then(|member| member.get("user"))
                .and_then(|user| user.get("id"))
                .and_then(parse_id::<UserMarker>)
        })?;
    let member = guild_id.and_then(|guild_id| {
        data.get("member")
            .and_then(|member| parse_member_info(member, Some(guild_id)))
    });
    Some(AppEvent::TypingStart {
        guild_id,
        channel_id,
        user_id,
        member,
    })
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
    let activity_type = value.get("type").and_then(Value::as_u64).unwrap_or(0);
    let kind = ActivityKind::from_code(activity_type);
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
    let details_url = text_field(value, "details_url");
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let state_url = text_field(value, "state_url");
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let application_id = snowflake_text_field(value, "application_id");
    let parent_application_id = snowflake_text_field(value, "parent_application_id");
    let emoji = value.get("emoji").and_then(parse_activity_emoji);
    let timestamps = value.get("timestamps").and_then(parse_activity_timestamps);
    let assets = value.get("assets").and_then(parse_activity_assets);
    let party = value.get("party").and_then(parse_activity_party);
    let secrets = value.get("secrets").and_then(parse_activity_secrets);
    let buttons = parse_activity_buttons(value);

    Some(ActivityInfo {
        id: text_field(value, "id"),
        kind,
        name,
        created_at: value.get("created_at").and_then(parse_i64),
        session_id: text_field(value, "session_id"),
        platform: text_field(value, "platform"),
        supported_platforms: string_array(value.get("supported_platforms")),
        details,
        details_url,
        state,
        state_url,
        url,
        application_id,
        parent_application_id,
        status_display_type: value
            .get("status_display_type")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok()),
        sync_id: text_field(value, "sync_id"),
        flags: value.get("flags").and_then(Value::as_u64),
        emoji,
        timestamps,
        assets,
        party,
        secrets,
        buttons,
        instance: value.get("instance").and_then(Value::as_bool),
        metadata: object_fields(value.get("metadata")),
        extra_fields: extra_fields(
            value,
            &[
                "id",
                "name",
                "type",
                "url",
                "created_at",
                "session_id",
                "platform",
                "supported_platforms",
                "timestamps",
                "application_id",
                "parent_application_id",
                "status_display_type",
                "details",
                "details_url",
                "state",
                "state_url",
                "sync_id",
                "flags",
                "buttons",
                "emoji",
                "party",
                "assets",
                "secrets",
                "metadata",
                "instance",
            ],
        ),
    })
}

fn parse_activity_timestamps(value: &Value) -> Option<ActivityTimestamps> {
    let start = value.get("start").and_then(parse_i64);
    let end = value.get("end").and_then(parse_i64);
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
        large_url: text("large_url"),
        small_image: text("small_image"),
        small_text: text("small_text"),
        small_url: text("small_url"),
        invite_cover_image: text("invite_cover_image"),
        extra_fields: extra_fields(
            value,
            &[
                "large_image",
                "large_text",
                "large_url",
                "small_image",
                "small_text",
                "small_url",
                "invite_cover_image",
            ],
        ),
    };
    if assets.large_image.is_none()
        && assets.large_text.is_none()
        && assets.large_url.is_none()
        && assets.small_image.is_none()
        && assets.small_text.is_none()
        && assets.small_url.is_none()
        && assets.invite_cover_image.is_none()
        && assets.extra_fields.is_empty()
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
            let current = u32::try_from(entries.first()?.as_u64()?).ok()?;
            let max = u32::try_from(entries.get(1)?.as_u64()?).ok()?;
            Some((current, max))
        });
    let privacy = value
        .get("privacy")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    let extra_fields = extra_fields(value, &["id", "size", "privacy"]);
    if id.is_none() && size.is_none() && privacy.is_none() && extra_fields.is_empty() {
        return None;
    }
    Some(ActivityParty {
        id,
        size,
        privacy,
        extra_fields,
    })
}

fn parse_activity_secrets(value: &Value) -> Option<ActivitySecrets> {
    let secrets = ActivitySecrets {
        join: text_field(value, "join"),
        spectate: text_field(value, "spectate"),
        extra_fields: extra_fields(value, &["join", "spectate"]),
    };
    (secrets.join.is_some() || secrets.spectate.is_some() || !secrets.extra_fields.is_empty())
        .then_some(secrets)
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

fn parse_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn snowflake_text_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|node| {
        node.as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| node.as_u64().map(|value| value.to_string()))
    })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn object_fields(value: Option<&Value>) -> BTreeMap<String, Value> {
    value
        .and_then(Value::as_object)
        .map(|fields| {
            fields
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn presence_user_id(value: &Value) -> Option<Id<UserMarker>> {
    value
        .get("user")
        .and_then(|user| user.get("id"))
        .or_else(|| value.get("user_id"))
        .or_else(|| value.get("id"))
        .and_then(parse_id::<UserMarker>)
}
