use std::fmt;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use crate::discord::MemberInfo;

/// Identifies the voice server a connection belongs to. Guild voice is keyed by
/// `guild_id`; DM and group-DM calls have no guild, so Discord keys the same
/// machinery by the DM `channel_id` with `guild_id` sent as null.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum VoiceScope {
    Guild(Id<GuildMarker>),
    Private(Id<ChannelMarker>),
}

impl VoiceScope {
    pub fn guild_id(self) -> Option<Id<GuildMarker>> {
        match self {
            Self::Guild(guild_id) => Some(guild_id),
            Self::Private(_) => None,
        }
    }

    pub fn private_channel_id(self) -> Option<Id<ChannelMarker>> {
        match self {
            Self::Private(channel_id) => Some(channel_id),
            Self::Guild(_) => None,
        }
    }

    /// Voice IDENTIFY `server_id`: the guild id for guild voice, the DM channel
    /// id for a private call.
    pub fn server_id_string(self) -> String {
        match self {
            Self::Guild(guild_id) => guild_id.to_string(),
            Self::Private(channel_id) => channel_id.to_string(),
        }
    }

    /// Derive a scope from a `(guild_id, channel_id)` pair. A guild wins; a
    /// private call is keyed by its channel; neither present (a DM leave) yields
    /// `None`.
    fn from_ids(
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Self> {
        match (guild_id, channel_id) {
            (Some(guild_id), _) => Some(Self::Guild(guild_id)),
            (None, Some(channel_id)) => Some(Self::Private(channel_id)),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoiceStateInfo {
    /// `None` for DM/group-DM call voice states, which carry a null `guild_id`.
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub user_id: Id<UserMarker>,
    pub session_id: Option<String>,
    pub member: Option<MemberInfo>,
    pub deaf: bool,
    pub mute: bool,
    pub self_deaf: bool,
    pub self_mute: bool,
    pub self_stream: bool,
}

#[cfg(test)]
#[allow(dead_code)]
impl VoiceStateInfo {
    pub(crate) fn test(
        guild_id: Id<GuildMarker>,
        channel_id: Option<Id<ChannelMarker>>,
        user_id: Id<UserMarker>,
    ) -> Self {
        Self {
            guild_id: Some(guild_id),
            channel_id,
            user_id,
            session_id: None,
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }
}

impl fmt::Debug for VoiceStateInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceStateInfo")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "<redacted>"),
            )
            .field("member", &self.member)
            .field("deaf", &self.deaf)
            .field("mute", &self.mute)
            .field("self_deaf", &self.self_deaf)
            .field("self_mute", &self.self_mute)
            .field("self_stream", &self.self_stream)
            .finish()
    }
}

impl VoiceStateInfo {
    pub fn scope(&self) -> Option<VoiceScope> {
        VoiceScope::from_ids(self.guild_id, self.channel_id)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoiceServerInfo {
    /// `None` for DM/group-DM call servers, which carry a `channel_id` instead.
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub endpoint: Option<String>,
    pub token: String,
}

impl VoiceServerInfo {
    pub fn scope(&self) -> Option<VoiceScope> {
        VoiceScope::from_ids(self.guild_id, self.channel_id)
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl VoiceServerInfo {
    pub(crate) fn test(guild_id: Id<GuildMarker>) -> Self {
        Self {
            guild_id: Some(guild_id),
            channel_id: None,
            endpoint: None,
            token: String::new(),
        }
    }
}

impl fmt::Debug for VoiceServerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceServerInfo")
            .field("guild_id", &self.guild_id)
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceSoundKind {
    Join,
    Leave,
}
