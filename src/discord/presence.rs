use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{Id, marker::EmojiMarker};

use super::emoji::custom_emoji_image_url;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
    Unknown,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::DoNotDisturb => "Do Not Disturb",
            Self::Offline => "Offline",
            Self::Unknown => "Unknown",
        }
    }

    pub(crate) fn gateway_status(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::DoNotDisturb => "dnd",
            Self::Offline => "invisible",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) const fn user_selectable() -> [Self; 4] {
        [Self::Online, Self::Idle, Self::DoNotDisturb, Self::Offline]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Custom,
    Competing,
    Hang,
    Unknown(u64),
}

impl ActivityKind {
    pub fn from_code(code: u64) -> Self {
        match code {
            0 => Self::Playing,
            1 => Self::Streaming,
            2 => Self::Listening,
            3 => Self::Watching,
            4 => Self::Custom,
            5 => Self::Competing,
            6 => Self::Hang,
            value => Self::Unknown(value),
        }
    }

    pub(crate) const fn gateway_code(self) -> u64 {
        match self {
            Self::Playing => 0,
            Self::Streaming => 1,
            Self::Listening => 2,
            Self::Watching => 3,
            Self::Custom => 4,
            Self::Competing => 5,
            Self::Hang => 6,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEmoji {
    pub name: String,
    pub id: Option<Id<EmojiMarker>>,
    pub animated: bool,
}

impl ActivityEmoji {
    /// CDN URL for a custom emoji (one with an `id`). `None` for unicode emojis,
    /// which render as text.
    pub fn image_url(&self) -> Option<String> {
        let id = self.id?;
        Some(custom_emoji_image_url(id.get(), self.animated))
    }
}

/// Start/end of the activity in Unix **milliseconds**.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityTimestamps {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

/// Image slots of a rich presence card. Each `*_image` is an app-asset key, a
/// numeric asset id, or an `mp:` external ref.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActivityAssets {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub large_url: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    pub small_url: Option<String>,
    pub invite_cover_image: Option<String>,
    pub extra_fields: BTreeMap<String, Value>,
}

/// Party grouping for an activity. `size` is `(current, max)` members.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActivityParty {
    pub id: Option<String>,
    pub size: Option<(u32, u32)>,
    /// RPC-only privacy value. It is retained when bridging RPC activities.
    pub privacy: Option<u8>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActivitySecrets {
    pub join: Option<String>,
    pub spectate: Option<String>,
    pub extra_fields: BTreeMap<String, Value>,
}

/// A clickable button. User-account gateway presence encodes these differently
/// from RPC's `{ label, url }` (see `activity_gateway_payload`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityButton {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityInfo {
    /// Receive-only activity identity. Kept for display and future features,
    /// but intentionally omitted from Update Presence payloads.
    pub id: Option<String>,
    pub kind: ActivityKind,
    pub name: String,
    /// Receive-only creation time in Unix milliseconds.
    pub created_at: Option<i64>,
    /// Receive-only session identity.
    pub session_id: Option<String>,
    pub platform: Option<String>,
    pub supported_platforms: Vec<String>,
    pub details: Option<String>,
    pub details_url: Option<String>,
    pub state: Option<String>,
    pub state_url: Option<String>,
    pub url: Option<String>,
    pub application_id: Option<String>,
    pub parent_application_id: Option<String>,
    pub status_display_type: Option<u8>,
    pub sync_id: Option<String>,
    pub flags: Option<u64>,
    pub emoji: Option<ActivityEmoji>,
    pub timestamps: Option<ActivityTimestamps>,
    pub assets: Option<ActivityAssets>,
    pub party: Option<ActivityParty>,
    pub secrets: Option<ActivitySecrets>,
    pub buttons: Vec<ActivityButton>,
    /// RPC represents the INSTANCE activity flag as a boolean.
    pub instance: Option<bool>,
    /// Activity metadata is arbitrary and must remain opaque.
    pub metadata: BTreeMap<String, Value>,
    /// Unknown fields are retained so a status change does not erase values
    /// introduced by a newer Discord payload.
    pub extra_fields: BTreeMap<String, Value>,
}

impl ActivityInfo {
    pub fn playing(name: impl Into<String>) -> Self {
        Self {
            id: None,
            kind: ActivityKind::Playing,
            name: name.into(),
            created_at: None,
            session_id: None,
            platform: None,
            supported_platforms: Vec::new(),
            details: None,
            details_url: None,
            state: None,
            state_url: None,
            url: None,
            application_id: None,
            parent_application_id: None,
            status_display_type: None,
            sync_id: None,
            flags: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            secrets: None,
            buttons: Vec::new(),
            instance: None,
            metadata: BTreeMap::new(),
            extra_fields: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl ActivityInfo {
    pub(crate) fn test(kind: ActivityKind, name: impl Into<String>) -> Self {
        Self {
            id: None,
            kind,
            name: name.into(),
            created_at: None,
            session_id: None,
            platform: None,
            supported_platforms: Vec::new(),
            details: None,
            details_url: None,
            state: None,
            state_url: None,
            url: None,
            application_id: None,
            parent_application_id: None,
            status_display_type: None,
            sync_id: None,
            flags: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            secrets: None,
            buttons: Vec::new(),
            instance: None,
            metadata: BTreeMap::new(),
            extra_fields: BTreeMap::new(),
        }
    }
}
