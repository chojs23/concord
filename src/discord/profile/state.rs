use std::collections::BTreeMap;

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};
use crate::discord::member::role_display_order;
use crate::discord::{RoleState, UserProfileInfo};

use crate::discord::state::DiscordState;
use crate::discord::state::{
    MAX_FETCHED_NOTE_CACHE_ENTRIES, MAX_USER_PROFILE_CACHE_ENTRIES, drain_over_limit, touch_recent,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::discord) struct UserProfileCacheKey {
    pub(in crate::discord) user_id: Id<UserMarker>,
    pub(in crate::discord) guild_id: Option<Id<GuildMarker>>,
}

impl UserProfileCacheKey {
    pub(in crate::discord) fn new(
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Self {
        Self { user_id, guild_id }
    }
}

pub(in crate::discord) type ProfileRoleIds =
    BTreeMap<(Id<GuildMarker>, Id<UserMarker>), Vec<Id<RoleMarker>>>;

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

    /// Resolves the role IDs returned with a guild-scoped profile against the
    /// guild role cache. Discord omits the base guild role from its own profile
    /// UI, so the role whose ID matches the guild ID is not returned here.
    pub(crate) fn user_profile_roles(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<Vec<&RoleState>> {
        let role_ids = self.profiles.profile_role_ids.get(&(guild_id, user_id))?;
        let role_map = self.guild_details.roles.get(&guild_id)?;
        let mut roles = role_ids
            .iter()
            .filter(|role_id| role_id.get() != guild_id.get())
            .filter_map(|role_id| role_map.get(role_id))
            .collect::<Vec<_>>();
        roles.sort_by(|left, right| role_display_order(left, right));
        Some(roles)
    }

    pub(in crate::discord) fn remember_profile_cache_key(&mut self, key: UserProfileCacheKey) {
        let profiles = self.profiles_mut();
        touch_recent(&mut profiles.profile_cache_order, key);
        let evicted = drain_over_limit(
            &mut profiles.profile_cache_order,
            MAX_USER_PROFILE_CACHE_ENTRIES,
        );
        for key in evicted {
            profiles.user_profiles.remove(&key);
            if let Some(guild_id) = key.guild_id {
                profiles.profile_role_ids.remove(&(guild_id, key.user_id));
            }
        }
        self.prune_profile_cache_order();
        self.prune_profile_role_ids_without_profiles();
    }

    pub(in crate::discord) fn remember_fetched_note(&mut self, user_id: Id<UserMarker>) {
        let profiles = self.profiles_mut();
        touch_recent(&mut profiles.fetched_note_order, user_id);
        for user_id in drain_over_limit(
            &mut profiles.fetched_note_order,
            MAX_FETCHED_NOTE_CACHE_ENTRIES,
        ) {
            profiles.fetched_notes.remove(&user_id);
        }
        self.prune_fetched_note_order();
    }

    pub(in crate::discord) fn remove_profiles_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.profiles_mut()
            .user_profiles
            .retain(|key, _| key.guild_id != Some(guild_id));
        self.profiles_mut()
            .profile_cache_order
            .retain(|key| key.guild_id != Some(guild_id));
    }

    fn prune_profile_cache_order(&mut self) {
        let profiles = self.profiles_mut();
        let user_profiles = &profiles.user_profiles;
        profiles
            .profile_cache_order
            .retain(|key| user_profiles.contains_key(key));
    }

    fn prune_fetched_note_order(&mut self) {
        let profiles = self.profiles_mut();
        let fetched_notes = &profiles.fetched_notes;
        profiles
            .fetched_note_order
            .retain(|user_id| fetched_notes.contains_key(user_id));
    }

    fn prune_profile_role_ids_without_profiles(&mut self) {
        let profiles = self.profiles_mut();
        let user_profiles = &profiles.user_profiles;
        profiles.profile_role_ids.retain(|(guild_id, user_id), _| {
            user_profiles.contains_key(&UserProfileCacheKey::new(*user_id, Some(*guild_id)))
        });
    }
}
