use crate::discord::ids::{Id, marker::EmojiMarker};

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
            Self::Unknown => "online",
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
    Unknown,
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
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn gateway_code(self) -> u8 {
        match self {
            Self::Playing => 0,
            Self::Streaming => 1,
            Self::Listening => 2,
            Self::Watching => 3,
            Self::Custom => 4,
            Self::Competing => 5,
            Self::Unknown => 0,
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
        let ext = if self.animated { "gif" } else { "png" };
        Some(format!(
            "https://cdn.discordapp.com/emojis/{}.{}",
            id.get(),
            ext
        ))
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityAssets {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

/// Party grouping for an activity. `size` is `(current, max)` members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityParty {
    pub id: Option<String>,
    pub size: Option<(u32, u32)>,
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
    pub kind: ActivityKind,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub application_id: Option<String>,
    pub emoji: Option<ActivityEmoji>,
    pub timestamps: Option<ActivityTimestamps>,
    pub assets: Option<ActivityAssets>,
    pub party: Option<ActivityParty>,
    pub buttons: Vec<ActivityButton>,
}

impl ActivityInfo {
    pub fn playing(name: impl Into<String>) -> Self {
        Self {
            kind: ActivityKind::Playing,
            name: name.into(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            buttons: Vec::new(),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl ActivityInfo {
    pub(crate) fn test(kind: ActivityKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
            timestamps: None,
            assets: None,
            party: None,
            buttons: Vec::new(),
        }
    }
}
