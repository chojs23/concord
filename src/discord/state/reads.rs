use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use super::DiscordState;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ChannelReadState {
    pub(super) last_acked_message_id: Option<Id<MessageMarker>>,
    pub(super) mention_count: u32,
    pub(super) notification_count: u32,
}

impl DiscordState {
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

    pub fn channel_last_acked_message_id(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Id<MessageMarker>> {
        self.notifications
            .read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id)
    }

    pub(super) fn mark_message_read_locally(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        let entry = self
            .notifications
            .read_states
            .entry(channel_id)
            .or_default();
        if entry
            .last_acked_message_id
            .is_none_or(|acked| acked < message_id)
        {
            entry.last_acked_message_id = Some(message_id);
        }
    }
}
