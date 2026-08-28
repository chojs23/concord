use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

/// Typing indicators stay visible for this long after the latest TYPING_START
/// from a given user. This matches Discord's documented 10-second window so the
/// label tracks what other clients show.
pub(in crate::discord) const TYPING_INDICATOR_TTL: Duration = Duration::from_secs(10);

pub use super::channel::{ChannelRecipientState, ChannelState, ChannelVisibilityStats};
pub use super::guild::GuildState;
pub use super::member::{GuildMemberListEntry, GuildMemberState, RoleState, TypingUserState};
use super::member::{GuildMemberListState, role_map, role_state};
use super::message::{MessageAuthorRoleIds, MessageUpdateFields};
pub use super::message::{MessageCapabilities, MessageState};
pub use super::notification::ChannelUnreadState;
use super::notification::{
    GuildNotificationSettingsState, MessageNotificationInput, MessageNotificationKind,
};
use super::profile::{ProfileRoleIds, UserProfileCacheKey};
use super::read::{ChannelReadState, NonChannelReadState};
use super::thread::ThreadCache;
pub use super::voice::{CurrentVoiceConnectionState, VoiceParticipantState, VoiceScope};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::{
    ActivityInfo, AppEvent, ChannelRecipientInfo, CustomEmojiInfo, FriendStatus, GuildFolder,
    MemberInfo, MessageInfo, PremiumTier, PresenceStatus, ReadStateInfo, ReadySnapshotInfo,
    RelationshipInfo, RelationshipUpdateInfo, ThreadMemberInfo, UserProfileInfo,
    channel::refresh_private_channel_name_from_recipients,
    display_name::display_name_from_parts_or_unknown,
};

/// Maximum number of recent messages kept per channel in the normal message cache.
const DEFAULT_MAX_MESSAGES_PER_CHANNEL: usize = 200;
/// Number of recently opened channels whose message bodies stay fully hydrated.
const DEFAULT_MAX_WARM_MESSAGE_CHANNELS: usize = 10;
/// Extra older-history window retained while the user scrolls above the newest messages.
pub(in crate::discord) const OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER: usize = 2;
/// Maximum cached profile payloads kept for quick profile popup reopening.
pub(in crate::discord) const MAX_USER_PROFILE_CACHE_ENTRIES: usize = 256;
/// Maximum cached user-note fetch results, including users with no note.
pub(in crate::discord) const MAX_FETCHED_NOTE_CACHE_ENTRIES: usize = 256;
/// Number of recently selected guilds whose member lists stay fully cached.
pub(in crate::discord) const MAX_RECENT_MEMBER_GUILDS: usize = 10;

pub(in crate::discord) fn is_fallback_identity(username: Option<&str>, display_name: &str) -> bool {
    username.is_none() && display_name == "unknown"
}

/// Caches sit behind `Arc` so a snapshot is refcount bumps, not a deep copy.
/// Writes go through the `*_mut` accessors, which copy an area only while a
/// live snapshot still references it.
#[derive(Clone, Debug)]
pub struct DiscordState {
    pub(in crate::discord) navigation: Arc<NavigationIndex>,
    pub(in crate::discord) message_cache: Arc<MessageCache>,
    pub(in crate::discord) guild_details: Arc<GuildDetailCache>,
    pub(in crate::discord) profiles: Arc<ProfileCache>,
    pub(in crate::discord) presence: Arc<PresenceCache>,
    pub(in crate::discord) voice: Arc<VoiceStateCache>,
    pub(in crate::discord) session: Arc<SessionState>,
    pub(in crate::discord) notifications: Arc<NotificationCache>,
    pub(in crate::discord) threads: Arc<ThreadCache>,
}

/// Durable cache facts that Discord can use to reduce a later READY payload.
///
/// This type deliberately contains no wire defaults or string conversions.
/// `DiscordState` owns the facts it has observed, while the Gateway layer owns
/// how missing values are represented in an IDENTIFY payload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::discord) struct ClientCacheState {
    pub(in crate::discord) highest_guild_message_id: Option<Id<MessageMarker>>,
    pub(in crate::discord) highest_private_message_id: Option<Id<MessageMarker>>,
    pub(in crate::discord) read_state_version: Option<i64>,
    pub(in crate::discord) user_guild_settings_version: Option<i64>,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGES_PER_CHANNEL)
    }
}

impl DiscordState {
    pub fn new(max_messages_per_channel: usize) -> Self {
        Self {
            navigation: Arc::default(),
            message_cache: Arc::new(MessageCache::new(max_messages_per_channel)),
            guild_details: Arc::default(),
            profiles: Arc::default(),
            presence: Arc::default(),
            voice: Arc::default(),
            session: Arc::default(),
            notifications: Arc::default(),
            threads: Arc::default(),
        }
    }

    pub(in crate::discord) fn navigation_mut(&mut self) -> &mut NavigationIndex {
        Arc::make_mut(&mut self.navigation)
    }

    pub(in crate::discord) fn message_cache_mut(&mut self) -> &mut MessageCache {
        Arc::make_mut(&mut self.message_cache)
    }

    pub(in crate::discord) fn guild_details_mut(&mut self) -> &mut GuildDetailCache {
        Arc::make_mut(&mut self.guild_details)
    }

    pub(in crate::discord) fn profiles_mut(&mut self) -> &mut ProfileCache {
        Arc::make_mut(&mut self.profiles)
    }

    pub(in crate::discord) fn presence_mut(&mut self) -> &mut PresenceCache {
        Arc::make_mut(&mut self.presence)
    }

    pub(in crate::discord) fn voice_mut(&mut self) -> &mut VoiceStateCache {
        Arc::make_mut(&mut self.voice)
    }

    pub(in crate::discord) fn session_mut(&mut self) -> &mut SessionState {
        Arc::make_mut(&mut self.session)
    }

    pub(in crate::discord) fn notifications_mut(&mut self) -> &mut NotificationCache {
        Arc::make_mut(&mut self.notifications)
    }

    pub(in crate::discord) fn threads_mut(&mut self) -> &mut ThreadCache {
        Arc::make_mut(&mut self.threads)
    }

    pub(in crate::discord) fn client_cache_state(&self) -> ClientCacheState {
        let mut cache = ClientCacheState {
            read_state_version: self.notifications.read_state_version,
            user_guild_settings_version: self.notifications.user_guild_settings_version,
            ..ClientCacheState::default()
        };

        // Discord defines both message cursors from channel `last_message_id`.
        // Read acknowledgements are intentionally excluded because they track
        // what the user has seen, not the latest message in the channel.
        for channel in self.navigation.channels.values() {
            let Some(last_message_id) = channel.last_message_id else {
                continue;
            };

            if channel.guild_id.is_some() {
                cache.highest_guild_message_id =
                    cache.highest_guild_message_id.max(Some(last_message_id));
            } else {
                cache.highest_private_message_id =
                    cache.highest_private_message_id.max(Some(last_message_id));
            }
        }

        cache
    }

    pub fn snapshot(&self, revision: SnapshotRevision) -> DiscordSnapshot {
        DiscordSnapshot {
            revision,
            navigation: NavigationSnapshot {
                navigation: Arc::clone(&self.navigation),
                guild_details: Arc::clone(&self.guild_details),
                profiles: Arc::clone(&self.profiles),
                presence: Arc::clone(&self.presence),
                voice: Arc::clone(&self.voice),
                session: Arc::clone(&self.session),
                threads: Arc::clone(&self.threads),
            },
            message: MessageSnapshot {
                message_cache: Arc::clone(&self.message_cache),
            },
            detail: DetailSnapshot {
                notifications: Arc::clone(&self.notifications),
            },
        }
    }

    pub fn restore_snapshot_areas(
        &mut self,
        snapshot: &DiscordSnapshot,
        previous_revision: SnapshotRevision,
    ) {
        let areas = snapshot.revision.changed_areas_since(previous_revision);
        self.attach_snapshot_areas(snapshot, areas);
    }

    /// The one place that maps snapshot areas back onto cache fields, so adding
    /// a cache cannot be half-wired between here and `DiscordSnapshot::to_state`.
    pub(in crate::discord) fn attach_snapshot_areas(
        &mut self,
        snapshot: &DiscordSnapshot,
        areas: SnapshotAreas,
    ) {
        if areas.navigation {
            self.navigation = Arc::clone(&snapshot.navigation.navigation);
            self.guild_details = Arc::clone(&snapshot.navigation.guild_details);
            self.profiles = Arc::clone(&snapshot.navigation.profiles);
            self.presence = Arc::clone(&snapshot.navigation.presence);
            self.voice = Arc::clone(&snapshot.navigation.voice);
            self.session = Arc::clone(&snapshot.navigation.session);
            self.threads = Arc::clone(&snapshot.navigation.threads);
        }
        if areas.message {
            self.message_cache = Arc::clone(&snapshot.message.message_cache);
        }
        // Settings live in the navigation area, read states in detail.
        if areas.navigation || areas.detail {
            self.notifications = Arc::clone(&snapshot.detail.notifications);
        }
    }

