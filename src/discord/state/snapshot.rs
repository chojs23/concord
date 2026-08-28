//! Snapshot and revision machinery: per-area revision counters the event
//! publisher bumps, and the shared caches the TUI reads lazily.

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
    pub(in crate::discord) navigation: Arc<NavigationIndex>,
    pub(in crate::discord) guild_details: Arc<GuildDetailCache>,
    pub(in crate::discord) profiles: Arc<ProfileCache>,
    pub(in crate::discord) presence: Arc<PresenceCache>,
    pub(in crate::discord) voice: Arc<VoiceStateCache>,
    pub(in crate::discord) session: Arc<SessionState>,
    pub(in crate::discord) threads: Arc<ThreadCache>,
}

#[derive(Clone, Debug)]
pub struct MessageSnapshot {
    pub(in crate::discord) message_cache: Arc<MessageCache>,
}

#[derive(Clone, Debug)]
pub struct DetailSnapshot {
    pub(in crate::discord) notifications: Arc<NotificationCache>,
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

    pub(in crate::discord) const fn message_and_detail() -> Self {
        Self {
            navigation: false,
            message: true,
            detail: true,
        }
    }
}

impl DiscordSnapshot {
    pub(crate) fn thread_card_catalog_changed_from(&self, state: &DiscordState) -> bool {
        !Arc::ptr_eq(&self.navigation.navigation, &state.navigation)
            || !Arc::ptr_eq(&self.navigation.threads, &state.threads)
    }

    pub fn to_state(&self) -> DiscordState {
        let mut state = DiscordState::default();
        state.attach_snapshot_areas(self, SnapshotAreas::all());
        state
    }
}
