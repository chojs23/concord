use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

mod primitives;

use primitives::{CursorRequests, OnDemandRequests, TimedRequestSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::{
    AppEvent, ArchivedThreadPageCursor, MessageHistoryAfterMode, MessageHistoryLoadTarget,
    member::normalize_member_search_query,
};

const APPLICATION_COMMAND_REQUEST_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_APPLICATION_COMMAND_REQUESTS: usize = 1_024;
const FORUM_POST_DATA_BATCH_LIMIT: usize = 5;

#[derive(Debug, Default)]
pub(super) struct HistoryRequests {
    requests: OnDemandRequests<Id<ChannelMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct ForumPostDataRequests {
    in_flight: HashSet<(Id<ChannelMarker>, Id<ChannelMarker>)>,
}

#[derive(Debug, Default)]
pub(super) struct ArchivedThreadRequests {
    last_channel: Option<Id<ChannelMarker>>,
    in_flight: HashSet<(Id<ChannelMarker>, ArchivedThreadPageCursor)>,
    completed: HashSet<(Id<ChannelMarker>, ArchivedThreadPageCursor)>,
    failed: HashSet<(Id<ChannelMarker>, ArchivedThreadPageCursor)>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ForumPostDataRequestTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) thread_ids: Vec<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArchivedThreadRequestTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) cursor: ArchivedThreadPageCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GuildMemberSearchTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) query: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GuildMemberSearchSurface {
    Autocomplete,
    Popup,
}

impl GuildMemberSearchSurface {
    const COUNT: usize = 2;

    const fn index(self) -> usize {
        self as usize
    }

    const fn min_query_chars(self) -> usize {
        match self {
            Self::Autocomplete => 2,
            Self::Popup => 1,
        }
    }

    pub(crate) const fn result_limit(self) -> u16 {
        match self {
            Self::Autocomplete => 10,
            Self::Popup => 100,
        }
    }
}

/// Batch member fetches deduped per (guild, user) with a TTL so lost
/// responses eventually retry. Every feature shares this coordinator so a
/// voice, typing, message, or permission demand cannot issue duplicate work.
#[derive(Debug)]
pub(super) struct MemberBatchRequests {
    requested: TimedRequestSet<(Id<GuildMarker>, Id<UserMarker>)>,
}

type OrderedUserIds = (BTreeSet<Id<UserMarker>>, Vec<Id<UserMarker>>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MemberListSubscriptionTarget {
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) thread_id: Option<Id<ChannelMarker>>,
    pub(crate) bucket: u32,
    pub(crate) refresh_generation: u64,
    pub(crate) ranges: Vec<(u32, u32)>,
}

#[derive(Debug, Default)]
pub(super) struct MemberListSubscriptionRequests {
    last_sent: Option<MemberListSubscriptionKey>,
    pending: Option<PendingMemberListSubscription>,
}

#[derive(Debug, Default)]
pub(super) struct GuildMemberSearchRequests {
    active_key: Option<GuildMemberSearchKey>,
    pending: Option<PendingGuildMemberSearch>,
}

#[derive(Debug, Default)]
pub(super) struct UserProfileRequests {
    in_flight: HashSet<UserProfileRequestKey>,
}

#[derive(Debug, Default)]
pub(super) struct UserNoteRequests {
    in_flight: HashSet<Id<UserMarker>>,
}

#[derive(Debug)]
struct ApplicationCommandRequests {
    pending: TimedRequestSet<String>,
}

impl Default for ApplicationCommandRequests {
    fn default() -> Self {
        Self {
            pending: TimedRequestSet::new(
                APPLICATION_COMMAND_REQUEST_TTL,
                MAX_APPLICATION_COMMAND_REQUESTS,
            ),
        }
    }
}

impl ApplicationCommandRequests {
    fn begin(&mut self, nonce: String, now: Instant) {
        self.pending.insert(nonce, now);
    }

