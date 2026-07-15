mod state;

pub use state::GuildState;

pub(crate) const GUILD_FEATURE_COMMUNITY: &str = "COMMUNITY";
pub(crate) const GUILD_FEATURE_MEMBER_VERIFICATION_GATE: &str = "MEMBER_VERIFICATION_GATE_ENABLED";

use std::sync::Arc;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker},
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

/// Discord's onboarding mode. Unknown values are preserved so a future mode
/// cannot silently be treated as one of the modes Concord already knows.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GuildOnboardingMode {
    Default,
    Advanced,
    Unknown(u64),
}

impl GuildOnboardingMode {
    pub fn from_value(value: u64) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::Advanced,
            value => Self::Unknown(value),
        }
    }
}

/// Onboarding configuration delivered by Discord.
///
/// The typed fields support current permission decisions. `raw` intentionally
/// keeps the complete object so prompts and future Discord fields are not lost.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildOnboardingInfo {
    pub guild_id: Id<GuildMarker>,
    pub enabled: Option<bool>,
    pub mode: Option<GuildOnboardingMode>,
    pub default_channel_ids: Vec<Id<ChannelMarker>>,
    pub raw: Arc<Value>,
}

impl GuildOnboardingInfo {
    pub fn prompts(&self) -> &[Value] {
        self.raw
            .get("prompts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
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
