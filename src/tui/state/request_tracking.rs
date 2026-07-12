use std::collections::{HashMap, VecDeque};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};
use crate::discord::{AppCommand, AppEvent, ForumPostArchiveState};

use super::DashboardState;

#[derive(Debug, Default)]
pub(super) struct ForumPostListState {
    pub(super) active_post_ids: Vec<Id<ChannelMarker>>,
    pub(super) archived_post_ids: Vec<Id<ChannelMarker>>,
    pub(super) has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LatestMessageHistoryState {
    Loading,
    Loaded,
    Failed,
}

#[derive(Debug, Default)]
pub(super) struct RequestTrackingState {
    pub(super) forum_post_lists: HashMap<Id<ChannelMarker>, ForumPostListState>,
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

impl DashboardState {
    pub(super) fn discord_event_for_apply(&self, event: &AppEvent) -> AppEvent {
        let AppEvent::ForumPostsLoaded {
            channel_id,
            archive_state: ForumPostArchiveState::Archived,
            offset,
            next_offset,
            threads,
            first_messages,
            has_more,
        } = event
        else {
            return event.clone();
        };

        let Some(list) = self.requests.forum_post_lists.get(channel_id) else {
            return event.clone();
        };
        AppEvent::ForumPostsLoaded {
            channel_id: *channel_id,
            archive_state: ForumPostArchiveState::Archived,
            offset: *offset,
            next_offset: *next_offset,
            threads: threads
                .iter()
                .filter(|thread| !list.active_post_ids.contains(&thread.channel_id))
                .cloned()
                .collect(),
            first_messages: first_messages
                .iter()
                .filter(|message| !list.active_post_ids.contains(&message.channel_id))
                .cloned()
                .collect(),
            has_more: *has_more,
        }
    }
}
