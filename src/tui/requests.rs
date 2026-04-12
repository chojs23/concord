use std::collections::{HashMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use crate::discord::AppEvent;

#[derive(Default)]
pub(super) struct HistoryRequests {
    requests: HashMap<Id<ChannelMarker>, HistoryRequestState>,
    last_channel: Option<Id<ChannelMarker>>,
}

impl HistoryRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::MessageHistoryLoaded { channel_id, .. } => {
                self.requests
                    .insert(*channel_id, HistoryRequestState::Loaded);
            }
            AppEvent::MessageHistoryLoadFailed { channel_id, .. } => {
                self.mark_failed(*channel_id);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        let Some(channel_id) = channel_id else {
            self.last_channel = None;
            return None;
        };
        let channel_changed = self.last_channel != Some(channel_id);
        self.last_channel = Some(channel_id);

        match self.requests.get(&channel_id).copied() {
            None => {
                self.requests
                    .insert(channel_id, HistoryRequestState::Requested);
                Some(channel_id)
            }
            Some(HistoryRequestState::Failed) if channel_changed => {
                self.requests
                    .insert(channel_id, HistoryRequestState::Requested);
                Some(channel_id)
            }
            Some(
                HistoryRequestState::Requested
                | HistoryRequestState::Loaded
                | HistoryRequestState::Failed,
            ) => None,
        }
    }

    pub(super) fn mark_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .insert(channel_id, HistoryRequestState::Failed);
    }
}

#[derive(Default)]
pub(super) struct MemberRequests {
    requests: HashSet<Id<GuildMarker>>,
}

impl MemberRequests {
    pub(super) fn next(&mut self, guild_id: Option<Id<GuildMarker>>) -> Option<Id<GuildMarker>> {
        let guild_id = guild_id?;
        self.requests.insert(guild_id).then_some(guild_id)
    }

    pub(super) fn remove(&mut self, guild_id: Id<GuildMarker>) {
        self.requests.remove(&guild_id);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryRequestState {
    Requested,
    Loaded,
    Failed,
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;

    use crate::discord::AppEvent;

    use super::{HistoryRequests, MemberRequests};

    #[test]
    fn history_request_is_sent_once_per_channel() {
        let mut requests = HistoryRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(None), None);
        assert_eq!(requests.next(Some(first)), Some(first));
        assert_eq!(requests.next(Some(first)), None);
        assert_eq!(requests.next(Some(second)), Some(second));
    }

    #[test]
    fn history_request_retries_failed_channel_after_reselect() {
        let mut requests = HistoryRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(Some(first)), Some(first));
        requests.record_event(&AppEvent::MessageHistoryLoadFailed {
            channel_id: first,
            message: "temporary failure".to_owned(),
        });
        assert_eq!(requests.next(Some(first)), None);
        assert_eq!(requests.next(Some(second)), Some(second));
        assert_eq!(requests.next(Some(first)), Some(first));
    }

    #[test]
    fn member_request_is_sent_once_per_active_guild() {
        let mut requests = MemberRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(None), None);
        assert_eq!(requests.next(Some(first)), Some(first));
        assert_eq!(requests.next(Some(first)), None);
        assert_eq!(requests.next(Some(second)), Some(second));
        assert_eq!(requests.next(Some(first)), None);
    }

    #[test]
    fn member_request_can_retry_after_remove() {
        let mut requests = MemberRequests::default();
        let guild_id = Id::new(1);

        assert_eq!(requests.next(Some(guild_id)), Some(guild_id));
        requests.remove(guild_id);

        assert_eq!(requests.next(Some(guild_id)), Some(guild_id));
    }
}
