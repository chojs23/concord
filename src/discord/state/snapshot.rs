//! Snapshot and revision machinery: per-area revision counters the event
//! publisher bumps, and the cloned snapshots the TUI reads lazily.

use super::*;

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
    pub(in crate::discord) navigation: NavigationIndex,
    pub(in crate::discord) guild_details: GuildDetailCache,
    pub(in crate::discord) profiles: ProfileCache,
    pub(in crate::discord) presence: PresenceCache,
    pub(in crate::discord) voice: VoiceStateCache,
    pub(in crate::discord) session: SessionState,
    pub(in crate::discord) notification_settings:
        BTreeMap<Id<GuildMarker>, GuildNotificationSettingsState>,
    pub(in crate::discord) private_notification_settings: Option<GuildNotificationSettingsState>,
}

#[derive(Clone, Debug)]
pub struct MessageSnapshot {
    pub(in crate::discord) message_cache: MessageCache,
}

#[derive(Clone, Debug)]
pub struct DetailSnapshot {
    pub(in crate::discord) read_states: BTreeMap<Id<ChannelMarker>, ChannelReadState>,
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

    pub(in crate::discord) const fn navigation() -> Self {
        Self {
            navigation: true,
            message: false,
            detail: false,
        }
    }

    pub(in crate::discord) const fn message() -> Self {
        Self {
            navigation: false,
            message: true,
            detail: false,
        }
    }

    pub(in crate::discord) const fn navigation_and_message() -> Self {
        Self {
            navigation: true,
            message: true,
            detail: false,
        }
    }

    pub(in crate::discord) const fn navigation_and_detail() -> Self {
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
        state.voice = self.navigation.voice.clone();
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
    pub voice_states: usize,
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
             typing_channels={} voice_states={} read_states={} notification_settings={} \
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
            self.voice_states,
            self.read_states,
            self.notification_settings,
            self.has_private_notification_settings,
        )
    }
}
