use std::collections::BTreeMap;

use crate::discord::UserProfileInfo;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};

use super::DiscordState;
use super::{MAX_FETCHED_NOTE_CACHE_ENTRIES, MAX_USER_PROFILE_CACHE_ENTRIES};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct UserProfileCacheKey {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
}

impl UserProfileCacheKey {
    pub(super) fn new(user_id: Id<UserMarker>, guild_id: Option<Id<GuildMarker>>) -> Self {
        Self { user_id, guild_id }
    }
}

pub(super) type ProfileRoleIds = BTreeMap<(Id<GuildMarker>, Id<UserMarker>), Vec<Id<RoleMarker>>>;

impl DiscordState {
    pub fn user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<&UserProfileInfo> {
        self.profiles
            .user_profiles
            .get(&UserProfileCacheKey::new(user_id, guild_id))
    }

    pub fn is_note_fetched(&self, user_id: Id<UserMarker>) -> bool {
        self.profiles.fetched_notes.contains_key(&user_id)
    }

    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.session.current_user_id
    }

    pub fn current_user(&self) -> Option<&str> {
        self.session.current_user.as_deref()
    }

    pub(super) fn remember_profile_cache_key(&mut self, key: UserProfileCacheKey) {
        self.profiles
            .profile_cache_order
            .retain(|existing| *existing != key);
        self.profiles.profile_cache_order.push_back(key);
        while self.profiles.profile_cache_order.len() > MAX_USER_PROFILE_CACHE_ENTRIES {
            let Some(evicted) = self.profiles.profile_cache_order.pop_front() else {
                break;
            };
            self.profiles.user_profiles.remove(&evicted);
            if let Some(guild_id) = evicted.guild_id {
                self.profiles
                    .profile_role_ids
                    .remove(&(guild_id, evicted.user_id));
            }
        }
        self.prune_profile_cache_order();
        self.prune_profile_role_ids_without_profiles();
    }

    pub(super) fn remember_fetched_note(&mut self, user_id: Id<UserMarker>) {
        self.profiles
            .fetched_note_order
            .retain(|existing| *existing != user_id);
        self.profiles.fetched_note_order.push_back(user_id);
        while self.profiles.fetched_note_order.len() > MAX_FETCHED_NOTE_CACHE_ENTRIES {
            let Some(evicted) = self.profiles.fetched_note_order.pop_front() else {
                break;
            };
            self.profiles.fetched_notes.remove(&evicted);
        }
        self.prune_fetched_note_order();
    }

    pub(super) fn remove_profiles_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.profiles
            .user_profiles
            .retain(|key, _| key.guild_id != Some(guild_id));
        self.profiles
            .profile_cache_order
            .retain(|key| key.guild_id != Some(guild_id));
    }

    fn prune_profile_cache_order(&mut self) {
        self.profiles
            .profile_cache_order
            .retain(|key| self.profiles.user_profiles.contains_key(key));
    }

    fn prune_fetched_note_order(&mut self) {
        self.profiles
            .fetched_note_order
            .retain(|user_id| self.profiles.fetched_notes.contains_key(user_id));
    }

    fn prune_profile_role_ids_without_profiles(&mut self) {
        self.profiles
            .profile_role_ids
            .retain(|(guild_id, user_id), _| {
                self.profiles
                    .user_profiles
                    .contains_key(&UserProfileCacheKey::new(*user_id, Some(*guild_id)))
            });
    }
}
