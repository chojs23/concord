use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use crate::discord::ids::{Id, marker::ChannelMarker};

const STALE_MESSAGE_HISTORY_RELOAD_AFTER: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Default)]
pub(super) struct MessageHistoryRefreshState {
    last_viewed_channels: HashMap<Id<ChannelMarker>, Instant>,
    stale_channels: HashSet<Id<ChannelMarker>>,
}

impl MessageHistoryRefreshState {
    pub(super) fn record_channel_left(&mut self, channel_id: Id<ChannelMarker>, now: Instant) {
        self.last_viewed_channels.insert(channel_id, now);
    }

    pub(super) fn mark_stale_if_elapsed(&mut self, channel_id: Id<ChannelMarker>, now: Instant) {
        if self
            .last_viewed_channels
            .get(&channel_id)
            .is_some_and(|last_viewed| {
                now.duration_since(*last_viewed) >= STALE_MESSAGE_HISTORY_RELOAD_AFTER
            })
        {
            self.stale_channels.insert(channel_id);
        }
    }

    pub(super) fn is_stale(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.stale_channels.contains(&channel_id)
    }

    pub(super) fn record_refreshed(&mut self, channel_id: Id<ChannelMarker>) {
        self.stale_channels.remove(&channel_id);
        self.last_viewed_channels.remove(&channel_id);
    }
}
