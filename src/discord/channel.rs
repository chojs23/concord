mod state;

pub(crate) use state::is_thread_kind;
pub use state::{ChannelRecipientState, ChannelState, ChannelVisibilityStats};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::PresenceStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub parent_id: Option<Id<ChannelMarker>>,
    /// Discord's `owner_id` channel field. For group DMs this is the group DM
    /// owner. For thread channels this is the user that started the thread.
    pub owner_id: Option<Id<UserMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
    /// Discord's `message_count` channel field. Discord only defines this for
    /// thread channels, where it counts messages in that one thread.
    pub message_count: Option<u64>,
    /// Discord's `member_count` channel field. Discord only defines this for
    /// thread channels and caps the approximate count at 50.
    pub member_count: Option<u64>,
    /// Discord's `total_message_sent` channel field. For thread channels this
    /// is the total number ever sent in that one thread and does not decrement
    /// when messages are deleted.
    pub total_message_sent: Option<u64>,
    /// Discord's `thread_metadata` channel field. Present only for thread
    /// channels and describes that one thread's archive/lock state.
    pub thread_metadata: Option<ThreadMetadataInfo>,
    /// Discord's raw `flags` channel bitfield. For thread channels in forum or
    /// media parents, `PINNED = 1 << 1` means this one thread is pinned.
    pub flags: Option<u64>,
    /// Discord's `rate_limit_per_user` channel field, the slow-mode cooldown in
    /// seconds. Present on text channels and forum post threads.
    pub rate_limit_per_user: Option<u64>,
    /// Tags the user can apply to posts in a forum or media parent channel.
    pub available_tags: Vec<ForumTagInfo>,
    /// Tags currently applied to a forum or media post thread.
    pub applied_tags: Vec<Id<ForumTagMarker>>,
    /// Whether Discord included a current-user thread membership object for
    /// this thread. `None` means the payload did not say either way.
    pub current_user_joined_thread: Option<bool>,
    pub recipients: Option<Vec<ChannelRecipientInfo>>,
    /// Channel-level permission overrides. The empty default means a
    /// gateway/REST payload that omitted the field is treated as "no
    /// channel-specific overrides", which matches Discord's behavior of
    /// inheriting from the guild base permissions.
    pub permission_overwrites: Vec<PermissionOverwriteInfo>,
    /// Discord's `is_message_request` DM field: a pending request from a
    /// non-friend. `None` when the payload omits it.
    pub is_message_request: Option<bool>,
    /// Discord's `is_spam` DM field: a message request classified as spam.
    pub is_spam: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumTagInfo {
    pub id: Id<ForumTagMarker>,
    pub name: String,
    pub moderated: bool,
    pub emoji_id: Option<Id<EmojiMarker>>,
    pub emoji_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadMetadataInfo {
    /// Discord's `thread_metadata.archived` field.
    pub archived: bool,
    /// Discord's `thread_metadata.auto_archive_duration` field, in minutes.
    pub auto_archive_duration: Option<u64>,
    /// Discord's `thread_metadata.archive_timestamp` field.
    pub archive_timestamp: Option<String>,
    /// Discord's `thread_metadata.locked` field.
    pub locked: bool,
    /// Discord's `thread_metadata.invitable` field. Only available on private
    /// threads.
    pub invitable: Option<bool>,
    /// Discord's `thread_metadata.create_timestamp` field. Discord only
    /// populates it for newer threads.
    pub create_timestamp: Option<String>,
}

impl ChannelInfo {
    pub fn thread_archived(&self) -> Option<bool> {
        self.thread_metadata
            .as_ref()
            .map(|metadata| metadata.archived)
    }

    pub fn thread_locked(&self) -> Option<bool> {
        self.thread_metadata
            .as_ref()
            .map(|metadata| metadata.locked)
    }

    pub fn thread_pinned(&self) -> Option<bool> {
        self.flags.map(|flags| flags & (1 << 1) != 0)
    }

    pub fn requires_forum_tag(&self) -> bool {
        const REQUIRE_TAG: u64 = 1 << 4;
        self.flags
            .is_some_and(|flags| flags & REQUIRE_TAG == REQUIRE_TAG)
    }
}

#[cfg(test)]
impl ChannelInfo {
    pub(crate) fn test(channel_id: Id<ChannelMarker>, kind: impl Into<String>) -> Self {
        Self {
            guild_id: None,
            channel_id,
            parent_id: None,
            owner_id: None,
            position: None,
            last_message_id: None,
            name: String::new(),
            kind: kind.into(),
            message_count: None,
            member_count: None,
            total_message_sent: None,
            thread_metadata: None,
            flags: None,
            rate_limit_per_user: None,
            available_tags: Vec::new(),
            applied_tags: Vec::new(),
            current_user_joined_thread: None,
            recipients: None,
            permission_overwrites: Vec::new(),
            is_message_request: None,
            is_spam: None,
        }
    }
}

#[cfg(test)]
impl ThreadMetadataInfo {
    pub(crate) fn test(archived: bool, locked: bool) -> Self {
        Self {
            archived,
            auto_archive_duration: None,
            archive_timestamp: None,
            locked,
            invitable: None,
            create_timestamp: None,
        }
    }
}

/// Whether a `PermissionOverwriteInfo` targets a role or an individual member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionOverwriteKind {
    Role,
    Member,
}

/// A single channel-level allow/deny pair against either a role or a member.
/// IDs are stored raw because the same field can refer to a role id, a member
/// id, or the guild id (the `@everyone` role is keyed by the guild snowflake).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PermissionOverwriteInfo {
    pub id: u64,
    pub kind: PermissionOverwriteKind,
    pub allow: u64,
    pub deny: u64,
}

#[cfg(test)]
#[allow(dead_code)]
impl PermissionOverwriteInfo {
    pub(crate) fn test(id: u64, kind: PermissionOverwriteKind) -> Self {
        Self {
            id,
            kind,
            allow: 0,
            deny: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelRecipientInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle (`User.name`). Kept alongside `display_name` so
    /// the @-mention picker can fuzzy-match on both the alias and the raw
    /// username. `None` when the source payload didn't carry a username.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub status: Option<PresenceStatus>,
}

#[cfg(test)]
#[allow(dead_code)]
impl ChannelRecipientInfo {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: None,
        }
    }
}