    fn clear(&mut self, nonce: &str) {
        self.pending.remove(&nonce.to_owned());
    }

    fn correlate(&mut self, nonce: &str, now: Instant) -> bool {
        self.pending.prune(now);
        let nonce = nonce.to_owned();
        let correlated = self.pending.contains(&nonce);
        if correlated {
            self.pending.remove(&nonce);
        }
        correlated
    }
}

#[derive(Debug, Default)]
pub(crate) struct RequestLifecycle {
    history: HistoryRequests,
    forum_post_data: ForumPostDataRequests,
    archived_threads: ArchivedThreadRequests,
    pinned_messages: PinnedMessageRequests,
    older_history: OlderHistoryRequests,
    newer_history: NewerHistoryRequests,
    read_acks: ReadAckRequests,
    member_hydration: MemberBatchRequests,
    member_list_subscriptions: MemberListSubscriptionRequests,
    member_searches: [GuildMemberSearchRequests; GuildMemberSearchSurface::COUNT],
    thread_previews: ThreadPreviewRequests,
    user_profiles: UserProfileRequests,
    user_notes: UserNoteRequests,
    pending_application_commands: ApplicationCommandRequests,
}

impl RequestLifecycle {
    pub(crate) fn record_event(&mut self, event: &AppEvent) {
        if matches!(event, AppEvent::GatewayReidentified) {
            self.reset_gateway_session();
        }
        self.history.record_event(event);
        self.older_history.record_event(event);
        self.newer_history.record_event(event);
        self.forum_post_data.record_event(event);
        self.archived_threads.record_event(event);
        self.pinned_messages.record_event(event);
        self.member_hydration.record_event(event);
        self.thread_previews.record_event(event);
        self.user_profiles.record_event(event);
        self.user_notes.record_event(event);
    }

    fn reset_gateway_session(&mut self) {
        self.member_hydration = MemberBatchRequests::default();
        self.forum_post_data = ForumPostDataRequests::default();
        self.archived_threads = ArchivedThreadRequests::default();
        self.member_list_subscriptions = MemberListSubscriptionRequests::default();
        self.member_searches = Default::default();
        self.pending_application_commands = ApplicationCommandRequests::default();
    }

    pub(crate) fn begin_application_command(&mut self, nonce: String, now: Instant) {
        self.pending_application_commands.begin(nonce, now);
    }

    pub(crate) fn clear_application_command(&mut self, nonce: &str) {
        self.pending_application_commands.clear(nonce);
    }

    pub(crate) fn correlate_interaction_event(&mut self, event: &mut AppEvent) {
        self.correlate_interaction_event_at(event, Instant::now());
    }

    fn correlate_interaction_event_at(&mut self, event: &mut AppEvent, now: Instant) {
        let (nonce, correlated) = match event {
            AppEvent::InteractionSucceeded {
                nonce, correlated, ..
            }
            | AppEvent::InteractionFailed {
                nonce, correlated, ..
            } => (nonce, correlated),
            _ => return,
        };
        *correlated = nonce
            .as_deref()
            .is_some_and(|nonce| self.pending_application_commands.correlate(nonce, now));
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

    pub(crate) fn next_forum_post_data_request(
        &mut self,
        target: Option<ForumPostDataRequestTarget>,
    ) -> Option<ForumPostDataRequestTarget> {
        self.forum_post_data.next(target)
    }

    pub(crate) fn mark_forum_post_data_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
        thread_ids: &[Id<ChannelMarker>],
    ) {
        self.forum_post_data.release(channel_id, thread_ids);
    }

    pub(crate) fn next_archived_thread_request(
        &mut self,
        target: Option<ArchivedThreadRequestTarget>,
    ) -> Option<ArchivedThreadRequestTarget> {
        self.archived_threads.next(target)
    }

