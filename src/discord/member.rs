mod state;

pub use state::{GuildMemberState, RoleState, TypingUserState};
pub(in crate::discord) use state::{
    role_map, role_state, selected_member_role_color, selected_role_ids_color,
};

use crate::discord::ids::{
    Id,
    marker::{RoleMarker, UserMarker},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle (`User.name`). Same role as in
    /// [`ChannelRecipientInfo::username`].
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
}

#[cfg(test)]
#[allow(dead_code)]
impl MemberInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleInfo {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
    /// Discord permission bitfield carried by this role. Used by
    /// `DiscordState::can_view_channel` to compute base permissions and
    /// detect ADMINISTRATOR.
    pub permissions: u64,
}

#[cfg(test)]
#[allow(dead_code)]
impl RoleInfo {
    pub(crate) fn test(id: Id<RoleMarker>, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            color: None,
            position: 0,
            hoist: false,
            permissions: 0,
        }
    }
}
