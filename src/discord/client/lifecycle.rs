use std::time::Instant;

use super::{
    AppEventPublisher, DiscordClient, DueMemberListSubscription, MemberListSubscriptionRequest,
    publish_app_event,
};
use crate::discord::{
    commands::{AppCommand, ForumPostArchiveState, MessageHistoryAfterMode},
    events::AppEvent,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
    request_lifecycle::{
        ForumPostRequestTarget, MemberListSubscriptionTarget, MentionMemberSearchTarget,
    },
    voice,
};

impl AppEventPublisher {
    pub(crate) async fn publish(&self, mut event: AppEvent) {
        self.prepare(&mut event);
        publish_app_event(
            &self.effects_tx,
            &self.snapshots_tx,
            &self.state,
            &self.revision,
            &self.publish_lock,
            &event,
        )
        .await;
        voice::forward_app_event(&self.voice_events_tx, &event);
    }

    fn prepare(&self, event: &mut AppEvent) {
        if let AppEvent::ApplicationCommandIndexUpdated { guild_id } = event {
            self.application_commands
                .lock()
                .expect("application command cache lock is not poisoned")
                .remove(&Some(*guild_id));
            self.application_command_requests
                .lock()
                .expect("application command request lock is not poisoned")
                .remove(&Some(*guild_id));
        }
        let mut requests = self
            .request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned");
        requests.correlate_interaction_event(event);
        requests.record_event(event);
    }
}

impl DiscordClient {
    pub(crate) fn next_message_history_request(
        &self,
        channel_id: Option<Id<ChannelMarker>>,
        force_reload: bool,
    ) -> Option<Id<ChannelMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_history_request(channel_id, force_reload)
    }

    pub(crate) fn mark_message_history_request_failed(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_history_failed(channel_id);
    }

    pub(crate) fn begin_older_message_history_request(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Id<MessageMarker>,
    ) -> bool {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_older_history_request(channel_id, before)
    }

    pub(crate) fn begin_message_history_after_request(
        &self,
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        mode: MessageHistoryAfterMode,
    ) -> bool {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_history_after_request(channel_id, after, mode)
    }

    pub(crate) fn next_forum_post_request(
        &self,
        target: Option<(Id<GuildMarker>, Id<ChannelMarker>, bool)>,
    ) -> Option<(
        Id<GuildMarker>,
        Id<ChannelMarker>,
        ForumPostArchiveState,
        usize,
    )> {
        let target =
            target.map(
                |(guild_id, channel_id, should_load_more)| ForumPostRequestTarget {
                    guild_id,
                    channel_id,
                    should_load_more,
                },
            );
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_forum_post_request(target)
    }

    pub(crate) fn mark_forum_post_request_failed(
        &self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_forum_post_failed(channel_id, archive_state, offset);
    }

    pub(crate) fn next_pinned_message_request(
        &self,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_pinned_message_request(channel_id)
    }

    pub(crate) fn mark_pinned_message_request_failed(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_pinned_message_failed(channel_id);
    }

    pub(crate) fn next_member_hydration_requests(
        &self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_member_hydration_requests(missing, now)
    }

    pub(crate) fn next_member_request(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<Id<GuildMarker>> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_member_request(guild_id)
    }

    pub(crate) fn remove_member_request(&self, guild_id: Id<GuildMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .remove_member_request(guild_id);
    }

    pub(crate) fn set_mention_member_search_target(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        query: Option<&str>,
        now: Instant,
    ) {
        let target = guild_id
            .zip(query)
            .map(|(guild_id, query)| MentionMemberSearchTarget {
                guild_id,
                query: query.to_owned(),
            });
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .set_mention_member_search_target(target, now);
    }

    pub(crate) fn mention_member_search_deadline(&self) -> Option<Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mention_member_search_deadline()
    }

    pub(crate) fn next_due_mention_member_search(
        &self,
        now: Instant,
    ) -> Option<(Id<GuildMarker>, String)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_due_mention_member_search(now)
            .map(|target| (target.guild_id, target.query))
    }

    pub(crate) fn set_member_list_subscription_target(
        &self,
        target: Option<MemberListSubscriptionRequest>,
        now: Instant,
    ) {
        let target =
            target.map(
                |(guild_id, channel_id, bucket, ranges)| MemberListSubscriptionTarget {
                    guild_id,
                    channel_id,
                    bucket,
                    ranges,
                },
            );
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .set_member_list_subscription_target(target, now);
    }

    pub(crate) fn member_list_subscription_deadline(&self) -> Option<Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .member_list_subscription_deadline()
    }

    pub(crate) fn next_due_member_list_subscription(
        &self,
        now: Instant,
    ) -> Option<DueMemberListSubscription> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_due_member_list_subscription(now)
            .map(|target| (target.guild_id, target.channel_id, target.ranges))
    }

    pub(crate) fn next_thread_preview_requests(
        &self,
        missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_thread_preview_requests(missing)
    }

    pub(crate) fn remove_thread_preview_request(
        &self,
        key: (Id<ChannelMarker>, Id<MessageMarker>),
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .remove_thread_preview_request(key);
    }

    pub(crate) fn next_user_profile_request(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<(Id<UserMarker>, Option<Id<GuildMarker>>, bool)> {
        let is_self = {
            let state = self.read_state();
            if state.user_profile(user_id, guild_id).is_some() {
                return None;
            }
            state.current_user_id() == Some(user_id)
        };
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_user_profile_request(user_id, guild_id)
            .then_some((user_id, guild_id, is_self))
    }

    pub(crate) fn next_user_note_request(&self, user_id: Id<UserMarker>) -> Option<Id<UserMarker>> {
        {
            let state = self.read_state();
            if state.is_note_fetched(user_id) {
                return None;
            }
        }
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .begin_user_note_request(user_id)
            .then_some(user_id)
    }

    pub(crate) fn mark_user_note_request_failed(&self, user_id: Id<UserMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .mark_user_note_failed(user_id);
    }

    pub(crate) fn schedule_read_ack(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        now: Instant,
    ) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .schedule_read_ack(channel_id, message_id, now);
    }

    pub(crate) async fn publish_optimistic_read_ack(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        let (flags, last_viewed) = self.read_state().channel_ack_metadata(channel_id);
        self.publish_event(AppEvent::MessageAck {
            channel_id,
            message_id,
            mention_count: Some(0),
            flags,
            last_viewed: Some(last_viewed),
        })
        .await;
    }

    pub(crate) async fn publish_optimistic_read_acks(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) {
        for (channel_id, message_id) in targets.iter().copied() {
            self.publish_optimistic_read_ack(channel_id, message_id)
                .await;
        }
    }

    pub(crate) fn clear_read_ack(&self, channel_id: Id<ChannelMarker>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .clear_read_ack(channel_id);
    }

    pub(crate) fn clear_read_acks(&self, channel_ids: impl IntoIterator<Item = Id<ChannelMarker>>) {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .clear_read_acks(channel_ids);
    }

    pub(crate) fn next_read_ack_deadline(&self) -> Option<Instant> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .next_read_ack_deadline()
    }

    pub(crate) fn flush_due_read_acks(
        &self,
        now: Instant,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.request_lifecycle
            .lock()
            .expect("request lifecycle lock is not poisoned")
            .flush_due_read_acks(now)
    }

    pub(crate) fn due_read_ack_commands(&self, now: Instant) -> Vec<AppCommand> {
        self.flush_due_read_acks(now)
            .into_iter()
            .map(|(channel_id, message_id)| AppCommand::AckChannel {
                channel_id,
                message_id,
            })
            .collect()
    }
}
