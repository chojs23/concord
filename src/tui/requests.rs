use std::collections::{HashMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};

use crate::discord::AppEvent;

#[derive(Default)]
pub(super) struct HistoryRequests {
    requests: HashMap<Id<ChannelMarker>, HistoryRequestState>,
    last_channel: Option<Id<ChannelMarker>>,
}

#[derive(Default)]
pub(super) struct ForumPostRequests {
    requests: HashMap<Id<ChannelMarker>, ForumPostRequestState>,
    last_channel: Option<Id<ChannelMarker>>,
}

#[derive(Default)]
pub(super) struct PinnedMessageRequests {
    requests: HashMap<Id<ChannelMarker>, PinnedMessageRequestState>,
    last_channel: Option<Id<ChannelMarker>>,
}

pub(super) struct ForumPostRequestTarget {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) should_load_more: bool,
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

impl ForumPostRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ForumPostsLoaded {
                channel_id,
                offset,
                posts,
                has_more,
                ..
            } => {
                self.requests.insert(
                    *channel_id,
                    ForumPostRequestState::Loaded {
                        next_offset: offset.saturating_add(posts.len()),
                        has_more: *has_more,
                    },
                );
            }
            AppEvent::ForumPostsLoadFailed {
                channel_id, offset, ..
            } => {
                self.mark_failed(*channel_id, *offset);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        target: Option<ForumPostRequestTarget>,
    ) -> Option<(Id<GuildMarker>, Id<ChannelMarker>, usize)> {
        let Some(ForumPostRequestTarget {
            guild_id,
            channel_id,
            should_load_more,
        }) = target
        else {
            self.last_channel = None;
            return None;
        };
        let channel_changed = self.last_channel != Some(channel_id);
        self.last_channel = Some(channel_id);

        match self.requests.get(&channel_id).copied() {
            None => {
                self.requests
                    .insert(channel_id, ForumPostRequestState::Requested { offset: 0 });
                Some((guild_id, channel_id, 0))
            }
            Some(ForumPostRequestState::Failed { offset }) if channel_changed => {
                self.requests
                    .insert(channel_id, ForumPostRequestState::Requested { offset });
                Some((guild_id, channel_id, offset))
            }
            Some(ForumPostRequestState::Loaded {
                next_offset,
                has_more: true,
            }) if should_load_more => {
                self.requests.insert(
                    channel_id,
                    ForumPostRequestState::Requested {
                        offset: next_offset,
                    },
                );
                Some((guild_id, channel_id, next_offset))
            }
            Some(ForumPostRequestState::Requested { .. })
            | Some(ForumPostRequestState::Loaded { .. })
            | Some(ForumPostRequestState::Failed { .. }) => None,
        }
    }

    pub(super) fn mark_failed(&mut self, channel_id: Id<ChannelMarker>, offset: usize) {
        self.requests
            .insert(channel_id, ForumPostRequestState::Failed { offset });
    }
}

impl PinnedMessageRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::PinnedMessagesLoaded { channel_id, .. } => {
                self.requests
                    .insert(*channel_id, PinnedMessageRequestState::Loaded);
            }
            AppEvent::PinnedMessagesLoadFailed { channel_id, .. } => {
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
                    .insert(channel_id, PinnedMessageRequestState::Requested);
                Some(channel_id)
            }
            Some(PinnedMessageRequestState::Failed) if channel_changed => {
                self.requests
                    .insert(channel_id, PinnedMessageRequestState::Requested);
                Some(channel_id)
            }
            Some(
                PinnedMessageRequestState::Requested
                | PinnedMessageRequestState::Loaded
                | PinnedMessageRequestState::Failed,
            ) => None,
        }
    }

    pub(super) fn mark_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .insert(channel_id, PinnedMessageRequestState::Failed);
    }
}

#[derive(Default)]
pub(super) struct MemberRequests {
    requests: HashSet<Id<GuildMarker>>,
}