    pub(crate) fn mark_archived_thread_request_send_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
        cursor: &ArchivedThreadPageCursor,
    ) {
        self.archived_threads.release(channel_id, cursor);
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

    pub(crate) fn next_member_hydration_requests(
        &mut self,
        missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.member_hydration.next(missing, now)
    }

    pub(crate) fn set_guild_member_search_target(
        &mut self,
        surface: GuildMemberSearchSurface,
        target: Option<GuildMemberSearchTarget>,
        now: Instant,
    ) {
        self.member_searches[surface.index()].set_target(target, surface.min_query_chars(), now);
    }

    pub(crate) fn guild_member_search_deadline(
        &self,
        surface: GuildMemberSearchSurface,
    ) -> Option<Instant> {
        self.member_searches[surface.index()].pending_deadline()
    }

    pub(crate) fn next_due_guild_member_search(
        &mut self,
        surface: GuildMemberSearchSurface,
        now: Instant,
    ) -> Option<GuildMemberSearchTarget> {
        self.member_searches[surface.index()].next_due(now)
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

impl ForumPostDataRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ForumPostDataLoaded {
                channel_id,
                requested_thread_ids,
                ..
            } => self.release(*channel_id, requested_thread_ids),
            AppEvent::ForumPostDataLoadFailed {
                channel_id,
                thread_ids,
                ..
            } => self.release(*channel_id, thread_ids),
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        target: Option<ForumPostDataRequestTarget>,
    ) -> Option<ForumPostDataRequestTarget> {
        let ForumPostDataRequestTarget {
            guild_id,
            channel_id,
            thread_ids,
        } = target?;
        let mut batch = Vec::with_capacity(FORUM_POST_DATA_BATCH_LIMIT);
        for thread_id in thread_ids {
            if batch.len() == FORUM_POST_DATA_BATCH_LIMIT {
                break;
            }
            if self.in_flight.insert((channel_id, thread_id)) {
                batch.push(thread_id);
            }
        }
        (!batch.is_empty()).then_some(ForumPostDataRequestTarget {
            guild_id,
            channel_id,
            thread_ids: batch,
        })
    }

    pub(super) fn release(
        &mut self,
        channel_id: Id<ChannelMarker>,
        thread_ids: &[Id<ChannelMarker>],
    ) {
        for thread_id in thread_ids {
            self.in_flight.remove(&(channel_id, *thread_id));
        }
    }
}

