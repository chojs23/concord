use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::state::DiscordState;

use super::{
    ArchivedThreadPageCursor, ArchivedThreadsPage, ThreadGatewayInfo, ThreadListSyncInfo,
    ThreadMemberInfo,
};

const THREAD_NOTIFICATION_FLAGS_MASK: u64 = (1 << 1) | (1 << 2) | (1 << 3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveThreadState {
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::discord) struct CurrentUserThreadState {
    pub(in crate::discord) guild_id: Option<Id<GuildMarker>>,
    pub(in crate::discord) join_timestamp: Option<String>,
    pub(in crate::discord) flags: Option<u64>,
    pub(in crate::discord) muted: bool,
    pub(in crate::discord) mute_end_time: Option<String>,
    pub(in crate::discord) selected_time_window: Option<i64>,
    pub(in crate::discord) extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ThreadParticipantState {
    guild_id: Id<GuildMarker>,
    user_ids: BTreeSet<Id<UserMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ArchivedThreadListStatus {
    Loaded {
        has_more: bool,
        next_before: Option<String>,
    },
    Failed {
        cursor: ArchivedThreadPageCursor,
    },
}

#[derive(Clone, Debug, Default)]
struct ArchivedThreadListState {
    guild_id: Option<Id<GuildMarker>>,
    thread_ids: Vec<Id<ChannelMarker>>,
    gateway_archived_ids: Vec<Id<ChannelMarker>>,
    status: Option<ArchivedThreadListStatus>,
    extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct ThreadCache {
    active: BTreeMap<Id<ChannelMarker>, ActiveThreadState>,
    archived: BTreeMap<Id<ChannelMarker>, ArchivedThreadListState>,
    joined: BTreeMap<Id<ChannelMarker>, CurrentUserThreadState>,
    participants: BTreeMap<Id<ChannelMarker>, ThreadParticipantState>,
    forum_post_data_loaded: BTreeSet<Id<ChannelMarker>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadCreatorState {
    pub guild_id: Option<Id<GuildMarker>>,
    pub user_id: Id<UserMarker>,
}

impl ThreadCache {
    pub(in crate::discord) fn replace_active_scope(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_ids: Option<&BTreeSet<Id<ChannelMarker>>>,
        threads: impl IntoIterator<Item = (Id<ChannelMarker>, Id<ChannelMarker>)>,
    ) {
        self.active.retain(|_, active| {
            active.guild_id != guild_id
                || parent_ids.is_some_and(|parents| !parents.contains(&active.parent_id))
        });
        for (thread_id, parent_id) in threads {
            self.active.insert(
                thread_id,
                ActiveThreadState {
                    guild_id,
                    parent_id,
                },
            );
        }
    }

    pub(in crate::discord) fn set_active(
        &mut self,
        thread_id: Id<ChannelMarker>,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        active: bool,
    ) {
        if active {
            self.active.insert(
                thread_id,
                ActiveThreadState {
                    guild_id,
                    parent_id,
                },
            );
        } else {
            self.active.remove(&thread_id);
        }
    }

    pub(in crate::discord) fn active_for_parent(
        &self,
        parent_id: Id<ChannelMarker>,
    ) -> Vec<Id<ChannelMarker>> {
        self.active
            .iter()
            .filter_map(|(thread_id, active)| (active.parent_id == parent_id).then_some(*thread_id))
            .collect()
    }

    pub(in crate::discord) fn is_active(&self, thread_id: Id<ChannelMarker>) -> bool {
        self.active.contains_key(&thread_id)
    }

    pub(in crate::discord) fn archived_for_parent(
        &self,
        parent_id: Id<ChannelMarker>,
    ) -> Vec<Id<ChannelMarker>> {
        self.archived
            .get(&parent_id)
            .map(|state| state.thread_ids.clone())
            .unwrap_or_default()
    }

    pub(in crate::discord) fn archived_has_response(&self, parent_id: Id<ChannelMarker>) -> bool {
        self.archived
            .get(&parent_id)
            .is_some_and(|state| state.status.is_some())
    }

    pub(in crate::discord) fn next_archived_page_cursor(
        &self,
        parent_id: Id<ChannelMarker>,
        should_load_more: bool,
    ) -> Option<ArchivedThreadPageCursor> {
        let Some(state) = self.archived.get(&parent_id) else {
            return Some(ArchivedThreadPageCursor::Initial);
        };
        match state.status.as_ref() {
            None => Some(ArchivedThreadPageCursor::Initial),
            Some(ArchivedThreadListStatus::Failed { cursor }) => Some(cursor.clone()),
            Some(ArchivedThreadListStatus::Loaded {
                has_more: true,
                next_before: Some(before),
            }) if should_load_more => Some(ArchivedThreadPageCursor::Before(before.clone())),
            Some(ArchivedThreadListStatus::Loaded { .. }) => None,
        }
    }

    pub(in crate::discord) fn record_archived_page(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        cursor: ArchivedThreadPageCursor,
        thread_ids: impl IntoIterator<Item = Id<ChannelMarker>>,
        page: &ArchivedThreadsPage,
    ) {
        let state = self.archived.entry(parent_id).or_default();
        state.guild_id = Some(guild_id);

        // The route's first page is an authoritative refresh of the archived
        // order. Keep any thread archived by a newer Gateway event at the
        // front until Discord's REST index includes it.
        let thread_ids = thread_ids.into_iter().collect::<Vec<_>>();
        let locally_archived = matches!(&cursor, ArchivedThreadPageCursor::Initial)
            .then(|| state.gateway_archived_ids.clone())
            .unwrap_or_default();
        if matches!(&cursor, ArchivedThreadPageCursor::Initial) {
            state.thread_ids.clear();
        }

        for thread_id in locally_archived
            .into_iter()
            .chain(thread_ids.iter().copied())
        {
            if !state.thread_ids.contains(&thread_id) {
                state.thread_ids.push(thread_id);
            }
        }
        for thread_id in thread_ids {
            state
                .gateway_archived_ids
                .retain(|candidate| *candidate != thread_id);
        }
        state.status = Some(ArchivedThreadListStatus::Loaded {
            has_more: page.has_more,
            next_before: page.next_before.clone(),
        });
        state.extra_fields = page.extra_fields.clone();
    }

    pub(in crate::discord) fn mark_archived_page_failed(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        cursor: ArchivedThreadPageCursor,
    ) {
        let state = self.archived.entry(parent_id).or_default();
        state.guild_id = Some(guild_id);
        state.status = Some(ArchivedThreadListStatus::Failed { cursor });
    }

    pub(in crate::discord) fn insert_gateway_archived(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        thread_id: Id<ChannelMarker>,
    ) {
        let state = self.archived.entry(parent_id).or_default();
        state.guild_id = Some(guild_id);
        state
            .gateway_archived_ids
            .retain(|candidate| *candidate != thread_id);
        state.gateway_archived_ids.insert(0, thread_id);
        state.thread_ids.retain(|id| *id != thread_id);
        state.thread_ids.insert(0, thread_id);
    }

    pub(in crate::discord) fn remove_archived_thread(&mut self, thread_id: Id<ChannelMarker>) {
        for state in self.archived.values_mut() {
            state.thread_ids.retain(|id| *id != thread_id);
            state
                .gateway_archived_ids
                .retain(|candidate| *candidate != thread_id);
        }
    }

    pub(in crate::discord) fn remove_archived_parent(&mut self, parent_id: Id<ChannelMarker>) {
        self.archived.remove(&parent_id);
    }

    pub(in crate::discord) fn upsert_current_user_member(
        &mut self,
        thread_id: Id<ChannelMarker>,
        guild_id: Option<Id<GuildMarker>>,
        member: &ThreadMemberInfo,
    ) {
        let existing = self.joined.get(&thread_id);
        let state = CurrentUserThreadState {
            guild_id: guild_id.or_else(|| existing.and_then(|state| state.guild_id)),
            join_timestamp: member
                .join_timestamp
                .clone()
                .or_else(|| existing.and_then(|state| state.join_timestamp.clone())),
            flags: member
                .flags
                .or_else(|| existing.and_then(|state| state.flags)),
            muted: member
                .muted
                .or_else(|| existing.map(|state| state.muted))
                .unwrap_or(false),
            mute_end_time: if member.muted == Some(false) {
                None
            } else {
                member
                    .mute_end_time
                    .clone()
                    .or_else(|| existing.and_then(|state| state.mute_end_time.clone()))
            },
            selected_time_window: if member.muted == Some(false) {
                None
            } else {
                member
                    .selected_time_window
                    .or_else(|| existing.and_then(|state| state.selected_time_window))
            },
            extra_fields: existing
                .map(|state| state.extra_fields.clone())
                .unwrap_or_default()
                .into_iter()
                .chain(member.extra_fields.clone())
                .collect(),
        };
        self.joined.insert(thread_id, state);
    }

    pub(in crate::discord) fn current_user_member(
        &self,
        thread_id: Id<ChannelMarker>,
    ) -> Option<&CurrentUserThreadState> {
        self.joined.get(&thread_id)
    }

    pub(in crate::discord) fn current_user_member_mut(
        &mut self,
        thread_id: Id<ChannelMarker>,
    ) -> Option<&mut CurrentUserThreadState> {
        self.joined.get_mut(&thread_id)
    }

    pub(in crate::discord) fn remove_current_user_member(&mut self, thread_id: Id<ChannelMarker>) {
        self.joined.remove(&thread_id);
    }

    pub(in crate::discord) fn replace_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        thread_id: Id<ChannelMarker>,
        user_ids: impl IntoIterator<Item = Id<UserMarker>>,
    ) {
        self.participants.insert(
            thread_id,
            ThreadParticipantState {
                guild_id,
                user_ids: user_ids.into_iter().collect(),
            },
        );
    }

    pub(in crate::discord) fn update_loaded_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        thread_id: Id<ChannelMarker>,
        added_user_ids: impl IntoIterator<Item = Id<UserMarker>>,
        removed_user_ids: &[Id<UserMarker>],
    ) {
        let Some(participants) = self.participants.get_mut(&thread_id) else {
            return;
        };
        if participants.guild_id != guild_id {
            return;
        }
        participants.user_ids.extend(added_user_ids);
        for user_id in removed_user_ids {
            participants.user_ids.remove(user_id);
        }
    }

    pub(in crate::discord) fn participant_ids(
        &self,
        guild_id: Id<GuildMarker>,
        thread_id: Id<ChannelMarker>,
    ) -> Option<&BTreeSet<Id<UserMarker>>> {
        self.participants
            .get(&thread_id)
            .filter(|participants| participants.guild_id == guild_id)
            .map(|participants| &participants.user_ids)
    }

    pub(in crate::discord) fn participant_ids_by_guild(
        &self,
    ) -> impl Iterator<Item = (Id<GuildMarker>, &BTreeSet<Id<UserMarker>>)> {
        self.participants
            .values()
            .map(|participants| (participants.guild_id, &participants.user_ids))
    }

    pub(in crate::discord) fn remove_user_from_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) {
        for participants in self
            .participants
            .values_mut()
            .filter(|participants| participants.guild_id == guild_id)
        {
            participants.user_ids.remove(&user_id);
        }
    }

    pub(in crate::discord) fn clear_participants(&mut self) {
        self.participants.clear();
    }

    pub(in crate::discord) fn retain_participant_guilds(
        &mut self,
        keep: &BTreeSet<Id<GuildMarker>>,
    ) {
        self.participants
            .retain(|_, participants| keep.contains(&participants.guild_id));
    }

    pub(in crate::discord) fn mark_post_data_loaded(
        &mut self,
        thread_ids: impl IntoIterator<Item = Id<ChannelMarker>>,
    ) {
        self.forum_post_data_loaded.extend(thread_ids);
    }

    pub(in crate::discord) fn post_data_loaded(&self, thread_id: Id<ChannelMarker>) -> bool {
        self.forum_post_data_loaded.contains(&thread_id)
    }

    pub(in crate::discord) fn remove_thread(&mut self, thread_id: Id<ChannelMarker>) {
        self.active.remove(&thread_id);
        self.remove_archived_parent(thread_id);
        self.remove_archived_thread(thread_id);
        self.joined.remove(&thread_id);
        self.participants.remove(&thread_id);
        self.forum_post_data_loaded.remove(&thread_id);
    }

    pub(in crate::discord) fn remove_guild(&mut self, guild_id: Id<GuildMarker>) {
        let thread_ids = self
            .active
            .iter()
            .filter_map(|(thread_id, active)| (active.guild_id == guild_id).then_some(*thread_id))
            .chain(
                self.participants
                    .iter()
                    .filter_map(|(thread_id, participants)| {
                        (participants.guild_id == guild_id).then_some(*thread_id)
                    }),
            )
            .chain(self.joined.iter().filter_map(|(thread_id, member)| {
                (member.guild_id == Some(guild_id)).then_some(*thread_id)
            }))
            .chain(
                self.archived
                    .values()
                    .filter(|state| state.guild_id == Some(guild_id))
                    .flat_map(|state| state.thread_ids.iter().copied()),
            )
            .collect::<BTreeSet<_>>();
        self.archived
            .retain(|_, state| state.guild_id != Some(guild_id));
        for thread_id in thread_ids {
            self.remove_thread(thread_id);
        }
    }
}

impl DiscordState {
    pub fn active_thread_ids_for_parent(
        &self,
        parent_id: Id<ChannelMarker>,
    ) -> Vec<Id<ChannelMarker>> {
        self.threads.active_for_parent(parent_id)
    }

    pub fn archived_thread_ids_for_parent(
        &self,
        parent_id: Id<ChannelMarker>,
    ) -> Vec<Id<ChannelMarker>> {
        self.threads.archived_for_parent(parent_id)
    }

    pub fn archived_threads_have_response(&self, parent_id: Id<ChannelMarker>) -> bool {
        self.threads.archived_has_response(parent_id)
    }

    pub(crate) fn next_archived_thread_page_cursor(
        &self,
        parent_id: Id<ChannelMarker>,
        should_load_more: bool,
    ) -> Option<ArchivedThreadPageCursor> {
        self.threads
            .next_archived_page_cursor(parent_id, should_load_more)
    }

    pub fn thread_is_joined(&self, thread_id: Id<ChannelMarker>) -> bool {
        self.threads.current_user_member(thread_id).is_some()
    }

    pub fn thread_notification_level_flags(&self, thread_id: Id<ChannelMarker>) -> Option<u64> {
        self.threads
            .current_user_member(thread_id)
            .and_then(|member| member.flags)
            .map(|flags| flags & THREAD_NOTIFICATION_FLAGS_MASK)
            .filter(|flags| *flags != 0)
    }

    pub fn thread_is_muted(&self, thread_id: Id<ChannelMarker>) -> bool {
        self.threads
            .current_user_member(thread_id)
            .is_some_and(|member| member.muted)
    }

    pub fn thread_mute_end_time(&self, thread_id: Id<ChannelMarker>) -> Option<&str> {
        self.threads
            .current_user_member(thread_id)
            .and_then(|member| member.mute_end_time.as_deref())
    }

    pub fn thread_mute_selected_time_window(&self, thread_id: Id<ChannelMarker>) -> Option<i64> {
        self.threads
            .current_user_member(thread_id)
            .and_then(|member| member.selected_time_window)
    }

    pub fn thread_post_data_loaded(&self, thread_id: Id<ChannelMarker>) -> bool {
        self.threads.post_data_loaded(thread_id)
    }

    pub fn thread_creator(&self, thread_id: Id<ChannelMarker>) -> Option<ThreadCreatorState> {
        let thread = self.navigation.channels.get(&thread_id)?;
        Some(ThreadCreatorState {
            guild_id: thread.guild_id,
            user_id: thread.owner_id?,
        })
    }

    pub(in crate::discord) fn apply_thread_gateway_upsert(
        &mut self,
        thread: &ThreadGatewayInfo,
        created: bool,
    ) {
        let parent_to_ack = if created && thread.channel.owner_id == self.session.current_user_id {
            thread.channel.parent_id.filter(|parent_id| {
                self.navigation
                    .channels
                    .get(parent_id)
                    .is_some_and(|parent| parent.is_forum())
            })
        } else {
            None
        };

        self.upsert_channel(&thread.channel);
        self.refresh_active_thread(thread.channel.channel_id);
        if let Some(member) = &thread.current_user_member {
            let guild_id = self.channel_guild_id(thread.channel.channel_id);
            self.threads_mut().upsert_current_user_member(
                thread.channel.channel_id,
                guild_id,
                member,
            );
        }
        if let Some(parent_id) = parent_to_ack {
            self.mark_message_read_locally(
                parent_id,
                Id::<MessageMarker>::new(thread.channel.channel_id.get()),
            );
        }
    }

    pub(in crate::discord) fn apply_archived_threads_page(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        cursor: ArchivedThreadPageCursor,
        page: &ArchivedThreadsPage,
    ) {
        let archived_threads = page
            .threads
            .iter()
            .filter(|thread| {
                thread.guild_id == Some(guild_id)
                    && thread.parent_id == Some(parent_id)
                    && thread.thread_archived() == Some(true)
                    && !self.threads.is_active(thread.channel_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let archived_thread_ids = archived_threads
            .iter()
            .map(|thread| thread.channel_id)
            .collect::<Vec<_>>();
        for thread in &archived_threads {
            // Archived REST data is useful channel metadata, but it must not
            // pass through the Gateway active-thread index.
            self.upsert_channel(thread);
        }

        self.apply_embedded_thread_members(guild_id, &page.members);
        for member in &page.members {
            let Some(thread_id) = member.thread_id else {
                continue;
            };
            self.upsert_current_user_thread_member(thread_id, Some(guild_id), member);
        }

        self.threads_mut().record_archived_page(
            guild_id,
            parent_id,
            cursor,
            archived_thread_ids,
            page,
        );
    }

    pub(in crate::discord) fn mark_archived_threads_page_failed(
        &mut self,
        guild_id: Id<GuildMarker>,
        parent_id: Id<ChannelMarker>,
        cursor: ArchivedThreadPageCursor,
    ) {
        self.threads_mut()
            .mark_archived_page_failed(guild_id, parent_id, cursor);
    }

    pub(in crate::discord) fn apply_thread_list_sync(&mut self, sync: &ThreadListSyncInfo) {
        for thread in &sync.threads {
            self.upsert_channel(thread);
        }

        let synced_thread_ids = sync
            .threads
            .iter()
            .map(|thread| thread.channel_id)
            .collect::<BTreeSet<_>>();
        if let Some(current_user_members) = &sync.current_user_members {
            let explicitly_joined_ids = current_user_members
                .iter()
                .filter_map(|member| member.thread_id)
                .filter(|thread_id| synced_thread_ids.contains(thread_id))
                .collect::<BTreeSet<_>>();

            // A present `THREAD_LIST_SYNC.members` array is authoritative for
            // the synced threads. User-account payloads can omit the field
            // entirely. Omission is a partial update and must not clear the
            // membership learned from the guild snapshot.
            for thread_id in synced_thread_ids
                .difference(&explicitly_joined_ids)
                .copied()
            {
                self.threads_mut().remove_current_user_member(thread_id);
            }
        }

        let parent_ids = sync
            .channel_ids
            .as_ref()
            .map(|ids| ids.iter().copied().collect::<BTreeSet<_>>());
        let active_threads = sync
            .threads
            .iter()
            .filter_map(|thread| {
                let cached = self.navigation.channels.get(&thread.channel_id)?;
                cached
                    .is_active_thread()
                    .then_some((cached.id, cached.parent_id?))
            })
            .collect::<Vec<_>>();
        self.threads_mut()
            .replace_active_scope(sync.guild_id, parent_ids.as_ref(), active_threads);

        if let Some(current_user_members) = &sync.current_user_members {
            for member in current_user_members {
                let Some(thread_id) = member.thread_id else {
                    continue;
                };
                if !synced_thread_ids.contains(&thread_id) {
                    continue;
                }
                self.threads_mut().upsert_current_user_member(
                    thread_id,
                    Some(sync.guild_id),
                    member,
                );
            }
        }
    }

    pub(in crate::discord) fn upsert_current_user_thread_member(
        &mut self,
        thread_id: Id<ChannelMarker>,
        guild_id: Option<Id<GuildMarker>>,
        member: &ThreadMemberInfo,
    ) {
        let guild_id = guild_id.or_else(|| self.channel_guild_id(thread_id));
        self.threads_mut()
            .upsert_current_user_member(thread_id, guild_id, member);
    }

    pub(in crate::discord) fn remove_current_user_thread_member(
        &mut self,
        thread_id: Id<ChannelMarker>,
    ) {
        self.threads_mut().remove_current_user_member(thread_id);
    }

    pub(in crate::discord) fn set_thread_notification_level(
        &mut self,
        thread_id: Id<ChannelMarker>,
        flags: u64,
    ) {
        let Some(member) = self.threads_mut().current_user_member_mut(thread_id) else {
            return;
        };
        let current = member.flags.unwrap_or(0);
        member.flags = Some(
            (current & !THREAD_NOTIFICATION_FLAGS_MASK) | (flags & THREAD_NOTIFICATION_FLAGS_MASK),
        );
    }

    pub(in crate::discord) fn set_thread_mute(
        &mut self,
        thread_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<String>,
        selected_time_window: Option<i64>,
    ) {
        let Some(member) = self.threads_mut().current_user_member_mut(thread_id) else {
            return;
        };
        member.muted = muted;
        member.mute_end_time = if muted { mute_end_time } else { None };
        member.selected_time_window = if muted { selected_time_window } else { None };
    }

    pub(in crate::discord) fn replace_thread_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        thread_id: Id<ChannelMarker>,
        user_ids: impl IntoIterator<Item = Id<UserMarker>>,
    ) {
        self.threads_mut()
            .replace_participants(guild_id, thread_id, user_ids);
    }

    pub(in crate::discord) fn update_loaded_thread_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        thread_id: Id<ChannelMarker>,
        added_user_ids: impl IntoIterator<Item = Id<UserMarker>>,
        removed_user_ids: &[Id<UserMarker>],
    ) {
        self.threads_mut().update_loaded_participants(
            guild_id,
            thread_id,
            added_user_ids,
            removed_user_ids,
        );
    }

    pub(in crate::discord) fn remove_member_from_thread_participants(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) {
        self.threads_mut()
            .remove_user_from_participants(guild_id, user_id);
    }

    pub(in crate::discord) fn clear_thread_participants(&mut self) {
        self.threads_mut().clear_participants();
    }

    pub(in crate::discord) fn retain_thread_participant_guilds(
        &mut self,
        keep: &BTreeSet<Id<GuildMarker>>,
    ) {
        self.threads_mut().retain_participant_guilds(keep);
    }

    pub(in crate::discord) fn remove_thread_state(&mut self, thread_id: Id<ChannelMarker>) {
        self.threads_mut().remove_thread(thread_id);
    }

    pub(in crate::discord) fn reset_threads_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.threads_mut().remove_guild(guild_id);
    }

    pub(in crate::discord) fn mark_forum_post_data_loaded(
        &mut self,
        thread_ids: impl IntoIterator<Item = Id<ChannelMarker>>,
    ) {
        self.threads_mut().mark_post_data_loaded(thread_ids);
    }

    fn refresh_active_thread(&mut self, thread_id: Id<ChannelMarker>) {
        let Some(thread) = self.navigation.channels.get(&thread_id) else {
            self.threads_mut().remove_thread(thread_id);
            return;
        };
        let Some(guild_id) = thread.guild_id else {
            self.threads_mut().remove_thread(thread_id);
            return;
        };
        let Some(parent_id) = thread.parent_id else {
            self.threads_mut().remove_thread(thread_id);
            return;
        };
        let active = thread.is_active_thread();
        let archived = thread.thread_archived() == Some(true);
        let threads = self.threads_mut();
        threads.set_active(thread_id, guild_id, parent_id, active);
        if active {
            threads.remove_archived_thread(thread_id);
        } else if archived {
            threads.insert_gateway_archived(guild_id, parent_id, thread_id);
        } else {
            threads.remove_archived_thread(thread_id);
        }
    }
}
