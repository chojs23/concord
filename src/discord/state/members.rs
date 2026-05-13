use std::collections::BTreeMap;
use std::time::Instant;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};
use crate::discord::{ActivityInfo, MemberInfo, PresenceStatus, RoleInfo};

use super::{DiscordState, TYPING_INDICATOR_TTL};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle. Mirrors `MemberInfo::username`. The @-mention
    /// picker matches against this in addition to `display_name`.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
    pub status: PresenceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleState {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
    /// Discord permission bitfield for the role. Used to compute the
    /// authenticated user's base permissions and detect ADMINISTRATOR.
    pub permissions: u64,
}

impl DiscordState {
    pub fn typing_users(&self, channel_id: Id<ChannelMarker>) -> Vec<Id<UserMarker>> {
        let now = Instant::now();
        let Some(channel_typers) = self.typing.get(&channel_id) else {
            return Vec::new();
        };
        let mut fresh: Vec<(Id<UserMarker>, Instant)> = channel_typers
            .iter()
            .filter(|(_, started)| now.duration_since(**started) <= TYPING_INDICATOR_TTL)
            .map(|(user_id, started)| (*user_id, *started))
            .collect();
        // Newest typer first so the "X is typing…" label tends to surface the
        // person who just hit a key.
        fresh.sort_by_key(|(_, started)| std::cmp::Reverse(*started));
        fresh.into_iter().map(|(user_id, _)| user_id).collect()
    }

    pub fn user_presence(&self, user_id: Id<UserMarker>) -> Option<PresenceStatus> {
        self.user_presences.get(&user_id).copied()
    }

    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.user_activities
            .get(&user_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn members_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&GuildMemberState> {
        self.members
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn roles_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&RoleState> {
        self.roles
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn member_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let member = self.members.get(&guild_id)?.get(&user_id)?;
        let roles = self.roles.get(&guild_id)?;
        selected_member_role_color(member, roles)
    }

    pub fn member_display_name(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<&str> {
        self.members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            .map(|member| member.display_name.as_str())
    }

    pub(super) fn update_user_activities(
        &mut self,
        user_id: Id<UserMarker>,
        activities: &[ActivityInfo],
    ) {
        if activities.is_empty() {
            self.user_activities.remove(&user_id);
        } else {
            self.user_activities.insert(user_id, activities.to_vec());
        }
    }
}

pub(super) fn upsert_member(
    map: &mut BTreeMap<Id<UserMarker>, GuildMemberState>,
    member: &MemberInfo,
    previous_status: Option<PresenceStatus>,
) {
    let status = previous_status.unwrap_or(PresenceStatus::Unknown);
    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name: member.display_name.clone(),
            username: member.username.clone(),
            is_bot: member.is_bot,
            avatar_url: member.avatar_url.clone(),
            role_ids: member.role_ids.clone(),
            status,
        },
    );
}

pub(super) fn role_map(roles: &[RoleInfo]) -> BTreeMap<Id<RoleMarker>, RoleState> {
    roles
        .iter()
        .map(|role| {
            (
                role.id,
                RoleState {
                    id: role.id,
                    name: role.name.clone(),
                    color: role.color,
                    position: role.position,
                    hoist: role.hoist,
                    permissions: role.permissions,
                },
            )
        })
        .collect()
}

pub(super) fn selected_member_role_color(
    member: &GuildMemberState,
    roles: &BTreeMap<Id<RoleMarker>, RoleState>,
) -> Option<u32> {
    selected_role_ids_color(&member.role_ids, roles)
}

pub(super) fn selected_role_ids_color(
    role_ids: &[Id<RoleMarker>],
    roles: &BTreeMap<Id<RoleMarker>, RoleState>,
) -> Option<u32> {
    role_ids
        .iter()
        .filter_map(|role_id| roles.get(role_id))
        .filter(|role| role.color.is_some_and(|color| color != 0))
        .min_by(|left, right| role_display_order(left, right))
        .and_then(|role| role.color)
}

fn role_display_order(left: &RoleState, right: &RoleState) -> std::cmp::Ordering {
    right
        .position
        .cmp(&left.position)
        .then(left.id.get().cmp(&right.id.get()))
}
