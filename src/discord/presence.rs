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
    /// CDN URL for the emoji image, when this is a custom emoji (i.e. carries
    /// an `id`). Returns `None` for unicode-only emojis, which render as text
    /// and don't need a network fetch.
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityInfo {
    pub kind: ActivityKind,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub application_id: Option<String>,
    pub emoji: Option<ActivityEmoji>,
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
        }
    }
}
