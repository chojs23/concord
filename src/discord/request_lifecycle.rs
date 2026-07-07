use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

mod primitives;

use primitives::{CursorRequests, LastSelection, OnDemandRequests, TimedRequestSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::{
    AppEvent, ForumPostArchiveState, MessageHistoryAfterMode, MessageHistoryLoadTarget,
};

#[derive(Debug, Default)]
pub(super) struct HistoryRequests {
    requests: OnDemandRequests<Id<ChannelMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct ForumPostRequests {
    requests: HashMap<Id<ChannelMarker>, ForumPostRequestState>,
    last_channel: LastSelection<Id<ChannelMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct PinnedMessageRequests {
    requests: OnDemandRequests<Id<ChannelMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct OlderHistoryRequests {
    requests: CursorRequests<Id<ChannelMarker>, Id<MessageMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct NewerHistoryRequests {
    requests: CursorRequests<Id<ChannelMarker>, Id<MessageMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct ReadAckRequests {
    pending: HashMap<Id<ChannelMarker>, PendingReadAck>,
}

#[derive(Debug)]
pub(crate) struct ForumPostRequestTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) should_load_more: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MentionMemberSearchTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) query: String,
}

/// Batch member fetches deduped per (guild, user) with a TTL so lost
/// responses eventually retry. Used for both message-author lookups and
/// initial unknown-member sweeps.
#[derive(Debug)]
pub(super) struct MemberBatchRequests {
    requested: TimedRequestSet<(Id<GuildMarker>, Id<UserMarker>)>,
}

#[derive(Debug)]
pub(crate) struct MemberListSubscriptionTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) bucket: u32,
    pub(crate) ranges: Vec<(u32, u32)>,
}

#[derive(Debug, Default)]
pub(super) struct MemberListSubscriptionRequests {
    last_sent: Option<MemberListSubscriptionKey>,
    pending: Option<PendingMemberListSubscription>,
}

#[derive(Debug)]
pub(super) struct MentionMemberSearchRequests {
    requested: TimedRequestSet<MentionMemberSearchKey>,
    pending: Option<PendingMentionMemberSearch>,
}

#[derive(Debug, Default)]
pub(super) struct UserProfileRequests {
    in_flight: HashSet<UserProfileRequestKey>,
}

#[derive(Debug, Default)]
pub(super) struct UserNoteRequests {
    in_flight: HashSet<Id<UserMarker>>,
}

#[derive(Debug, Default)]
pub(crate) struct RequestLifecycle {
    history: HistoryRequests,
    forum_posts: ForumPostRequests,
    pinned_messages: PinnedMessageRequests,
    older_history: OlderHistoryRequests,
    newer_history: NewerHistoryRequests,
    read_acks: ReadAckRequests,
    message_author_members: MemberBatchRequests,
    initial_unknown_members: MemberBatchRequests,
    member_list_subscriptions: MemberListSubscriptionRequests,
    mention_member_searches: MentionMemberSearchRequests,
    members: MemberRequests,
    thread_previews: ThreadPreviewRequests,
    user_profiles: UserProfileRequests,
    user_notes: UserNoteRequests,
}

impl RequestLifecycle {
    pub(crate) fn record_event(&mut self, event: &AppEvent) {
        self.history.record_event(event);
        self.older_history.record_event(event);
        self.newer_history.record_event(event);
        self.forum_posts.record_event(event);
        self.pinned_messages.record_event(event);
        // initial_unknown_members skips this. Arrived members stop being
        // reported as missing, so its entries can simply age out.
        self.message_author_members.record_event(event);
        self.thread_previews.record_event(event);
        self.user_profiles.record_event(event);
        self.user_notes.record_event(event);
    }

    pub(crate) fn next_history_request(
        &mut self,
        channel_id: Option<Id<ChannelMarker>>,
        force_reload: bool,
    ) -> Option<Id<ChannelMarker>> {
        self.history.next(channel_id, force_reload)
    }

    pub(crate) fn mark_history_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.history.mark_failed(channel_id);
    }

    pub(crate) fn begin_older_history_request(
        &mut self,
        channel_id: Id<ChannelMarker>,
        before: Id<MessageMarker>,
    ) -> bool {
        self.older_history.begin_request(channel_id, before)
    }

    pub(crate) fn begin_history_after_request(
        &mut self,
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        mode: MessageHistoryAfterMode,
    ) -> bool {
        self.newer_history.begin_request(channel_id, after, mode)
    }

    pub(crate) fn next_forum_post_request(
        &mut self,
        target: Option<ForumPostRequestTarget>,
    ) -> Option<(
        Id<GuildMarker>,
        Id<ChannelMarker>,
        ForumPostArchiveState,
        usize,
    )> {
        self.forum_posts.next(target)
    }

    pub(crate) fn mark_forum_post_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) {
        self.forum_posts
            .mark_failed(channel_id, archive_state, offset);
    }

    pub(crate) fn next_pinned_message_request(
        &mut self,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        self.pinned_messages.next(channel_id)
    }

    pub(crate) fn mark_pinned_message_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.pinned_messages.mark_failed(channel_id);
    }

    pub(crate) fn next_message_author_member_requests(
        &mut self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.message_author_members.next(missing, now)
    }

    pub(crate) fn next_initial_unknown_member_requests(
        &mut self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.initial_unknown_members.next(missing, now)
    }

    pub(crate) fn next_member_request(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<Id<GuildMarker>> {
        self.members.next(guild_id)
    }

    pub(crate) fn remove_member_request(&mut self, guild_id: Id<GuildMarker>) {
        self.members.remove(guild_id);
    }

    pub(crate) fn set_mention_member_search_target(
        &mut self,
        target: Option<MentionMemberSearchTarget>,
        now: Instant,
    ) {
        self.mention_member_searches.set_target(target, now);
    }

    pub(crate) fn mention_member_search_deadline(&self) -> Option<Instant> {
        self.mention_member_searches.pending_deadline()
    }

    pub(crate) fn next_due_mention_member_search(
        &mut self,
        now: Instant,
    ) -> Option<MentionMemberSearchTarget> {
        self.mention_member_searches.next_due(now)
    }

    pub(crate) fn set_member_list_subscription_target(
        &mut self,
        target: Option<MemberListSubscriptionTarget>,
        now: Instant,
    ) {
        self.member_list_subscriptions.set_target(target, now);
    }

    pub(crate) fn member_list_subscription_deadline(&self) -> Option<Instant> {
        self.member_list_subscriptions.pending_deadline()
    }

    pub(crate) fn next_due_member_list_subscription(
        &mut self,
        now: Instant,
    ) -> Option<MemberListSubscriptionTarget> {
        self.member_list_subscriptions.next_due(now)
    }

    pub(crate) fn next_thread_preview_requests(
        &mut self,
        missing: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.thread_previews.next(missing)
    }

    pub(crate) fn remove_thread_preview_request(
        &mut self,
        key: (Id<ChannelMarker>, Id<MessageMarker>),
    ) {
        self.thread_previews.remove(key);
    }

    pub(crate) fn begin_user_profile_request(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> bool {
        self.user_profiles.begin_request(user_id, guild_id)
    }

    pub(crate) fn begin_user_note_request(&mut self, user_id: Id<UserMarker>) -> bool {
        self.user_notes.begin_request(user_id)
    }

    pub(crate) fn mark_user_note_failed(&mut self, user_id: Id<UserMarker>) {
        self.user_notes.mark_failed(user_id);
    }

    pub(crate) fn schedule_read_ack(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        now: Instant,
    ) {
        self.read_acks.schedule(channel_id, message_id, now);
    }

    pub(crate) fn clear_read_ack(&mut self, channel_id: Id<ChannelMarker>) {
        self.read_acks.clear(channel_id);
    }

    pub(crate) fn clear_read_acks(
        &mut self,
        channel_ids: impl IntoIterator<Item = Id<ChannelMarker>>,
    ) {
        for channel_id in channel_ids {
            self.clear_read_ack(channel_id);
        }
    }

    pub(crate) fn next_read_ack_deadline(&self) -> Option<Instant> {
        self.read_acks.next_deadline()
    }

    pub(crate) fn flush_due_read_acks(
        &mut self,
        now: Instant,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.read_acks.flush_due(now)
    }
}

impl UserProfileRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::UserProfileLoaded { guild_id, profile } => {
                self.in_flight.remove(&UserProfileRequestKey {
                    user_id: profile.user_id,
                    guild_id: *guild_id,
                });
            }
            AppEvent::UserProfileLoadFailed {
                user_id, guild_id, ..
            } => {
                self.in_flight.remove(&UserProfileRequestKey {
                    user_id: *user_id,
                    guild_id: *guild_id,
                });
            }
            _ => {}
        }
    }

    pub(super) fn begin_request(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> bool {
        self.in_flight
            .insert(UserProfileRequestKey { user_id, guild_id })
    }
}

