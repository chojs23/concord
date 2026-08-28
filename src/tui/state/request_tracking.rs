use std::collections::{HashMap, VecDeque};

use crate::discord::AppCommand;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};

use super::DashboardState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LatestMessageHistoryState {
    Loading,
    Loaded,
    Failed,
}

#[derive(Debug, Default)]
pub(super) struct RequestTrackingState {
    latest_message_history: HashMap<Id<ChannelMarker>, LatestMessageHistoryState>,
    pub(super) pending_commands: VecDeque<AppCommand>,
}

impl DashboardState {
    pub(in crate::tui) fn drain_pending_commands(&mut self) -> Vec<AppCommand> {
        self.requests.pending_commands.drain(..).collect()
    }

    pub(in crate::tui) fn enqueue_pending_command(&mut self, command: AppCommand) {
        self.requests.pending_commands.push_back(command);
    }

    pub(super) fn queue_application_command_load(&mut self, guild_id: Option<Id<GuildMarker>>) {
        self.enqueue_pending_command(AppCommand::LoadApplicationCommands { guild_id });
    }

    pub(super) fn queue_ack_channel_command(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.enqueue_pending_command(AppCommand::AckChannel {
            channel_id,
            message_id,
        });
    }

    pub(super) fn queue_scheduled_ack_channel_command(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.enqueue_pending_command(AppCommand::ScheduleAckChannel {
            channel_id,
            message_id,
        });
    }

    pub(super) fn queue_ack_channels_command(
        &mut self,
        targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) {
        self.enqueue_pending_command(AppCommand::AckChannels { targets });
    }

    pub(super) fn record_latest_message_history_loaded(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Loaded);
    }

    pub(super) fn record_latest_message_history_loading(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Loading);
    }

    pub(super) fn record_latest_message_history_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Failed);
    }

    pub(super) fn latest_message_history_state(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> LatestMessageHistoryState {
        self.requests
            .latest_message_history
            .get(&channel_id)
            .copied()
            .unwrap_or(LatestMessageHistoryState::Loading)
    }
}
