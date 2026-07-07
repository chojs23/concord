mod state;

pub(in crate::discord) use state::{ProfileRoleIds, UserProfileCacheKey};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FriendStatus {
    None,
    Friend,
    Blocked,
    IncomingRequest,
    OutgoingRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipInfo {
    pub user_id: Id<UserMarker>,
    pub status: FriendStatus,
    /// Friend nickname set by the current user. This is distinct from guild
    /// nicknames and only applies to 1:1 friendships / DMs.
    pub nickname: Option<String>,
    /// Best available non-nickname label from the relationship payload,
    /// usually `global_name` and otherwise the username.
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
impl RelationshipInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, status: FriendStatus) -> Self {
        Self {
            user_id,
            status,
            nickname: None,
            display_name: None,
            username: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutualGuildInfo {
    pub guild_id: Id<GuildMarker>,
    pub nick: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfileInfo {
    pub user_id: Id<UserMarker>,
    pub username: String,
    pub global_name: Option<String>,
    pub guild_nick: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub guild_pronouns: Option<String>,
    pub mutual_guilds: Vec<MutualGuildInfo>,
    pub mutual_friends_count: u32,
    pub friend_status: FriendStatus,
    pub note: Option<String>,
}

impl UserProfileInfo {
    pub fn display_name(&self) -> &str {
        self.guild_nick
            .as_deref()
            .or(self.global_name.as_deref())
            .unwrap_or(&self.username)
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl UserProfileInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, username: impl Into<String>) -> Self {
        Self {
            user_id,
            username: username.into(),
            global_name: None,
            guild_nick: None,
            role_ids: Vec::new(),
            avatar_url: None,
            bio: None,
            pronouns: None,
            guild_pronouns: None,
            mutual_guilds: Vec::new(),
            mutual_friends_count: 0,
            friend_status: FriendStatus::None,
            note: None,
        }
    }
}