impl UserNoteRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        if let AppEvent::UserNoteLoaded { user_id, .. } = event {
            self.in_flight.remove(user_id);
        }
    }

    pub(super) fn begin_request(&mut self, user_id: Id<UserMarker>) -> bool {
        self.in_flight.insert(user_id)
    }

    pub(super) fn mark_failed(&mut self, user_id: Id<UserMarker>) {
        self.in_flight.remove(&user_id);
    }
}

impl HistoryRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before: None,
                ..
            }
            | AppEvent::MessageHistoryRefreshed { channel_id, .. } => {
                self.requests.mark_loaded(*channel_id);
            }
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                target: MessageHistoryLoadTarget::Latest,
                ..
            } => {
                self.mark_failed(*channel_id);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        channel_id: Option<Id<ChannelMarker>>,
        force_reload: bool,
    ) -> Option<Id<ChannelMarker>> {
        self.requests.next(channel_id, force_reload)
    }

    pub(super) fn mark_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests.mark_failed(channel_id);
    }
}

impl ForumPostRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset: _,
                next_offset,
                has_more,
                ..
            } => {
                self.requests.entry(*channel_id).or_default().set_loaded(
                    *archive_state,
                    *next_offset,
                    *has_more,
                );
            }
            AppEvent::ForumPostsLoadFailed {
                channel_id,
                archive_state,
                offset,
                ..
            } => {
                self.mark_failed(*channel_id, *archive_state, *offset);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        target: Option<ForumPostRequestTarget>,
    ) -> Option<(
        Id<GuildMarker>,
        Id<ChannelMarker>,
        ForumPostArchiveState,
        usize,
    )> {
        let Some(ForumPostRequestTarget {
            guild_id,
            channel_id,
            should_load_more,
        }) = target
        else {
            self.last_channel.clear();
            return None;
        };
        let channel_changed = self.last_channel.select(channel_id);

        let state = self.requests.entry(channel_id).or_default();
        let next = state.next(channel_changed, should_load_more)?;
        Some((guild_id, channel_id, next.archive_state, next.offset))
    }

    pub(super) fn mark_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) {
        self.requests
            .entry(channel_id)
            .or_default()
            .set_failed(archive_state, offset);
    }
}

