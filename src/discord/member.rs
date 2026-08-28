mod list;
mod state;

use chrono::{DateTime, Utc};

pub use list::GuildMemberListEntry;
pub(in crate::discord) use list::GuildMemberListState;
pub use state::{GuildMemberState, RoleState, TypingUserState};
pub(in crate::discord) use state::{
    role_display_order, role_map, role_state, selected_member_role_color, selected_role_ids_color,
};

use crate::discord::ids::{
    Id,
    marker::{RoleMarker, UserMarker},
};

pub(crate) const MEMBER_FLAG_COMPLETED_ONBOARDING: u64 = 1 << 1;
pub(crate) const MEMBER_FLAG_BYPASSES_VERIFICATION: u64 = 1 << 2;
pub(crate) const MEMBER_FLAG_STARTED_ONBOARDING: u64 = 1 << 3;
pub(crate) const MEMBER_SEARCH_MAX_QUERY_CHARS: usize = 64;

pub(crate) fn normalize_member_search_query(query: &str, min_query_chars: usize) -> Option<String> {
    let mut normalized = String::new();
    let mut count = 0usize;
    for ch in query.trim().chars() {
        for lowered in ch.to_lowercase() {
            if count >= MEMBER_SEARCH_MAX_QUERY_CHARS {
                return (count >= min_query_chars).then_some(normalized);
            }
            normalized.push(lowered);
            count += 1;
        }
    }
    (count >= min_query_chars).then_some(normalized)
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemberOnboardingStatus {
    NotStarted,
    InProgress,
    Completed,
}

pub(crate) fn onboarding_status_from_flags(flags: Option<u64>) -> Option<MemberOnboardingStatus> {
    let flags = flags?;
    if flags & MEMBER_FLAG_COMPLETED_ONBOARDING != 0 {
        Some(MemberOnboardingStatus::Completed)
    } else if flags & MEMBER_FLAG_STARTED_ONBOARDING != 0 {
        Some(MemberOnboardingStatus::InProgress)
    } else {
        Some(MemberOnboardingStatus::NotStarted)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle (`User.name`). Same role as in
    /// [`ChannelRecipientInfo::username`].
    pub username: Option<String>,
    /// Guild-specific nickname. Opcode 8 searches this field and `username`,
    /// but does not search a user's global display name.
    pub nickname: Option<String>,
    /// Distinguishes an omitted nickname from an explicit null that clears it.
    pub nickname_present: bool,
    pub is_bot: bool,
    /// Whether the source payload included the user's `bot` field. Partial
    /// member payloads often omit the nested user fields, so `false` alone
    /// cannot safely replace a cached value.
    pub is_bot_present: bool,
    pub avatar_url: Option<String>,
    /// Whether the source payload carried enough avatar fields to replace a
    /// cached avatar without guessing from an incomplete nested user.
    pub avatar_url_present: bool,
    pub role_ids: Vec<Id<RoleMarker>>,
    /// Whether the source payload included `roles`. An omitted field is a
    /// partial patch, while an empty array means the member has no roles.
    pub role_ids_present: bool,
    /// When this member joined the server. Required for HIGH verification.
    pub joined_at: Option<DateTime<Utc>>,
    /// Discord guild member flags, including BYPASSES_VERIFICATION.
    pub flags: Option<u64>,
    /// Whether the member still needs to complete membership screening.
    pub pending: Option<bool>,
    /// When Discord's member timeout expires. A future value temporarily
    /// restricts the member to viewing channels and reading message history.
    pub communication_disabled_until: Option<DateTime<Utc>>,
    /// Whether the source payload included `communication_disabled_until`.
    /// Discord uses an explicit null to clear a timeout, so update merging
    /// must distinguish null from an omitted field.
    pub communication_disabled_until_present: bool,
}

#[cfg(test)]
#[allow(dead_code)]
impl MemberInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
            username: None,
            nickname: None,
            nickname_present: false,
            is_bot: false,
            is_bot_present: true,
            avatar_url: None,
            avatar_url_present: true,
            role_ids: Vec::new(),
            role_ids_present: true,
            joined_at: None,
            flags: Some(0),
            pending: None,
            communication_disabled_until: None,
            communication_disabled_until_present: false,
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
