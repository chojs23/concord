use crate::discord::AppEvent;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use super::DashboardState;

impl DashboardState {
    pub(in crate::tui::state) fn active_channel_message_create(
        &self,
        event: &AppEvent,
    ) -> Option<(Id<ChannelMarker>, Id<MessageMarker>)> {
        let AppEvent::MessageCreate { message } = event else {
            let AppEvent::MessageHistoryAfterLoaded {
                channel_id,
                after,
                messages,
                mode,
                ..
            } = event
            else {
                return None;
            };
            if !mode.is_catch_up() {
                return None;
            }
            let first_newer_message_id = messages
                .iter()
                .filter(|message| message.channel_id == *channel_id && message.message_id > *after)
                .map(|message| message.message_id)
                .min()?;
            return (Some(*channel_id) == self.navigation.channels.active_channel_id)
                .then_some((*channel_id, first_newer_message_id));
        };
        (Some(message.channel_id) == self.navigation.channels.active_channel_id)
            .then_some((message.channel_id, message.message_id))
    }

    pub(in crate::tui::state) fn event_is_self_message_in_active_channel(
        &self,
        event: &AppEvent,
    ) -> bool {
        let AppEvent::MessageCreate { message } = event else {
            return false;
        };
        Some(message.author_id) == self.discord.current_user_id
            && Some(message.channel_id) == self.navigation.channels.active_channel_id
    }
}