    /// Applies `event` and returns the revision the caller should publish.
    ///
    /// A MESSAGE_CREATE only counts as a detail change when it actually moved
    /// an unread count, so the detail area is narrowed by comparing signatures
    /// around the write. Both the client's publisher and the test harness go
    /// through here so the two cannot drift apart.
    pub(crate) fn apply_event_advancing(
        &mut self,
        event: &AppEvent,
        mut areas: SnapshotAreas,
        revision: SnapshotRevision,
    ) -> SnapshotRevision {
        let detail_before = matches!(event, AppEvent::MessageCreate { .. })
            .then(|| self.detail_revision_signature());
        self.apply_event(event);
        if let Some(before) = detail_before {
            areas.detail = self.detail_revision_signature() != before;
        }
        revision.advance(areas)
    }

    pub(crate) fn detail_revision_signature(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for (channel_id, read_state) in &self.notifications.read_states {
            channel_id.hash(&mut hasher);
            read_state.last_acked_message_id.hash(&mut hasher);
            read_state.mention_count.hash(&mut hasher);
            read_state.notification_count.hash(&mut hasher);
            read_state.last_pin_timestamp.hash(&mut hasher);
            read_state.flags.hash(&mut hasher);
            read_state.last_viewed.hash(&mut hasher);
        }
        for (key, read_state) in &self.notifications.non_channel_read_states {
            key.hash(&mut hasher);
            read_state.last_acked_id.hash(&mut hasher);
            read_state.badge_count.hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate { .. } => self.apply_guild_create_event(event),
            AppEvent::GuildUpdate {
                guild_id,
                name,
                owner_id,
                boost_tier,
                boost_count,
                verification_level,
                mfa_level,
                features,
                onboarding,
                roles,
                emojis,
            } => {
                if let Some(guild) = self.navigation_mut().guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                    if let Some(owner_id) = owner_id {
                        guild.owner_id = Some(*owner_id);
                    }
                    if let Some(boost_tier) = boost_tier {
                        guild.boost_tier = *boost_tier;
                    }
                    if let Some(boost_count) = boost_count {
                        guild.boost_count = *boost_count;
                    }
                    if let Some(verification_level) = verification_level {
                        guild.verification_level = Some(*verification_level);
                    }
                    if let Some(mfa_level) = mfa_level {
                        guild.mfa_level = Some(*mfa_level);
                    }
                    if let Some(features) = features {
                        guild.features = Some(features.clone());
                    }
                    if let Some(onboarding) = onboarding {
                        guild.onboarding = Some(onboarding.clone());
                    }
                }
                if let Some(roles) = roles {
                    self.guild_details_mut()
                        .roles
                        .insert(*guild_id, role_map(roles));
                }
                if let Some(emojis) = emojis {
                    self.navigation_mut()
                        .custom_emojis
                        .insert(*guild_id, emojis.clone());
                }
            }
            AppEvent::GuildOnboardingUpdate {
                guild_id,
                onboarding,
            } => {
                if let Some(guild) = self.navigation_mut().guilds.get_mut(guild_id) {
                    guild.onboarding = Some(onboarding.clone());
                }
            }
            AppEvent::GuildRolesUpdate { guild_id, roles } => {
                self.guild_details_mut()
                    .roles
                    .insert(*guild_id, role_map(roles));
            }
            AppEvent::GuildRoleUpsert { guild_id, role } => {
                if let Some(roles) = self.guild_details_mut().roles.get_mut(guild_id) {
                    roles.insert(role.id, role_state(role));
                }
            }
            AppEvent::GuildRoleDelete { guild_id, role_id } => {
                if let Some(roles) = self.guild_details_mut().roles.get_mut(guild_id) {
                    roles.remove(role_id);
                }
                if let Some(members) = self.guild_details_mut().members.get_mut(guild_id) {
                    for member in members.values_mut() {
                        member
                            .role_ids
                            .retain(|member_role_id| member_role_id != role_id);
                    }
                }
                if let Some(role_ids) = self
                    .guild_details_mut()
                    .current_user_role_ids
                    .get_mut(guild_id)
                {
                    role_ids.retain(|member_role_id| member_role_id != role_id);
                }
            }
            AppEvent::GuildEmojisUpdate { guild_id, emojis } => {
                self.navigation_mut()
                    .custom_emojis
                    .insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildDelete { guild_id } => self.apply_guild_delete(guild_id),
            AppEvent::GuildUnavailable { .. } => {}
            AppEvent::SelectedGuildChanged { guild_id } => {
                self.record_selected_member_guild(*guild_id);
            }
            AppEvent::SelectedMessageChannelChanged { channel_id } => {
                self.session_mut().selected_message_channel_known = true;
                self.session_mut().selected_message_channel_id = *channel_id;
                if let Some(channel_id) = channel_id {
                    self.touch_warm_message_channel(*channel_id);
                }
            }
            AppEvent::ChannelUpsert(channel) => {
                if super::channel::is_thread_kind(&channel.kind) {
                    self.apply_thread_gateway_upsert(
                        &super::ThreadGatewayInfo {
                            channel: channel.clone(),
                            current_user_member: None,
                        },
                        false,
                    );
                } else {
                    self.upsert_channel(channel);
                }
            }
            AppEvent::LazyPrivateChannelUpsert {
                channel,
                recipient_ids,
            } => {
                let mut channel = channel.clone();
                let mut recipients = recipient_ids
                    .iter()
                    .filter_map(|user_id| self.session.ready_users.get(user_id).cloned())
                    .collect::<Vec<_>>();
                if channel.kind == "group-dm"
                    && let Some(current_user_id) = self.session.current_user_id
                    && !recipients
                        .iter()
                        .any(|recipient| recipient.user_id == current_user_id)
                    && let Some(current_user) = self.session.ready_users.get(&current_user_id)
                {
                    recipients.push(current_user.clone());
                }
                if !recipients.is_empty() {
                    let synthetic_label = format!("dm-{}", channel.channel_id.get());
                    if channel.name == synthetic_label {
                        channel.name = recipients
                            .iter()
                            .map(|recipient| recipient.display_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                    }
                    channel.recipients = Some(recipients);
                }
                self.upsert_channel(&channel);
            }
            AppEvent::ChannelRecipientAdd {
                channel_id,
                recipient,
            } => self.apply_channel_recipient_add(*channel_id, recipient),
            AppEvent::ChannelRecipientRemove {
                channel_id,
                user_id,
            } => self.apply_channel_recipient_remove(*channel_id, *user_id),
            AppEvent::ThreadUpsert { thread, created } => {
                self.apply_thread_gateway_upsert(thread, *created);
            }
            AppEvent::ThreadListSync { sync } => {
                self.apply_thread_list_sync(sync);
                if let Some(current_user_members) = &sync.current_user_members {
                    self.apply_embedded_thread_members(sync.guild_id, current_user_members);
                }
            }
            AppEvent::ThreadMembersUpdateDispatch { update } => {
                if let Some(member_count) = update.member_count {
                    self.set_thread_member_count(update.channel_id, member_count);
                }
                if let Some(guild_id) = update.guild_id {
                    self.apply_embedded_thread_members(guild_id, &update.added_members);
                    self.update_loaded_thread_participants(
                        guild_id,
                        update.channel_id,
                        update
                            .added_members
                            .iter()
                            .filter_map(|member| member.user_id),
                        &update.removed_user_ids,
                    );
                }
                if let Some(current_user_id) = self.session.current_user_id {
                    if let Some(member) = update
                        .added_members
                        .iter()
                        .find(|member| member.user_id == Some(current_user_id))
                    {
                        self.upsert_current_user_thread_member(
                            update.channel_id,
                            update.guild_id,
                            member,
                        );
                    } else if update.removed_user_ids.contains(&current_user_id) {
                        self.remove_current_user_thread_member(update.channel_id);
                    }
                }
            }
            AppEvent::ThreadMemberListUpdate { update } => {
                self.apply_embedded_thread_members(update.guild_id, &update.members);
                // This is a participant snapshot, not the current account's
                // thread-member settings. Opening an old post must not promote
                // it into the active channel tree.
                let member_ids = update
                    .members
                    .iter()
                    .filter_map(|member| member.user_id)
                    .collect::<BTreeSet<_>>();
                self.replace_thread_participants(
                    update.guild_id,
                    update.channel_id,
                    member_ids.iter().copied(),
                );
                self.set_thread_member_count(
                    update.channel_id,
                    u64::try_from(member_ids.len()).unwrap_or(u64::MAX),
                );
            }
            AppEvent::ThreadMemberUpdate {
                guild_id,
                channel_id,
                member,
            } => {
                if let Some(guild_id) = guild_id {
                    self.apply_embedded_thread_members(*guild_id, std::slice::from_ref(member));
                    self.update_loaded_thread_participants(
                        *guild_id,
                        *channel_id,
                        member.user_id,
                        &[],
                    );
                }
                self.upsert_current_user_thread_member(*channel_id, *guild_id, member);
            }
            AppEvent::ForumPostDataLoaded {
                channel_id,
                requested_thread_ids,
                posts,
            } => {
                self.mark_forum_post_data_loaded(requested_thread_ids.iter().copied());
                let guild_id = self.channel_guild_id(*channel_id);
                for post in posts {
                    if let (Some(guild_id), Some(owner)) = (guild_id, post.owner.as_ref()) {
                        self.upsert_guild_member(guild_id, owner);
                        self.refresh_message_author_display_name(guild_id, owner);
                    }
                    if let Some(message) = &post.first_message {
                        self.merge_detached_message_history(
                            message.channel_id,
                            std::slice::from_ref(message),
                        );
                    }
                }
            }
            AppEvent::ArchivedThreadsLoaded {
                guild_id,
                channel_id,
                before,
                page,
            } => {
                self.apply_archived_threads_page(
                    *guild_id,
                    *channel_id,
                    super::ArchivedThreadPageCursor::from_before(before.clone()),
                    page,
                );
            }
            AppEvent::ArchivedThreadsLoadFailed {
                guild_id,
                channel_id,
                before,
                ..
            } => {
                self.mark_archived_threads_page_failed(
                    *guild_id,
                    *channel_id,
                    super::ArchivedThreadPageCursor::from_before(before.clone()),
                );
            }
            AppEvent::ChannelDelete { channel_id, .. } => self.remove_channel(*channel_id),
            AppEvent::MessageCreate { message } => self.apply_message_create(message),
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => {
                self.merge_message_history(*channel_id, *before, messages);
                if before.is_none() {
                    self.touch_warm_message_channel(*channel_id);
                }
            }
            AppEvent::MessageHistoryRefreshed {
                channel_id,
                messages,
            } => {
                self.replace_message_history(*channel_id, messages);
            }
            AppEvent::MessageHistoryAfterLoaded {
                channel_id,
                after,
                messages,
                has_more,
                ..
            } => {
                self.merge_message_history_after(*channel_id, *after, messages, *has_more);
            }
            AppEvent::MessageHistoryAroundLoaded {
                channel_id,
                message_id,
                messages,
            } => {
                self.merge_message_history_around(*channel_id, *message_id, messages);
            }
            AppEvent::MessageSearchLoaded { .. } => {}
            AppEvent::ThreadPreviewLoaded {
                channel_id,
                message,
            } => {
                self.merge_detached_message_history(*channel_id, std::slice::from_ref(message));
            }
            // Inbox loads keep their own snapshot (see notification_inbox) and
            // never touch the shared cache. They are handled as UI effects.
            AppEvent::InboxMentionsLoaded { .. }
            | AppEvent::InboxMentionsLoadFailed { .. }
            | AppEvent::InboxRecentMentionDeleted { .. }
            | AppEvent::InboxRecentMentionDeleteFailed { .. }
            | AppEvent::InboxChannelMessagesLoaded { .. }
            | AppEvent::InboxChannelMessagesLoadFailed { .. } => {}
            // Detected Rich Presence is UI-only. It does not mutate the shared cache.
            AppEvent::RichPresenceDetected { .. } => {}
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::MessageSearchLoadFailed { .. } => {}
            AppEvent::MessageUpdateDispatch { update } => {
                self.update_message(
                    update.channel_id,
                    update.message_id,
                    MessageUpdateFields {
                        body: update.fields.clone(),
                        reactions: None,
                        retain_body: self.should_retain_message_update_body(
                            update.channel_id,
                            update.message_id,
                        ),
                    },
                );
                if let Some(pinned) = update.fields.pinned {
                    self.set_cached_message_pinned(update.channel_id, update.message_id, pinned);
                }
            }
            AppEvent::CurrentUserReactionAdd {
                channel_id,
                message_id,
                emoji,
            } => self.add_reaction(*channel_id, *message_id, emoji.clone()),
            AppEvent::CurrentUserReactionRemove {
                channel_id,
                message_id,
                emoji,
            } => self.remove_reaction(*channel_id, *message_id, emoji),
            AppEvent::MessageReactionAdd {
                channel_id,
                message_id,
                user_id,
                emoji,
                ..
            } => self.add_gateway_reaction(*channel_id, *message_id, *user_id, emoji.clone()),
            AppEvent::MessageReactionRemove {
                channel_id,
                message_id,
                user_id,
                emoji,
                ..
            } => self.remove_gateway_reaction(*channel_id, *message_id, *user_id, emoji),
            AppEvent::MessageReactionRemoveAll {
                channel_id,
                message_id,
                ..
            } => self.clear_gateway_reactions(*channel_id, *message_id),
            AppEvent::MessageReactionRemoveEmoji {
                channel_id,
                message_id,
                emoji,
                ..
            } => self.clear_gateway_reaction_emoji(*channel_id, *message_id, emoji),
            AppEvent::MessagePinnedUpdate {
                channel_id,
                message_id,
                pinned,
            } => self.set_cached_message_pinned(*channel_id, *message_id, *pinned),
            AppEvent::ChannelPinsUpdate {
                channel_id,
                last_pin_timestamp,
                ..
            } => {
                self.invalidate_pinned_messages(*channel_id);
                self.notifications_mut()
                    .read_states
                    .entry(*channel_id)
                    .or_default()
                    .latest_pin_timestamp = last_pin_timestamp.clone();
            }
            AppEvent::PinnedMessagesLoaded {
                channel_id,
                messages,
            } => self.replace_pinned_messages(*channel_id, messages),
            AppEvent::PinnedMessagesLoadFailed { .. } => {}
            AppEvent::CurrentUserPollVoteUpdate {
                channel_id,
                message_id,
                answer_ids,
            } => self.update_current_user_poll_vote(*channel_id, *message_id, answer_ids),
            AppEvent::MessageDelete {
                channel_id,
                message_id,
                ..
            } => self.delete_message(*channel_id, *message_id),
            AppEvent::MessageDeleteBulk {
                channel_id,
                message_ids,
                ..
            } => self.delete_messages(*channel_id, message_ids),
            AppEvent::GuildMemberListUpdate { update } => {
                self.apply_member_list_update(update);
                if let Some(online) = update.online_count
                    && let Some(guild) = self.navigation_mut().guilds.get_mut(&update.guild_id)
                {
                    guild.online_count = Some(online);
                }
                if let Some(member_count) = update.member_count
                    && let Some(guild) = self.navigation_mut().guilds.get_mut(&update.guild_id)
                {
                    guild.member_count = Some(member_count);
                }
                let member_items = update
                    .ops
                    .iter()
                    .flat_map(|operation| operation.items())
                    .filter_map(|item| item.member())
                    .map(|(member, presence)| (member.clone(), presence.cloned()))
                    .collect::<Vec<_>>();
                for (member, _) in &member_items {
                    self.upsert_guild_member(update.guild_id, member);
                }
                let members = member_items
                    .iter()
                    .map(|(member, _)| member.clone())
                    .collect::<Vec<_>>();
                self.refresh_message_author_display_names(update.guild_id, &members);
                for presence in member_items
                    .iter()
                    .filter_map(|(_, presence)| presence.as_ref())
                {
                    self.apply_event(&AppEvent::PresenceUpdate {
                        guild_id: Some(update.guild_id),
                        presence: presence.clone(),
                    });
                }
            }
            AppEvent::GuildMembersChunk { chunk } => {
                for member in &chunk.members {
                    self.upsert_guild_member(chunk.guild_id, member);
                }
                self.refresh_message_author_display_names(chunk.guild_id, &chunk.members);
                for presence in &chunk.presences {
                    self.apply_event(&AppEvent::PresenceUpdate {
                        guild_id: Some(chunk.guild_id),
                        presence: presence.clone(),
                    });
                }
            }
            AppEvent::GuildMemberAdd { guild_id, member } => {
                self.upsert_guild_member(*guild_id, member);
                self.refresh_message_author_display_name(*guild_id, member);
            }
            AppEvent::GuildMemberUpsert { guild_id, member } => {
                self.upsert_guild_member(*guild_id, member);
                self.refresh_message_author_display_name(*guild_id, member);
            }
            AppEvent::GuildMemberRemove { guild_id, user_id } => {
                if let Some(entry) = self.guild_details_mut().members.get_mut(guild_id) {
                    entry.remove(user_id);
                }
                if let Some(member_ids) = self
                    .guild_details_mut()
                    .current_member_ids
                    .get_mut(guild_id)
                {
                    member_ids.remove(user_id);
                }
                self.remove_member_from_list(*guild_id, *user_id);
                self.remove_member_from_thread_participants(*guild_id, *user_id);
                self.remove_voice_state(*guild_id, *user_id);
            }
            AppEvent::PresenceUpdate { guild_id, presence } => {
                let user_id = presence.user_id;
                let status = presence.status;
                if let Some(guild_id) = guild_id {
                    self.presence_mut()
                        .guild_user_presences
                        .insert((*guild_id, user_id), status);
                    self.update_guild_user_activities(*guild_id, user_id, &presence.activities);
                    let entry = self
                        .guild_details_mut()
                        .members
                        .entry(*guild_id)
                        .or_default();
                    if let Some(member) = entry.get_mut(&user_id) {
                        member.status = status;
                    }
                }
                self.presence_mut().user_presences.insert(user_id, status);
                if guild_id.is_some()
                    && (self.session.current_user_id != Some(user_id)
                        || !presence.activities.is_empty())
                {
                    self.update_user_activities(user_id, &presence.activities);
                }
                if guild_id.is_none() {
                    self.update_user_activities(user_id, &presence.activities);
                    if self.session.current_user_id == Some(user_id) {
                        self.update_cached_guild_activities_for_user(user_id, &presence.activities);
                    }
                    self.update_cached_guild_presence_for_user(user_id, status);
                }
                self.update_channel_recipient_presence(user_id, status);
            }
            AppEvent::VoiceStateUpdate { state } => {
                // Member objects ride along only on guild voice states. DM call
                // states have no guild and no member to upsert.
                if let (Some(member), Some(guild_id)) = (state.member.as_ref(), state.guild_id) {
                    self.upsert_guild_member(guild_id, member);
                    self.refresh_message_author_display_name(guild_id, member);
                }
                self.update_voice_state(state);
            }
            AppEvent::VoiceSpeakingUpdate {
                scope,
                channel_id,
                user_id,
                speaking,
            } => {
                self.update_voice_speaking(*scope, *channel_id, *user_id, *speaking);
            }
            AppEvent::StreamCreate { stream } => self.record_stream_create(stream),
            AppEvent::StreamUpdate { stream } => self.record_stream_update(stream),
            AppEvent::StreamDelete { stream } => self.remove_stream(&stream.stream_key),
            AppEvent::CallDelete { channel_id } => {
                self.remove_voice_states_for_channel(*channel_id);
            }
            AppEvent::TypingStart {
                guild_id,
                channel_id,
                user_id,
                member,
            } => {
                if let (Some(guild_id), Some(member)) = (guild_id, member) {
                    self.upsert_guild_member(*guild_id, member);
                    self.refresh_message_author_display_name(*guild_id, member);
                }
                // Record (or refresh) the typing entry, then sweep this
                // channel's stale entries while we already hold the mutable
                // borrow. Read paths see only fresh entries.
                let now = Instant::now();
                let bucket = self.presence_mut().typing.entry(*channel_id).or_default();
                bucket.insert(*user_id, TypingIndicator { started: now });
                bucket.retain(|_, indicator| {
                    now.duration_since(indicator.started) <= TYPING_INDICATOR_TTL
                });
                if bucket.is_empty() {
                    self.presence_mut().typing.remove(channel_id);
                }
            }
            AppEvent::UserSettingsUpdate { settings } => {
                if let Some(folders) = &settings.guild_folders {
                    self.navigation_mut().guild_folders = folders.clone();
                }
            }
            AppEvent::UserNotificationSettingsUpdate { flags } => {
                self.notifications_mut().user_notification_flags = *flags;
            }
            AppEvent::UserProfileLoaded { guild_id, profile } => {
                self.apply_user_profile_loaded(guild_id, profile);
            }
            AppEvent::UserNoteLoaded { user_id, note } => {
                self.profiles_mut()
                    .fetched_notes
                    .insert(*user_id, note.clone());
                self.remember_fetched_note(*user_id);
                for profile in self
                    .profiles_mut()
                    .user_profiles
                    .values_mut()
                    .filter(|profile| profile.user_id == *user_id)
                {
                    profile.note = note.clone();
                }
            }
            AppEvent::RelationshipsLoaded { relationships } => {
                self.apply_relationships_loaded(relationships);
            }
            AppEvent::RelationshipUpsert { relationship } => {
                self.apply_relationship_upsert(relationship);
            }
            AppEvent::RelationshipUpdate { update } => self.apply_relationship_update(update),
            AppEvent::RelationshipRemove { user_id, status } => {
                self.apply_relationship_remove(user_id, *status)
            }
            AppEvent::UserIdentityUpdate {
                user_id,
                username,
                global_name,
                avatar_url,
                is_bot,
            } => self.apply_user_identity_update(
                *user_id,
                username,
                global_name.as_deref(),
                avatar_url.as_deref(),
                *is_bot,
            ),
            AppEvent::Ready { user, user_id } => {
                self.session_mut().current_user = Some(user.clone());
                if let Some(user_id) = user_id {
                    self.session_mut().current_user_id = Some(*user_id);
                    self.refresh_current_user_role_cache();
                }
            }
            AppEvent::ReadyUserDirectory { users } => {
                let ready_users: BTreeMap<_, _> = users
                    .iter()
                    .map(|user| (user.user_id, user.clone()))
                    .collect();
                self.session_mut().ready_users = ready_users.clone();

                // DEDUPE_USER_OBJECTS splits guild role data and global user
                // identity across `merged_members` and READY's top-level
                // `users`. Join those halves in shared state instead of
                // leaving a role-complete member named `unknown`.
                let mut refreshed_members = Vec::new();
                for (guild_id, members) in &mut self.guild_details_mut().members {
                    for member in members.values_mut() {
                        let Some(user) = ready_users.get(&member.user_id) else {
                            continue;
                        };
                        if is_fallback_identity(member.username.as_deref(), &member.display_name)
                            && user.display_name != "unknown"
                        {
                            member.display_name = user.display_name.clone();
                        }
                        if user.username.is_some() {
                            member.username = user.username.clone();
                        }
                        // Discord omits `bot` for normal users. The flattened
                        // READY directory cannot distinguish an omitted value
                        // from an explicit false, while a known bot account
                        // does not become a normal account. Keep the stronger
                        // cached fact instead of clearing it by omission.
                        member.is_bot |= user.is_bot;
                        if user.avatar_url.is_some() || member.avatar_url.is_none() {
                            member.avatar_url = user.avatar_url.clone();
                        }
                        refreshed_members.push((
                            *guild_id,
                            MemberInfo {
                                user_id: member.user_id,
                                display_name: member.display_name.clone(),
                                username: member.username.clone(),
                                nickname: member.nickname.clone(),
                                nickname_present: false,
                                is_bot: member.is_bot,
                                is_bot_present: true,
                                avatar_url: member.avatar_url.clone(),
                                avatar_url_present: true,
                                role_ids: member.role_ids.clone(),
                                role_ids_present: member.role_ids_known,
                                joined_at: member.joined_at,
                                flags: member.flags,
                                pending: member.pending,
                                communication_disabled_until: member.communication_disabled_until,
                                communication_disabled_until_present: false,
                            },
                        ));
                    }
                }
                for (guild_id, member) in refreshed_members {
                    self.refresh_message_author_display_name(guild_id, &member);
                }
            }
            AppEvent::CurrentUserCapabilities { premium_tier } => {
                self.session_mut().current_user_premium_tier = Some(*premium_tier);
            }
            AppEvent::CurrentUserVerification {
                email_verified,
                phone_verified,
                mfa_enabled,
            } => {
                if let Some(email_verified) = email_verified {
                    self.session_mut().current_user_email_verified = Some(*email_verified);
                }
                if let Some(phone_verified) = phone_verified {
                    self.session_mut().current_user_phone_verified = Some(*phone_verified);
                }
                if let Some(mfa_enabled) = mfa_enabled {
                    self.session_mut().current_user_mfa_enabled = Some(*mfa_enabled);
                }
            }
            AppEvent::ReadStateInit { entries } => self.apply_read_state_init(entries),
            AppEvent::ReadStateSync {
                entries,
                partial,
                version,
            } => self.apply_read_state_sync(entries, *partial, *version),
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
                flags,
                last_viewed,
                version,
            } => self.apply_message_ack(
                channel_id,
                message_id,
                mention_count,
                flags,
                last_viewed,
                *version,
            ),
            AppEvent::FeatureReadStateAck {
                read_state_type,
                resource_id,
                entity_id,
                version,
            } => {
                let entry = self
                    .notifications_mut()
                    .non_channel_read_states
                    .entry((*read_state_type, *resource_id))
                    .or_default();
                entry.last_acked_id = Some(*entity_id);
                entry.badge_count = 0;
                self.advance_read_state_version(*version);
            }
            AppEvent::ChannelPinsAck {
                channel_id,
                timestamp,
                version,
            } => {
                self.notifications_mut()
                    .read_states
                    .entry(*channel_id)
                    .or_default()
                    .last_pin_timestamp = Some(timestamp.clone());
                self.advance_read_state_version(*version);
            }
            AppEvent::ChannelUnreadUpdate { channels, .. } => {
                for update in channels {
                    if let Some(last_message_id) = update.last_message_id
                        && let Some(channel) =
                            self.navigation_mut().channels.get_mut(&update.channel_id)
                    {
                        channel.last_message_id = last_message_id;
                    }
                    if let Some(last_pin_timestamp) = &update.last_pin_timestamp {
                        self.notifications_mut()
                            .read_states
                            .entry(update.channel_id)
                            .or_default()
                            .latest_pin_timestamp = last_pin_timestamp.clone();
                    }
                }
            }
            AppEvent::UserGuildSettingsInit { settings } => {
                self.apply_user_guild_settings_sync(settings, false, None);
            }
            AppEvent::UserGuildSettingsSync {
                settings,
                partial,
                version,
            } => self.apply_user_guild_settings_sync(settings, *partial, *version),
            AppEvent::UserGuildSettingsUpdate { settings } => {
                self.upsert_notification_settings(&settings.notification_settings);
                if let Ok(version) = i64::try_from(settings.notification_settings.version) {
                    self.advance_user_guild_settings_version(version);
                }
            }
            AppEvent::ThreadNotificationLevelUpdate { channel_id, flags } => {
                self.set_thread_notification_level(*channel_id, *flags);
            }
            AppEvent::ThreadMuteUpdate {
                channel_id,
                muted,
                mute_end_time,
                selected_time_window,
            } => {
                self.set_thread_mute(
                    *channel_id,
                    *muted,
                    mute_end_time.clone(),
                    *selected_time_window,
                );
            }
            AppEvent::GatewayDispatchReceived { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::CaptchaRequired { .. }
            | AppEvent::MessageSendFailed { .. }
            | AppEvent::MessageSendRateLimited { .. }
            | AppEvent::MessageSendCooldownStarted { .. }
            | AppEvent::SignedOut
            | AppEvent::MediaPlaybackWindowReady { .. }
            | AppEvent::StreamPlaybackWindowReady { .. }
            | AppEvent::StreamPlaybackEnded { .. }
            | AppEvent::StreamCaptureTargetsLoaded { .. }
            | AppEvent::VoiceAudioSourcesLoaded { .. }
            | AppEvent::VoiceAudioSourcesApplyFailed { .. }
            | AppEvent::StreamBroadcastStarted { .. }
            | AppEvent::StreamBroadcastAudioUnavailable { .. }
            | AppEvent::StreamBroadcastStartFailed { .. }
            | AppEvent::StreamBroadcastEnded { .. }
            | AppEvent::ApplicationCommandsLoaded { .. }
            | AppEvent::ApplicationCommandIndexUpdated { .. }
            | AppEvent::InteractionSucceeded { .. }
            | AppEvent::InteractionFailed { .. }
            | AppEvent::ApplicationCommandAutocompleteResponse { .. }
            | AppEvent::AttachmentDownloadStarted { .. }
            | AppEvent::AttachmentDownloadProgress { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::AttachmentDownloadFailed { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::ReactionUsersLoadFailed { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostDataLoadFailed { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::UserProfileUpdateFailed { .. }
            | AppEvent::VoiceServerUpdate { .. }
            | AppEvent::StreamServerUpdate { .. }
            | AppEvent::VoiceConnectionStatusChanged { .. }
            | AppEvent::VoiceSound { .. }
            | AppEvent::GatewayResumed
            | AppEvent::GatewayClosed => {}
            AppEvent::GatewayReidentified => self.invalidate_member_lists_for_new_session(),
            AppEvent::ReadySnapshotComplete { snapshot } => {
                self.apply_ready_snapshot_complete(snapshot);
            }
            AppEvent::ReadySupplementalComplete {
                private_channel_ids,
            } => self.apply_ready_supplemental_complete(private_channel_ids),
        }
    }

    fn remove_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.navigation_mut().channels.remove(&channel_id);
        self.remove_thread_state(channel_id);
        self.message_cache_mut().timelines.remove(&channel_id);
        self.message_cache_mut()
            .cold_message_channels
            .remove(&channel_id);
        self.message_cache_mut()
            .warm_message_channels
            .retain(|warm_channel_id| *warm_channel_id != channel_id);
        self.message_cache_mut().pinned_messages.remove(&channel_id);
        self.message_cache_mut()
            .message_author_role_ids
            .retain(|(message_channel_id, _), _| *message_channel_id != channel_id);
        self.notifications_mut().read_states.remove(&channel_id);
        if self.session.selected_message_channel_id == Some(channel_id) {
            self.session_mut().selected_message_channel_id = None;
        }
        self.remove_voice_states_for_channel(channel_id);
    }

    fn remove_channels_matching(&mut self, predicate: impl Fn(&ChannelState) -> bool) {
        let channel_ids = self
            .navigation
            .channels
            .values()
            .filter(|channel| predicate(channel))
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        for channel_id in channel_ids {
            self.remove_channel(channel_id);
        }
    }

    fn apply_channel_recipient_add(
        &mut self,
        channel_id: Id<ChannelMarker>,
        recipient: &ChannelRecipientInfo,
    ) {
        let Some(channel) = self
            .navigation
            .channels
            .get(&channel_id)
            .filter(|channel| channel.guild_id.is_none())
        else {
            return;
        };
        let previous_names = channel
            .recipients
            .iter()
            .map(|recipient| recipient.display_name.clone())
            .collect::<Vec<_>>();
        let previous = channel
            .recipients
            .iter()
            .find(|existing| existing.user_id == recipient.user_id)
            .cloned();
        let ready_user = self.session.ready_users.get(&recipient.user_id);
        let known_status = self
            .presence
            .user_presences
            .get(&recipient.user_id)
            .copied();
        let display_name = self.private_user_display_name(
            recipient.user_id,
            Some(&recipient.display_name),
            recipient
                .username
                .as_deref()
                .or_else(|| ready_user.and_then(|user| user.username.as_deref()))
                .or_else(|| previous.as_ref().and_then(|user| user.username.as_deref())),
        );
        let recipient = ChannelRecipientState::from_info(
            recipient,
            previous.as_ref(),
            ready_user,
            known_status,
            display_name,
        );

        let Some(channel) = self.navigation_mut().channels.get_mut(&channel_id) else {
            return;
        };
        if let Some(existing) = channel
            .recipients
            .iter_mut()
            .find(|existing| existing.user_id == recipient.user_id)
        {
            *existing = recipient;
        } else {
            channel.recipients.push(recipient);
        }
        refresh_private_channel_name_from_recipients(channel, &previous_names);
    }

    fn apply_channel_recipient_remove(
        &mut self,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) {
        let Some(channel) = self
            .navigation_mut()
            .channels
            .get_mut(&channel_id)
            .filter(|channel| channel.guild_id.is_none())
        else {
            return;
        };
        let previous_names = channel
            .recipients
            .iter()
            .map(|recipient| recipient.display_name.clone())
            .collect::<Vec<_>>();
        channel
            .recipients
            .retain(|recipient| recipient.user_id != user_id);
        refresh_private_channel_name_from_recipients(channel, &previous_names);
    }

    pub(in crate::discord) fn apply_embedded_thread_members(
        &mut self,
        guild_id: Id<GuildMarker>,
        thread_members: &[ThreadMemberInfo],
    ) {
        let members = thread_members
            .iter()
            .filter_map(|thread_member| thread_member.member.clone())
            .collect::<Vec<_>>();
        for member in &members {
            self.upsert_guild_member(guild_id, member);
        }
        self.refresh_message_author_display_names(guild_id, &members);
        for presence in thread_members
            .iter()
            .filter_map(|thread_member| thread_member.presence.as_ref())
        {
            self.apply_event(&AppEvent::PresenceUpdate {
                guild_id: Some(guild_id),
                presence: presence.clone(),
            });
        }
    }

    fn apply_ready_snapshot_complete(&mut self, snapshot: &ReadySnapshotInfo) {
        if let Some(guild_ids) = &snapshot.guild_ids {
            let guild_ids = guild_ids.iter().copied().collect::<BTreeSet<_>>();
            let stale_guild_ids = self
                .navigation
                .guilds
                .keys()
                .filter(|guild_id| !guild_ids.contains(guild_id))
                .copied()
                .collect::<Vec<_>>();
            for guild_id in stale_guild_ids {
                self.apply_guild_delete(&guild_id);
            }
        }

        for (guild_id, channel_ids) in &snapshot.guild_channel_ids {
            let channel_ids = channel_ids.iter().copied().collect::<BTreeSet<_>>();
            self.remove_channels_matching(|channel| {
                channel.guild_id == Some(*guild_id) && !channel_ids.contains(&channel.id)
            });
        }

        self.session_mut().pending_ready_private_channel_ids = snapshot
            .private_channel_ids
            .as_ref()
            .map(|channel_ids| channel_ids.iter().copied().collect());
    }

    fn apply_ready_supplemental_complete(
        &mut self,
        supplemental_channel_ids: &[Id<ChannelMarker>],
    ) {
        let Some(mut current_channel_ids) =
            self.session_mut().pending_ready_private_channel_ids.take()
        else {
            return;
        };
        current_channel_ids.extend(supplemental_channel_ids.iter().copied());
        self.remove_channels_matching(|channel| {
            channel.guild_id.is_none() && !current_channel_ids.contains(&channel.id)
        });
    }

    fn apply_guild_create_event(&mut self, event: &AppEvent) {
        let AppEvent::GuildCreate {
            guild_id,
            name,
            member_count,
            owner_id,
            boost_tier,
            boost_count,
            verification_level,
            mfa_level,
            features,
            onboarding,
            channels,
            thread_snapshot_complete,
            current_user_thread_members,
            members,
            presences,
            roles,
            emojis,
        } = event
        else {
            unreachable!("guild create helper only handles guild create events");
        };

        self.remove_voice_states_for_guild(*guild_id);
        self.navigation_mut().guilds.insert(
            *guild_id,
            GuildState {
                id: *guild_id,
                name: name.clone(),
                member_count: *member_count,
                online_count: None,
                owner_id: *owner_id,
                boost_tier: *boost_tier,
                boost_count: *boost_count,
                verification_level: *verification_level,
                mfa_level: *mfa_level,
                features: features.clone(),
                onboarding: onboarding.clone(),
            },
        );

        if *thread_snapshot_complete {
            self.reset_threads_for_guild(*guild_id);
        }
        for channel in channels {
            if super::channel::is_thread_kind(&channel.kind) {
                self.apply_thread_gateway_upsert(
                    &super::ThreadGatewayInfo {
                        channel: channel.clone(),
                        current_user_member: None,
                    },
                    false,
                );
            } else {
                self.upsert_channel(channel);
            }
        }
        for member in current_user_thread_members {
            if let Some(thread_id) = member.thread_id {
                self.upsert_current_user_thread_member(thread_id, Some(*guild_id), member);
            }
        }
        self.apply_embedded_thread_members(*guild_id, current_user_thread_members);

        self.guild_details_mut()
            .current_member_ids
            .insert(*guild_id, BTreeSet::new());
        for member in members {
            self.upsert_guild_member(*guild_id, member);
        }
        self.reset_member_list_from_guild_snapshot(*guild_id, *member_count);
        for presence in presences {
            self.apply_event(&AppEvent::PresenceUpdate {
                guild_id: Some(*guild_id),
                presence: presence.clone(),
            });
        }
        if let Some(roles) = roles {
            self.guild_details_mut()
                .roles
                .insert(*guild_id, role_map(roles));
        } else {
            self.guild_details_mut().roles.remove(guild_id);
        }
        self.navigation_mut()
            .custom_emojis
            .insert(*guild_id, emojis.clone());
    }

    fn apply_guild_delete(&mut self, guild_id: &Id<GuildMarker>) {
        self.navigation_mut().guilds.remove(guild_id);
        self.remove_channels_matching(|channel| channel.guild_id == Some(*guild_id));
        self.reset_threads_for_guild(*guild_id);
        self.guild_details_mut().members.remove(guild_id);
        self.guild_details_mut().current_member_ids.remove(guild_id);
        self.guild_details_mut().member_lists.remove(guild_id);
        self.guild_details_mut()
            .member_cache_guild_order
            .retain(|cached_guild_id| cached_guild_id != guild_id);
        self.guild_details_mut().roles.remove(guild_id);
        self.guild_details_mut()
            .current_user_role_ids
            .remove(guild_id);
        self.presence_mut()
            .guild_user_presences
            .retain(|(presence_guild_id, _), _| presence_guild_id != guild_id);
        self.presence_mut()
            .guild_user_activities
            .retain(|(presence_guild_id, _), _| presence_guild_id != guild_id);
        self.remove_voice_states_for_guild(*guild_id);
        self.profiles_mut()
            .profile_role_ids
            .retain(|(profile_guild_id, _), _| profile_guild_id != guild_id);
        self.remove_profiles_for_guild(*guild_id);
        self.navigation_mut().custom_emojis.remove(guild_id);
        self.navigation_mut()
            .guild_folders
            .iter_mut()
            .for_each(|folder| {
                folder
                    .guild_ids
                    .retain(|folder_guild_id| folder_guild_id != guild_id);
            });
        self.navigation_mut()
            .guild_folders
            .retain(|folder| !folder.guild_ids.is_empty());
        self.notifications_mut()
            .notification_settings
            .remove(guild_id);
    }

    fn apply_message_create(&mut self, message: &MessageInfo) {
        let remove_typing_channel =
            if let Some(bucket) = self.presence_mut().typing.get_mut(&message.channel_id) {
                bucket.remove(&message.author_id);
                bucket.is_empty()
            } else {
                false
            };
        if remove_typing_channel {
            self.presence_mut().typing.remove(&message.channel_id);
        }

        let guild_id = message
            .guild_id
            .or_else(|| self.channel_guild_id(message.channel_id));
        let is_current_user_message = self.session.current_user_id == Some(message.author_id);
        self.record_author_role_ids(
            message.channel_id,
            message.message_id,
            &message.author_role_ids,
            message.author_role_ids_present,
        );
        match self.message_create_notification_kind(MessageNotificationInput {
            guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            author_id: message.author_id,
            mentions: &message.mentions,
            mention_everyone: message.mention_everyone,
            mention_roles: &message.mention_roles,
            flags: message.flags,
            message_kind: message.message_kind,
        }) {
            MessageNotificationKind::Mention { .. } => {
                let entry = self
                    .notifications_mut()
                    .read_states
                    .entry(message.channel_id)
                    .or_default();
                entry.record_mention(false);
            }
            MessageNotificationKind::LowImportanceMention => {
                let entry = self
                    .notifications_mut()
                    .read_states
                    .entry(message.channel_id)
                    .or_default();
                entry.record_mention(true);
            }
            MessageNotificationKind::Notify => {
                let entry = self
                    .notifications_mut()
                    .read_states
                    .entry(message.channel_id)
                    .or_default();
                entry.record_notification();
            }
            MessageNotificationKind::None => {}
        }
        let mut state = self.message_state_from_info(guild_id, message);
        let retain_body = self.should_retain_live_message_body(
            message.channel_id,
            message.author_id,
            &message.mentions,
        );
        if !retain_body {
            state.redact_body();
        }
        if self.retained_live_message_warms_channel(message.channel_id) {
            self.message_cache_mut()
                .cold_message_channels
                .remove(&message.channel_id);
        } else if !retain_body {
            self.message_cache_mut()
                .cold_message_channels
                .insert(message.channel_id);
        }
        self.upsert_message(state);
        if is_current_user_message && !message.message_kind.is_poll_result() {
            self.mark_message_read_locally(message.channel_id, message.message_id);
        }
    }

    fn apply_user_profile_loaded(
        &mut self,
        guild_id: &Option<Id<GuildMarker>>,
        profile: &UserProfileInfo,
    ) {
        let mut profile = profile.clone();
        if let Some(guild_id) = guild_id
            && profile.role_ids_present
        {
            self.profiles_mut()
                .profile_role_ids
                .insert((*guild_id, profile.user_id), profile.role_ids.clone());
        }
        profile.friend_status = self
            .profiles
            .relationships
            .get(&profile.user_id)
            .map(|relationship| relationship.status)
            .unwrap_or(FriendStatus::None);
        if let Some(note) = self.profiles.fetched_notes.get(&profile.user_id) {
            profile.note = note.clone();
        }
        let profile_display_name = profile.display_name().to_owned();
        let avatar_url = profile.avatar_url.clone();
        let username = profile.username.clone();
        let user_id = profile.user_id;
        let profile_key = UserProfileCacheKey::new(profile.user_id, *guild_id);
        self.profiles_mut()
            .user_profiles
            .insert(profile_key, profile);
        self.remember_profile_cache_key(profile_key);
        let display_name = if guild_id.is_some() {
            profile_display_name.clone()
        } else {
            self.private_user_display_name(
                user_id,
                Some(profile_display_name.as_str()),
                Some(username.as_str()),
            )
        };
        self.refresh_message_author_from_profile(
            *guild_id,
            user_id,
            &display_name,
            avatar_url.as_deref(),
        );
        if let Some(guild_id) = guild_id {
            if let Some(member) = self
                .guild_details_mut()
                .members
                .get_mut(guild_id)
                .and_then(|members| members.get_mut(&user_id))
                && member.username.is_none()
            {
                member.display_name = profile_display_name;
                member.username = Some(username);
            }
        } else {
            self.refresh_dm_channel_info_from_profile(
                user_id,
                &display_name,
                Some(username.as_str()),
                avatar_url.as_deref(),
            );
        }
    }

    fn apply_relationships_loaded(&mut self, relationships: &[RelationshipInfo]) {
        let previous = std::mem::take(&mut self.profiles_mut().relationships);
        for relationship in relationships {
            self.profiles_mut()
                .relationships
                .insert(relationship.user_id, relationship.clone());
        }
        let affected_users: BTreeSet<Id<UserMarker>> = previous
            .keys()
            .copied()
            .chain(self.profiles.relationships.keys().copied())
            .collect();
        for user_id in affected_users {
            let status = self
                .profiles
                .relationships
                .get(&user_id)
                .map(|relationship| relationship.status)
                .unwrap_or(FriendStatus::None);
            self.finish_relationship_change(user_id, status, previous.get(&user_id));
        }
    }

    fn apply_relationship_upsert(&mut self, relationship: &RelationshipInfo) {
        let previous = self
            .profiles
            .relationships
            .get(&relationship.user_id)
            .cloned();
        let relationship = merge_relationship_info(previous.as_ref(), relationship);
        self.profiles_mut()
            .relationships
            .insert(relationship.user_id, relationship.clone());
        self.finish_relationship_change(
            relationship.user_id,
            relationship.status,
            previous.as_ref(),
        );
        match relationship.status {
            FriendStatus::IncomingRequest => self.adjust_notification_center_badge(true),
            FriendStatus::Friend => self.adjust_notification_center_badge(false),
            FriendStatus::None
            | FriendStatus::Blocked
            | FriendStatus::OutgoingRequest
            | FriendStatus::Implicit => {}
        }
    }

    fn apply_relationship_update(&mut self, update: &RelationshipUpdateInfo) {
        let previous = self.profiles.relationships.get(&update.user_id).cloned();
        let Some(previous_or_status) = previous
            .as_ref()
            .map(|relationship| relationship.status)
            .or(update.status)
        else {
            return;
        };
        let relationship = RelationshipInfo {
            user_id: update.user_id,
            status: update.status.unwrap_or(previous_or_status),
            nickname: update.nickname.clone().unwrap_or_else(|| {
                previous
                    .as_ref()
                    .and_then(|relationship| relationship.nickname.clone())
            }),
            display_name: update.display_name.clone().unwrap_or_else(|| {
                previous
                    .as_ref()
                    .and_then(|relationship| relationship.display_name.clone())
            }),
            username: update.username.clone().unwrap_or_else(|| {
                previous
                    .as_ref()
                    .and_then(|relationship| relationship.username.clone())
            }),
            ignored: update
                .ignored
                .unwrap_or_else(|| previous.as_ref().is_some_and(|value| value.ignored)),
        };
        self.profiles_mut()
            .relationships
            .insert(update.user_id, relationship.clone());
        self.finish_relationship_change(update.user_id, relationship.status, previous.as_ref());
    }

    fn apply_relationship_remove(
        &mut self,
        user_id: &Id<UserMarker>,
        removed_status: Option<FriendStatus>,
    ) {
        let previous = self.profiles_mut().relationships.remove(user_id);
        if removed_status.or_else(|| previous.as_ref().map(|relationship| relationship.status))
            == Some(FriendStatus::IncomingRequest)
        {
            self.adjust_notification_center_badge(false);
        }
        self.finish_relationship_change(*user_id, FriendStatus::None, previous.as_ref());
    }

    fn adjust_notification_center_badge(&mut self, increment: bool) {
        const NOTIFICATION_CENTER_READ_STATE: u8 = 2;

        let Some(current_user_id) = self.session.current_user_id else {
            return;
        };
        let state = self
            .notifications_mut()
            .non_channel_read_states
            .entry((NOTIFICATION_CENTER_READ_STATE, current_user_id.get()))
            .or_default();
        state.badge_count = if increment {
            state.badge_count.saturating_add(1)
        } else {
            state.badge_count.saturating_sub(1)
        };
    }

    fn finish_relationship_change(
        &mut self,
        user_id: Id<UserMarker>,
        status: FriendStatus,
        previous: Option<&RelationshipInfo>,
    ) {
        for profile in self
            .profiles_mut()
            .user_profiles
            .values_mut()
            .filter(|profile| profile.user_id == user_id)
        {
            profile.friend_status = status;
        }
        self.refresh_private_user_display_name(
            user_id,
            previous.and_then(|relationship| relationship.display_name.as_deref()),
            previous.and_then(|relationship| relationship.username.as_deref()),
            previous.and_then(|relationship| relationship.nickname.as_deref()),
        );
    }

    fn apply_read_state_init(&mut self, entries: &[ReadStateInfo]) {
        self.apply_read_state_sync(entries, false, None);
    }

    fn apply_read_state_sync(
        &mut self,
        entries: &[ReadStateInfo],
        partial: bool,
        version: Option<i64>,
    ) {
        if !partial {
            self.notifications_mut().read_states.clear();
            self.notifications_mut().non_channel_read_states.clear();
            self.notifications_mut().read_state_version = version;
        } else if version.is_some() {
            self.notifications_mut().read_state_version = version;
        }
        for entry in entries {
            if entry.read_state_type == 0 {
                self.notifications_mut().read_states.insert(
                    entry.channel_id,
                    ChannelReadState {
                        last_acked_message_id: entry.last_acked_message_id,
                        mention_count: entry.mention_count,
                        notification_count: 0,
                        last_pin_timestamp: entry.last_pin_timestamp.clone(),
                        latest_pin_timestamp: None,
                        flags: entry.flags,
                        last_viewed: entry.last_viewed,
                    },
                );
            } else {
                self.notifications_mut().non_channel_read_states.insert(
                    (entry.read_state_type, entry.channel_id.get()),
                    NonChannelReadState {
                        last_acked_id: entry.last_acked_message_id.map(Id::get),
                        badge_count: entry.badge_count,
                    },
                );
            }
        }
    }

    fn apply_user_guild_settings_sync(
        &mut self,
        settings: &[super::events::UserGuildSettingsInfo],
        partial: bool,
        version: Option<i64>,
    ) {
        if !partial {
            self.notifications_mut().notification_settings.clear();
            self.notifications_mut().private_notification_settings = None;
            self.notifications_mut().user_guild_settings_version = version;
        } else if version.is_some() {
            self.notifications_mut().user_guild_settings_version = version;
        }
        for setting in settings {
            self.upsert_notification_settings(&setting.notification_settings);
        }
    }

    fn apply_message_ack(
        &mut self,
        channel_id: &Id<ChannelMarker>,
        message_id: &Id<MessageMarker>,
        mention_count: &Option<u32>,
        flags: &Option<u64>,
        last_viewed: &Option<u64>,
        version: Option<i64>,
    ) {
        let entry = self
            .notifications_mut()
            .read_states
            .entry(*channel_id)
            .or_default();
        entry.apply_server_ack(*message_id, *mention_count, *flags, *last_viewed);
        if let Some(version) = version {
            self.advance_read_state_version(version);
        }
    }

    fn advance_read_state_version(&mut self, version: i64) {
        let current = &mut self.notifications_mut().read_state_version;
        if current.is_none_or(|current| version > current) {
            *current = Some(version);
        }
    }

    fn advance_user_guild_settings_version(&mut self, version: i64) {
        let current = &mut self.notifications_mut().user_guild_settings_version;
        if current.is_none_or(|current| version > current) {
            *current = Some(version);
        }
    }

    pub(in crate::discord) fn private_user_display_name(
        &self,
        user_id: Id<UserMarker>,
        fallback_display_name: Option<&str>,
        fallback_username: Option<&str>,
    ) -> String {
        if let Some(nickname) = self
            .profiles
            .relationships
            .get(&user_id)
            .and_then(|relationship| relationship.nickname.as_deref())
        {
            return nickname.to_owned();
        }
        if let Some(display_name) = self
            .profiles
            .relationships
            .get(&user_id)
            .and_then(|relationship| relationship.display_name.as_deref())
        {
            return display_name.to_owned();
        }
        if let Some(profile) = self
            .profiles
            .user_profiles
            .get(&UserProfileCacheKey::new(user_id, None))
        {
            return profile.display_name().to_owned();
        }
        let ready_user = self.session.ready_users.get(&user_id);
        display_name_from_parts_or_unknown(
            None,
            fallback_display_name.or_else(|| ready_user.map(|user| user.display_name.as_str())),
            fallback_username.or_else(|| ready_user.and_then(|user| user.username.as_deref())),
        )
    }

    /// Resolves one user through the identity sources valid for a channel.
    /// Guild member data stays authoritative for guild nicknames, while global
    /// user data is only a provisional display fallback and never supplies
    /// guild roles or permissions.
    pub fn user_display_name_for_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<String> {
        let channel = self.channel(channel_id);
        if let Some(guild_id) = channel.and_then(|channel| channel.guild_id) {
            if let Some(member) = self
                .guild_details
                .members
                .get(&guild_id)
                .and_then(|members| members.get(&user_id))
                .filter(|member| {
                    !is_fallback_identity(member.username.as_deref(), &member.display_name)
                })
            {
                return Some(member.display_name.clone());
            }
            if let Some(profile) = self
                .profiles
                .user_profiles
                .get(&UserProfileCacheKey::new(user_id, Some(guild_id)))
                .or_else(|| {
                    self.profiles
                        .user_profiles
                        .get(&UserProfileCacheKey::new(user_id, None))
                })
            {
                return Some(profile.display_name().to_owned());
            }
            return self
                .session
                .ready_users
                .get(&user_id)
                .map(|user| user.display_name.clone())
                .filter(|name| name != "unknown");
        }

        if self.session.current_user_id == Some(user_id) {
            return self.session.current_user.clone();
        }
        let recipient = channel
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == user_id);
        let ready_user = self.session.ready_users.get(&user_id);
        let display_name = self.private_user_display_name(
            user_id,
            recipient
                .map(|recipient| recipient.display_name.as_str())
                .or_else(|| ready_user.map(|user| user.display_name.as_str())),
            recipient
                .and_then(|recipient| recipient.username.as_deref())
                .or_else(|| ready_user.and_then(|user| user.username.as_deref())),
        );
        (display_name != "unknown").then_some(display_name)
    }

    fn refresh_private_user_display_name(
        &mut self,
        user_id: Id<UserMarker>,
        fallback_display_name: Option<&str>,
        fallback_username: Option<&str>,
        previous_nickname: Option<&str>,
    ) {
        let (channel_display_name, channel_username) =
            self.current_private_recipient_identity(user_id);
        let channel_display_name = channel_display_name
            .filter(|display_name| previous_nickname != Some(display_name.as_str()));
        let display_name = self.private_user_display_name(
            user_id,
            fallback_display_name
                .or(channel_display_name.as_deref())
                .filter(|value| !value.is_empty()),
            fallback_username
                .or(channel_username.as_deref())
                .filter(|value| !value.is_empty()),
        );
        let username = self
            .profiles
            .relationships
            .get(&user_id)
            .and_then(|relationship| relationship.username.clone())
            .or(channel_username)
            .or_else(|| fallback_username.map(str::to_owned));
        self.refresh_message_author_from_profile(None, user_id, &display_name, None);
        self.refresh_dm_channel_info_from_profile(
            user_id,
            &display_name,
            username.as_deref(),
            None,
        );
    }

    fn apply_user_identity_update(
        &mut self,
        user_id: Id<UserMarker>,
        username: &str,
        global_name: Option<&str>,
        avatar_url: Option<&str>,
        is_bot: bool,
    ) {
        let mut previous_global_labels = HashSet::new();
        for profile in self
            .profiles
            .user_profiles
            .values()
            .filter(|profile| profile.user_id == user_id)
        {
            if let Some(global_name) = profile.global_name.as_ref() {
                previous_global_labels.insert(global_name.clone());
            }
            previous_global_labels.insert(profile.username.clone());
        }
        if let Some(relationship) = self.profiles.relationships.get(&user_id) {
            if let Some(display_name) = relationship.display_name.as_ref() {
                previous_global_labels.insert(display_name.clone());
            }
            if let Some(username) = relationship.username.as_ref() {
                previous_global_labels.insert(username.clone());
            }
        }

        let display_name = display_name_from_parts_or_unknown(None, global_name, Some(username));
        self.session_mut()
            .ready_users
            .entry(user_id)
            .and_modify(|user| {
                user.display_name = display_name.clone();
                user.username = Some(username.to_owned());
                user.is_bot = is_bot;
                user.avatar_url = avatar_url.map(str::to_owned);
            })
            .or_insert_with(|| ChannelRecipientInfo {
                user_id,
                display_name: display_name.clone(),
                username: Some(username.to_owned()),
                is_bot,
                avatar_url: avatar_url.map(str::to_owned),
                status: None,
            });
        if self.session.current_user_id == Some(user_id) {
            self.session_mut().current_user = Some(display_name.clone());
        }

        for profile in self
            .profiles_mut()
            .user_profiles
            .values_mut()
            .filter(|profile| profile.user_id == user_id)
        {
            profile.username = username.to_owned();
            profile.global_name = global_name.map(str::to_owned);
            profile.avatar_url = avatar_url.map(str::to_owned);
        }
        if let Some(relationship) = self.profiles_mut().relationships.get_mut(&user_id) {
            relationship.display_name = Some(display_name.clone());
            relationship.username = Some(username.to_owned());
        }

        let mut refreshed_members = Vec::new();
        for (guild_id, members) in &mut self.guild_details_mut().members {
            let Some(member) = members.get_mut(&user_id) else {
                continue;
            };
            let old_display_name = member.display_name.clone();
            let old_username = member.username.clone();
            member.username = Some(username.to_owned());
            member.is_bot = is_bot;
            if !member
                .avatar_url
                .as_deref()
                .is_some_and(is_guild_member_avatar_url)
                && (avatar_url.is_some() || member.avatar_url.is_none())
            {
                member.avatar_url = avatar_url.map(str::to_owned);
            }
            if old_username.as_deref() == Some(old_display_name.as_str())
                || previous_global_labels.contains(&old_display_name)
            {
                member.display_name = display_name.clone();
            }
            refreshed_members.push((
                *guild_id,
                MemberInfo {
                    user_id: member.user_id,
                    display_name: member.display_name.clone(),
                    username: member.username.clone(),
                    nickname: member.nickname.clone(),
                    nickname_present: false,
                    is_bot: member.is_bot,
                    is_bot_present: true,
                    avatar_url: member.avatar_url.clone(),
                    avatar_url_present: true,
                    role_ids: member.role_ids.clone(),
                    role_ids_present: member.role_ids_known,
                    joined_at: member.joined_at,
                    flags: member.flags,
                    pending: member.pending,
                    communication_disabled_until: member.communication_disabled_until,
                    communication_disabled_until_present: false,
                },
            ));
        }
        for (guild_id, member) in refreshed_members {
            self.refresh_message_author_display_name(guild_id, &member);
        }

        let private_display_name =
            self.private_user_display_name(user_id, Some(display_name.as_str()), Some(username));
        self.refresh_message_author_from_profile(None, user_id, &private_display_name, avatar_url);
        self.refresh_dm_channel_info_from_profile(
            user_id,
            &private_display_name,
            Some(username),
            avatar_url,
        );
    }

    fn current_private_recipient_identity(
        &self,
        user_id: Id<UserMarker>,
    ) -> (Option<String>, Option<String>) {
        self.navigation
            .channels
            .values()
            .filter(|channel| channel.guild_id.is_none())
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == user_id)
            .map(|recipient| {
                (
                    Some(recipient.display_name.clone()),
                    recipient.username.clone(),
                )
            })
            .unwrap_or((None, None))
    }

    fn update_cached_guild_presence_for_user(
        &mut self,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
    ) {
        for ((_, presence_user_id), presence_status) in
            &mut self.presence_mut().guild_user_presences
        {
            if *presence_user_id == user_id {
                *presence_status = status;
            }
        }
        for members in self.guild_details_mut().members.values_mut() {
            if let Some(member) = members.get_mut(&user_id) {
                member.status = status;
            }
        }
    }
}

fn is_guild_member_avatar_url(url: &str) -> bool {
    url.contains("/guilds/") && url.contains("/users/") && url.contains("/avatars/")
}

fn merge_relationship_info(
    previous: Option<&RelationshipInfo>,
    incoming: &RelationshipInfo,
) -> RelationshipInfo {
    RelationshipInfo {
        user_id: incoming.user_id,
        status: incoming.status,
        nickname: incoming.nickname.clone(),
        display_name: incoming
            .display_name
            .clone()
            .or_else(|| previous.and_then(|relationship| relationship.display_name.clone())),
        username: incoming
            .username
            .clone()
            .or_else(|| previous.and_then(|relationship| relationship.username.clone())),
        ignored: incoming.ignored,
    }
}

mod caches;
mod snapshot;
#[cfg(test)]
mod tests;

pub(in crate::discord) use caches::*;
pub use snapshot::*;
