use std::collections::BTreeMap;

use crate::discord::UserProfileInfo;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};

use super::DiscordState;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct UserProfileCacheKey {
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
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
        self.user_profiles
            .get(&UserProfileCacheKey::new(user_id, guild_id))
    }

    pub fn is_note_fetched(&self, user_id: Id<UserMarker>) -> bool {
        self.fetched_notes.contains_key(&user_id)
    }

    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.current_user_id
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }
}
