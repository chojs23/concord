use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{ChannelInfo, ChannelRecipientInfo, PermissionOverwriteInfo, PresenceStatus};

use super::{DiscordState, permissions};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelState {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
    pub message_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub thread_archived: Option<bool>,
    pub thread_locked: Option<bool>,
    pub thread_pinned: Option<bool>,
    pub recipients: Vec<ChannelRecipientState>,
    /// Channel-level permission overrides used by `can_view_channel`. Threads
    /// inherit from their parent channel, so this stays empty for threads
    /// even after a payload arrives.
    pub permission_overwrites: Vec<PermissionOverwriteInfo>,
}

impl ChannelState {
    pub fn is_category(&self) -> bool {
        matches!(self.kind.as_str(), "category" | "GuildCategory")
    }

    pub fn is_thread(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "thread" | "GuildPublicThread" | "GuildPrivateThread" | "GuildNewsThread"
        )
    }

    pub fn is_forum(&self) -> bool {
        matches!(self.kind.as_str(), "forum" | "GuildForum")
    }

    pub fn is_private_thread(&self) -> bool {
        matches!(self.kind.as_str(), "GuildPrivateThread" | "private-thread")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelRecipientState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle. Mirrors `ChannelRecipientInfo::username`. The
    /// @-mention picker matches against this in addition to `display_name`.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub status: PresenceStatus,
}

impl ChannelRecipientState {
    fn from_info(
        recipient: &ChannelRecipientInfo,
        previous_status: Option<PresenceStatus>,
        known_status: Option<PresenceStatus>,
    ) -> Self {
        Self {
            user_id: recipient.user_id,
            display_name: recipient.display_name.clone(),
            username: recipient.username.clone(),
            is_bot: recipient.is_bot,
            avatar_url: recipient.avatar_url.clone(),
            status: recipient
                .status
                .or(previous_status)
                .or(known_status)
                .unwrap_or(PresenceStatus::Unknown),
        }
    }
}

/// Counts of viewable vs. permission-hidden channels for a single scope.
/// Surfaced in the debug-log popup so the user can confirm whether a
/// channel they expected to see is actually being filtered out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelVisibilityStats {
    pub visible: usize,
    pub hidden: usize,
}

impl DiscordState {
    pub fn channels_for_guild(&self, guild_id: Option<Id<GuildMarker>>) -> Vec<&ChannelState> {
        self.channels
            .values()
            .filter(|channel| channel.guild_id == guild_id)
            .collect()
    }

    /// Same as `channels_for_guild` but skips channels the authenticated user
    /// cannot see. Use this when populating UI surfaces (sidebar, member-list
    /// subscription targets) so we never present a channel that would 403
    /// when fetched. DMs always pass through unchanged.
    pub fn viewable_channels_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Vec<&ChannelState> {
        self.channels
            .values()
            .filter(|channel| channel.guild_id == guild_id)
            .filter(|channel| self.can_view_channel(channel))
            .collect()
    }

    /// Visible/hidden channel counts for a guild scope. DM scope reports
    /// `(visible, 0)` since DMs are never hidden. Threads are excluded from
    /// both sides. The debug-panel readout focuses on top-level channels
    /// because those are what the user navigates by.
    pub fn channel_visibility_stats(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> ChannelVisibilityStats {
        let mut visible: usize = 0;
        let mut hidden: usize = 0;
        for channel in self.channels.values() {
            if channel.guild_id != guild_id || channel.is_thread() {
                continue;
            }
            if self.can_view_channel(channel) {
                visible += 1;
            } else {
                hidden += 1;
            }
        }
        ChannelVisibilityStats { visible, hidden }
    }

    pub fn channel(&self, channel_id: Id<ChannelMarker>) -> Option<&ChannelState> {
        self.channels.get(&channel_id)
    }

    pub(super) fn channel_guild_id(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Id<GuildMarker>> {
        self.channels
            .get(&channel_id)
            .and_then(|channel| channel.guild_id)
    }

    pub(super) fn upsert_channel(&mut self, channel: &ChannelInfo) {
        let existing = self.channels.get(&channel.channel_id);
        let last_message_id = existing
            .and_then(|existing| existing.last_message_id)
            .max(channel.last_message_id);
        let recipients = channel
            .recipients
            .as_ref()
            .map(|recipients| {
                recipients
                    .iter()
                    .map(|recipient| {
                        let previous_status = existing
                            .and_then(|existing| {
                                existing
                                    .recipients
                                    .iter()
                                    .find(|existing| existing.user_id == recipient.user_id)
                            })
                            .map(|recipient| recipient.status);
                        let known_status = self.user_presences.get(&recipient.user_id).copied();
                        ChannelRecipientState::from_info(recipient, previous_status, known_status)
                    })
                    .collect()
            })
            .or_else(|| existing.map(|existing| existing.recipients.clone()))
            .unwrap_or_default();

        // Threads do not own channel-level overwrites. `permitted` is decided
        // by the parent. For everything else, take the newest payload as
        // authoritative because CHANNEL_UPDATE always carries the full array.
        let permission_overwrites = if permissions::is_thread_kind(&channel.kind) {
            existing
                .map(|existing| existing.permission_overwrites.clone())
                .unwrap_or_default()
        } else {
            channel.permission_overwrites.clone()
        };

        self.channels.insert(
            channel.channel_id,
            ChannelState {
                id: channel.channel_id,
                guild_id: channel.guild_id,
                parent_id: channel.parent_id,
                position: channel.position,
                last_message_id,
                name: channel.name.clone(),
                kind: channel.kind.clone(),
                message_count: channel.message_count,
                total_message_sent: channel.total_message_sent,
                thread_archived: channel.thread_archived,
                thread_locked: channel.thread_locked,
                thread_pinned: channel.thread_pinned,
                recipients,
                permission_overwrites,
            },
        );
    }

    pub(super) fn update_channel_recipient_presence(
        &mut self,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
    ) {
        for channel in self.channels.values_mut() {
            for recipient in &mut channel.recipients {
                if recipient.user_id == user_id {
                    recipient.status = status;
                }
            }
        }
    }

    pub(super) fn record_channel_message_id(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.last_message_id = channel.last_message_id.max(Some(message_id));
        }
    }

    pub(super) fn increment_thread_message_counts(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self
            .channels
            .get_mut(&channel_id)
            .filter(|channel| channel.is_thread())
        else {
            return;
        };

        if let Some(count) = channel.message_count.as_mut() {
            *count = count.saturating_add(1);
        }
        if let Some(count) = channel.total_message_sent.as_mut() {
            *count = count.saturating_add(1);
        }
    }
}
