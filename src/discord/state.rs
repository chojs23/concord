use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

mod channels;
mod guilds;
mod members;
mod messages;
mod notifications;
mod permissions;
mod profiles;
mod reads;

/// Typing indicators stay visible for this long after the latest TYPING_START
/// from a given user. This matches Discord's documented 10-second window so the
/// label tracks what other clients show.
pub(super) const TYPING_INDICATOR_TTL: Duration = Duration::from_secs(10);

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};
pub use channels::{ChannelRecipientState, ChannelState, ChannelVisibilityStats};
pub use guilds::GuildState;
use members::role_map;
pub use members::{GuildMemberState, RoleState};
use messages::{MessageAuthorRoleIds, MessageUpdateFields};
pub use messages::{MessageCapabilities, MessageState};
pub use notifications::ChannelUnreadState;
use notifications::{GuildNotificationSettingsState, MessageNotificationKind};
use profiles::{ProfileRoleIds, UserProfileCacheKey};
use reads::ChannelReadState;

use super::{
    ActivityInfo, AppEvent, CustomEmojiInfo, FriendStatus, GuildFolder, PresenceStatus,
    RelationshipInfo, UserProfileInfo, display_name::display_name_from_parts_or_unknown,
};

/// Maximum number of recent messages kept per channel in the normal message cache.
const DEFAULT_MAX_MESSAGES_PER_CHANNEL: usize = 200;
/// Number of recently opened channels whose message bodies stay fully hydrated.
const DEFAULT_MAX_WARM_MESSAGE_CHANNELS: usize = 10;
/// Extra older-history window retained while the user scrolls above the newest messages.
pub(super) const OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER: usize = 2;
/// Maximum cached profile payloads kept for quick profile popup reopening.
pub(super) const MAX_USER_PROFILE_CACHE_ENTRIES: usize = 256;
/// Maximum cached user-note fetch results, including users with no note.
pub(super) const MAX_FETCHED_NOTE_CACHE_ENTRIES: usize = 256;
/// Number of recently selected guilds whose member lists stay fully cached.
pub(super) const MAX_RECENT_MEMBER_GUILDS: usize = 3;

pub(super) fn is_fallback_identity(username: Option<&str>, display_name: &str) -> bool {
    username.is_none() && display_name == "unknown"
}

#[derive(Clone, Debug)]
pub struct DiscordState {
    navigation: NavigationIndex,
    message_cache: MessageCache,
    guild_details: GuildDetailCache,
    profiles: ProfileCache,
    presence: PresenceCache,
    session: SessionState,
    notifications: NotificationCache,
}

#[derive(Clone, Debug, Default)]
struct NavigationIndex {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    custom_emojis: BTreeMap<Id<GuildMarker>, Vec<CustomEmojiInfo>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it. The dashboard falls back to a flat guild list.
    guild_folders: Vec<GuildFolder>,
}

#[derive(Clone, Debug)]
struct MessageCache {
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    cold_message_channels: BTreeSet<Id<ChannelMarker>>,
    warm_message_channels: VecDeque<Id<ChannelMarker>>,
    pinned_messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    message_author_role_ids: MessageAuthorRoleIds,
    max_messages_per_channel: usize,
    max_warm_message_channels: usize,
}

