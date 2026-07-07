mod state;

pub use state::ChannelUnreadState;
pub(in crate::discord) use state::{
    GuildNotificationSettingsState, MessageNotificationInput, MessageNotificationKind,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationLevel {
    AllMessages,
    OnlyMentions,
    NoMessages,
    ParentDefault,
}

impl NotificationLevel {
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::AllMessages),
            1 => Some(Self::OnlyMentions),
            2 => Some(Self::NoMessages),
            3 => Some(Self::ParentDefault),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelNotificationOverrideInfo {
    pub channel_id: Id<ChannelMarker>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
impl ChannelNotificationOverrideInfo {
    pub(crate) fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            message_notifications: None,
            muted: false,
            mute_end_time: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildNotificationSettingsInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub channel_overrides: Vec<ChannelNotificationOverrideInfo>,
}

#[cfg(test)]
#[allow(dead_code)]
impl GuildNotificationSettingsInfo {
    pub(crate) fn test(guild_id: Option<Id<GuildMarker>>) -> Self {
        Self {
            guild_id,
            message_notifications: None,
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }
    }
}
