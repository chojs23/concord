mod state;

pub use state::GuildState;

use crate::discord::ids::{
    Id,
    marker::{EmojiMarker, GuildMarker},
};

/// One entry from the user's `guild_folders` setting. A folder with `id ==
/// None` and a single member is an ungrouped guild. Discord stores those as
/// "folders" too just for ordering. Real folders carry an integer id, an
/// optional name, and an optional RGB color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildFolder {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<Id<GuildMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomEmojiInfo {
    pub id: Id<EmojiMarker>,
    pub name: String,
    pub animated: bool,
    pub available: bool,
}

/// Server-wide checks Discord applies before a member may participate.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GuildVerificationLevel {
    #[default]
    None,
    Low,
    Medium,
    High,
    VeryHigh,
    Unknown(u64),
}

impl GuildVerificationLevel {
    /// Preserve unknown values so a newer Discord level fails closed.
    pub fn from_value(value: u64) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Low,
            2 => Self::Medium,
            3 => Self::High,
            4 => Self::VeryHigh,
            value => Self::Unknown(value),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl CustomEmojiInfo {
    pub(crate) fn test(id: Id<EmojiMarker>, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            animated: false,
            available: true,
        }
    }
}
