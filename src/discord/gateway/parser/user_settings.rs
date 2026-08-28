use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::{
    GuildFolder, UserCustomStatusInfo, UserFriendSourceFlagsInfo, UserSettingsInfo,
    events::AppEvent,
    ids::{
        Id,
        marker::{EmojiMarker, GuildMarker},
    },
};

use super::shared::parse_id;

pub(super) fn parse_user_settings_update(data: &Value) -> Option<AppEvent> {
    let settings = data.get("user_settings").unwrap_or(data);
    parse_user_settings_info(settings).map(|settings| AppEvent::UserSettingsUpdate { settings })
}

pub(super) fn parse_user_notification_settings_update(data: &Value) -> Option<AppEvent> {
    let flags = data.get("flags").and_then(Value::as_u64)?;
    Some(AppEvent::UserNotificationSettingsUpdate { flags })
}

pub(super) fn parse_user_settings_info(settings: &Value) -> Option<UserSettingsInfo> {
    settings.as_object()?;

    Some(UserSettingsInfo {
        activity_restricted_guild_ids: parse_id_list(settings, "activity_restricted_guild_ids"),
        activity_joining_restricted_guild_ids: parse_id_list(
            settings,
            "activity_joining_restricted_guild_ids",
        ),
        afk_timeout: parse_u64_field(settings, "afk_timeout"),
        allow_accessibility_detection: parse_bool_field(settings, "allow_accessibility_detection"),
        allow_activity_party_privacy_friends: parse_bool_field(
            settings,
            "allow_activity_party_privacy_friends",
        ),
        allow_activity_party_privacy_voice_channel: parse_bool_field(
            settings,
            "allow_activity_party_privacy_voice_channel",
        ),
        animate_emoji: parse_bool_field(settings, "animate_emoji"),
        animate_stickers: parse_u64_field(settings, "animate_stickers"),
        contact_sync_enabled: parse_bool_field(settings, "contact_sync_enabled"),
        convert_emoticons: parse_bool_field(settings, "convert_emoticons"),
        custom_status: parse_nullable_custom_status(settings.get("custom_status")),
        default_guilds_restricted: parse_bool_field(settings, "default_guilds_restricted"),
        detect_platform_accounts: parse_bool_field(settings, "detect_platform_accounts"),
        developer_mode: parse_bool_field(settings, "developer_mode"),
        disable_games_tab: parse_bool_field(settings, "disable_games_tab"),
        enable_tts_command: parse_bool_field(settings, "enable_tts_command"),
        explicit_content_filter: parse_u64_field(settings, "explicit_content_filter"),
        friend_discovery_flags: parse_u64_field(settings, "friend_discovery_flags"),
        friend_source_flags: settings
            .get("friend_source_flags")
            .and_then(parse_friend_source_flags),
        gif_auto_play: parse_bool_field(settings, "gif_auto_play"),
        guild_folders: parse_guild_folders(settings),
        inline_attachment_media: parse_bool_field(settings, "inline_attachment_media"),
        inline_embed_media: parse_bool_field(settings, "inline_embed_media"),
        locale: parse_string_field(settings, "locale"),
        message_display_compact: parse_bool_field(settings, "message_display_compact"),
        native_phone_integration_enabled: parse_bool_field(
            settings,
            "native_phone_integration_enabled",
        ),
        passwordless: parse_bool_field(settings, "passwordless"),
        render_embeds: parse_bool_field(settings, "render_embeds"),
        render_reactions: parse_bool_field(settings, "render_reactions"),
        restricted_guilds: parse_id_list(settings, "restricted_guilds"),
        show_current_game: parse_bool_field(settings, "show_current_game"),
        slayer_sdk_receive_dms_in_game: parse_u64_field(settings, "slayer_sdk_receive_dms_in_game"),
        soundboard_volume: settings.get("soundboard_volume").and_then(Value::as_f64),
        status: parse_string_field(settings, "status"),
        stream_notifications_enabled: parse_bool_field(settings, "stream_notifications_enabled"),
        theme: parse_string_field(settings, "theme"),
        timezone_offset: settings.get("timezone_offset").and_then(Value::as_i64),
        view_nsfw_commands: parse_bool_field(settings, "view_nsfw_commands"),
        view_nsfw_guilds: parse_bool_field(settings, "view_nsfw_guilds"),
        extra_fields: parse_extra_user_setting_fields(settings),
    })
}

