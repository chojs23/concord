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
use super::message::{MessageAuthorRoleIds, MessageHistoryGap, MessageUpdateFields};
pub use super::message::{MessageCapabilities, MessageState};
pub use super::notification::ChannelUnreadState;
use super::notification::{
    GuildNotificationSettingsState, MessageNotificationInput, MessageNotificationKind,
};
use super::profile::{ProfileRoleIds, UserProfileCacheKey};
use super::read::ChannelReadState;
pub use super::voice::{CurrentVoiceConnectionState, VoiceParticipantState, VoiceScope};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};

use super::{
    ActivityInfo, AppEvent, ChannelInfo, CustomEmojiInfo, FriendStatus, GuildFolder, MemberInfo,
    PremiumTier, PresenceStatus, RelationshipInfo, UserProfileInfo,
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

#[derive(Clone, Debug)]
pub struct DiscordState {
    pub(in crate::discord) navigation: NavigationIndex,
    pub(in crate::discord) message_cache: MessageCache,
    pub(in crate::discord) guild_details: GuildDetailCache,
    pub(in crate::discord) profiles: ProfileCache,
    pub(in crate::discord) presence: PresenceCache,
    pub(in crate::discord) voice: VoiceStateCache,
    pub(in crate::discord) session: SessionState,
    pub(in crate::discord) notifications: NotificationCache,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGES_PER_CHANNEL)
    }
}

