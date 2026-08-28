mod state;

use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::{ChannelInfo, MemberInfo, MessageInfo, PresenceEventFields};

pub(in crate::discord) use state::ThreadCache;
pub use state::ThreadCreatorState;

/// Cursor used by Discord's public archived-thread endpoint. The first page
/// omits `before`; later pages use the oldest `archive_timestamp` returned by
/// the previous response.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ArchivedThreadPageCursor {
    Initial,
    Before(String),
}

impl ArchivedThreadPageCursor {
    pub(crate) fn from_before(before: Option<String>) -> Self {
        before.map_or(Self::Initial, Self::Before)
    }

    pub(crate) fn into_before(self) -> Option<String> {
        match self {
            Self::Initial => None,
            Self::Before(before) => Some(before),
        }
    }
}

/// One page from `GET /channels/{channel.id}/threads/archived/public`.
///
/// `members` contains current-account thread memberships for returned rows. It
/// is not the participant list used by the member pane.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchivedThreadsPage {
    pub threads: Vec<ChannelInfo>,
    pub members: Vec<ThreadMemberInfo>,
    pub has_more: bool,
    pub next_before: Option<String>,
    pub extra_fields: BTreeMap<String, Value>,
}

/// Active thread metadata and the optional current-account membership carried
/// beside a `THREAD_CREATE` or `THREAD_UPDATE` dispatch.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadGatewayInfo {
    pub channel: ChannelInfo,
    pub current_user_member: Option<ThreadMemberInfo>,
}

/// A replacement snapshot of active threads for a guild or selected parents.
/// The optional parallel member array contains current-account settings only.
/// Discord user-account syncs can omit this field, which means existing
/// membership state must be preserved. It is not the participant list shown in
/// the member pane.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadListSyncInfo {
    pub guild_id: Id<GuildMarker>,
    pub channel_ids: Option<Vec<Id<ChannelMarker>>>,
    pub threads: Vec<ChannelInfo>,
    pub current_user_members: Option<Vec<ThreadMemberInfo>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMemberInfo {
    pub thread_id: Option<Id<ChannelMarker>>,
    pub user_id: Option<Id<UserMarker>>,
    pub join_timestamp: Option<String>,
    pub flags: Option<u64>,
    pub muted: Option<bool>,
    pub mute_end_time: Option<String>,
    pub selected_time_window: Option<i64>,
    pub member: Option<MemberInfo>,
    pub presence: Option<PresenceEventFields>,
    pub extra_fields: BTreeMap<String, Value>,
}

impl ThreadMemberInfo {
    /// User-account guild snapshots only contain threads the current account
    /// has joined. Those channel objects can omit the redundant `member`
    /// object, so preserve the membership fact without inventing settings.
    pub(in crate::discord) fn joined_snapshot(thread_id: Id<ChannelMarker>) -> Self {
        Self {
            thread_id: Some(thread_id),
            user_id: None,
            join_timestamp: None,
            flags: None,
            muted: None,
            mute_end_time: None,
            selected_time_window: None,
            member: None,
            presence: None,
            extra_fields: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMembersUpdateInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub member_count: Option<u64>,
    pub added_members: Vec<ThreadMemberInfo>,
    pub removed_user_ids: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMemberListUpdateInfo {
    pub guild_id: Id<GuildMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub members: Vec<ThreadMemberInfo>,
    pub extra_fields: BTreeMap<String, Value>,
}

/// Owner and starter-message data returned by the forum `post-data` endpoint
/// for a thread ID that was already discovered through Gateway state.
#[derive(Clone, Debug, PartialEq)]
pub struct ForumPostDataInfo {
    pub thread_id: Id<ChannelMarker>,
    pub owner: Option<MemberInfo>,
    pub first_message: Option<MessageInfo>,
    pub extra_fields: BTreeMap<String, Value>,
}
