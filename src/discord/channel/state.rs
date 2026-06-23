use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{
    ChannelInfo, ChannelRecipientInfo, ForumTagInfo, PermissionOverwriteInfo, PresenceStatus,
};

use crate::discord::state::DiscordState;

pub(crate) fn is_thread_kind(kind: &str) -> bool {
    matches!(
        kind,
        "thread"
            | "GuildPublicThread"
            | "GuildPrivateThread"
            | "GuildNewsThread"
            | "private-thread"
    )
}

pub(crate) fn is_private_thread_kind(kind: &str) -> bool {
    matches!(kind, "GuildPrivateThread" | "private-thread")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelState {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub owner_id: Option<Id<UserMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
    pub message_count: Option<u64>,
    pub member_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub thread_metadata: Option<crate::discord::ThreadMetadataInfo>,
    pub flags: Option<u64>,
    /// Slow-mode cooldown in seconds (Discord's `rate_limit_per_user`).
    pub rate_limit_per_user: Option<u64>,
    pub available_tags: Vec<ForumTagInfo>,
    pub applied_tags: Vec<Id<ForumTagMarker>>,
    pub current_user_joined_thread: bool,
    /// Current user's notification level for this thread (2=all, 4=mentions,
    /// 8=none). Optimistic only; `None` is unknown (treated as 4).
    pub current_user_thread_notification_flags: Option<u64>,
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
        is_thread_kind(&self.kind)
    }

    pub fn is_forum(&self) -> bool {
        matches!(self.kind.as_str(), "forum" | "media" | "GuildForum")
    }

    pub fn is_voice(&self) -> bool {
        matches!(self.kind.as_str(), "voice" | "GuildVoice")
    }

    pub fn is_private_thread(&self) -> bool {
        is_private_thread_kind(&self.kind)
    }

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
    pub(super) fn from_info(
        recipient: &ChannelRecipientInfo,
        previous_status: Option<PresenceStatus>,
        known_status: Option<PresenceStatus>,
        display_name: String,
    ) -> Self {
        Self {
            user_id: recipient.user_id,
            display_name,
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
        self.navigation
            .channels
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
        self.navigation
            .channels
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
        for channel in self.navigation.channels.values() {
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
        self.navigation.channels.get(&channel_id)
    }

    pub(in crate::discord) fn channel_guild_id(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Id<GuildMarker>> {
        self.navigation
            .channels
            .get(&channel_id)
            .and_then(|channel| channel.guild_id)
    }

    pub(in crate::discord) fn upsert_channel(&mut self, channel: &ChannelInfo) {
        let existing = self.navigation.channels.get(&channel.channel_id);
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
                        let known_status = self
                            .presence
                            .user_presences
                            .get(&recipient.user_id)
                            .copied();
                        let display_name = self.private_user_display_name(
                            recipient.user_id,
                            Some(recipient.display_name.as_str()),
                            recipient.username.as_deref(),
                        );
                        ChannelRecipientState::from_info(
                            recipient,
                            previous_status,
                            known_status,
                            display_name,
                        )
                    })
                    .collect()
            })
            .or_else(|| existing.map(|existing| existing.recipients.clone()))
            .unwrap_or_default();

        let incoming_recipient_names: Vec<String> = channel
            .recipients
            .as_ref()
            .map(|recipients| {
                recipients
                    .iter()
                    .map(|recipient| recipient.display_name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let existing_name_follows_recipients = existing.is_some_and(|existing| {
            private_channel_name_follows_recipients(
                &existing.kind,
                &existing.name,
                existing.id,
                &existing
                    .recipients
                    .iter()
                    .map(|recipient| recipient.display_name.clone())
                    .collect::<Vec<_>>(),
            )
        });
        let name = if channel.guild_id.is_none()
            && !recipients.is_empty()
            && (private_channel_name_follows_recipients(
                &channel.kind,
                &channel.name,
                channel.channel_id,
                &incoming_recipient_names,
            ) || existing_name_follows_recipients)
        {
            joined_recipient_display_names(&recipients)
        } else {
            channel.name.clone()
        };

        // Threads do not own channel-level overwrites. `permitted` is decided
        // by the parent. For everything else, take the newest payload as
        // authoritative because CHANNEL_UPDATE always carries the full array.
        let permission_overwrites = if is_thread_kind(&channel.kind) {
            existing
                .map(|existing| existing.permission_overwrites.clone())
                .unwrap_or_default()
        } else {
            channel.permission_overwrites.clone()
        };
        let current_user_joined_thread = channel
            .current_user_joined_thread
            .or_else(|| existing.map(|existing| existing.current_user_joined_thread))
            .or_else(|| {
                let created_by_current_user = is_thread_kind(&channel.kind)
                    && channel.owner_id.is_some()
                    && channel.owner_id == self.session.current_user_id;
                created_by_current_user.then_some(true)
            })
            .unwrap_or(false);

        self.navigation.channels.insert(
            channel.channel_id,
            ChannelState {
                id: channel.channel_id,
                guild_id: channel.guild_id,
                parent_id: channel.parent_id,
                owner_id: channel.owner_id,
                position: channel.position,
                last_message_id,
                name,
                kind: channel.kind.clone(),
                message_count: channel.message_count,
                member_count: channel.member_count,
                total_message_sent: channel.total_message_sent,
                thread_metadata: channel.thread_metadata.clone(),
                flags: channel.flags,
                rate_limit_per_user: channel
                    .rate_limit_per_user
                    .or_else(|| existing.and_then(|existing| existing.rate_limit_per_user)),
                available_tags: channel.available_tags.clone(),
                applied_tags: channel.applied_tags.clone(),
                current_user_joined_thread,
                // Never comes from the gateway; preserved across upserts so an
                // optimistic update from the command handler is not lost.
                current_user_thread_notification_flags: existing
                    .and_then(|e| e.current_user_thread_notification_flags),
                recipients,
                permission_overwrites,
            },
        );
    }

    pub(in crate::discord) fn set_thread_notification_flags(
        &mut self,
        channel_id: Id<ChannelMarker>,
        flags: u64,
    ) {
        if let Some(channel) = self.navigation.channels.get_mut(&channel_id) {
            channel.current_user_thread_notification_flags = Some(flags);
        }
    }

    pub(in crate::discord) fn set_current_user_thread_membership(
        &mut self,
        channel_id: Id<ChannelMarker>,
        joined: bool,
    ) {
        if let Some(channel) = self
            .navigation
            .channels
            .get_mut(&channel_id)
            .filter(|channel| channel.is_thread())
        {
            channel.current_user_joined_thread = joined;
        }
    }

    pub(in crate::discord) fn refresh_dm_channel_info_from_profile(
        &mut self,
        user_id: Id<UserMarker>,
        display_name: &str,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) {
        for channel in self.navigation.channels.values_mut() {
            if channel.guild_id.is_some() {
                continue;
            }
            let previous_names: Vec<String> = channel
                .recipients
                .iter()
                .map(|recipient| recipient.display_name.clone())
                .collect();
            let mut updated = false;
            for recipient in &mut channel.recipients {
                if recipient.user_id == user_id {
                    recipient.display_name = display_name.to_owned();
                    if let Some(username) = username {
                        recipient.username = Some(username.to_owned());
                    }
                    if avatar_url.is_some() || recipient.avatar_url.is_none() {
                        recipient.avatar_url = avatar_url.map(str::to_owned);
                    }
                    updated = true;
                }
            }
            if updated {
                refresh_private_channel_name_from_recipients(channel, &previous_names);
            }
        }
    }

    pub(in crate::discord) fn update_channel_recipient_presence(
        &mut self,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
    ) {
        for channel in self.navigation.channels.values_mut() {
            for recipient in &mut channel.recipients {
                if recipient.user_id == user_id {
                    recipient.status = status;
                }
            }
        }
    }

    pub(in crate::discord) fn record_channel_message_id(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(channel) = self.navigation.channels.get_mut(&channel_id) {
            channel.last_message_id = channel.last_message_id.max(Some(message_id));
        }
    }

    pub(in crate::discord) fn increment_thread_message_counts(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        let Some(channel) = self
            .navigation
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

pub(super) fn joined_recipient_display_names(recipients: &[ChannelRecipientState]) -> String {
    recipients
        .iter()
        .map(|recipient| recipient.display_name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn private_channel_name_follows_recipients(
    kind: &str,
    current_name: &str,
    channel_id: Id<ChannelMarker>,
    recipient_names: &[String],
) -> bool {
    matches!(kind, "dm" | "Private")
        || current_name == format!("dm-{}", channel_id.get())
        || current_name == recipient_names.join(", ")
}

pub(super) fn refresh_private_channel_name_from_recipients(
    channel: &mut ChannelState,
    previous_names: &[String],
) {
    if channel.guild_id.is_some() {
        return;
    }
    if !private_channel_name_follows_recipients(
        &channel.kind,
        &channel.name,
        channel.id,
        previous_names,
    ) {
        return;
    }
    let new_name = joined_recipient_display_names(&channel.recipients);
    if !new_name.is_empty() {
        channel.name = new_name;
    }
}

#[cfg(test)]
mod tests {
    use super::{is_private_thread_kind, is_thread_kind};

    #[test]
    fn classifies_all_thread_channel_kinds() {
        for kind in [
            "thread",
            "GuildPublicThread",
            "GuildPrivateThread",
            "GuildNewsThread",
            "private-thread",
        ] {
            assert!(is_thread_kind(kind), "{kind} should be a thread kind");
        }

        for kind in [
            "dm",
            "Private",
            "voice",
            "GuildVoice",
            "forum",
            "GuildForum",
        ] {
            assert!(!is_thread_kind(kind), "{kind} should not be a thread kind");
        }
    }

    #[test]
    fn classifies_private_thread_channel_kinds() {
        assert!(is_private_thread_kind("GuildPrivateThread"));
        assert!(is_private_thread_kind("private-thread"));
        assert!(!is_private_thread_kind("GuildPublicThread"));
    }
}