impl MessageCache {
    fn new(max_messages_per_channel: usize) -> Self {
        Self {
            messages: BTreeMap::new(),
            cold_message_channels: BTreeSet::new(),
            warm_message_channels: VecDeque::new(),
            pinned_messages: BTreeMap::new(),
            message_author_role_ids: BTreeMap::new(),
            max_messages_per_channel,
            max_warm_message_channels: DEFAULT_MAX_WARM_MESSAGE_CHANNELS,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct GuildDetailCache {
    members: BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    member_cache_guild_order: VecDeque<Id<GuildMarker>>,
    roles: BTreeMap<Id<GuildMarker>, BTreeMap<Id<RoleMarker>, RoleState>>,
    current_user_role_ids: BTreeMap<Id<GuildMarker>, Vec<Id<RoleMarker>>>,
}

#[derive(Clone, Debug, Default)]
struct ProfileCache {
    profile_role_ids: ProfileRoleIds,
    /// Cached profile lookups so the profile popup can render instantly when
    /// the same user is opened again.
    user_profiles: BTreeMap<UserProfileCacheKey, UserProfileInfo>,
    profile_cache_order: VecDeque<UserProfileCacheKey>,
    fetched_notes: BTreeMap<Id<UserMarker>, Option<String>>,
    fetched_note_order: VecDeque<Id<UserMarker>>,
    /// Friend / blocked / pending request state delivered through READY's
    /// `relationships` array. Used to colour the profile popup's friend
    /// indicator and to enrich `UserProfileInfo` on insert.
    relationships: BTreeMap<Id<UserMarker>, RelationshipInfo>,
}

#[derive(Clone, Debug, Default)]
struct PresenceCache {
    /// Guild-scoped presence and activity. These are keyed by both guild and
    /// user so evicting an old guild can drop its display-heavy presence data
    /// without affecting the same user's DM fallback or another guild.
    guild_user_presences: BTreeMap<(Id<GuildMarker>, Id<UserMarker>), PresenceStatus>,
    guild_user_activities: BTreeMap<(Id<GuildMarker>, Id<UserMarker>), Vec<ActivityInfo>>,
    /// Last known global presence by user id. This gives DM/profile views a
    /// fallback when the private-channel recipient payload omitted status.
    user_presences: BTreeMap<Id<UserMarker>, PresenceStatus>,
    user_activities: BTreeMap<Id<UserMarker>, Vec<ActivityInfo>>,
    /// Most recent TYPING_START arrival per (channel, user). Discord renews
    /// the indicator every ~10 seconds. Readers prune stale entries via
    /// `typing_users` so the map stays small.
    typing: BTreeMap<Id<ChannelMarker>, BTreeMap<Id<UserMarker>, Instant>>,
}

#[derive(Clone, Debug, Default)]
struct SessionState {
    /// Snowflake of the authenticated user. Captured from the READY payload
    /// and consulted by `can_view_channel` to look up our own roles and
    /// match member-level permission overwrites.
    current_user_id: Option<Id<UserMarker>>,
    current_user: Option<String>,
    selected_message_channel_known: bool,
    selected_message_channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Default)]
struct NotificationCache {
    read_states: BTreeMap<Id<ChannelMarker>, ChannelReadState>,
    notification_settings: BTreeMap<Id<GuildMarker>, GuildNotificationSettingsState>,
    private_notification_settings: Option<GuildNotificationSettingsState>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotRevision {
    pub global: u64,
    pub navigation: u64,
    pub message: u64,
    pub detail: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotAreas {
    pub navigation: bool,
    pub message: bool,
    pub detail: bool,
}

#[derive(Clone, Debug)]
pub struct DiscordSnapshot {
    pub revision: SnapshotRevision,
    pub navigation: NavigationSnapshot,
    pub message: MessageSnapshot,
    pub detail: DetailSnapshot,
}

#[derive(Clone, Debug)]
pub struct NavigationSnapshot {
    navigation: NavigationIndex,
    guild_details: GuildDetailCache,
    profiles: ProfileCache,
    presence: PresenceCache,
    session: SessionState,
    notification_settings: BTreeMap<Id<GuildMarker>, GuildNotificationSettingsState>,
    private_notification_settings: Option<GuildNotificationSettingsState>,
}

#[derive(Clone, Debug)]
pub struct MessageSnapshot {
    message_cache: MessageCache,
}

#[derive(Clone, Debug)]
pub struct DetailSnapshot {
    read_states: BTreeMap<Id<ChannelMarker>, ChannelReadState>,
}

impl SnapshotRevision {
    pub fn advance(self, areas: SnapshotAreas) -> Self {
        let global = self.global.saturating_add(1);
        Self {
            global,
            navigation: if areas.navigation {
                global
            } else {
                self.navigation
            },
            message: if areas.message { global } else { self.message },
            detail: if areas.detail { global } else { self.detail },
        }
    }

    pub fn changed_areas_since(self, previous: Self) -> SnapshotAreas {
        SnapshotAreas {
            navigation: self.navigation != previous.navigation,
            message: self.message != previous.message,
            detail: self.detail != previous.detail,
        }
    }
}

impl SnapshotAreas {
    pub const fn all() -> Self {
        Self {
            navigation: true,
            message: true,
            detail: true,
        }
    }

    const fn navigation() -> Self {
        Self {
            navigation: true,
            message: false,
            detail: false,
        }
    }

    const fn message() -> Self {
        Self {
            navigation: false,
            message: true,
            detail: false,
        }
    }

    const fn navigation_and_message() -> Self {
        Self {
            navigation: true,
            message: true,
            detail: false,
        }
    }

    const fn navigation_and_detail() -> Self {
        Self {
            navigation: true,
            message: false,
            detail: true,
        }
    }
}

impl DiscordSnapshot {
    pub fn to_state(&self) -> DiscordState {
        let mut state = DiscordState::new(self.message.message_cache.max_messages_per_channel);
        state.navigation = self.navigation.navigation.clone();
        state.guild_details = self.navigation.guild_details.clone();
        state.profiles = self.navigation.profiles.clone();
        state.presence = self.navigation.presence.clone();
        state.session = self.navigation.session.clone();
        state.message_cache = self.message.message_cache.clone();
        state.notifications = NotificationCache {
            read_states: self.detail.read_states.clone(),
            notification_settings: self.navigation.notification_settings.clone(),
            private_notification_settings: self.navigation.private_notification_settings.clone(),
        };
        state
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscordStateCacheCounts {
    pub guilds: usize,
    pub channels: usize,
    pub messages: usize,
    pub message_channels: usize,
    pub pinned_messages: usize,
    pub pinned_message_channels: usize,
    pub message_author_role_ids: usize,
    pub members: usize,
    pub member_guilds: usize,
    pub roles: usize,
    pub role_guilds: usize,
    pub current_user_role_guilds: usize,
    pub profile_role_ids: usize,
    pub custom_emojis: usize,
    pub custom_emoji_guilds: usize,
    pub guild_folders: usize,
    pub user_profiles: usize,
    pub fetched_notes: usize,
    pub relationships: usize,
    pub guild_user_presences: usize,
    pub guild_user_activities: usize,
    pub user_presences: usize,
    pub user_activities: usize,
    pub typing_users: usize,
    pub typing_channels: usize,
    pub read_states: usize,
    pub notification_settings: usize,
    pub has_private_notification_settings: bool,
}

impl DiscordStateCacheCounts {
    pub fn log_fields(&self) -> String {
        format!(
            "guilds={} channels={} messages={} message_channels={} \
             pinned_messages={} pinned_message_channels={} message_author_role_ids={} \
             members={} member_guilds={} roles={} role_guilds={} current_user_role_guilds={} \
             profile_role_ids={} \
             custom_emojis={} custom_emoji_guilds={} guild_folders={} user_profiles={} \
             fetched_notes={} relationships={} guild_user_presences={} \
             guild_user_activities={} user_presences={} user_activities={} typing_users={} \
             typing_channels={} read_states={} notification_settings={} \
             has_private_notification_settings={}",
            self.guilds,
            self.channels,
            self.messages,
            self.message_channels,
            self.pinned_messages,
            self.pinned_message_channels,
            self.message_author_role_ids,
            self.members,
            self.member_guilds,
            self.roles,
            self.role_guilds,
            self.current_user_role_guilds,
            self.profile_role_ids,
            self.custom_emojis,
            self.custom_emoji_guilds,
            self.guild_folders,
            self.user_profiles,
            self.fetched_notes,
            self.relationships,
            self.guild_user_presences,
            self.guild_user_activities,
            self.user_presences,
            self.user_activities,
            self.typing_users,
            self.typing_channels,
            self.read_states,
            self.notification_settings,
            self.has_private_notification_settings,
        )
    }
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
            session: SessionState::default(),
            notifications: NotificationCache::default(),
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
        if !event.mutates_discord_state() {
            return None;
        }

        Some(match event {
            AppEvent::GuildCreate { .. }
            | AppEvent::GuildUpdate { .. }
            | AppEvent::GuildDelete { .. }
            | AppEvent::ChannelUpsert(_)
            | AppEvent::ChannelDelete { .. }
            | AppEvent::ForumPostsLoaded { .. }
            | AppEvent::Ready { .. } => SnapshotAreas::all(),

            AppEvent::MessageCreate { .. } => SnapshotAreas::all(),

            AppEvent::MessageHistoryLoaded { .. }
            | AppEvent::ThreadPreviewLoaded { .. }
            | AppEvent::MessageUpdate { .. }
            | AppEvent::CurrentUserReactionAdd { .. }
            | AppEvent::CurrentUserReactionRemove { .. }
            | AppEvent::MessageReactionAdd { .. }
            | AppEvent::MessageReactionRemove { .. }
            | AppEvent::MessageReactionRemoveAll { .. }
            | AppEvent::MessageReactionRemoveEmoji { .. }
            | AppEvent::MessagePinnedUpdate { .. }
            | AppEvent::PinnedMessagesLoaded { .. }
            | AppEvent::CurrentUserPollVoteUpdate { .. }
            | AppEvent::MessageDelete { .. } => SnapshotAreas::message(),

            AppEvent::SelectedMessageChannelChanged { .. } => {
                SnapshotAreas::navigation_and_message()
            }

            AppEvent::GuildMemberAdd { .. }
            | AppEvent::GuildMemberUpsert { .. }
            | AppEvent::UserProfileLoaded { .. }
            | AppEvent::RelationshipsLoaded { .. }
            | AppEvent::RelationshipUpsert { .. }
            | AppEvent::RelationshipRemove { .. } => SnapshotAreas::navigation_and_message(),

            AppEvent::SelectedGuildChanged { .. }
            | AppEvent::GuildRolesUpdate { .. }
            | AppEvent::GuildEmojisUpdate { .. }
            | AppEvent::GuildMemberListCounts { .. }
            | AppEvent::GuildMemberRemove { .. }
            | AppEvent::PresenceUpdate { .. }
            | AppEvent::UserPresenceUpdate { .. }
            | AppEvent::TypingStart { .. }
            | AppEvent::GuildFoldersUpdate { .. }
            | AppEvent::UserNoteLoaded { .. }
            | AppEvent::UserGuildNotificationSettingsInit { .. }
            | AppEvent::UserGuildNotificationSettingsUpdate { .. } => SnapshotAreas::navigation(),

            AppEvent::ReadStateInit { .. } | AppEvent::MessageAck { .. } => {
                SnapshotAreas::navigation_and_detail()
            }

            AppEvent::MessageHistoryLoadFailed { .. }
            | AppEvent::PinnedMessagesLoadFailed { .. }
            | AppEvent::CurrentUserCapabilities { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::ActivateChannel { .. }
            | AppEvent::GatewayClosed => {
                unreachable!("non-mutating events return before snapshot area classification")
            }
        })
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate {
                guild_id,
                name,
                member_count,
                owner_id,
                channels,
                members,
                presences,
                roles,
                emojis,
            } => {
                self.navigation.guilds.insert(
                    *guild_id,
                    GuildState {
                        id: *guild_id,
                        name: name.clone(),
                        member_count: *member_count,
                        online_count: None,
                        owner_id: *owner_id,
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
                self.guild_details.roles.insert(*guild_id, role_map(roles));
                self.navigation
                    .custom_emojis
                    .insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildUpdate {
                guild_id,
                name,
                owner_id,
                roles,
                emojis,
            } => {
                if let Some(guild) = self.navigation.guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                    if let Some(owner_id) = owner_id {
                        guild.owner_id = Some(*owner_id);
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
            AppEvent::GuildRolesUpdate { guild_id, roles } => {
                self.guild_details.roles.insert(*guild_id, role_map(roles));
            }
            AppEvent::GuildEmojisUpdate { guild_id, emojis } => {
                self.navigation
                    .custom_emojis
                    .insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildDelete { guild_id } => {
                self.navigation.guilds.remove(guild_id);
                self.navigation
                    .channels
                    .retain(|_, channel| channel.guild_id != Some(*guild_id));
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
                self.profiles
                    .profile_role_ids
                    .retain(|(profile_guild_id, _), _| profile_guild_id != guild_id);
                self.remove_profiles_for_guild(*guild_id);
                self.navigation.custom_emojis.remove(guild_id);
            }
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
            AppEvent::ForumPostsLoaded {
                posts,
                preview_messages,
                ..
            } => {
                for post in posts {
                    self.upsert_channel(post);
                }
                for message in preview_messages {
                    self.merge_message_history(
                        message.channel_id,
                        None,
                        std::slice::from_ref(message),
                    );
                }
            }
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.navigation.channels.remove(channel_id);
                self.message_cache.messages.remove(channel_id);
                self.message_cache.cold_message_channels.remove(channel_id);
                self.message_cache
                    .warm_message_channels
                    .retain(|warm_channel_id| warm_channel_id != channel_id);
                self.message_cache.pinned_messages.remove(channel_id);
                self.message_cache
                    .message_author_role_ids
                    .retain(|(message_channel_id, _), _| message_channel_id != channel_id);
            }
            AppEvent::MessageCreate {
                guild_id,
                channel_id,
                message_id,
                author_id,
                author,
                author_avatar_url,
                author_role_ids,
                message_kind,
                reference,
                reply,
                poll,
                content,
                sticker_names,
                mentions,
                attachments,
                embeds,
                forwarded_snapshots,
                ..
            } => {
                let guild_id = guild_id.or_else(|| self.channel_guild_id(*channel_id));
                let is_current_user_message = self.session.current_user_id == Some(*author_id);
                self.record_author_role_ids(*channel_id, *message_id, author_role_ids);
                match self.message_create_notification_kind(
                    guild_id,
                    *channel_id,
                    *message_id,
                    *author_id,
                    content.as_deref(),
                    mentions,
                ) {
                    MessageNotificationKind::Mention => {
                        let entry = self
                            .notifications
                            .read_states
                            .entry(*channel_id)
                            .or_default();
                        entry.mention_count = entry.mention_count.saturating_add(1);
                    }
                    MessageNotificationKind::Notify => {
                        let entry = self
                            .notifications
                            .read_states
                            .entry(*channel_id)
                            .or_default();
                        entry.notification_count = entry.notification_count.saturating_add(1);
                    }
                    MessageNotificationKind::None => {}
                }
                let mut message = MessageState {
                    id: *message_id,
                    guild_id,
                    channel_id: *channel_id,
                    author_id: *author_id,
                    author: self.message_author_display_name(guild_id, *author_id, author),
                    author_avatar_url: self.message_author_avatar_url(
                        guild_id,
                        *author_id,
                        author_avatar_url,
                    ),
                    message_kind: *message_kind,
                    reference: reference.clone(),
                    reply: reply.clone(),
                    poll: poll.clone(),
                    pinned: false,
                    reactions: Vec::new(),
                    content: content.clone(),
                    sticker_names: sticker_names.clone(),
                    mentions: mentions.clone(),
                    attachments: attachments.clone(),
                    embeds: embeds.clone(),
                    forwarded_snapshots: forwarded_snapshots.clone(),
                    edited_timestamp: None,
                };
                let retain_body =
                    self.should_retain_live_message_body(*channel_id, *author_id, mentions);
                if !retain_body {
                    message.redact_body();
                }
                if self.retained_live_message_warms_channel(*channel_id) {
                    self.message_cache.cold_message_channels.remove(channel_id);
                } else if !retain_body {
                    self.message_cache.cold_message_channels.insert(*channel_id);
                }
                self.upsert_message(message);
                if is_current_user_message {
                    self.mark_message_read_locally(*channel_id, *message_id);
                }
            }
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
            AppEvent::ThreadPreviewLoaded {
                channel_id,
                message,
            } => {
                self.merge_message_history(*channel_id, None, std::slice::from_ref(message));
            }
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::MessageUpdate {
                channel_id,
                message_id,
                poll,
                content,
                mentions,
                sticker_names,
                attachments,
                embeds,
                edited_timestamp,
                ..
            } => self.update_message(
                *channel_id,
                *message_id,
                MessageUpdateFields {
                    poll: poll.clone(),
                    content: content.clone(),
                    sticker_names: sticker_names.clone(),
                    mentions: mentions.clone(),
                    attachments: attachments.clone(),
                    embeds: embeds.clone(),
                    edited_timestamp: edited_timestamp.clone(),
                    pinned: None,
                    reactions: None,
                    retain_body: self.should_retain_message_update_body(*channel_id, *message_id),
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
            AppEvent::GuildMemberListCounts { guild_id, online } => {
                if let Some(guild) = self.navigation.guilds.get_mut(guild_id) {
                    guild.online_count = Some(*online);
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
            }
            AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
                activities,
            } => {
                self.presence
                    .guild_user_presences
                    .insert((*guild_id, *user_id), *status);
                self.update_guild_user_activities(*guild_id, *user_id, activities);
                self.presence.user_presences.insert(*user_id, *status);
                self.update_user_activities(*user_id, activities);
                let entry = self.guild_details.members.entry(*guild_id).or_default();
                if let Some(member) = entry.get_mut(user_id) {
                    member.status = *status;
                }
                self.update_channel_recipient_presence(*user_id, *status);
            }
            AppEvent::UserPresenceUpdate {
                user_id,
                status,
                activities,
            } => {
                self.presence.user_presences.insert(*user_id, *status);
                self.update_user_activities(*user_id, activities);
                self.update_channel_recipient_presence(*user_id, *status);
            }
            AppEvent::TypingStart {
                channel_id,
                user_id,
            } => {
                // Record (or refresh) the typing entry, then sweep this
                // channel's stale entries while we already hold the mutable
                // borrow. Read paths see only fresh entries.
                let now = Instant::now();
                let bucket = self.presence.typing.entry(*channel_id).or_default();
                bucket.insert(*user_id, now);
                bucket.retain(|_, started| now.duration_since(*started) <= TYPING_INDICATOR_TTL);
                if bucket.is_empty() {
                    self.presence.typing.remove(channel_id);
                }
            }
            AppEvent::GuildFoldersUpdate { folders } => {
                self.navigation.guild_folders = folders.clone();
            }
            AppEvent::UserProfileLoaded { guild_id, profile } => {
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
                    {
                        if member.username.is_none() {
                            member.display_name = profile_display_name;
                            member.username = Some(username);
                        }
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
            AppEvent::RelationshipsLoaded { relationships } => {
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
            AppEvent::RelationshipUpsert { relationship } => {
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
            AppEvent::RelationshipRemove { user_id } => {
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
            AppEvent::Ready { user, user_id } => {
                self.session.current_user = Some(user.clone());
                if let Some(user_id) = user_id {
                    self.session.current_user_id = Some(*user_id);
                    self.refresh_current_user_role_cache();
                }
            }
            AppEvent::CurrentUserCapabilities { .. } => {}
            AppEvent::ReadStateInit { entries } => {
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
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
            } => {
                let entry = self
                    .notifications
                    .read_states
                    .entry(*channel_id)
                    .or_default();
                entry.last_acked_message_id = Some(*message_id);
                entry.mention_count = *mention_count;
                entry.notification_count = 0;
            }
            AppEvent::UserGuildNotificationSettingsInit { settings } => {
                self.notifications.notification_settings.clear();
                self.notifications.private_notification_settings = None;
                for setting in settings {
                    self.upsert_notification_settings(setting);
                }
            }
            AppEvent::UserGuildNotificationSettingsUpdate { settings } => {
                self.upsert_notification_settings(settings);
            }
            AppEvent::GatewayError { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::ActivateChannel { .. }
            | AppEvent::GatewayClosed => {}
        }
    }

    fn private_user_display_name(
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

#[cfg(test)]
mod tests {
    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    };

    use crate::discord::{
        ActivityInfo, ActivityKind, AppEvent, AttachmentUpdate, ChannelInfo,
        ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
        ChannelVisibilityStats, CustomEmojiInfo, DiscordState, FriendStatus,
        GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo, MessageKind,
        MessageReferenceInfo, MessageSnapshotInfo, MessageState, MutualGuildInfo,
        NotificationLevel, PermissionOverwriteInfo, PermissionOverwriteKind, PollAnswerInfo,
        PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo, ReadStateInfo, RelationshipInfo,
        ReplyInfo, RoleInfo, UserProfileInfo,
    };

    fn profile_info(user_id: u64, guild_nick: Option<&str>) -> UserProfileInfo {
        UserProfileInfo {
            user_id: Id::new(user_id),
            username: format!("user-{user_id}"),
            global_name: None,
            guild_nick: guild_nick.map(str::to_owned),
            role_ids: Vec::new(),
            avatar_url: None,
            bio: None,
            pronouns: None,
            mutual_guilds: Vec::<MutualGuildInfo>::new(),
            mutual_friends_count: 0,
            friend_status: FriendStatus::None,
            note: None,
        }
    }

    fn relationship_info(
        user_id: u64,
        status: FriendStatus,
        nickname: Option<&str>,
        display_name: Option<&str>,
        username: Option<&str>,
    ) -> RelationshipInfo {
        RelationshipInfo {
            user_id: Id::new(user_id),
            status,
            nickname: nickname.map(str::to_owned),
            display_name: display_name.map(str::to_owned),
            username: username.map(str::to_owned),
        }
    }

    fn guild_text_channel(guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }

    fn private_channel(channel_id: Id<ChannelMarker>) -> ChannelInfo {
        ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "dm".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }

    fn notification_settings(
        guild_id: Id<GuildMarker>,
        level: NotificationLevel,
    ) -> GuildNotificationSettingsInfo {
        GuildNotificationSettingsInfo {
            guild_id: Some(guild_id),
            message_notifications: Some(level),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }
    }

    fn private_notification_settings(level: NotificationLevel) -> GuildNotificationSettingsInfo {
        GuildNotificationSettingsInfo {
            guild_id: None,
            message_notifications: Some(level),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
        }
    }

    fn message_create(
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_id: Id<UserMarker>,
        content: &str,
        mentions: Vec<MentionInfo>,
    ) -> AppEvent {
        AppEvent::MessageCreate {
            guild_id,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(content.to_owned()),
            sticker_names: Vec::new(),
            mentions,
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }

    #[test]
    fn all_message_notification_settings_show_numeric_badge() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![guild_text_channel(guild_id, channel_id)],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::SelectedMessageChannelChanged { channel_id: None });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![notification_settings(
                guild_id,
                NotificationLevel::AllMessages,
            )],
        });

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Notified(1)
        );
        assert_eq!(
            state.guild_unread(guild_id),
            ChannelUnreadState::Notified(1)
        );
        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, None);
    }

    #[test]
    fn loaded_guild_messages_use_notification_numeric_badge() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![guild_text_channel(guild_id, channel_id)],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(29)),
                mention_count: 0,
            }],
        });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![notification_settings(
                guild_id,
                NotificationLevel::AllMessages,
            )],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(30),
                author_id,
                author: "neo".to_owned(),
                content: Some("loaded".to_owned()),
                ..MessageInfo::default()
            }],
        });

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Notified(1)
        );
        assert_eq!(
            state.guild_unread(guild_id),
            ChannelUnreadState::Notified(1)
        );
    }

    #[test]
    fn muted_channel_does_not_add_numeric_notification_badge() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();
        let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                mute_end_time: None,
            });

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![guild_text_channel(guild_id, channel_id)],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![settings],
        });

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(state.channel_unread_message_count(channel_id), 0);
        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            ChannelUnreadState::Seen
        );
        assert_eq!(
            state.guild_sidebar_unread(guild_id),
            ChannelUnreadState::Seen
        );
    }

    #[test]
    fn muted_parent_category_does_not_add_server_sidebar_unread() {
        let guild_id = Id::new(1);
        let category_id = Id::new(2);
        let channel_id = Id::new(3);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();
        let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id: category_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                mute_end_time: None,
            });

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: category_id,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "category".to_owned(),
                    kind: "category".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: Some(Id::new(30)),
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![settings],
        });

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert!(state.channel_notification_muted(channel_id));
        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            ChannelUnreadState::Seen
        );
        assert_eq!(
            state.guild_sidebar_unread(guild_id),
            ChannelUnreadState::Seen
        );
    }

    #[test]
    fn explicit_channel_unmute_override_beats_muted_parent_category() {
        let guild_id = Id::new(1);
        let category_id = Id::new(2);
        let channel_id = Id::new(3);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();
        let mut settings = notification_settings(guild_id, NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id: category_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                mute_end_time: None,
            });
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: false,
                mute_end_time: None,
            });

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: category_id,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "category".to_owned(),
                    kind: "category".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: Some(Id::new(30)),
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![settings],
        });

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert!(!state.channel_notification_muted(channel_id));
        assert_eq!(state.channel_unread_message_count(channel_id), 1);
        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Notified(1)
        );
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            ChannelUnreadState::Notified(1)
        );
    }

    #[test]
    fn only_mentions_settings_count_direct_mentions() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![guild_text_channel(guild_id, channel_id)],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![notification_settings(
                guild_id,
                NotificationLevel::OnlyMentions,
            )],
        });

        state.apply_event(&message_create(
            Some(guild_id),
            channel_id,
            Id::new(30),
            author_id,
            "hello @me",
            vec![mention_info(current_user_id.get(), "me")],
        ));

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Mentioned(1)
        );
    }

    #[test]
    fn private_all_messages_settings_show_numeric_badge() {
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![private_notification_settings(
                NotificationLevel::AllMessages,
            )],
        });

        state.apply_event(&message_create(
            None,
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Notified(1)
        );
        assert_eq!(state.channel_unread_message_count(channel_id), 1);
    }

    #[test]
    fn private_channel_override_no_messages_suppresses_numeric_badge() {
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();
        let mut settings = private_notification_settings(NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id,
                message_notifications: Some(NotificationLevel::NoMessages),
                muted: false,
                mute_end_time: None,
            });

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![settings],
        });

        state.apply_event(&message_create(
            None,
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(state.channel_unread_message_count(channel_id), 0);
        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
    }

    #[test]
    fn muted_private_channel_override_suppresses_numeric_badge() {
        let channel_id = Id::new(2);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();
        let mut settings = private_notification_settings(NotificationLevel::AllMessages);
        settings
            .channel_overrides
            .push(ChannelNotificationOverrideInfo {
                channel_id,
                message_notifications: Some(NotificationLevel::AllMessages),
                muted: true,
                mute_end_time: None,
            });

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(private_channel(channel_id)));
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![settings],
        });

        state.apply_event(&message_create(
            None,
            channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));

        assert_eq!(state.channel_unread_message_count(channel_id), 0);
        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
        assert_eq!(
            state.channel_sidebar_unread(channel_id),
            ChannelUnreadState::Seen
        );
        assert_eq!(state.direct_message_unread_count(), 0);
    }

    #[test]
    fn notification_settings_init_replaces_private_settings() {
        let guild_id = Id::new(1);
        let guild_channel_id = Id::new(2);
        let private_channel_id = Id::new(3);
        let current_user_id = Id::new(10);
        let author_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![guild_text_channel(guild_id, guild_channel_id)],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::ChannelUpsert(private_channel(
            private_channel_id,
        )));
        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![private_notification_settings(NotificationLevel::NoMessages)],
        });

        state.apply_event(&message_create(
            None,
            private_channel_id,
            Id::new(30),
            author_id,
            "hello",
            Vec::new(),
        ));
        assert_eq!(
            state.channel_unread(private_channel_id),
            ChannelUnreadState::Unread
        );

        state.apply_event(&AppEvent::UserGuildNotificationSettingsInit {
            settings: vec![notification_settings(
                guild_id,
                NotificationLevel::OnlyMentions,
            )],
        });

        assert_eq!(
            state.channel_unread(private_channel_id),
            ChannelUnreadState::Notified(1)
        );
    }

    #[test]
    fn applies_guild_channels_and_messages() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.guilds().len(), 1);
        assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(state.messages_for_channel(channel_id).len(), 1);
    }

    #[test]
    fn user_profile_cache_is_scoped_by_guild() {
        let user_id = Id::new(10);
        let guild_a = Id::new(1);
        let guild_b = Id::new(2);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_a),
            profile: profile_info(user_id.get(), Some("guild a nick")),
        });
        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_b),
            profile: profile_info(user_id.get(), Some("guild b nick")),
        });

        assert_eq!(
            state
                .user_profile(user_id, Some(guild_a))
                .and_then(|profile| profile.guild_nick.as_deref()),
            Some("guild a nick")
        );
        assert_eq!(
            state
                .user_profile(user_id, Some(guild_b))
                .and_then(|profile| profile.guild_nick.as_deref()),
            Some("guild b nick")
        );
        assert!(state.user_profile(user_id, None).is_none());
    }

    #[test]
    fn message_author_uses_cached_member_display_name() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: vec![MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "server alias");
    }

    #[test]
    fn dm_message_author_prefers_friend_nickname() {
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::RelationshipsLoaded {
            relationships: vec![relationship_info(
                author_id.get(),
                FriendStatus::Friend,
                Some("Bestie"),
                Some("Alice Global"),
                Some("alice"),
            )],
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "Alice Global".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "Bestie");
    }

    #[test]
    fn relationship_nickname_update_refreshes_existing_dm_message_authors() {
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::RelationshipsLoaded {
            relationships: vec![relationship_info(
                author_id.get(),
                FriendStatus::Friend,
                Some("Bestie"),
                Some("Alice Global"),
                Some("alice"),
            )],
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "Alice Global".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::RelationshipUpsert {
            relationship: relationship_info(
                author_id.get(),
                FriendStatus::Friend,
                None,
                None,
                None,
            ),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "Alice Global");
    }

    #[test]
    fn member_update_refreshes_existing_message_author() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "server alias");
    }

    #[test]
    fn stores_channel_parent_and_position() {
        let guild_id = Id::new(1);
        let category_id = Id::new(2);
        let channel_id = Id::new(3);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: Some(category_id),
            position: Some(7),
            last_message_id: Some(Id::new(9)),
            name: "general".to_owned(),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.parent_id, Some(category_id));
        assert_eq!(channel.position, Some(7));
        assert_eq!(channel.last_message_id, Some(Id::new(9)));
    }

    #[test]
    fn channel_upsert_stores_and_preserves_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: Some("https://cdn.discordapp.com/avatar.png".to_owned()),
                status: Some(PresenceStatus::Online),
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "renamed project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.name, "renamed project chat");
        assert_eq!(channel.recipients.len(), 1);
        assert_eq!(channel.recipients[0].user_id, Id::new(20));
        assert_eq!(channel.recipients[0].display_name, "alice");
        assert_eq!(
            channel.recipients[0].avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/avatar.png")
        );
        assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
    }

    #[test]
    fn dm_channel_upsert_prefers_friend_nickname() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::RelationshipsLoaded {
            relationships: vec![relationship_info(
                20,
                FriendStatus::Friend,
                Some("Bestie"),
                Some("Alice Global"),
                Some("alice"),
            )],
        });
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "Alice Global".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "Alice Global".to_owned(),
                username: Some("alice".to_owned()),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.name, "Bestie");
        assert_eq!(channel.recipients[0].display_name, "Bestie");
    }

    #[test]
    fn relationships_without_user_fields_preserve_existing_dm_names() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let user_id: Id<UserMarker> = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "Alice Global".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id,
                display_name: "Alice Global".to_owned(),
                username: Some("alice".to_owned()),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(3),
            author_id: user_id,
            author: "Alice Global".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::RelationshipsLoaded {
            relationships: vec![relationship_info(
                user_id.get(),
                FriendStatus::Friend,
                None,
                None,
                None,
            )],
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.name, "Alice Global");
        assert_eq!(channel.recipients[0].display_name, "Alice Global");
        assert_eq!(
            state.messages_for_channel(channel_id)[0].author,
            "Alice Global"
        );
    }

    #[test]
    fn relationship_nickname_refresh_preserves_explicit_group_dm_name() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "Alice Global".to_owned(),
                username: Some("alice".to_owned()),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::RelationshipsLoaded {
            relationships: vec![relationship_info(
                20,
                FriendStatus::Friend,
                Some("Bestie"),
                Some("Alice Global"),
                Some("alice"),
            )],
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.name, "project chat");
        assert_eq!(channel.recipients[0].display_name, "Bestie");
    }

    #[test]
    fn channel_upsert_preserves_recipient_status_when_omitted() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "renamed project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice renamed".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].display_name, "alice renamed");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
    }

    #[test]
    fn channel_upsert_defaults_missing_recipient_status_to_unknown() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Unknown);
    }

    #[test]
    fn channel_upsert_uses_cached_user_presence_when_status_is_omitted() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let user_id: Id<UserMarker> = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::UserPresenceUpdate {
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        });
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "test-user".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id,
                display_name: "test-user".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
        assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
    }

    #[test]
    fn user_presence_update_updates_channel_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::UserPresenceUpdate {
            user_id: Id::new(20),
            status: PresenceStatus::DoNotDisturb,
            activities: Vec::new(),
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::DoNotDisturb);
    }

    #[test]
    fn presence_update_caches_user_activities() {
        let mut state = DiscordState::default();
        let user_id: Id<UserMarker> = Id::new(20);
        let activity = ActivityInfo {
            kind: ActivityKind::Playing,
            name: "Concord".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        };

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: Id::new(1),
            user_id,
            status: PresenceStatus::Online,
            activities: vec![activity.clone()],
        });

        assert_eq!(
            state.user_activities(user_id),
            std::slice::from_ref(&activity)
        );

        // Empty activities array clears the cached entry.
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: Id::new(1),
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        });
        assert!(state.user_activities(user_id).is_empty());
    }

    #[test]
    fn guild_presence_activities_are_scoped_by_guild() {
        let mut state = DiscordState::default();
        let user_id: Id<UserMarker> = Id::new(20);
        let guild_a: Id<GuildMarker> = Id::new(1);
        let guild_b: Id<GuildMarker> = Id::new(2);
        let activity_a = ActivityInfo {
            kind: ActivityKind::Playing,
            name: "Guild A".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        };
        let activity_b = ActivityInfo {
            kind: ActivityKind::Listening,
            name: "Guild B".to_owned(),
            details: None,
            state: None,
            url: None,
            application_id: None,
            emoji: None,
        };

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: guild_a,
            user_id,
            status: PresenceStatus::Online,
            activities: vec![activity_a.clone()],
        });
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: guild_b,
            user_id,
            status: PresenceStatus::Idle,
            activities: vec![activity_b.clone()],
        });

        assert_eq!(
            state.user_presence_for_guild(Some(guild_a), user_id),
            Some(PresenceStatus::Online)
        );
        assert_eq!(
            state.user_presence_for_guild(Some(guild_b), user_id),
            Some(PresenceStatus::Idle)
        );
        assert_eq!(
            state.user_activities_for_guild(Some(guild_a), user_id),
            std::slice::from_ref(&activity_a)
        );
        assert_eq!(
            state.user_activities_for_guild(Some(guild_b), user_id),
            std::slice::from_ref(&activity_b)
        );
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: guild_a,
            user_id,
            status: PresenceStatus::DoNotDisturb,
            activities: Vec::new(),
        });

        assert!(
            state
                .user_activities_for_guild(Some(guild_a), user_id)
                .is_empty()
        );
        assert_eq!(
            state.user_activities_for_guild(Some(guild_b), user_id),
            std::slice::from_ref(&activity_b)
        );
    }

    #[test]
    fn guild_presence_update_updates_matching_channel_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: Id::new(1),
            user_id: Id::new(20),
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
    }

    #[test]
    fn bounds_messages_per_channel() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(1);

        for id in [1, 2] {
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: None,
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("message {id}")),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id.get(), 2);
    }

    #[test]
    fn stores_message_kind_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: Some("reply".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].message_kind, MessageKind::new(19));
    }

    #[test]
    fn duplicate_message_create_refreshes_message_kind() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("cached".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("cached"));
        assert_eq!(messages[0].message_kind, MessageKind::new(19));
    }

    #[test]
    fn duplicate_message_create_adds_missing_mentions() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
    }

    #[test]
    fn stores_reply_preview_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author_id: None,
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .map(|reply| reply.author.as_str()),
            Some("Alex")
        );
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .and_then(|reply| reply.content.as_deref()),
            Some("잘되는군")
        );
    }

    #[test]
    fn duplicate_message_create_preserves_cached_reply_preview() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author_id: None,
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .and_then(|reply| reply.content.as_deref()),
            Some("잘되는군")
        );
    }

    #[test]
    fn stores_poll_payload_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0].poll.as_ref().map(|poll| poll.question.as_str()),
            Some("오늘 뭐 먹지?")
        );
    }

    #[test]
    fn duplicate_message_create_preserves_cached_poll_payload() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].poll.as_ref().map(|poll| poll.answers.len()),
            Some(2)
        );
    }

    #[test]
    fn message_update_refreshes_cached_poll_results() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        let mut updated_poll = poll_info();
        updated_poll.results_finalized = Some(true);
        updated_poll.answers[0].vote_count = Some(5);
        updated_poll.answers[1].vote_count = Some(3);
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: Some(updated_poll),
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        let poll = messages[0].poll.as_ref().expect("poll should stay cached");
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].vote_count, Some(5));
        assert_eq!(poll.answers[1].vote_count, Some(3));
    }

    #[test]
    fn current_user_poll_vote_update_refreshes_cached_poll_counts() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: vec![2],
        });
        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(2));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(3));

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: Vec::new(),
        });
        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(1));
        assert!(!poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(2));
    }

    #[test]
    fn current_user_poll_vote_update_handles_missing_answer_counts() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();
        let mut poll = poll_info();
        poll.answers[1].vote_count = None;

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: vec![2],
        });

        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(1));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(3));
    }

    #[test]
    fn message_update_handles_mentions_tristate() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let cases = [
            (
                Vec::new(),
                Some(vec![mention_info(10, "alice")]),
                vec![mention_info(10, "alice")],
            ),
            (
                vec![mention_info(10, "alice")],
                None,
                vec![mention_info(10, "alice")],
            ),
            (
                vec![mention_info(10, "alice")],
                Some(Vec::new()),
                Vec::new(),
            ),
        ];

        for (initial_mentions, update_mentions, expected_mentions) in cases {
            let mut state = DiscordState::default();
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: None,
                channel_id,
                message_id,
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some("hello <@10>".to_owned()),
                sticker_names: Vec::new(),
                mentions: initial_mentions,
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
            state.apply_event(&AppEvent::MessageUpdate {
                guild_id: None,
                channel_id,
                message_id,
                poll: None,
                content: Some("hello".to_owned()),
                sticker_names: None,
                mentions: update_mentions,
                attachments: AttachmentUpdate::Unchanged,
                embeds: None,
                edited_timestamp: None,
            });

            assert_eq!(
                state.messages_for_channel(channel_id)[0].mentions,
                expected_mentions
            );
        }
    }

    #[test]
    fn message_capabilities_preserve_overlapping_traits() {
        let mut message = message_state("hello");
        assert_eq!(message.capabilities(), Default::default());

        message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
        let capabilities = message.capabilities();
        assert!(capabilities.has_image);
        assert!(!capabilities.has_poll);

        message.poll = Some(poll_info());
        let capabilities = message.capabilities();
        assert!(capabilities.has_image);
        assert!(capabilities.has_poll);
    }

    #[test]
    fn message_capabilities_expose_action_facets_for_chat_messages_only() {
        let mut message = message_state("system body");
        message.message_kind = MessageKind::new(19);
        message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
        message.poll = Some(poll_info());

        let capabilities = message.capabilities();
        assert!(capabilities.has_poll);
        assert!(capabilities.has_image);

        message.message_kind = MessageKind::new(7);
        message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
        message.poll = Some(poll_info());

        let capabilities = message.capabilities();
        assert!(!capabilities.has_poll);
        assert!(!capabilities.has_image);
    }

    #[test]
    fn message_capabilities_track_reply_and_forwarded_traits() {
        let mut message = message_state("reply body");
        message.reply = Some(ReplyInfo {
            author_id: None,
            author: "neo".to_owned(),
            content: Some("original".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
        });
        message.forwarded_snapshots = vec![snapshot_info("forwarded")];

        let capabilities = message.capabilities();

        assert!(capabilities.is_reply);
        assert!(capabilities.is_forwarded);
    }

    #[test]
    fn keeps_known_content_when_gateway_echo_has_no_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(30);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("hello"));
    }

    #[test]
    fn merges_history_in_chronological_order() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("live".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 20, "history 20"),
                message_info(channel_id, 10, "history 10"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn history_merge_preserves_message_reference() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let reference = MessageReferenceInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Some(Id::new(20)),
            message_id: Some(Id::new(30)),
        };

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reference: Some(reference.clone()),
                ..message_info(channel_id, 20, "history")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].reference, Some(reference));
    }

    #[test]
    fn history_dedupes_and_preserves_known_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("known".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                pinned: false,
                reactions: Vec::new(),
                content: Some(String::new()),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("known"));
    }

    #[test]
    fn pinned_messages_loaded_stay_out_of_normal_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "latest")],
        });
        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 5, "old pin")],
        });

        assert_eq!(
            state
                .messages_for_channel(channel_id)
                .into_iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![20]
        );
        assert_eq!(
            state
                .pinned_messages_for_channel(channel_id)
                .into_iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![5]
        );
    }

    #[test]
    fn pinned_messages_loaded_mark_overlapping_normal_messages() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "normal")],
        });
        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "normal")],
        });

        assert!(state.messages_for_channel(channel_id)[0].pinned);
        assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);
    }

    #[test]
    fn later_history_preserves_pin_state_from_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "pin")],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "pin")],
        });

        assert!(state.messages_for_channel(channel_id)[0].pinned);
    }

    #[test]
    fn message_pinned_update_updates_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "normal")],
        });
        state.apply_event(&AppEvent::MessagePinnedUpdate {
            channel_id,
            message_id: Id::new(20),
            pinned: true,
        });
        assert!(state.messages_for_channel(channel_id)[0].pinned);
        assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

        state.apply_event(&AppEvent::MessagePinnedUpdate {
            channel_id,
            message_id: Id::new(20),
            pinned: false,
        });
        assert!(!state.messages_for_channel(channel_id)[0].pinned);
        assert!(state.pinned_messages_for_channel(channel_id).is_empty());
    }

    #[test]
    fn reaction_events_update_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let emoji = ReactionEmoji::Unicode("👍".to_owned());

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "pin")],
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });

        let pinned = state.pinned_messages_for_channel(channel_id)[0];
        assert_eq!(pinned.reactions.len(), 1);
        assert_eq!(pinned.reactions[0].emoji, emoji);
        assert_eq!(pinned.reactions[0].count, 1);

        state.apply_event(&AppEvent::MessageReactionRemoveAll {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
        });
        assert!(
            state.pinned_messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn poll_vote_updates_update_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let mut message = message_info(channel_id, 20, "poll");
        message.poll = Some(poll_info());

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message],
        });
        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id: Id::new(20),
            answer_ids: vec![2],
        });

        let poll = state.pinned_messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("pinned poll should stay cached");
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(2));
        assert_eq!(poll.total_votes, Some(3));
    }

    #[test]
    fn history_merge_replaces_mentions_from_authoritative_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                mentions: vec![mention_info(10, "alice")],
                ..message_info(channel_id, 20, "hello <@10>")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "hello")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert!(messages[0].mentions.is_empty());
    }

    #[test]
    fn history_merge_preserves_richer_gateway_mention_display_name() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "global alias")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                mentions: vec![mention_info(10, "username")],
                ..message_info(channel_id, 20, "hello <@10>")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "global alias")]);
    }

    #[test]
    fn history_merge_clears_reactions_from_authoritative_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: vec![ReactionInfo {
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    count: 2,
                    me: true,
                }],
                ..message_info(channel_id, 20, "hello")
            }],
        });
        assert_eq!(state.messages_for_channel(channel_id)[0].reactions.len(), 1);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: Vec::new(),
                ..message_info(channel_id, 20, "hello")
            }],
        });

        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn stores_and_merges_message_attachments() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                pinned: false,
                reactions: Vec::new(),
                content: Some(String::new()),
                attachments: Vec::new(),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(messages[0].attachments[0].filename, "cat.png");
    }

    #[test]
    fn stores_forwarded_snapshots_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![snapshot_info("forwarded text")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].forwarded_snapshots.len(), 1);
        assert_eq!(
            messages[0].forwarded_snapshots[0].content.as_deref(),
            Some("forwarded text")
        );
    }

    #[test]
    fn history_merge_preserves_existing_forwarded_snapshots() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![snapshot_info("live snapshot")],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0].forwarded_snapshots[0].content.as_deref(),
            Some("live snapshot")
        );
    }

    #[test]
    fn message_update_handles_attachment_update_tristate() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let cases = [
            (AttachmentUpdate::Unchanged, 1),
            (AttachmentUpdate::Replace(Vec::new()), 0),
        ];

        for (attachments, expected_len) in cases {
            let mut state = DiscordState::default();
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: None,
                channel_id,
                message_id: Id::new(20),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(String::new()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: vec![attachment_info(1, "cat.png", "image/png")],
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
            state.apply_event(&AppEvent::MessageUpdate {
                guild_id: None,
                channel_id,
                message_id: Id::new(20),
                poll: None,
                content: None,
                sticker_names: None,
                mentions: None,
                attachments,
                embeds: None,
                edited_timestamp: None,
            });

            let messages = state.messages_for_channel(channel_id);
            assert_eq!(messages[0].attachments.len(), expected_len);
            if expected_len == 1 {
                assert_eq!(messages[0].attachments[0].filename, "cat.png");
            }
        }
    }

    #[test]
    fn history_respects_message_limit_after_merge() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(2);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 20, "middle"),
                message_info(channel_id, 30, "new"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![20, 30]
        );
    }

    #[test]
    fn older_history_preserves_existing_messages_when_message_limit_is_reached() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(3);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5, "older")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![5, 10, 11, 12]
        );
    }

    #[test]
    fn older_history_is_bounded_by_extra_window() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(3);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![
                message_info(channel_id, 1, "older 1"),
                message_info(channel_id, 2, "older 2"),
                message_info(channel_id, 3, "older 3"),
                message_info(channel_id, 4, "older 4"),
                message_info(channel_id, 5, "older 5"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 6);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 10]
        );
    }

    #[test]
    fn live_message_after_older_history_keeps_newer_window() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(4);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5, "older")],
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(13),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("newest".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![10, 11, 12, 13]
        );
    }

    #[test]
    fn live_messages_update_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("new".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(10),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("old".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(
            state
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id),
            Some(Id::new(30))
        );
    }

    #[test]
    fn live_thread_messages_increment_cached_counts_once() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        for _ in 0..2 {
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: Some(Id::new(1)),
                channel_id,
                message_id: Id::new(30),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some("new".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(30)),
            messages: vec![message_info(channel_id, 20, "old")],
        });

        let channel = state
            .channel(channel_id)
            .expect("thread should stay cached");
        assert_eq!(channel.message_count, Some(13));
        assert_eq!(channel.total_message_sent, Some(15));
    }

    #[test]
    fn history_updates_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 40, "new"),
            ],
        });

        assert_eq!(
            state
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id),
            Some(Id::new(40))
        );
    }

    #[test]
    fn channel_upsert_does_not_regress_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "neo renamed".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo renamed again".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.name, "neo renamed again");
        assert_eq!(channel.last_message_id, Some(Id::new(30)));
    }

    #[test]
    fn channel_delete_removes_cached_thread() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelDelete {
            guild_id: Some(guild_id),
            channel_id,
        });

        assert_eq!(state.channel(channel_id), None);
    }

    #[test]
    fn tracks_members_and_presences() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(100),
            channels: Vec::new(),
            members: vec![
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            ],
            presences: vec![(alice, PresenceStatus::Online)],
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(100));
        assert_eq!(members.len(), 2);
        let alice_state = members.iter().find(|m| m.user_id == alice).unwrap();
        assert_eq!(alice_state.status, PresenceStatus::Online);
        let bob_state = members.iter().find(|m| m.user_id == bob).unwrap();
        assert_eq!(bob_state.status, PresenceStatus::Unknown);

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: bob,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        });
        assert_eq!(
            state
                .members_for_guild(guild_id)
                .iter()
                .find(|m| m.user_id == bob)
                .unwrap()
                .status,
            PresenceStatus::Idle,
        );
    }

    #[test]
    fn presence_update_does_not_create_fallback_member() {
        let guild_id = Id::new(1);
        let user_id = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(100),
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        });

        assert!(state.members_for_guild(guild_id).is_empty());
        assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
    }

    #[test]
    fn real_member_add_and_remove_update_known_member_count() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(1),
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id: alice,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

        state.apply_event(&AppEvent::GuildMemberAdd {
            guild_id,
            member: MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

        state.apply_event(&AppEvent::GuildMemberAdd {
            guild_id,
            member: MemberInfo {
                user_id: Id::new(30),
                display_name: "carol".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));

        state.apply_event(&AppEvent::GuildMemberRemove {
            guild_id,
            user_id: Id::new(30),
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));
    }

    #[test]
    fn guild_member_remove_decrements_known_count_for_unloaded_member() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(3),
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        state.apply_event(&AppEvent::GuildMemberRemove {
            guild_id,
            user_id: Id::new(99),
        });

        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));
        assert!(state.members_for_guild(guild_id).is_empty());
    }

    #[test]
    fn guild_create_caches_roles_and_member_role_ids() {
        let guild_id = Id::new(1);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![role_id],
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Admin".to_owned(),
                color: Some(0xFFAA00),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });

        let roles = state.roles_for_guild(guild_id);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].name, "Admin");
        let members = state.members_for_guild(guild_id);
        assert_eq!(members[0].role_ids, vec![role_id]);
    }

    #[test]
    fn message_author_role_color_uses_history_author_roles_when_member_is_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        message.author_role_ids = vec![role_id];
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_uses_live_author_roles_when_member_is_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id: user_id,
            author: "test-user".to_owned(),
            author_avatar_url: None,
            author_role_ids: vec![role_id],
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_uses_profile_roles_when_message_roles_are_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });
        let mut profile = profile_info(user_id.get(), Some("test-user"));
        profile.role_ids = vec![role_id];
        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_id),
            profile,
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_does_not_use_message_roles_when_member_is_cached() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let stale_role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id,
                display_name: "test-user".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: stale_role_id,
                name: "Old Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        message.author_role_ids = vec![stale_role_id];
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            None
        );
    }

    #[test]
    fn chunk_style_member_upserts_populate_member_list() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        for (user_id, display_name) in [(alice, "alice"), (bob, "bob")] {
            state.apply_event(&AppEvent::GuildMemberUpsert {
                guild_id,
                member: MemberInfo {
                    user_id,
                    display_name: display_name.to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            });
        }
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: alice,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(members.len(), 2);
        assert_eq!(
            members
                .iter()
                .find(|member| member.user_id == alice)
                .map(|member| member.status),
            Some(PresenceStatus::Online)
        );
        assert_eq!(
            members
                .iter()
                .find(|member| member.user_id == bob)
                .map(|member| member.status),
            Some(PresenceStatus::Unknown)
        );
    }

    #[test]
    fn stores_and_clears_custom_guild_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: true,
                available: true,
            }],
            owner_id: None,
        });

        assert_eq!(state.custom_emojis_for_guild(guild_id).len(), 1);
        assert_eq!(state.custom_emojis_for_guild(guild_id)[0].name, "party");

        state.apply_event(&AppEvent::GuildDelete { guild_id });

        assert!(state.custom_emojis_for_guild(guild_id).is_empty());
    }

    #[test]
    fn guild_emojis_update_replaces_cached_custom_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildEmojisUpdate {
            guild_id,
            emojis: vec![CustomEmojiInfo {
                id: Id::new(60),
                name: "wave".to_owned(),
                animated: true,
                available: true,
            }],
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(60));
        assert_eq!(emojis[0].name, "wave");
        assert!(emojis[0].animated);
    }

    #[test]
    fn guild_update_replaces_custom_emojis_when_field_is_present() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id,
            name: "guild renamed".to_owned(),
            roles: None,
            emojis: Some(vec![CustomEmojiInfo {
                id: Id::new(70),
                name: "dance".to_owned(),
                animated: true,
                available: true,
            }]),
            owner_id: None,
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(70));
        assert_eq!(emojis[0].name, "dance");
    }

    #[test]
    fn guild_update_without_emoji_field_keeps_cached_custom_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id,
            name: "guild renamed".to_owned(),
            roles: None,
            emojis: None,
            owner_id: None,
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].name, "party");
    }

    #[test]
    fn member_upsert_preserves_existing_status() {
        let guild_id = Id::new(1);
        let user = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: user,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice-renamed".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let member = state
            .members_for_guild(guild_id)
            .into_iter()
            .find(|m| m.user_id == user)
            .unwrap();
        assert_eq!(member.display_name, "alice-renamed");
        assert_eq!(member.status, PresenceStatus::Online);
    }

    #[test]
    fn current_user_reaction_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserReactionAdd {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        });
        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(message.reactions[0].me);

        state.apply_event(&AppEvent::CurrentUserReactionRemove {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        });
        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn gateway_reaction_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let emoji = ReactionEmoji::Unicode("👍".to_owned());
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(51),
            emoji: emoji.clone(),
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 2);
        assert!(!message.reactions[0].me);

        state.apply_event(&AppEvent::MessageReactionRemove {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(!message.reactions[0].me);
    }

    #[test]
    fn current_user_gateway_reaction_events_reconcile_optimistic_updates() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let current_user_id = Id::new(7);
        let emoji = ReactionEmoji::Unicode("👍".to_owned());
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserReactionAdd {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: current_user_id,
            emoji: emoji.clone(),
        });
        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions[0].count, 1);
        assert!(message.reactions[0].me);

        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::CurrentUserReactionRemove {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionRemove {
            guild_id: None,
            channel_id,
            message_id,
            user_id: current_user_id,
            emoji,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(!message.reactions[0].me);
    }

    #[test]
    fn gateway_reaction_clear_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let thumbs_up = ReactionEmoji::Unicode("👍".to_owned());
        let party = ReactionEmoji::Unicode("🎉".to_owned());
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: vec![
                    ReactionInfo {
                        emoji: thumbs_up.clone(),
                        count: 2,
                        me: true,
                    },
                    ReactionInfo {
                        emoji: party,
                        count: 1,
                        me: false,
                    },
                ],
                ..message_info(channel_id, message_id.get(), "hello")
            }],
        });

        state.apply_event(&AppEvent::MessageReactionRemoveEmoji {
            guild_id: None,
            channel_id,
            message_id,
            emoji: thumbs_up,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(
            message.reactions[0].emoji,
            ReactionEmoji::Unicode("🎉".to_owned())
        );

        state.apply_event(&AppEvent::MessageReactionRemoveAll {
            guild_id: None,
            channel_id,
            message_id,
        });

        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
    const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
    const MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
    const ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
    const READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
    const ADMINISTRATOR: u64 = 0x0000_0000_0000_0008;
    const ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
    const PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;

    fn perm_role(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
        PermissionOverwriteInfo {
            id,
            kind: PermissionOverwriteKind::Role,
            allow,
            deny,
        }
    }

    fn perm_member(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
        PermissionOverwriteInfo {
            id,
            kind: PermissionOverwriteKind::Member,
            allow,
            deny,
        }
    }

    /// Build a single-guild state with one text channel, one member, and the
    /// given role permissions / channel overwrites. The current user is set
    /// from `READY` so permission lookups have an identity to consult.
    fn guild_with_permissions(
        owner_id: Id<UserMarker>,
        my_id: Id<UserMarker>,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        my_role_ids: Vec<Id<RoleMarker>>,
        roles: Vec<RoleInfo>,
        overwrites: Vec<PermissionOverwriteInfo>,
    ) -> DiscordState {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(my_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: Some(owner_id),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: overwrites,
            }],
            members: vec![MemberInfo {
                user_id: my_id,
                display_name: "me".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: my_role_ids,
            }],
            presences: Vec::new(),
            roles,
            emojis: Vec::new(),
        });
        state
    }

    #[test]
    fn dm_channels_are_always_viewable() {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id: Id::new(99),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let channels = state.viewable_channels_for_guild(None);
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn guild_owner_sees_everything_even_when_everyone_denies() {
        let me = Id::new(10);
        let guild = Id::new(1);
        let channel = Id::new(2);
        // @everyone explicitly denies VIEW_CHANNEL, but the owner short-circuit
        // must still grant access.
        let state = guild_with_permissions(
            me,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: 0,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn administrator_role_bypasses_channel_overwrites() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let admin_role = Id::new(50);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![admin_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: admin_role,
                    name: "Admin".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: ADMINISTRATOR,
                },
            ],
            // Channel-level deny is irrelevant for ADMINISTRATOR holders.
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn everyone_deny_hides_channel_for_plain_member() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        // @everyone has VIEW_CHANNEL by default, but the channel-level
        // overwrite revokes it for a plain member.
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
    }

    #[test]
    fn role_allow_overrides_everyone_deny() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let staff_role = Id::new(50);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: 0,
                },
            ],
            vec![
                perm_role(guild.get(), 0, VIEW_CHANNEL),
                perm_role(staff_role.get(), VIEW_CHANNEL, 0),
            ],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn current_user_roles_handle_partial_and_complete_member_upserts() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let staff_role = Id::new(50);
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
            ],
            Vec::new(),
        );
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id: guild,
            member: MemberInfo {
                user_id: me,
                display_name: "unknown".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild),
            channel_id: channel,
            message_id: Id::new(100),
            author_id: Id::new(99),
            author: "sender".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("hello <@&{}>", staff_role.get())),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        assert_eq!(
            state.channel_unread(channel),
            ChannelUnreadState::Mentioned(1)
        );

        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
            ],
            Vec::new(),
        );
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id: guild,
            member: MemberInfo {
                user_id: me,
                display_name: "me".to_owned(),
                username: Some("me".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
    }

    #[test]
    fn member_overwrite_has_the_final_word() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let staff_role = Id::new(50);
        // Role-level grants VIEW, but the member-specific deny removes it.
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
            ],
            vec![perm_member(me.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
    }

    #[test]
    fn threads_inherit_parent_permission() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let parent = Id::new(2);
        let thread = Id::new(3);
        // Parent denies VIEW_CHANNEL. The thread carries no overwrites of its
        // own and must inherit the same answer.
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            parent,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(parent),
            position: None,
            last_message_id: None,
            name: "design-discussion".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
    }

    #[test]
    fn message_create_for_hidden_channel_does_not_promote_it() {
        // Regression guard: a MESSAGE_CREATE for a permission-hidden channel
        // must not flip the channel into the visible bucket. The message
        // itself is still tracked (it's a real Discord message), but the
        // sidebar must keep filtering the channel out and the visibility
        // stats must continue to count it as hidden.
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );

        // Sanity check: starts hidden.
        assert_eq!(
            state.channel_visibility_stats(Some(guild)),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());

        // A message arrives for the hidden channel with the same author as a
        // legitimate Discord push.
        let message_id = Id::new(900);
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild),
            channel_id: channel,
            message_id,
            author_id: owner,
            author: "owner".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::default(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hidden chatter".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        // The channel must remain hidden because no permission promotion happened.
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
        assert_eq!(
            state.channel_visibility_stats(Some(guild)),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        // The underlying channel record still exists and the message was
        // stored. Gating is a sidebar concern, not a data-purge concern.
        assert!(state.channel(channel).is_some());
        assert_eq!(state.messages_for_channel(channel).len(), 1);
    }

    #[test]
    fn cannot_send_when_role_overwrite_denies_send_messages() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                // VIEW + SEND globally, but channel overwrite revokes SEND.
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            vec![perm_role(guild.get(), 0, SEND_MESSAGES)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
    }

    #[test]
    fn cannot_send_when_view_channel_is_denied() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
    }

    #[test]
    fn cannot_attach_when_role_overwrite_denies_attach_files() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                // VIEW + SEND + ATTACH globally, channel revokes only ATTACH.
                permissions: VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
            }],
            vec![perm_role(guild.get(), 0, ATTACH_FILES)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_send_in_channel(ch));
        assert!(!state.can_attach_in_channel(ch));
    }

    #[test]
    fn cannot_attach_when_send_messages_is_missing() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | ATTACH_FILES,
            }],
            Vec::new(),
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
        assert!(!state.can_attach_in_channel(ch));
    }

    #[test]
    fn manage_messages_requires_explicit_guild_permission() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | MANAGE_MESSAGES,
            }],
            Vec::new(),
        );

        let ch = state.channel(channel).expect("channel");
        assert!(state.can_manage_messages_in_channel(ch));
    }

    #[test]
    fn manage_messages_defaults_permissive_while_guild_member_roles_hydrate() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(me),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id: guild,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: Some(owner),
            channels: vec![ChannelInfo {
                guild_id: Some(guild),
                channel_id: channel,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            emojis: Vec::new(),
        });

        let ch = state.channel(channel).expect("channel");
        assert!(state.can_manage_messages_in_channel(ch));
    }

    #[test]
    fn manage_messages_is_never_granted_for_dm_channels() {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id: Id::new(99),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let ch = state.channel(Id::new(99)).expect("channel");
        assert!(!state.can_manage_messages_in_channel(ch));
    }

    #[test]
    fn pin_and_reaction_helpers_use_documented_permission_bits() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | READ_MESSAGE_HISTORY | ADD_REACTIONS | PIN_MESSAGES,
            }],
            Vec::new(),
        );

        let ch = state.channel(channel).expect("channel");
        assert!(state.can_read_message_history_in_channel(ch));
        assert!(state.can_add_reactions_in_channel(ch));
        assert!(state.can_pin_messages_in_channel(ch));
    }

    #[test]
    fn owner_can_send_and_attach_unconditionally() {
        let me = Id::new(10);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            me,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: 0,
            }],
            vec![perm_role(
                guild.get(),
                0,
                VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
            )],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_send_in_channel(ch));
        assert!(state.can_attach_in_channel(ch));
    }

    #[test]
    fn private_threads_are_hidden_without_membership_state() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let parent = Id::new(2);
        let thread = Id::new(3);
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            parent,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            Vec::new(),
        );
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(parent),
            position: None,
            last_message_id: None,
            name: "private planning".to_owned(),
            kind: "GuildPrivateThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
        assert!(!state.can_send_in_channel(thread_state));
    }

    #[test]
    fn private_threads_are_hidden_while_permission_state_is_missing() {
        let guild = Id::new(1);
        let thread = Id::new(3);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "private planning".to_owned(),
            kind: "GuildPrivateThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
    }

    #[test]
    fn channel_visibility_stats_count_only_top_level() {
        // Threads should not skew the stats. The user navigates by channel, and
        // a thread under a hidden parent already inherits the parent's visibility.
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let visible_channel = Id::new(2);
        let hidden_channel = Id::new(3);
        let visible_thread = Id::new(20);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(me),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id: guild,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: Some(owner),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: visible_channel,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "GuildText".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: hidden_channel,
                    parent_id: None,
                    position: Some(1),
                    last_message_id: None,
                    name: "secret".to_owned(),
                    kind: "GuildText".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
                },
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: visible_thread,
                    parent_id: Some(visible_channel),
                    position: None,
                    last_message_id: None,
                    name: "design".to_owned(),
                    kind: "GuildPublicThread".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: Some(false),
                    thread_locked: Some(false),
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: vec![MemberInfo {
                user_id: me,
                display_name: "me".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            emojis: Vec::new(),
        });

        let stats = state.channel_visibility_stats(Some(guild));
        assert_eq!(
            stats,
            ChannelVisibilityStats {
                visible: 1,
                hidden: 1,
            },
            "expected the thread to be excluded from both buckets"
        );
    }

    #[test]
    fn missing_current_user_id_falls_back_to_visible() {
        // Until READY arrives we cannot decide. Be permissive so the sidebar is
        // not empty during the brief window between connect and READY.
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::GuildCreate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            member_count: None,
            owner_id: Some(Id::new(99)),
            channels: vec![ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(2),
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: vec![perm_role(1, 0, VIEW_CHANNEL)],
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: Id::new(1),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            emojis: Vec::new(),
        });
        let ch = state.channel(Id::new(2)).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
        MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(content.to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            ..MessageInfo::default()
        }
    }

    fn message_state(content: &str) -> MessageState {
        MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            ..MessageState::default()
        }
    }

    fn attachment_info(
        id: u64,
        filename: &str,
        content_type: &str,
    ) -> crate::discord::AttachmentInfo {
        crate::discord::AttachmentInfo {
            id: Id::new(id),
            filename: filename.to_owned(),
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: Some(content_type.to_owned()),
            size: 1000,
            width: Some(100),
            height: Some(100),
            description: None,
        }
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: None,
            display_name: display_name.to_owned(),
        }
    }

    fn poll_info() -> PollInfo {
        PollInfo {
            question: "오늘 뭐 먹지?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "김치찌개".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "라멘".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect: false,
            results_finalized: Some(false),
            total_votes: Some(3),
        }
    }

    fn snapshot_info(content: &str) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(content.to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn channel_with_last_message(
        channel_id: Id<ChannelMarker>,
        last_message_id: u64,
    ) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(last_message_id)),
            name: "general".to_owned(),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }

    #[test]
    fn channel_unread_state_follows_ack_pointer() {
        let cases = [
            (100, None, ChannelUnreadState::Unread),
            (200, Some(150), ChannelUnreadState::Unread),
            (200, Some(200), ChannelUnreadState::Seen),
        ];

        for (latest_message_id, last_acked_message_id, expected) in cases {
            let channel_id = Id::new(7);
            let mut state = DiscordState::default();
            state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
                channel_id,
                latest_message_id,
            )));
            if let Some(last_acked_message_id) = last_acked_message_id {
                state.apply_event(&AppEvent::ReadStateInit {
                    entries: vec![ReadStateInfo {
                        channel_id,
                        last_acked_message_id: Some(Id::new(last_acked_message_id)),
                        mention_count: 0,
                    }],
                });
            }

            assert_eq!(state.channel_unread(channel_id), expected);
        }
    }

    #[test]
    fn current_user_message_create_keeps_channel_seen() {
        let channel_id = Id::new(7);
        let current_user_id = Id::new(10);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 100,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 0,
            }],
        });

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id,
            message_id: Id::new(200),
            author_id: current_user_id,
            author: "me".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("sent from this account".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
        assert_eq!(state.channel_ack_target(channel_id), None);
        assert_eq!(state.channel_unread_message_count(channel_id), 0);
    }

    #[test]
    fn channel_with_pending_mentions_reports_mention_count() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 200,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(200)),
                mention_count: 3,
            }],
        });

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Mentioned(3)
        );
    }

    #[test]
    fn guild_unread_sums_channel_mentions_before_plain_unread() {
        let first_channel_id = Id::new(7);
        let second_channel_id = Id::new(8);
        let third_channel_id = Id::new(9);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            first_channel_id,
            200,
        )));
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            second_channel_id,
            300,
        )));
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            third_channel_id,
            400,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![
                ReadStateInfo {
                    channel_id: first_channel_id,
                    last_acked_message_id: Some(Id::new(200)),
                    mention_count: 2,
                },
                ReadStateInfo {
                    channel_id: second_channel_id,
                    last_acked_message_id: Some(Id::new(300)),
                    mention_count: 3,
                },
                ReadStateInfo {
                    channel_id: third_channel_id,
                    last_acked_message_id: Some(Id::new(100)),
                    mention_count: 0,
                },
            ],
        });

        assert_eq!(
            state.guild_unread(Id::new(1)),
            ChannelUnreadState::Mentioned(5)
        );
    }

    #[test]
    fn message_ack_clears_outstanding_mentions_and_advances_pointer() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 500,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 5,
            }],
        });
        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Mentioned(5)
        );

        state.apply_event(&AppEvent::MessageAck {
            channel_id,
            message_id: Id::new(500),
            mention_count: 0,
        });

        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
        assert_eq!(
            state.channel_ack_target(channel_id),
            None,
            "fully-acked channels need no further ack"
        );
    }

    #[test]
    fn channel_ack_target_returns_latest_when_unread() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 500,
        )));

        // No ack pointer at all -> ack target is the channel's last message.
        assert_eq!(state.channel_ack_target(channel_id), Some(Id::new(500)));
    }
}