impl DiscordState {
    pub fn new(max_messages_per_channel: usize) -> Self {
        Self {
            navigation: NavigationIndex::default(),
            message_cache: MessageCache::new(max_messages_per_channel),
            guild_details: GuildDetailCache::default(),
            profiles: ProfileCache::default(),
            presence: PresenceCache::default(),
            voice: VoiceStateCache::default(),
            session: SessionState::default(),
            notifications: NotificationCache::default(),
        }
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
            self.navigation
                .thread_creators
                .insert(thread.channel_id, ThreadCreatorState { guild_id, user_id });
        }
    }

    pub fn cache_counts(&self) -> DiscordStateCacheCounts {
        DiscordStateCacheCounts {
            guilds: self.navigation.guilds.len(),
            channels: self.navigation.channels.len(),
            messages: self
                .message_cache
                .messages
                .values()
                .map(VecDeque::len)
                .sum(),
            message_channels: self.message_cache.messages.len(),
            pinned_messages: self
                .message_cache
                .pinned_messages
                .values()
                .map(VecDeque::len)
                .sum(),
            pinned_message_channels: self.message_cache.pinned_messages.len(),
            message_author_role_ids: self.message_cache.message_author_role_ids.len(),
            members: self.guild_details.members.values().map(BTreeMap::len).sum(),
            member_guilds: self.guild_details.members.len(),
            roles: self.guild_details.roles.values().map(BTreeMap::len).sum(),
            role_guilds: self.guild_details.roles.len(),
            current_user_role_guilds: self.guild_details.current_user_role_ids.len(),
            profile_role_ids: self.profiles.profile_role_ids.len(),
            custom_emojis: self.navigation.custom_emojis.values().map(Vec::len).sum(),
            custom_emoji_guilds: self.navigation.custom_emojis.len(),
            guild_folders: self.navigation.guild_folders.len(),
            user_profiles: self.profiles.user_profiles.len(),
            fetched_notes: self.profiles.fetched_notes.len(),
            relationships: self.profiles.relationships.len(),
            guild_user_presences: self.presence.guild_user_presences.len(),
            guild_user_activities: self.presence.guild_user_activities.len(),
            user_presences: self.presence.user_presences.len(),
            user_activities: self.presence.user_activities.len(),
            typing_users: self.presence.typing.values().map(BTreeMap::len).sum(),
            typing_channels: self.presence.typing.len(),
            voice_states: self.voice.states.len(),
            read_states: self.notifications.read_states.len(),
            notification_settings: self.notifications.notification_settings.len(),
            has_private_notification_settings: self
                .notifications
                .private_notification_settings
                .is_some(),
        }
    }

    pub fn snapshot(&self, revision: SnapshotRevision) -> DiscordSnapshot {
        DiscordSnapshot {
            revision,
            navigation: NavigationSnapshot {
                navigation: self.navigation.clone(),
                guild_details: self.guild_details.clone(),
                profiles: self.profiles.clone(),
                presence: self.presence.clone(),
                voice: self.voice.clone(),
                session: self.session.clone(),
                notification_settings: self.notifications.notification_settings.clone(),
                private_notification_settings: self
                    .notifications
                    .private_notification_settings
                    .clone(),
            },
            message: MessageSnapshot {
                message_cache: self.message_cache.clone(),
            },
            detail: DetailSnapshot {
                read_states: self.notifications.read_states.clone(),
            },
        }
    }

    pub fn restore_snapshot_areas(
        &mut self,
        snapshot: &DiscordSnapshot,
        previous_revision: SnapshotRevision,
    ) {
        let areas = snapshot.revision.changed_areas_since(previous_revision);
        if areas.navigation {
            self.navigation = snapshot.navigation.navigation.clone();
            self.guild_details = snapshot.navigation.guild_details.clone();
            self.profiles = snapshot.navigation.profiles.clone();
            self.presence = snapshot.navigation.presence.clone();
            self.voice = snapshot.navigation.voice.clone();
            self.session = snapshot.navigation.session.clone();
            self.notifications.notification_settings =
                snapshot.navigation.notification_settings.clone();
            self.notifications.private_notification_settings =
                snapshot.navigation.private_notification_settings.clone();
        }
        if areas.message {
            self.message_cache = snapshot.message.message_cache.clone();
        }
        if areas.detail {
            self.notifications.read_states = snapshot.detail.read_states.clone();
        }
    }

    pub fn snapshot_areas_for_event(event: &AppEvent) -> Option<SnapshotAreas> {
        event.snapshot_areas()
    }

    pub(crate) fn detail_revision_signature(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for (channel_id, read_state) in &self.notifications.read_states {
            channel_id.hash(&mut hasher);
            read_state.last_acked_message_id.hash(&mut hasher);
            read_state.mention_count.hash(&mut hasher);
            read_state.notification_count.hash(&mut hasher);
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
                if let Some(guild) = self.navigation.guilds.get_mut(guild_id) {
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
                    self.guild_details.roles.insert(*guild_id, role_map(roles));
                }
                if let Some(emojis) = emojis {
                    self.navigation
                        .custom_emojis
                        .insert(*guild_id, emojis.clone());
                }
            }
            AppEvent::GuildOnboardingUpdate {
                guild_id,
                onboarding,
            } => {
                if let Some(guild) = self.navigation.guilds.get_mut(guild_id) {
                    guild.onboarding = Some(onboarding.clone());
                }
            }
            AppEvent::GuildRolesUpdate { guild_id, roles } => {
                self.guild_details.roles.insert(*guild_id, role_map(roles));
            }
            AppEvent::GuildRoleUpsert { guild_id, role } => {
                if let Some(roles) = self.guild_details.roles.get_mut(guild_id) {
                    roles.insert(role.id, role_state(role));
                }
            }
            AppEvent::GuildRoleDelete { guild_id, role_id } => {
                if let Some(roles) = self.guild_details.roles.get_mut(guild_id) {
                    roles.remove(role_id);
                }
                if let Some(members) = self.guild_details.members.get_mut(guild_id) {
                    for member in members.values_mut() {
                        member
                            .role_ids
                            .retain(|member_role_id| member_role_id != role_id);
                    }
                }
                if let Some(role_ids) = self.guild_details.current_user_role_ids.get_mut(guild_id) {
                    role_ids.retain(|member_role_id| member_role_id != role_id);
                }
            }
            AppEvent::GuildEmojisUpdate { guild_id, emojis } => {
                self.navigation
                    .custom_emojis
                    .insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildDelete { .. } => self.apply_guild_delete_event(event),
            AppEvent::SelectedGuildChanged { guild_id } => {
                self.record_selected_member_guild(*guild_id);
            }
            AppEvent::SelectedMessageChannelChanged { channel_id } => {
                self.session.selected_message_channel_known = true;
                self.session.selected_message_channel_id = *channel_id;
                if let Some(channel_id) = channel_id {
                    self.touch_warm_message_channel(*channel_id);
                }
            }
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
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
                    self.merge_message_history(
                        message.channel_id,
                        None,
                        std::slice::from_ref(message),
                    );
                }
            }
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.navigation.channels.remove(channel_id);
                self.navigation.thread_creators.remove(channel_id);
                self.message_cache.messages.remove(channel_id);
                self.message_cache.cold_message_channels.remove(channel_id);
                self.message_cache
                    .warm_message_channels
                    .retain(|warm_channel_id| warm_channel_id != channel_id);
                self.message_cache.pinned_messages.remove(channel_id);
                self.message_cache
                    .message_author_role_ids
                    .retain(|(message_channel_id, _), _| message_channel_id != channel_id);
                self.remove_voice_states_for_channel(*channel_id);
            }
            AppEvent::MessageCreate { .. } => self.apply_message_create_event(event),
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
            AppEvent::MessageSearchLoaded { page } => {
                let mut by_channel: std::collections::BTreeMap<_, Vec<_>> =
                    std::collections::BTreeMap::new();
                for message in &page.messages {
                    by_channel
                        .entry(message.channel_id)
                        .or_default()
                        .push(message.clone());
                }
                for (channel_id, messages) in by_channel {
                    self.merge_message_history(channel_id, None, &messages);
                }
            }
            AppEvent::ThreadPreviewLoaded {
                channel_id,
                message,
            } => {
                self.merge_message_history(*channel_id, None, std::slice::from_ref(message));
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
            AppEvent::ChannelPinsUpdate { channel_id, .. } => {
                self.invalidate_pinned_messages(*channel_id);
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
                    && let Some(guild) = self.navigation.guilds.get_mut(&update.guild_id)
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
                if let Some(entry) = self.guild_details.members.get_mut(guild_id) {
                    entry.remove(user_id);
                }
                self.decrement_guild_member_count(*guild_id);
                self.remove_voice_state(*guild_id, *user_id);
            }
            AppEvent::PresenceUpdate { guild_id, presence } => {
                let user_id = presence.user_id;
                let status = presence.status;
                if let Some(guild_id) = guild_id {
                    self.presence
                        .guild_user_presences
                        .insert((*guild_id, user_id), status);
                    self.update_guild_user_activities(*guild_id, user_id, &presence.activities);
                    let entry = self.guild_details.members.entry(*guild_id).or_default();
                    if let Some(member) = entry.get_mut(&user_id) {
                        member.status = status;
                    }
                }
                self.presence.user_presences.insert(user_id, status);
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
                let bucket = self.presence.typing.entry(*channel_id).or_default();
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
                    self.presence.typing.remove(channel_id);
                }
            }
            AppEvent::UserSettingsUpdate { settings } => {
                if let Some(folders) = &settings.guild_folders {
                    self.navigation.guild_folders = folders.clone();
                }
            }
            AppEvent::UserProfileLoaded { .. } => self.apply_user_profile_loaded_event(event),
            AppEvent::UserNoteLoaded { user_id, note } => {
                self.profiles.fetched_notes.insert(*user_id, note.clone());
                self.remember_fetched_note(*user_id);
                for profile in self
                    .profiles
                    .user_profiles
                    .values_mut()
                    .filter(|profile| profile.user_id == *user_id)
                {
                    profile.note = note.clone();
                }
            }
            AppEvent::RelationshipsLoaded { .. } => self.apply_relationships_loaded_event(event),
            AppEvent::RelationshipUpsert { .. } => self.apply_relationship_upsert_event(event),
            AppEvent::RelationshipRemove { .. } => self.apply_relationship_remove_event(event),
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
                self.session.current_user = Some(user.clone());
                if let Some(user_id) = user_id {
                    self.session.current_user_id = Some(*user_id);
                    self.refresh_current_user_role_cache();
                }
            }
            AppEvent::CurrentUserCapabilities { premium_tier } => {
                self.session.current_user_premium_tier = Some(*premium_tier);
            }
            AppEvent::CurrentUserVerification {
                email_verified,
                phone_verified,
                mfa_enabled,
            } => {
                if let Some(email_verified) = email_verified {
                    self.session.current_user_email_verified = Some(*email_verified);
                }
                if let Some(phone_verified) = phone_verified {
                    self.session.current_user_phone_verified = Some(*phone_verified);
                }
                if let Some(mfa_enabled) = mfa_enabled {
                    self.session.current_user_mfa_enabled = Some(*mfa_enabled);
                }
            }
            AppEvent::ReadStateInit { .. } => self.apply_read_state_init_event(event),
            AppEvent::MessageAck { .. } => self.apply_message_ack_event(event),
            AppEvent::UserGuildSettingsInit { settings } => {
                self.notifications.notification_settings.clear();
                self.notifications.private_notification_settings = None;
                for setting in settings {
                    self.upsert_notification_settings(&setting.notification_settings);
                }
            }
            AppEvent::UserGuildSettingsUpdate { settings } => {
                self.upsert_notification_settings(&settings.notification_settings);
            }
            AppEvent::ThreadNotificationLevelUpdate { channel_id, flags } => {
                self.set_thread_notification_flags(*channel_id, *flags);
            }
            AppEvent::GatewayDispatchReceived { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::CaptchaRequired { .. }
            | AppEvent::MessageSendRateLimited { .. }
            | AppEvent::MessageSendCooldownStarted { .. }
            | AppEvent::SignedOut
            | AppEvent::MediaPlaybackWindowReady { .. }
            | AppEvent::ApplicationCommandsLoaded { .. }
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
            | AppEvent::VoiceConnectionStatusChanged { .. }
            | AppEvent::VoiceSound { .. }
            | AppEvent::ActivateChannel { .. }
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
        self.navigation.guilds.insert(
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
        let entry = self.guild_details.members.entry(*guild_id).or_default();
        for (user_id, status) in presences {
            self.presence
                .guild_user_presences
                .insert((*guild_id, *user_id), *status);
            self.presence.user_presences.insert(*user_id, *status);
            if let Some(member) = entry.get_mut(user_id) {
                member.status = *status;
            }
        }
        if let Some(roles) = roles {
            self.guild_details.roles.insert(*guild_id, role_map(roles));
        } else {
            self.guild_details.roles.remove(guild_id);
        }
        self.navigation
            .custom_emojis
            .insert(*guild_id, emojis.clone());
    }

    fn apply_guild_delete_event(&mut self, event: &AppEvent) {
        let AppEvent::GuildDelete { guild_id } = event else {
            unreachable!("guild delete helper only handles guild delete events");
        };

        self.navigation.guilds.remove(guild_id);
        self.navigation
            .channels
            .retain(|_, channel| channel.guild_id != Some(*guild_id));
        self.navigation
            .thread_creators
            .retain(|channel_id, _| self.navigation.channels.contains_key(channel_id));
        self.message_cache
            .messages
            .retain(|channel_id, _| self.navigation.channels.contains_key(channel_id));
        self.message_cache
            .cold_message_channels
            .retain(|channel_id| self.navigation.channels.contains_key(channel_id));
        self.message_cache
            .warm_message_channels
            .retain(|channel_id| self.navigation.channels.contains_key(channel_id));
        self.message_cache
            .pinned_messages
            .retain(|channel_id, _| self.navigation.channels.contains_key(channel_id));
        self.message_cache
            .message_author_role_ids
            .retain(|(channel_id, _), _| self.navigation.channels.contains_key(channel_id));
        self.guild_details.members.remove(guild_id);
        self.guild_details.roles.remove(guild_id);
        self.guild_details.current_user_role_ids.remove(guild_id);
        self.presence
            .guild_user_presences
            .retain(|(presence_guild_id, _), _| presence_guild_id != guild_id);
        self.presence
            .guild_user_activities
            .retain(|(presence_guild_id, _), _| presence_guild_id != guild_id);
        self.remove_voice_states_for_guild(*guild_id);
        self.profiles
            .profile_role_ids
            .retain(|(profile_guild_id, _), _| profile_guild_id != guild_id);
        self.remove_profiles_for_guild(*guild_id);
        self.navigation.custom_emojis.remove(guild_id);
    }

    fn apply_message_create_event(&mut self, event: &AppEvent) {
        let AppEvent::MessageCreate { message } = event else {
            unreachable!("message create helper only handles message create events");
        };

        let remove_typing_channel =
            if let Some(bucket) = self.presence.typing.get_mut(&message.channel_id) {
                bucket.remove(&message.author_id);
                bucket.is_empty()
            } else {
                false
            };
        if remove_typing_channel {
            self.presence.typing.remove(&message.channel_id);
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
                    .notifications
                    .read_states
                    .entry(message.channel_id)
                    .or_default();
                entry.mention_count = entry.mention_count.saturating_add(1);
            }
            MessageNotificationKind::Notify => {
                let entry = self
                    .notifications
                    .read_states
                    .entry(message.channel_id)
                    .or_default();
                entry.notification_count = entry.notification_count.saturating_add(1);
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
            self.message_cache
                .cold_message_channels
                .remove(&message.channel_id);
        } else if !retain_body {
            self.message_cache
                .cold_message_channels
                .insert(message.channel_id);
        }
        self.upsert_message(state);
        if is_current_user_message {
            self.mark_message_read_locally(message.channel_id, message.message_id);
        }
    }

    fn apply_user_profile_loaded_event(&mut self, event: &AppEvent) {
        let AppEvent::UserProfileLoaded { guild_id, profile } = event else {
            unreachable!("user profile helper only handles user profile loaded events");
        };

        let mut profile = profile.clone();
        if let Some(guild_id) = guild_id {
            self.profiles
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
        self.profiles.user_profiles.insert(profile_key, profile);
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
                .guild_details
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

    fn apply_relationships_loaded_event(&mut self, event: &AppEvent) {
        let AppEvent::RelationshipsLoaded { relationships } = event else {
            unreachable!("relationships helper only handles relationships loaded events");
        };

        let previous = std::mem::take(&mut self.profiles.relationships);
        for relationship in relationships {
            self.profiles
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
                .profiles
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

    fn apply_relationship_upsert_event(&mut self, event: &AppEvent) {
        let AppEvent::RelationshipUpsert { relationship } = event else {
            unreachable!("relationship helper only handles relationship upsert events");
        };

        let previous = self
            .profiles
            .relationships
            .get(&relationship.user_id)
            .cloned();
        let relationship = merge_relationship_info(previous.as_ref(), relationship);
        self.profiles
            .relationships
            .insert(relationship.user_id, relationship.clone());
        for profile in self
            .profiles
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

    fn apply_relationship_remove_event(&mut self, event: &AppEvent) {
        let AppEvent::RelationshipRemove { user_id } = event else {
            unreachable!("relationship helper only handles relationship remove events");
        };

        let previous = self.profiles.relationships.remove(user_id);
        for profile in self
            .profiles
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

    fn apply_read_state_init_event(&mut self, event: &AppEvent) {
        let AppEvent::ReadStateInit { entries } = event else {
            unreachable!("read state helper only handles read state init events");
        };

        self.notifications.read_states.clear();
        for entry in entries {
            self.notifications.read_states.insert(
                entry.channel_id,
                ChannelReadState {
                    last_acked_message_id: entry.last_acked_message_id,
                    mention_count: entry.mention_count,
                    notification_count: 0,
                },
            );
        }
    }

    fn apply_message_ack_event(&mut self, event: &AppEvent) {
        let AppEvent::MessageAck {
            channel_id,
            message_id,
            mention_count,
        } = event
        else {
            unreachable!("message ack helper only handles message ack events");
        };

        let entry = self
            .notifications
            .read_states
            .entry(*channel_id)
            .or_default();
        if entry
            .last_acked_message_id
            .is_some_and(|acked| acked > *message_id)
        {
            return;
        }
        entry.last_acked_message_id = Some(*message_id);
        entry.mention_count = *mention_count;
        entry.notification_count = 0;
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
            self.session.current_user = Some(display_name.clone());
        }

        for profile in self
            .profiles
            .user_profiles
            .values_mut()
            .filter(|profile| profile.user_id == user_id)
        {
            profile.username = username.to_owned();
            profile.global_name = global_name.map(str::to_owned);
            profile.avatar_url = avatar_url.map(str::to_owned);
        }
        if let Some(relationship) = self.profiles.relationships.get_mut(&user_id) {
            relationship.display_name = Some(display_name.clone());
            relationship.username = Some(username.to_owned());
        }

        let mut refreshed_members = Vec::new();
        for (guild_id, members) in &mut self.guild_details.members {
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
        for ((_, presence_user_id), presence_status) in &mut self.presence.guild_user_presences {
            if *presence_user_id == user_id {
                *presence_status = status;
            }
        }
        for members in self.guild_details.members.values_mut() {
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
