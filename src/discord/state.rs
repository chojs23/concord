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
pub use super::member::{GuildMemberState, RoleState, TypingUserState};
use super::member::{role_map, role_state};
use super::message::{MessageAuthorRoleIds, MessageUpdateFields};
pub use super::message::{MessageCapabilities, MessageState};
pub use super::notification::ChannelUnreadState;
use super::notification::{
    GuildNotificationSettingsState, MessageNotificationInput, MessageNotificationKind,
};
use super::profile::{ProfileRoleIds, UserProfileCacheKey};
use super::read::{ChannelReadState, NonChannelReadState};
pub use super::voice::{CurrentVoiceConnectionState, VoiceParticipantState, VoiceScope};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::{
    ActivityInfo, AppEvent, ChannelInfo, CustomEmojiInfo, FriendStatus, GuildFolder, MemberInfo,
    MessageInfo, PremiumTier, PresenceStatus, ReadStateInfo, RelationshipInfo, UserProfileInfo,
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

    pub fn thread_creator(&self, thread_id: Id<ChannelMarker>) -> Option<ThreadCreatorState> {
        self.navigation.thread_creators.get(&thread_id).copied()
    }

    fn record_thread_creators(&mut self, threads: &[ChannelInfo]) {
        for thread in threads {
            let Some(user_id) = thread.owner_id else {
                continue;
            };
            let guild_id = thread.guild_id.or_else(|| {
                self.navigation
                    .channels
                    .get(&thread.channel_id)
                    .and_then(|channel| channel.guild_id)
            });
            self.navigation_mut()
                .thread_creators
                .insert(thread.channel_id, ThreadCreatorState { guild_id, user_id });
        }
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
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
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
            AppEvent::ThreadListSync { sync } => {
                for thread in &sync.threads {
                    self.upsert_channel(thread);
                }
            }
            AppEvent::ThreadMembersUpdateDispatch { update } => {
                let Some(current_user_id) = self.session.current_user_id else {
                    return;
                };
                if update.added_user_ids.contains(&current_user_id) {
                    self.set_current_user_thread_membership(update.channel_id, true);
                } else if update.removed_user_ids.contains(&current_user_id) {
                    self.set_current_user_thread_membership(update.channel_id, false);
                }
            }
            AppEvent::ThreadMemberUpdate {
                channel_id, flags, ..
            } => {
                self.set_current_user_thread_membership(*channel_id, true);
                if let Some(flags) = flags {
                    self.set_thread_notification_flags(*channel_id, *flags);
                }
            }
            AppEvent::ForumPostsLoaded {
                threads,
                first_messages,
                ..
            } => {
                for thread in threads {
                    self.upsert_channel(thread);
                }
                self.record_thread_creators(threads);
                for message in first_messages {
                    self.merge_detached_message_history(
                        message.channel_id,
                        std::slice::from_ref(message),
                    );
                }
            }
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.navigation_mut().channels.remove(channel_id);
                self.navigation_mut().thread_creators.remove(channel_id);
                self.message_cache_mut().timelines.remove(channel_id);
                self.message_cache_mut()
                    .cold_message_channels
                    .remove(channel_id);
                self.message_cache_mut()
                    .warm_message_channels
                    .retain(|warm_channel_id| warm_channel_id != channel_id);
                self.message_cache_mut().pinned_messages.remove(channel_id);
                self.message_cache_mut()
                    .message_author_role_ids
                    .retain(|(message_channel_id, _), _| message_channel_id != channel_id);
                self.remove_voice_states_for_channel(*channel_id);
            }
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
            AppEvent::MessageUpdateDispatch { update } => self.update_message(
                update.channel_id,
                update.message_id,
                MessageUpdateFields {
                    body: update.fields.clone(),
                    pinned: None,
                    reactions: None,
                    retain_body: self
                        .should_retain_message_update_body(update.channel_id, update.message_id),
                },
            ),
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
                if let Some(online) = update.online_count
                    && let Some(guild) = self.navigation_mut().guilds.get_mut(&update.guild_id)
                {
                    guild.online_count = Some(online);
                }
                for member in &update.members {
                    self.upsert_guild_member(update.guild_id, member);
                }
                self.refresh_message_author_display_names(update.guild_id, &update.members);
                for presence in &update.presences {
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
                let was_known = self.upsert_guild_member(*guild_id, member);
                if !was_known {
                    self.increment_guild_member_count(*guild_id);
                }
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
                self.decrement_guild_member_count(*guild_id);
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
                channel_id,
                user_id,
                display_name,
            } => {
                // Record (or refresh) the typing entry, then sweep this
                // channel's stale entries while we already hold the mutable
                // borrow. Read paths see only fresh entries.
                let now = Instant::now();
                let bucket = self.presence_mut().typing.entry(*channel_id).or_default();
                bucket.insert(
                    *user_id,
                    TypingIndicator {
                        started: now,
                        display_name: display_name.clone(),
                    },
                );
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
            AppEvent::RelationshipRemove { user_id } => self.apply_relationship_remove(user_id),
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
                self.session_mut().ready_users = users
                    .iter()
                    .map(|user| (user.user_id, user.clone()))
                    .collect();
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
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
                flags,
                last_viewed,
            } => self.apply_message_ack(channel_id, message_id, mention_count, flags, last_viewed),
            AppEvent::FeatureReadStateAck {
                read_state_type,
                resource_id,
                entity_id,
                ..
            } => {
                let entry = self
                    .notifications_mut()
                    .non_channel_read_states
                    .entry((*read_state_type, *resource_id))
                    .or_default();
                entry.last_acked_id = Some(*entity_id);
                entry.badge_count = 0;
            }
            AppEvent::ChannelPinsAck {
                channel_id,
                timestamp,
                ..
            } => {
                self.notifications_mut()
                    .read_states
                    .entry(*channel_id)
                    .or_default()
                    .last_pin_timestamp = Some(timestamp.clone());
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
                self.notifications_mut().notification_settings.clear();
                self.notifications_mut().private_notification_settings = None;
                for setting in settings {
                    self.upsert_notification_settings(&setting.notification_settings);
                }
            }
            AppEvent::UserGuildSettingsUpdate { settings } => {
                self.upsert_notification_settings(&settings.notification_settings);
            }
            AppEvent::ThreadNotificationLevelUpdate { channel_id, flags } => {
                self.set_thread_notification_level(*channel_id, *flags);
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
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::UserProfileUpdateFailed { .. }
            | AppEvent::VoiceServerUpdate { .. }
            | AppEvent::StreamServerUpdate { .. }
            | AppEvent::VoiceConnectionStatusChanged { .. }
            | AppEvent::VoiceSound { .. }
            | AppEvent::GatewayResumed
            | AppEvent::GatewayReidentified
            | AppEvent::GatewayClosed => {}
        }
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

        for channel in channels {
            self.upsert_channel(channel);
        }

        for member in members {
            self.upsert_guild_member(*guild_id, member);
        }
        let Self {
            guild_details,
            presence,
            ..
        } = self;
        let members = Arc::make_mut(guild_details)
            .members
            .entry(*guild_id)
            .or_default();
        let presence = Arc::make_mut(presence);
        for (user_id, status) in presences {
            presence
                .guild_user_presences
                .insert((*guild_id, *user_id), *status);
            presence.user_presences.insert(*user_id, *status);
            if let Some(member) = members.get_mut(user_id) {
                member.status = *status;
            }
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
        let navigation = self.navigation_mut();
        navigation.guilds.remove(guild_id);
        navigation
            .channels
            .retain(|_, channel| channel.guild_id != Some(*guild_id));
        let surviving = &navigation.channels;
        navigation
            .thread_creators
            .retain(|channel_id, _| surviving.contains_key(channel_id));

        // Split the borrow so the pruned channel index stays readable while the
        // message caches drop everything that belonged to the deleted guild.
        let Self {
            navigation,
            message_cache,
            ..
        } = self;
        let surviving = &navigation.channels;
        let message_cache = Arc::make_mut(message_cache);
        message_cache
            .timelines
            .retain(|channel_id, _| surviving.contains_key(channel_id));
        message_cache
            .cold_message_channels
            .retain(|channel_id| surviving.contains_key(channel_id));
        message_cache
            .warm_message_channels
            .retain(|channel_id| surviving.contains_key(channel_id));
        message_cache
            .pinned_messages
            .retain(|channel_id, _| surviving.contains_key(channel_id));
        message_cache
            .message_author_role_ids
            .retain(|(channel_id, _), _| surviving.contains_key(channel_id));
        self.guild_details_mut().members.remove(guild_id);
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
        }) {
            MessageNotificationKind::Mention => {
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
        if let Some(guild_id) = guild_id {
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
            for profile in self
                .profiles_mut()
                .user_profiles
                .values_mut()
                .filter(|profile| profile.user_id == user_id)
            {
                profile.friend_status = status;
            }
            let previous = previous.get(&user_id);
            self.refresh_private_user_display_name(
                user_id,
                previous.and_then(|relationship| relationship.display_name.as_deref()),
                previous.and_then(|relationship| relationship.username.as_deref()),
                previous.and_then(|relationship| relationship.nickname.as_deref()),
            );
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
        for profile in self
            .profiles_mut()
            .user_profiles
            .values_mut()
            .filter(|profile| profile.user_id == relationship.user_id)
        {
            profile.friend_status = relationship.status;
        }
        self.refresh_private_user_display_name(
            relationship.user_id,
            previous
                .as_ref()
                .and_then(|relationship| relationship.display_name.as_deref()),
            previous
                .as_ref()
                .and_then(|relationship| relationship.username.as_deref()),
            previous
                .as_ref()
                .and_then(|relationship| relationship.nickname.as_deref()),
        );
    }

    fn apply_relationship_remove(&mut self, user_id: &Id<UserMarker>) {
        let previous = self.profiles_mut().relationships.remove(user_id);
        for profile in self
            .profiles_mut()
            .user_profiles
            .values_mut()
            .filter(|profile| profile.user_id == *user_id)
        {
            profile.friend_status = FriendStatus::None;
        }
        self.refresh_private_user_display_name(
            *user_id,
            previous
                .as_ref()
                .and_then(|relationship| relationship.display_name.as_deref()),
            previous
                .as_ref()
                .and_then(|relationship| relationship.username.as_deref()),
            previous
                .as_ref()
                .and_then(|relationship| relationship.nickname.as_deref()),
        );
    }

    fn apply_read_state_init(&mut self, entries: &[ReadStateInfo]) {
        self.notifications_mut().read_states.clear();
        self.notifications_mut().non_channel_read_states.clear();
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

    fn apply_message_ack(
        &mut self,
        channel_id: &Id<ChannelMarker>,
        message_id: &Id<MessageMarker>,
        mention_count: &Option<u32>,
        flags: &Option<u64>,
        last_viewed: &Option<u64>,
    ) {
        let entry = self
            .notifications_mut()
            .read_states
            .entry(*channel_id)
            .or_default();
        entry.apply_server_ack(*message_id, *mention_count, *flags, *last_viewed);
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
        display_name_from_parts_or_unknown(None, fallback_display_name, fallback_username)
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
                    is_bot: member.is_bot,
                    avatar_url: member.avatar_url.clone(),
                    role_ids: member.role_ids.clone(),
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
    }
}

mod caches;
mod snapshot;
#[cfg(test)]
mod tests;

pub use caches::*;
pub use snapshot::*;