impl PinnedMessageRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::PinnedMessagesLoaded { channel_id, .. } => {
                self.requests.mark_loaded(*channel_id);
            }
            AppEvent::PinnedMessagesLoadFailed { channel_id, .. } => {
                self.mark_failed(*channel_id);
            }
            // The pin set changed, so the next selection reloads it.
            AppEvent::ChannelPinsUpdate { channel_id, .. } => {
                self.requests.reset(channel_id);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        self.requests.next(channel_id, false)
    }

    pub(super) fn mark_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests.mark_failed(channel_id);
    }
}

impl OlderHistoryRequests {
    fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before: Some(response_before),
                messages,
            } => {
                self.requests
                    .record_loaded(*channel_id, *response_before, messages.is_empty());
            }
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                target: MessageHistoryLoadTarget::Older { before },
                ..
            } => {
                self.requests.record_failed(*channel_id, *before);
            }
            _ => {}
        }
    }

    fn begin_request(&mut self, channel_id: Id<ChannelMarker>, before: Id<MessageMarker>) -> bool {
        // An empty page always means the top of the history was reached.
        self.requests.begin_request(channel_id, before, true)
    }
}

impl NewerHistoryRequests {
    fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::MessageHistoryAfterLoaded {
                channel_id,
                after: response_after,
                messages,
                ..
            } => {
                self.requests
                    .record_loaded(*channel_id, *response_after, messages.is_empty());
            }
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                target: MessageHistoryLoadTarget::Newer { after },
                ..
            } => {
                self.requests.record_failed(*channel_id, *after);
            }
            _ => {}
        }
    }

    fn begin_request(
        &mut self,
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        mode: MessageHistoryAfterMode,
    ) -> bool {
        self.requests
            .begin_request(channel_id, after, mode.exhausts_on_empty())
    }
}

impl MemberBatchRequests {
    const REQUEST_TTL: Duration = Duration::from_secs(30);
    const MAX_REQUESTED: usize = 4096;

    /// Clears the dedupe entry when the member arrives through the gateway.
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildMemberUpsert { guild_id, member }
            | AppEvent::GuildMemberAdd { guild_id, member } => {
                self.requested.remove(&(*guild_id, member.user_id));
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.requested.prune(now);

        let mut requests = Vec::new();
        for (guild_id, user_ids) in missing {
            let fresh_user_ids = user_ids
                .into_iter()
                .filter(|user_id| self.requested.insert((guild_id, *user_id), now))
                .collect::<Vec<_>>();
            if !fresh_user_ids.is_empty() {
                requests.push((guild_id, fresh_user_ids));
            }
        }
        requests
    }
}

impl Default for MemberBatchRequests {
    fn default() -> Self {
        Self {
            requested: TimedRequestSet::new(Self::REQUEST_TTL, Self::MAX_REQUESTED),
        }
    }
}

impl MemberListSubscriptionRequests {
    const DEBOUNCE: Duration = Duration::from_millis(100);

    pub(super) fn set_target(
        &mut self,
        target: Option<MemberListSubscriptionTarget>,
        now: Instant,
    ) {
        let Some(target) = target else {
            self.pending = None;
            self.last_sent = None;
            return;
        };
        let key = target.key();

        // The initial guild subscription already covers bucket 0. Only send a
        // bucket-0 update when it resets a previously wider subscription.
        if self.last_sent.is_none() && key.bucket == 0 {
            self.pending = None;
            return;
        }
        if self.last_sent.as_ref() == Some(&key) {
            self.pending = None;
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.target.key() == key)
        {
            return;
        }
        self.pending = Some(PendingMemberListSubscription {
            target,
            ready_at: now + Self::DEBOUNCE,
        });
    }

    pub(super) fn pending_deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.ready_at)
    }

    pub(super) fn next_due(&mut self, now: Instant) -> Option<MemberListSubscriptionTarget> {
        let pending = self.pending.as_ref()?;
        if pending.ready_at > now {
            return None;
        }
        let pending = self.pending.take()?;
        self.last_sent = Some(pending.target.key());
        Some(pending.target)
    }
}

#[derive(Debug, Default)]
pub(super) struct MemberRequests {
    requests: HashSet<Id<GuildMarker>>,
}