#[derive(Default)]
pub(super) struct ThreadPreviewRequests {
    requested: HashSet<(Id<ChannelMarker>, Id<MessageMarker>)>,
    failed: HashSet<(Id<ChannelMarker>, Id<MessageMarker>)>,
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

impl ThreadPreviewRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ThreadPreviewLoaded {
                channel_id,
                message,
            } => {
                let key = (*channel_id, message.message_id);
                self.requested.remove(&key);
            }
            AppEvent::ThreadPreviewLoadFailed {
                channel_id,
                message_id,
            } => {
                let key = (*channel_id, *message_id);
                self.requested.remove(&key);
                self.failed.insert(key);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        let visible = missing.iter().copied().collect::<HashSet<_>>();
        self.failed.retain(|key| visible.contains(key));

        missing
            .into_iter()
            .filter(|key| !self.failed.contains(key))
            .filter(|key| self.requested.insert(*key))
            .collect()
    }

    pub(super) fn remove(&mut self, key: (Id<ChannelMarker>, Id<MessageMarker>)) {
        self.requested.remove(&key);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryRequestState {
    Requested,
    Loaded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForumPostRequestState {
    Requested { offset: usize },
    Loaded { next_offset: usize, has_more: bool },
    Failed { offset: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PinnedMessageRequestState {
    Requested,
    Loaded,
    Failed,
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;

    use crate::discord::{AppEvent, ChannelInfo};

    use super::{
        ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberRequests,
        ThreadPreviewRequests,
    };

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
    fn forum_post_request_is_sent_once_per_channel() {
        let mut requests = ForumPostRequests::default();
        let guild = Id::new(100);
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(None), None);
        assert_eq!(
            requests.next(Some(target(guild, first, false))),
            Some((guild, first, 0))
        );
        assert_eq!(requests.next(Some(target(guild, first, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, second, false))),
            Some((guild, second, 0))
        );
    }

    #[test]
    fn forum_post_request_retries_failed_channel_after_reselect() {
        let mut requests = ForumPostRequests::default();
        let guild = Id::new(100);
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(
            requests.next(Some(target(guild, first, false))),
            Some((guild, first, 0))
        );
        requests.record_event(&AppEvent::ForumPostsLoadFailed {
            channel_id: first,
            offset: 0,
            message: "temporary failure".to_owned(),
        });
        assert_eq!(requests.next(Some(target(guild, first, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, second, false))),
            Some((guild, second, 0))
        );
        assert_eq!(
            requests.next(Some(target(guild, first, false))),
            Some((guild, first, 0))
        );
    }

    #[test]
    fn forum_post_request_loads_next_page_when_visible() {
        let mut requests = ForumPostRequests::default();
        let guild = Id::new(100);
        let channel = Id::new(1);

        assert_eq!(
            requests.next(Some(target(guild, channel, false))),
            Some((guild, channel, 0))
        );
        requests.record_event(&AppEvent::ForumPostsLoaded {
            channel_id: channel,
            offset: 0,
            posts: vec![forum_post(channel, 10), forum_post(channel, 11)],
            preview_messages: Vec::new(),
            has_more: true,
        });

        assert_eq!(requests.next(Some(target(guild, channel, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, 2))
        );
        requests.record_event(&AppEvent::ForumPostsLoaded {
            channel_id: channel,
            offset: 2,
            posts: vec![forum_post(channel, 12)],
            preview_messages: Vec::new(),
            has_more: false,
        });

        assert_eq!(requests.next(Some(target(guild, channel, true))), None);
    }

    fn target(
        guild_id: Id<crate::discord::ids::marker::GuildMarker>,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        should_load_more: bool,
    ) -> ForumPostRequestTarget {
        ForumPostRequestTarget {
            guild_id,
            channel_id,
            should_load_more,
        }
    }

    fn forum_post(
        forum_id: Id<crate::discord::ids::marker::ChannelMarker>,
        channel_id: u64,
    ) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(Id::new(100)),
            channel_id: Id::new(channel_id),
            parent_id: Some(forum_id),
            position: None,
            last_message_id: None,
            name: format!("post {channel_id}"),
            kind: "GuildPublicThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
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

    #[test]
    fn thread_preview_request_retries_after_failed_card_is_revisited() {
        let mut requests = ThreadPreviewRequests::default();
        let key = (Id::new(10), Id::new(30));

        assert_eq!(requests.next(vec![key]), vec![key]);
        requests.record_event(&AppEvent::ThreadPreviewLoadFailed {
            channel_id: key.0,
            message_id: key.1,
        });

        assert_eq!(requests.next(vec![key]), Vec::new());
        assert_eq!(requests.next(Vec::new()), Vec::new());
        assert_eq!(requests.next(vec![key]), vec![key]);
    }
}