fn parse_guild_folders(settings: &Value) -> Option<Vec<GuildFolder>> {
    let folders = settings.get("guild_folders")?.as_array()?;
    let folders: Vec<GuildFolder> = folders.iter().filter_map(parse_guild_folder).collect();
    Some(folders)
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
    let color = value
        .get("color")
        .and_then(Value::as_u64)
        .and_then(|color| u32::try_from(color).ok());

    Some(GuildFolder {
        id,
        name,
        color,
        guild_ids,
    })
}

fn parse_bool_field(settings: &Value, field: &str) -> Option<bool> {
    settings.get(field).and_then(Value::as_bool)
}

fn parse_u64_field(settings: &Value, field: &str) -> Option<u64> {
    settings.get(field).and_then(Value::as_u64)
}

fn parse_string_field(settings: &Value, field: &str) -> Option<String> {
    settings
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn parse_id_list(settings: &Value, field: &str) -> Option<Vec<Id<GuildMarker>>> {
    let values = settings.get(field)?.as_array()?;
    Some(values.iter().filter_map(parse_id::<GuildMarker>).collect())
}

fn parse_nullable_custom_status(value: Option<&Value>) -> Option<Option<UserCustomStatusInfo>> {
    match value {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => Some(Some(UserCustomStatusInfo {
            text: value.get("text").and_then(Value::as_str).map(str::to_owned),
            emoji_id: value.get("emoji_id").and_then(parse_id::<EmojiMarker>),
            emoji_name: value
                .get("emoji_name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            expires_at: value
                .get("expires_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })),
    }
}

fn parse_friend_source_flags(value: &Value) -> Option<UserFriendSourceFlagsInfo> {
    value.as_object()?;
    Some(UserFriendSourceFlagsInfo {
        all: value.get("all").and_then(Value::as_bool),
        mutual_friends: value.get("mutual_friends").and_then(Value::as_bool),
        mutual_guilds: value.get("mutual_guilds").and_then(Value::as_bool),
    })
}

fn parse_extra_user_setting_fields(settings: &Value) -> BTreeMap<String, Value> {
    let Some(settings) = settings.as_object() else {
        return BTreeMap::new();
    };
    settings
        .iter()
        .filter(|(field, _)| !is_known_user_setting_field(field))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}

fn is_known_user_setting_field(field: &str) -> bool {
    matches!(
        field,
        "activity_restricted_guild_ids"
            | "activity_joining_restricted_guild_ids"
            | "afk_timeout"
            | "allow_accessibility_detection"
            | "allow_activity_party_privacy_friends"
            | "allow_activity_party_privacy_voice_channel"
            | "animate_emoji"
            | "animate_stickers"
            | "contact_sync_enabled"
            | "convert_emoticons"
            | "custom_status"
            | "default_guilds_restricted"
            | "detect_platform_accounts"
            | "developer_mode"
            | "disable_games_tab"
            | "enable_tts_command"
            | "explicit_content_filter"
            | "friend_discovery_flags"
            | "friend_source_flags"
            | "gif_auto_play"
            | "guild_folders"
            | "inline_attachment_media"
            | "inline_embed_media"
            | "locale"
            | "message_display_compact"
            | "native_phone_integration_enabled"
            | "passwordless"
            | "render_embeds"
            | "render_reactions"
            | "restricted_guilds"
            | "show_current_game"
            | "slayer_sdk_receive_dms_in_game"
            | "soundboard_volume"
            | "status"
            | "stream_notifications_enabled"
            | "theme"
            | "timezone_offset"
            | "view_nsfw_commands"
            | "view_nsfw_guilds"
    )
}
