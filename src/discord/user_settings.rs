use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{EmojiMarker, GuildMarker},
};

use super::GuildFolder;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct UserSettingsInfo {
    pub activity_restricted_guild_ids: Option<Vec<Id<GuildMarker>>>,
    pub activity_joining_restricted_guild_ids: Option<Vec<Id<GuildMarker>>>,
    pub afk_timeout: Option<u64>,
    pub allow_accessibility_detection: Option<bool>,
    pub allow_activity_party_privacy_friends: Option<bool>,
    pub allow_activity_party_privacy_voice_channel: Option<bool>,
    pub animate_emoji: Option<bool>,
    pub animate_stickers: Option<u64>,
    pub contact_sync_enabled: Option<bool>,
    pub convert_emoticons: Option<bool>,
    pub custom_status: Option<Option<UserCustomStatusInfo>>,
    pub default_guilds_restricted: Option<bool>,
    pub detect_platform_accounts: Option<bool>,
    pub developer_mode: Option<bool>,
    pub disable_games_tab: Option<bool>,
    pub enable_tts_command: Option<bool>,
    pub explicit_content_filter: Option<u64>,
    pub friend_discovery_flags: Option<u64>,
    pub friend_source_flags: Option<UserFriendSourceFlagsInfo>,
    pub gif_auto_play: Option<bool>,
    pub guild_folders: Option<Vec<GuildFolder>>,
    pub inline_attachment_media: Option<bool>,
    pub inline_embed_media: Option<bool>,
    pub locale: Option<String>,
    pub message_display_compact: Option<bool>,
    pub native_phone_integration_enabled: Option<bool>,
    pub passwordless: Option<bool>,
    pub render_embeds: Option<bool>,
    pub render_reactions: Option<bool>,
    pub restricted_guilds: Option<Vec<Id<GuildMarker>>>,
    pub show_current_game: Option<bool>,
    pub slayer_sdk_receive_dms_in_game: Option<u64>,
    pub soundboard_volume: Option<f64>,
    pub status: Option<String>,
    pub stream_notifications_enabled: Option<bool>,
    pub theme: Option<String>,
    pub timezone_offset: Option<i64>,
    pub view_nsfw_commands: Option<bool>,
    pub view_nsfw_guilds: Option<bool>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserCustomStatusInfo {
    pub text: Option<String>,
    pub emoji_id: Option<Id<EmojiMarker>>,
    pub emoji_name: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserFriendSourceFlagsInfo {
    pub all: Option<bool>,
    pub mutual_friends: Option<bool>,
    pub mutual_guilds: Option<bool>,
}
