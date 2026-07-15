//! The per-area caches `DiscordState` is composed of. Splitting them out
//! keeps the root focused on event application and mutation.

use super::*;

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct NavigationIndex {
    pub(in crate::discord) guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    pub(in crate::discord) channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    pub(in crate::discord) thread_creators: BTreeMap<Id<ChannelMarker>, ThreadCreatorState>,
    pub(in crate::discord) custom_emojis: BTreeMap<Id<GuildMarker>, Vec<CustomEmojiInfo>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it. The dashboard falls back to a flat guild list.
    pub(in crate::discord) guild_folders: Vec<GuildFolder>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadCreatorState {
    pub guild_id: Option<Id<GuildMarker>>,
    pub user_id: Id<UserMarker>,
}

#[derive(Clone, Debug)]
pub(in crate::discord) struct MessageCache {
    pub(in crate::discord) messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    pub(in crate::discord) message_gaps: BTreeMap<Id<ChannelMarker>, Vec<MessageHistoryGap>>,
    pub(in crate::discord) cold_message_channels: BTreeSet<Id<ChannelMarker>>,
    pub(in crate::discord) warm_message_channels: VecDeque<Id<ChannelMarker>>,
    pub(in crate::discord) pinned_messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    pub(in crate::discord) message_author_role_ids: MessageAuthorRoleIds,
    pub(in crate::discord) max_messages_per_channel: usize,
    pub(in crate::discord) max_warm_message_channels: usize,
}

impl MessageCache {
    pub(super) fn new(max_messages_per_channel: usize) -> Self {
        Self {
            messages: BTreeMap::new(),
            message_gaps: BTreeMap::new(),
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
pub(in crate::discord) struct GuildDetailCache {
    pub(in crate::discord) members:
        BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    pub(in crate::discord) member_cache_guild_order: VecDeque<Id<GuildMarker>>,
    pub(in crate::discord) roles: BTreeMap<Id<GuildMarker>, BTreeMap<Id<RoleMarker>, RoleState>>,
    pub(in crate::discord) current_user_role_ids: BTreeMap<Id<GuildMarker>, Vec<Id<RoleMarker>>>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct ProfileCache {
    pub(in crate::discord) profile_role_ids: ProfileRoleIds,
    /// Cached profile lookups so the profile popup can render instantly when
    /// the same user is opened again.
    pub(in crate::discord) user_profiles: BTreeMap<UserProfileCacheKey, UserProfileInfo>,
    pub(in crate::discord) profile_cache_order: VecDeque<UserProfileCacheKey>,
    pub(in crate::discord) fetched_notes: BTreeMap<Id<UserMarker>, Option<String>>,
    pub(in crate::discord) fetched_note_order: VecDeque<Id<UserMarker>>,
    /// Friend / blocked / pending request state delivered through READY's
    /// `relationships` array. Used to colour the profile popup's friend
    /// indicator and to enrich `UserProfileInfo` on insert.
    pub(in crate::discord) relationships: BTreeMap<Id<UserMarker>, RelationshipInfo>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct PresenceCache {
    /// Guild-scoped presence and activity. These are keyed by both guild and
    /// user so evicting an old guild can drop its display-heavy presence data
    /// without affecting the same user's DM fallback or another guild.
    pub(in crate::discord) guild_user_presences:
        BTreeMap<(Id<GuildMarker>, Id<UserMarker>), PresenceStatus>,
    pub(in crate::discord) guild_user_activities:
        BTreeMap<(Id<GuildMarker>, Id<UserMarker>), Vec<ActivityInfo>>,
    /// Last known global presence by user id. This gives DM/profile views a
    /// fallback when the private-channel recipient payload omitted status.
    pub(in crate::discord) user_presences: BTreeMap<Id<UserMarker>, PresenceStatus>,
    pub(in crate::discord) user_activities: BTreeMap<Id<UserMarker>, Vec<ActivityInfo>>,
    /// Most recent TYPING_START arrival per (channel, user). Discord renews
    /// the indicator every ~10 seconds. Readers filter stale entries, and the
    /// next typing event for a channel prunes its expired entries.
    pub(in crate::discord) typing:
        BTreeMap<Id<ChannelMarker>, BTreeMap<Id<UserMarker>, TypingIndicator>>,
}

#[derive(Clone, Debug)]
pub(in crate::discord) struct TypingIndicator {
    pub(in crate::discord) started: Instant,
    pub(in crate::discord) display_name: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct VoiceStateCache {
    pub(in crate::discord) states:
        BTreeMap<(VoiceScope, Id<UserMarker>), crate::discord::voice::VoiceState>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct SessionState {
    /// Snowflake of the authenticated user. Captured from the READY payload
    /// and consulted by `can_view_channel` to look up our own roles and
    /// match member-level permission overwrites.
    pub(in crate::discord) current_user_id: Option<Id<UserMarker>>,
    pub(in crate::discord) current_user: Option<String>,
    pub(in crate::discord) current_user_premium_tier: Option<PremiumTier>,
    pub(in crate::discord) current_user_email_verified: Option<bool>,
    pub(in crate::discord) current_user_phone_verified: Option<bool>,
    pub(in crate::discord) current_user_mfa_enabled: Option<bool>,
    pub(in crate::discord) selected_message_channel_known: bool,
    pub(in crate::discord) selected_message_channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct NotificationCache {
    pub(in crate::discord) read_states: BTreeMap<Id<ChannelMarker>, ChannelReadState>,
    pub(in crate::discord) notification_settings:
        BTreeMap<Id<GuildMarker>, GuildNotificationSettingsState>,
    pub(in crate::discord) private_notification_settings: Option<GuildNotificationSettingsState>,
}