#[derive(Debug, Default)]
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

impl MentionMemberSearchRequests {
    const MIN_QUERY_CHARS: usize = 2;
    const MAX_QUERY_CHARS: usize = 64;
    const DEBOUNCE: Duration = Duration::from_millis(250);
    const REQUEST_TTL: Duration = Duration::from_secs(30);
    const MAX_REQUESTED: usize = 128;

    pub(super) fn set_target(&mut self, target: Option<MentionMemberSearchTarget>, now: Instant) {
        self.requested.prune(now);
        let Some(target) = target.and_then(normalize_mention_member_search_target) else {
            self.pending = None;
            return;
        };
        let key = target.key();
        if self.requested.contains(&key) {
            self.pending = None;
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.target.key() == key)
        {
            return;
        }
        self.pending = Some(PendingMentionMemberSearch {
            target,
            ready_at: now + Self::DEBOUNCE,
        });
    }

    pub(super) fn pending_deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.ready_at)
    }

    pub(super) fn next_due(&mut self, now: Instant) -> Option<MentionMemberSearchTarget> {
        self.requested.prune(now);
        let pending = self.pending.as_ref()?;
        if pending.ready_at > now {
            return None;
        }
        let pending = self.pending.take()?;
        let key = pending.target.key();
        if !self.requested.insert(key, now) {
            return None;
        }
        Some(pending.target)
    }
}

impl Default for MentionMemberSearchRequests {
    fn default() -> Self {
        Self {
            requested: TimedRequestSet::new(Self::REQUEST_TTL, Self::MAX_REQUESTED),
            pending: None,
        }
    }
}

type MentionMemberSearchKey = (Id<GuildMarker>, String);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct UserProfileRequestKey {
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
}

const READ_ACK_DEBOUNCE: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingReadAck {
    message_id: Id<MessageMarker>,
    deadline: Instant,
}

#[derive(Debug, PartialEq)]
struct MemberListSubscriptionKey {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    bucket: u32,
}

#[derive(Debug)]
struct PendingMentionMemberSearch {
    target: MentionMemberSearchTarget,
    ready_at: Instant,
}

#[derive(Debug)]
struct PendingMemberListSubscription {
    target: MemberListSubscriptionTarget,
    ready_at: Instant,
}

impl ReadAckRequests {
    fn schedule(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        now: Instant,
    ) {
        let deadline = now + READ_ACK_DEBOUNCE;
        self.pending
            .entry(channel_id)
            .and_modify(|pending| {
                pending.message_id = pending.message_id.max(message_id);
            })
            .or_insert(PendingReadAck {
                message_id,
                deadline,
            });
    }

    fn clear(&mut self, channel_id: Id<ChannelMarker>) {
        self.pending.remove(&channel_id);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|pending| pending.deadline).min()
    }

    fn flush_due(&mut self, now: Instant) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        let mut due = Vec::new();
        self.pending.retain(|channel_id, pending| {
            if pending.deadline <= now {
                due.push((*channel_id, pending.message_id));
                false
            } else {
                true
            }
        });
        due
    }
}

impl MentionMemberSearchTarget {
    fn key(&self) -> MentionMemberSearchKey {
        (self.guild_id, self.query.clone())
    }
}

impl MemberListSubscriptionTarget {
    fn key(&self) -> MemberListSubscriptionKey {
        MemberListSubscriptionKey {
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            bucket: self.bucket,
        }
    }
}

fn normalize_mention_member_search_target(
    target: MentionMemberSearchTarget,
) -> Option<MentionMemberSearchTarget> {
    let query = normalize_mention_member_search_query(&target.query);
    (query.chars().count() >= MentionMemberSearchRequests::MIN_QUERY_CHARS).then_some(
        MentionMemberSearchTarget {
            guild_id: target.guild_id,
            query,
        },
    )
}

