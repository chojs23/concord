use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use crate::discord::notification::READ_STATE_MENTION_LOW_IMPORTANCE;
use crate::discord::state::DiscordState;

const DISCORD_EPOCH_UNIX_SECONDS: i64 = 1_420_070_400;
const SECONDS_PER_DAY: i64 = 86_400;
const READ_STATE_IS_GUILD_CHANNEL: u64 = 1 << 0;
const READ_STATE_IS_THREAD: u64 = 1 << 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::discord) struct ChannelReadState {
    pub(in crate::discord) last_acked_message_id: Option<Id<MessageMarker>>,
    pub(in crate::discord) mention_count: u32,
    pub(in crate::discord) notification_count: u32,
    pub(in crate::discord) last_pin_timestamp: Option<String>,
    pub(in crate::discord) latest_pin_timestamp: Option<String>,
    pub(in crate::discord) flags: u64,
    pub(in crate::discord) last_viewed: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::discord) struct NonChannelReadState {
    pub(in crate::discord) last_acked_id: Option<u64>,
    pub(in crate::discord) badge_count: u32,
}

impl ChannelReadState {
    pub(in crate::discord) fn record_mention(&mut self, low_importance: bool) {
        if low_importance {
            if self.mention_count == 0 || self.flags & READ_STATE_MENTION_LOW_IMPORTANCE != 0 {
                self.flags |= READ_STATE_MENTION_LOW_IMPORTANCE;
            }
        } else {
            self.flags &= !READ_STATE_MENTION_LOW_IMPORTANCE;
        }
        self.mention_count = self.mention_count.saturating_add(1);
    }

    pub(in crate::discord) fn record_notification(&mut self) {
        self.notification_count = self.notification_count.saturating_add(1);
    }

    pub(in crate::discord) fn mark_read(&mut self, message_id: Id<MessageMarker>) {
        if self
            .last_acked_message_id
            .is_some_and(|acked| acked >= message_id)
        {
            return;
        }
        self.last_acked_message_id = Some(message_id);
        self.clear_unread_counts();
    }

    pub(in crate::discord) fn apply_server_ack(
        &mut self,
        message_id: Id<MessageMarker>,
        mention_count: Option<u32>,
        flags: Option<u64>,
        last_viewed: Option<u64>,
    ) {
        if self
            .last_acked_message_id
            .is_some_and(|acked| acked > message_id)
        {
            return;
        }
        self.last_acked_message_id = Some(message_id);
        self.notification_count = 0;
        if let Some(mention_count) = mention_count {
            self.mention_count = mention_count;
        }
        if let Some(flags) = flags {
            self.flags = flags;
        } else if mention_count == Some(0) {
            self.flags &= !READ_STATE_MENTION_LOW_IMPORTANCE;
        }
        if let Some(last_viewed) = last_viewed {
            self.last_viewed = Some(last_viewed);
        }
    }

    fn clear_unread_counts(&mut self) {
        self.mention_count = 0;
        self.notification_count = 0;
        self.flags &= !READ_STATE_MENTION_LOW_IMPORTANCE;
    }
}

impl DiscordState {
    pub(in crate::discord) fn channel_ack_metadata(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> (Option<u64>, u64) {
        let flags = self.navigation.channels.get(&channel_id).map(|channel| {
            let mut flags = 0;
            if channel.guild_id.is_some() {
                flags |= READ_STATE_IS_GUILD_CHANNEL;
            }
            if channel.is_thread() {
                flags |= READ_STATE_IS_THREAD;
            }
            flags
        });
        let elapsed_days =
            (chrono::Utc::now().timestamp() - DISCORD_EPOCH_UNIX_SECONDS).max(0) / SECONDS_PER_DAY;
        (flags, elapsed_days as u64)
    }

    pub fn channel_ack_target(&self, channel_id: Id<ChannelMarker>) -> Option<Id<MessageMarker>> {
        let channel = self.navigation.channels.get(&channel_id)?;
        let latest = channel.last_message_id?;
        let acked = self
            .notifications
            .read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id);
        match acked {
            Some(acked) if acked >= latest => None,
            _ => Some(latest),
        }
    }

    pub fn forum_child_ack_targets(
        &self,
        forum_id: Id<ChannelMarker>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        if !self
            .navigation
            .channels
            .get(&forum_id)
            .is_some_and(|channel| channel.is_forum())
        {
            return Vec::new();
        }

        self.navigation
            .channels
            .values()
            .filter(|channel| {
                channel.is_thread()
                    && channel.parent_id == Some(forum_id)
                    && self.channel_notification_eligible(channel.id)
            })
            .filter_map(|channel| {
                self.channel_ack_target(channel.id)
                    .map(|message_id| (channel.id, message_id))
            })
            .collect()
    }

    /// Total unread mentions across channels, from the server read state.
    pub fn total_mention_count(&self) -> u32 {
        self.notifications
            .read_states
            .values()
            .map(|state| state.mention_count)
            .fold(0u32, u32::saturating_add)
    }

    pub fn channel_last_acked_message_id(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Id<MessageMarker>> {
        self.notifications
            .read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id)
    }

    pub(in crate::discord) fn mark_message_read_locally(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        let entry = self
            .notifications_mut()
            .read_states
            .entry(channel_id)
            .or_default();
        entry.mark_read(message_id);
    }
}