impl ArchivedThreadRequests {
    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::ArchivedThreadsLoaded {
                channel_id, before, ..
            } => {
                let key = (
                    *channel_id,
                    ArchivedThreadPageCursor::from_before(before.clone()),
                );
                self.in_flight.remove(&key);
                self.failed.remove(&key);
                self.completed.insert(key);
            }
            AppEvent::ArchivedThreadsLoadFailed {
                channel_id, before, ..
            } => {
                let key = (
                    *channel_id,
                    ArchivedThreadPageCursor::from_before(before.clone()),
                );
                self.in_flight.remove(&key);
                self.failed.insert(key);
            }
            _ => {}
        }
    }

    pub(super) fn next(
        &mut self,
        target: Option<ArchivedThreadRequestTarget>,
    ) -> Option<ArchivedThreadRequestTarget> {
        let Some(target) = target else {
            self.last_channel = None;
            return None;
        };
        let channel_changed =
            self.last_channel.replace(target.channel_id) != Some(target.channel_id);
        if channel_changed {
            self.failed
                .retain(|(channel_id, _)| *channel_id != target.channel_id);
        }

        let key = (target.channel_id, target.cursor.clone());
        if self.completed.contains(&key)
            || self.failed.contains(&key)
            || !self.in_flight.insert(key)
        {
            return None;
        }
        Some(target)
    }

    pub(super) fn release(
        &mut self,
        channel_id: Id<ChannelMarker>,
        cursor: &ArchivedThreadPageCursor,
    ) {
        self.in_flight.remove(&(channel_id, cursor.clone()));
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
            AppEvent::GuildMembersChunk { chunk } => {
                for member in &chunk.members {
                    self.requested.remove(&(chunk.guild_id, member.user_id));
                }
            }
            AppEvent::GuildMemberListUpdate { update } => {
                for (member, _) in update
                    .ops
                    .iter()
                    .flat_map(|operation| operation.items())
                    .filter_map(|item| item.member())
                {
                    self.requested.remove(&(update.guild_id, member.user_id));
                }
            }
            AppEvent::VoiceStateUpdate { state } => {
                if let (Some(guild_id), Some(member)) = (state.guild_id, state.member.as_ref()) {
                    self.requested.remove(&(guild_id, member.user_id));
                }
            }
            AppEvent::TypingStart {
                guild_id: Some(guild_id),
                member: Some(member),
                ..
            } => {
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

        let mut by_guild: BTreeMap<Id<GuildMarker>, OrderedUserIds> = BTreeMap::new();
        for (guild_id, user_ids) in missing {
            for user_id in user_ids {
                let (seen, ordered) = by_guild.entry(guild_id).or_default();
                if seen.insert(user_id) {
                    ordered.push(user_id);
                }
            }
        }
        by_guild
            .into_iter()
            .filter_map(|(guild_id, (_, ordered_user_ids))| {
                let fresh_user_ids = ordered_user_ids
                    .into_iter()
                    .filter(|user_id| self.requested.insert((guild_id, *user_id), now))
                    .collect::<Vec<_>>();
                (!fresh_user_ids.is_empty()).then_some((guild_id, fresh_user_ids))
            })
            .collect()
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
pub(super) struct ThreadPreviewRequests {
    requested: HashSet<(Id<ChannelMarker>, Id<MessageMarker>)>,
    failed: HashSet<(Id<ChannelMarker>, Id<MessageMarker>)>,
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

impl GuildMemberSearchRequests {
    const DEBOUNCE: Duration = Duration::from_millis(250);

    pub(super) fn set_target(
        &mut self,
        target: Option<GuildMemberSearchTarget>,
        min_query_chars: usize,
        now: Instant,
    ) {
        let Some(target) =
            target.and_then(|target| normalize_guild_member_search_target(target, min_query_chars))
        else {
            self.active_key = None;
            self.pending = None;
            return;
        };
        let key = target.key();
        if self.active_key.as_ref() == Some(&key) {
            return;
        }
        self.active_key = Some(key);
        self.pending = Some(PendingGuildMemberSearch {
            target,
            ready_at: now + Self::DEBOUNCE,
        });
    }

    pub(super) fn pending_deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.ready_at)
    }

    pub(super) fn next_due(&mut self, now: Instant) -> Option<GuildMemberSearchTarget> {
        let pending = self.pending.as_ref()?;
        if pending.ready_at > now {
            return None;
        }
        self.pending.take().map(|pending| pending.target)
    }
}

type GuildMemberSearchKey = (Id<GuildMarker>, String);

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
    thread_id: Option<Id<ChannelMarker>>,
    bucket: u32,
    refresh_generation: u64,
}

#[derive(Debug)]
struct PendingGuildMemberSearch {
    target: GuildMemberSearchTarget,
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

impl GuildMemberSearchTarget {
    fn key(&self) -> GuildMemberSearchKey {
        (self.guild_id, self.query.clone())
    }
}

impl MemberListSubscriptionTarget {
    fn key(&self) -> MemberListSubscriptionKey {
        MemberListSubscriptionKey {
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            thread_id: self.thread_id,
            bucket: self.bucket,
            refresh_generation: self.refresh_generation,
        }
    }
}

fn normalize_guild_member_search_target(
    target: GuildMemberSearchTarget,
    min_query_chars: usize,
) -> Option<GuildMemberSearchTarget> {
    let query = normalize_member_search_query(&target.query, min_query_chars)?;
    Some(GuildMemberSearchTarget {
        guild_id: target.guild_id,
        query,
    })
}

#[cfg(test)]
mod tests;