fn normalize_mention_member_search_query(query: &str) -> String {
    let mut normalized = String::new();
    let mut count = 0usize;
    for ch in query.trim().chars() {
        for lowered in ch.to_lowercase() {
            if count >= MentionMemberSearchRequests::MAX_QUERY_CHARS {
                return normalized;
            }
            normalized.push(lowered);
            count += 1;
        }
    }
    normalized
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ForumPostRequestCursor {
    archive_state: ForumPostArchiveState,
    offset: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ForumPostRequestState {
    active: ForumPostPageRequestState,
    archived: ForumPostPageRequestState,
}

impl ForumPostRequestState {
    fn next(
        &mut self,
        channel_changed: bool,
        should_load_more: bool,
    ) -> Option<ForumPostRequestCursor> {
        if let Some(offset) = self.active.next(channel_changed, true, should_load_more) {
            return Some(ForumPostRequestCursor {
                archive_state: ForumPostArchiveState::Active,
                offset,
            });
        }
        // Only start the archived stream once the active search is fully
        // drained. While an active page is still in flight, `active.next`
        // returns `None` even though more active posts are coming, so without
        // this guard the archived section would start loading and interleave
        // before the active list finishes.
        let allow_archived_initial = should_load_more && self.active.is_exhausted();
        if let Some(offset) =
            self.archived
                .next(channel_changed, allow_archived_initial, should_load_more)
        {
            return Some(ForumPostRequestCursor {
                archive_state: ForumPostArchiveState::Archived,
                offset,
            });
        }
        None
    }

    fn set_loaded(
        &mut self,
        archive_state: ForumPostArchiveState,
        next_offset: usize,
        has_more: bool,
    ) {
        self.page_mut(archive_state)
            .set_loaded(next_offset, has_more);
    }

    fn set_failed(&mut self, archive_state: ForumPostArchiveState, offset: usize) {
        self.page_mut(archive_state).set_failed(offset);
    }

    fn page_mut(&mut self, archive_state: ForumPostArchiveState) -> &mut ForumPostPageRequestState {
        match archive_state {
            ForumPostArchiveState::Active => &mut self.active,
            ForumPostArchiveState::Archived => &mut self.archived,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ForumPostPageRequestState {
    #[default]
    NotRequested,
    Requested {
        offset: usize,
    },
    Loaded {
        next_offset: usize,
        has_more: bool,
    },
    Failed {
        offset: usize,
    },
}

impl ForumPostPageRequestState {
    fn next(
        &mut self,
        channel_changed: bool,
        allow_initial: bool,
        should_load_more: bool,
    ) -> Option<usize> {
        match *self {
            Self::NotRequested if allow_initial => {
                *self = Self::Requested { offset: 0 };
                Some(0)
            }
            Self::Failed { offset } if channel_changed => {
                *self = Self::Requested { offset };
                Some(offset)
            }
            Self::Loaded {
                next_offset,
                has_more: true,
            } if should_load_more => {
                *self = Self::Requested {
                    offset: next_offset,
                };
                Some(next_offset)
            }
            Self::NotRequested
            | Self::Requested { .. }
            | Self::Loaded { .. }
            | Self::Failed { .. } => None,
        }
    }

    fn set_loaded(&mut self, next_offset: usize, has_more: bool) {
        *self = Self::Loaded {
            next_offset,
            has_more,
        };
    }

    fn set_failed(&mut self, offset: usize) {
        *self = Self::Failed { offset };
    }

    /// A page stream is drained once a loaded page reported no more results.
    /// A pending (`Requested`) page is not exhausted: more results may follow.
    fn is_exhausted(&self) -> bool {
        matches!(
            self,
            Self::Loaded {
                has_more: false,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;

    use crate::discord::{
        AppEvent, ChannelInfo, ForumPostArchiveState, MemberInfo, MessageHistoryAfterMode,
        MessageHistoryLoadTarget, UserProfileInfo,
    };

    use crate::discord::test_builders::{
        ChannelPinsUpdateFixture, ForumPostsLoadFailedFixture, ForumPostsLoadedFixture,
        MessageHistoryAfterLoadedFixture, MessageHistoryLoadFailedFixture,
        MessageHistoryLoadedFixture, UserProfileLoadFailedFixture, channel_pins_update_event,
        forum_posts_load_failed_event, forum_posts_loaded_event,
        message_history_after_loaded_event, message_history_load_failed_event,
        message_history_loaded_event, user_profile_load_failed_event,
    };

    use super::{
        ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberBatchRequests,
        MemberListSubscriptionRequests, MemberListSubscriptionTarget, MemberRequests,
        MentionMemberSearchRequests, MentionMemberSearchTarget, PinnedMessageRequests,
        RequestLifecycle, ThreadPreviewRequests, UserNoteRequests, UserProfileRequests,
    };

    #[test]
    fn history_request_is_sent_once_and_retries_failed_channel_after_reselect() {
        let mut requests = HistoryRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(None, false), None);
        assert_eq!(requests.next(Some(first), false), Some(first));
        assert_eq!(requests.next(Some(first), false), None);
        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id: first,
                target: MessageHistoryLoadTarget::Latest,
                message: "temporary failure".to_owned(),
            },
        ));
        assert_eq!(requests.next(Some(first), false), None);
        assert_eq!(requests.next(Some(second), false), Some(second));
        assert_eq!(requests.next(Some(first), false), Some(first));

        let mut requests = HistoryRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(Some(first), false), Some(first));
        requests.record_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
            channel_id: first,
            ..MessageHistoryLoadedFixture::new()
        }));
        assert_eq!(requests.next(Some(first), true), None);
        assert_eq!(requests.next(Some(second), false), Some(second));
        assert_eq!(requests.next(Some(first), true), Some(first));
    }

    #[test]
    fn pinned_message_request_is_on_demand_and_retries_failed_channel_after_reselect() {
        let mut requests = PinnedMessageRequests::default();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(requests.next(None), None);
        assert_eq!(requests.next(Some(first)), Some(first));
        assert_eq!(requests.next(Some(first)), None);
        requests.record_event(&AppEvent::PinnedMessagesLoaded {
            channel_id: first,
            messages: Vec::new(),
        });
        assert_eq!(requests.next(Some(first)), None);
        assert_eq!(requests.next(Some(second)), Some(second));
        assert_eq!(requests.next(Some(first)), None);

        let mut requests = PinnedMessageRequests::default();
        assert_eq!(requests.next(Some(first)), Some(first));
        requests.record_event(&AppEvent::PinnedMessagesLoadFailed {
            channel_id: first,
            message: "temporary failure".to_owned(),
        });
        assert_eq!(requests.next(Some(first)), None);
        assert_eq!(requests.next(Some(second)), Some(second));
        assert_eq!(requests.next(Some(first)), Some(first));
    }

    #[test]
    fn pinned_message_request_reloads_after_channel_pins_update() {
        let mut requests = PinnedMessageRequests::default();
        let channel_id = Id::new(1);

        assert_eq!(requests.next(Some(channel_id)), Some(channel_id));
        requests.record_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: Vec::new(),
        });
        assert_eq!(requests.next(Some(channel_id)), None);

        requests.record_event(&channel_pins_update_event(ChannelPinsUpdateFixture {
            channel_id,
            ..ChannelPinsUpdateFixture::new()
        }));

        assert_eq!(requests.next(Some(channel_id)), Some(channel_id));
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
            Some((guild, first, ForumPostArchiveState::Active, 0))
        );
        assert_eq!(requests.next(Some(target(guild, first, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, second, false))),
            Some((guild, second, ForumPostArchiveState::Active, 0))
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
            Some((guild, first, ForumPostArchiveState::Active, 0))
        );
        requests.record_event(&forum_posts_load_failed_event(
            ForumPostsLoadFailedFixture {
                channel_id: first,
                archive_state: ForumPostArchiveState::Active,
                message: "temporary failure".to_owned(),
                ..ForumPostsLoadFailedFixture::new()
            },
        ));
        assert_eq!(requests.next(Some(target(guild, first, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, second, false))),
            Some((guild, second, ForumPostArchiveState::Active, 0))
        );
        assert_eq!(
            requests.next(Some(target(guild, first, false))),
            Some((guild, first, ForumPostArchiveState::Active, 0))
        );
    }

    #[test]
    fn forum_post_request_tracks_active_archived_and_server_offsets() {
        let mut requests = ForumPostRequests::default();
        let guild = Id::new(100);
        let channel = Id::new(1);

        assert_eq!(
            requests.next(Some(target(guild, channel, false))),
            Some((guild, channel, ForumPostArchiveState::Active, 0))
        );
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Active,
            next_offset: 2,
            threads: vec![forum_post(channel, 10), forum_post(channel, 11)],
            has_more: true,
            ..ForumPostsLoadedFixture::new()
        }));

        assert_eq!(requests.next(Some(target(guild, channel, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Active, 2))
        );
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Active,
            offset: 2,
            next_offset: 3,
            threads: vec![forum_post(channel, 12)],
            ..ForumPostsLoadedFixture::new()
        }));

        assert_eq!(requests.next(Some(target(guild, channel, false))), None);
        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Archived, 0))
        );
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Archived,
            next_offset: 2,
            threads: vec![forum_post(channel, 11), forum_post(channel, 12)],
            has_more: true,
            ..ForumPostsLoadedFixture::new()
        }));

        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Archived, 2))
        );

        let mut requests = ForumPostRequests::default();
        let channel = Id::new(2);

        assert_eq!(
            requests.next(Some(target(guild, channel, false))),
            Some((guild, channel, ForumPostArchiveState::Active, 0))
        );
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Active,
            next_offset: 25,
            threads: vec![forum_post(channel, 10), forum_post(channel, 11)],
            has_more: true,
            ..ForumPostsLoadedFixture::new()
        }));

        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Active, 25))
        );
    }

    #[test]
    fn archived_forum_posts_wait_for_the_active_search_to_drain() {
        let mut requests = ForumPostRequests::default();
        let guild = Id::new(100);
        let channel = Id::new(1);

        assert_eq!(
            requests.next(Some(target(guild, channel, false))),
            Some((guild, channel, ForumPostArchiveState::Active, 0))
        );
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Active,
            next_offset: 25,
            threads: vec![forum_post(channel, 10)],
            has_more: true,
            ..ForumPostsLoadedFixture::new()
        }));

        // Scrolling fetches the next active page.
        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Active, 25))
        );
        // While that page is still in flight, archived must not start and
        // interleave ahead of the rest of the active posts.
        assert_eq!(requests.next(Some(target(guild, channel, true))), None);

        // Only after the active search reports it is drained does archived begin.
        requests.record_event(&forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id: channel,
            archive_state: ForumPostArchiveState::Active,
            offset: 25,
            next_offset: 26,
            threads: vec![forum_post(channel, 11)],
            ..ForumPostsLoadedFixture::new()
        }));
        assert_eq!(
            requests.next(Some(target(guild, channel, true))),
            Some((guild, channel, ForumPostArchiveState::Archived, 0))
        );
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
            parent_id: Some(forum_id),
            name: format!("post {channel_id}"),
            thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(channel_id), "GuildPublicThread")
        }
    }

    fn subscription_target(bucket: u32) -> MemberListSubscriptionTarget {
        let ranges = if bucket == 0 {
            vec![(0, 99)]
        } else {
            vec![(0, 99), (bucket * 100, bucket * 100 + 99)]
        };
        MemberListSubscriptionTarget {
            guild_id: Id::new(1),
            channel_id: Id::new(2),
            bucket,
            ranges,
        }
    }

    fn user_profile(user_id: Id<crate::discord::ids::marker::UserMarker>) -> UserProfileInfo {
        UserProfileInfo::test(user_id, "neo")
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
    fn user_profile_request_dedupes_until_success_or_failure() {
        let mut requests = UserProfileRequests::default();
        let user_id = Id::new(10);
        let guild_id = Some(Id::new(1));

        assert!(requests.begin_request(user_id, guild_id));
        assert!(!requests.begin_request(user_id, guild_id));

        requests.record_event(&AppEvent::UserProfileLoaded {
            guild_id,
            profile: user_profile(user_id),
        });
        assert!(requests.begin_request(user_id, guild_id));

        requests.record_event(&user_profile_load_failed_event(
            UserProfileLoadFailedFixture {
                user_id,
                guild_id,
                message: "temporary failure".to_owned(),
            },
        ));
        assert!(requests.begin_request(user_id, guild_id));
    }

    #[test]
    fn user_note_request_dedupes_until_success_or_failure() {
        let mut requests = UserNoteRequests::default();
        let user_id = Id::new(10);

        assert!(requests.begin_request(user_id));
        assert!(!requests.begin_request(user_id));

        requests.record_event(&AppEvent::UserNoteLoaded {
            user_id,
            note: Some("note".to_owned()),
        });
        assert!(requests.begin_request(user_id));

        requests.mark_failed(user_id);
        assert!(requests.begin_request(user_id));
    }

    #[test]
    fn message_author_member_request_dedupes_until_member_arrives_or_ttl_expires() {
        let mut requests = MemberBatchRequests::default();
        let guild_id = Id::new(1);
        let user_id = Id::new(10);
        let other_user_id = Id::new(20);
        let now = std::time::Instant::now();

        assert_eq!(
            requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
            vec![(guild_id, vec![user_id, other_user_id])]
        );
        assert_eq!(
            requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
            Vec::new()
        );

        requests.record_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                username: Some("neo".to_owned()),
                ..MemberInfo::test(user_id, "neo")
            },
        });
        assert_eq!(
            requests.next(vec![(guild_id, vec![user_id, other_user_id])], now),
            vec![(guild_id, vec![user_id])]
        );

        let retry_at = now + MemberBatchRequests::REQUEST_TTL + std::time::Duration::from_millis(1);
        assert_eq!(
            requests.next(vec![(guild_id, vec![other_user_id])], retry_at),
            vec![(guild_id, vec![other_user_id])]
        );
    }

    #[test]
    fn member_list_subscription_debounces_and_coalesces_bucket_updates() {
        let mut requests = MemberListSubscriptionRequests::default();
        let now = std::time::Instant::now();

        requests.set_target(Some(subscription_target(0)), now);
        assert_eq!(requests.pending_deadline(), None);

        requests.set_target(Some(subscription_target(1)), now);
        let first_deadline = requests
            .pending_deadline()
            .expect("bucket one should arm debounce");
        assert!(
            requests
                .next_due(first_deadline - std::time::Duration::from_millis(1))
                .is_none()
        );

        requests.set_target(
            Some(subscription_target(2)),
            now + std::time::Duration::from_millis(1),
        );
        let second_deadline = requests
            .pending_deadline()
            .expect("latest bucket should stay pending");
        let target = requests
            .next_due(second_deadline)
            .expect("latest bucket should be sent after debounce");
        assert_eq!(target.bucket, 2);
        assert_eq!(target.ranges, vec![(0, 99), (200, 299)]);

        requests.set_target(Some(subscription_target(2)), second_deadline);
        assert_eq!(requests.pending_deadline(), None);

        requests.set_target(Some(subscription_target(0)), second_deadline);
        assert!(requests.pending_deadline().is_some());
    }

    #[test]
    fn mention_member_search_debounces_bounds_and_retries_queries() {
        let mut requests = MentionMemberSearchRequests::default();
        let guild_id = Id::new(1);
        let now = std::time::Instant::now();

        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: "A".to_owned(),
            }),
            now,
        );
        assert_eq!(requests.pending_deadline(), None);

        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: " Alice ".to_owned(),
            }),
            now,
        );
        let deadline = requests
            .pending_deadline()
            .expect("valid query should arm debounce");
        assert_eq!(
            requests.next_due(deadline - std::time::Duration::from_millis(1)),
            None
        );
        assert_eq!(
            requests.next_due(deadline),
            Some(MentionMemberSearchTarget {
                guild_id,
                query: "alice".to_owned(),
            })
        );

        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: "ALICE".to_owned(),
            }),
            now + std::time::Duration::from_secs(1),
        );
        assert_eq!(requests.pending_deadline(), None);

        let retry_at = deadline
            + MentionMemberSearchRequests::REQUEST_TTL
            + std::time::Duration::from_millis(1);
        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: "alice".to_owned(),
            }),
            retry_at,
        );
        assert!(requests.pending_deadline().is_some());

        let long_query = "A".repeat(MentionMemberSearchRequests::MAX_QUERY_CHARS + 10);
        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: long_query,
            }),
            retry_at + std::time::Duration::from_millis(1),
        );
        let deadline = requests
            .pending_deadline()
            .expect("long query should still search by capped prefix");
        let target = requests
            .next_due(deadline)
            .expect("capped query should be due");
        assert_eq!(
            target.query.chars().count(),
            MentionMemberSearchRequests::MAX_QUERY_CHARS
        );
        assert!(target.query.chars().all(|ch| ch == 'a'));

        let expanding_query = "İ".repeat(MentionMemberSearchRequests::MAX_QUERY_CHARS + 10);
        requests.set_target(
            Some(MentionMemberSearchTarget {
                guild_id,
                query: expanding_query,
            }),
            retry_at + std::time::Duration::from_millis(2),
        );
        let deadline = requests
            .pending_deadline()
            .expect("expanding query should still search by capped prefix");
        let target = requests
            .next_due(deadline)
            .expect("expanded lowercase query should be due");
        assert_eq!(
            target.query.chars().count(),
            MentionMemberSearchRequests::MAX_QUERY_CHARS
        );
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

    #[test]
    fn older_history_request_dedupes_and_tracks_exhausted_cursor() {
        let mut requests = RequestLifecycle::default();
        let channel_id = Id::new(10);
        let before = Id::new(30);

        assert!(requests.begin_older_history_request(channel_id, before));
        assert!(!requests.begin_older_history_request(channel_id, before));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Newer { after: Id::new(40) },
                message: "unrelated newer failure".to_owned(),
            },
        ));
        assert!(!requests.begin_older_history_request(channel_id, before));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Older {
                    before: Id::new(31),
                },
                message: "stale older failure".to_owned(),
            },
        ));
        assert!(!requests.begin_older_history_request(channel_id, before));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Older { before },
                message: "temporary failure".to_owned(),
            },
        ));
        assert!(requests.begin_older_history_request(channel_id, before));

        requests.record_event(&message_history_loaded_event(MessageHistoryLoadedFixture {
            channel_id,
            before: Some(before),
            ..MessageHistoryLoadedFixture::new()
        }));
        assert!(!requests.begin_older_history_request(channel_id, before));
        assert!(requests.begin_older_history_request(channel_id, Id::new(20)));
    }

    #[test]
    fn newer_history_request_dedupes_and_tracks_exhausted_cursor() {
        let mut requests = RequestLifecycle::default();
        let channel_id = Id::new(10);
        let after = Id::new(30);

        assert!(requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));
        assert!(!requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Older {
                    before: Id::new(20),
                },
                message: "unrelated older failure".to_owned(),
            },
        ));
        assert!(!requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Newer { after: Id::new(31) },
                message: "stale newer failure".to_owned(),
            },
        ));
        assert!(!requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));

        requests.record_event(&message_history_load_failed_event(
            MessageHistoryLoadFailedFixture {
                channel_id,
                target: MessageHistoryLoadTarget::Newer { after },
                message: "temporary failure".to_owned(),
            },
        ));
        assert!(requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));

        requests.record_event(&message_history_after_loaded_event(
            MessageHistoryAfterLoadedFixture {
                channel_id,
                after,
                mode: MessageHistoryAfterMode::GapFill,
                ..MessageHistoryAfterLoadedFixture::new()
            },
        ));
        assert!(!requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::GapFill
        ));
        assert!(requests.begin_history_after_request(
            channel_id,
            Id::new(31),
            MessageHistoryAfterMode::GapFill
        ));
    }

    #[test]
    fn catch_up_history_request_dedupes_without_exhausting_empty_cursor() {
        let mut requests = RequestLifecycle::default();
        let channel_id = Id::new(10);
        let after = Id::new(30);

        assert!(requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::CatchUp
        ));
        assert!(!requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::CatchUp
        ));

        requests.record_event(&message_history_after_loaded_event(
            MessageHistoryAfterLoadedFixture {
                channel_id,
                after,
                mode: MessageHistoryAfterMode::CatchUp,
                ..MessageHistoryAfterLoadedFixture::new()
            },
        ));

        assert!(requests.begin_history_after_request(
            channel_id,
            after,
            MessageHistoryAfterMode::CatchUp
        ));
    }

    #[test]
    fn read_ack_request_debounces_and_coalesces_by_channel() {
        let mut requests = RequestLifecycle::default();
        let now = std::time::Instant::now();
        let channel_id = Id::new(10);

        requests.schedule_read_ack(channel_id, Id::new(30), now);
        requests.schedule_read_ack(
            channel_id,
            Id::new(31),
            now + std::time::Duration::from_millis(1),
        );
        let deadline = requests
            .next_read_ack_deadline()
            .expect("read ack deadline should be armed");

        assert!(
            requests
                .flush_due_read_acks(deadline - std::time::Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(
            requests.flush_due_read_acks(deadline),
            vec![(channel_id, Id::new(31))]
        );
        assert_eq!(requests.next_read_ack_deadline(), None);
    }
}
