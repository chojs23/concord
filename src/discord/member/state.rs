use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};
use crate::discord::{ActivityInfo, MemberInfo, PresenceStatus, RoleInfo};

use crate::discord::state::{
    DiscordState, MAX_RECENT_MEMBER_GUILDS, TYPING_INDICATOR_TTL, is_fallback_identity,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypingUserState {
    pub user_id: Id<UserMarker>,
    pub display_name: Option<String>,
}

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

#[cfg(test)]
#[allow(dead_code)]
impl GuildMemberState {
    pub(crate) fn test(user_id: Id<UserMarker>, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
            status: PresenceStatus::Offline,
        }
    }
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
    pub fn typing_users(&self, channel_id: Id<ChannelMarker>) -> Vec<TypingUserState> {
        let now = Instant::now();
        let Some(channel_typers) = self.presence.typing.get(&channel_id) else {
            return Vec::new();
        };
        let mut fresh: Vec<(Id<UserMarker>, Instant, Option<String>)> = channel_typers
            .iter()
            .filter(|(_, indicator)| now.duration_since(indicator.started) <= TYPING_INDICATOR_TTL)
            .map(|(user_id, indicator)| {
                (*user_id, indicator.started, indicator.display_name.clone())
            })
            .collect();
        // Newest typer first so the "X is typing…" label tends to surface the
        // person who just hit a key.
        fresh.sort_by_key(|(_, started, _)| std::cmp::Reverse(*started));
        fresh
            .into_iter()
            .map(|(user_id, _, display_name)| TypingUserState {
                user_id,
                display_name,
            })
            .collect()
    }

    pub fn user_presence(&self, user_id: Id<UserMarker>) -> Option<PresenceStatus> {
        self.user_presence_for_guild(None, user_id)
    }

    pub fn user_presence_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        user_id: Id<UserMarker>,
    ) -> Option<PresenceStatus> {
        guild_id
            .and_then(|guild_id| {
                self.presence
                    .guild_user_presences
                    .get(&(guild_id, user_id))
                    .copied()
            })
            .or_else(|| self.presence.user_presences.get(&user_id).copied())
    }

    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.user_activities_for_guild(None, user_id)
    }

    pub fn user_activities_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        user_id: Id<UserMarker>,
    ) -> &[ActivityInfo] {
        guild_id
            .and_then(|guild_id| {
                self.presence
                    .guild_user_activities
                    .get(&(guild_id, user_id))
            })
            .or_else(|| self.presence.user_activities.get(&user_id))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn members_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&GuildMemberState> {
        self.guild_details
            .members
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn roles_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&RoleState> {
        self.guild_details
            .roles
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn member_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let member = self.guild_details.members.get(&guild_id)?.get(&user_id)?;
        let roles = self.guild_details.roles.get(&guild_id)?;
        selected_member_role_color(member, roles)
    }

    pub fn member_display_name(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<&str> {
        self.guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            .map(|member| member.display_name.as_str())
    }

    pub fn member_has_known_name(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> bool {
        self.guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            .map(|member| member.username.is_some())
            .unwrap_or(false)
    }

    pub(in crate::discord) fn update_user_activities(
        &mut self,
        user_id: Id<UserMarker>,
        activities: &[ActivityInfo],
    ) {
        if activities.is_empty() {
            self.presence.user_activities.remove(&user_id);
        } else {
            self.presence
                .user_activities
                .insert(user_id, activities.to_vec());
        }
    }

    pub(in crate::discord) fn update_guild_user_activities(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        activities: &[ActivityInfo],
    ) {
        let key = (guild_id, user_id);
        if activities.is_empty() {
            self.presence.guild_user_activities.remove(&key);
        } else {
            self.presence
                .guild_user_activities
                .insert(key, activities.to_vec());
        }
    }

    pub(in crate::discord) fn update_cached_guild_activities_for_user(
        &mut self,
        user_id: Id<UserMarker>,
        activities: &[ActivityInfo],
    ) {
        let guild_ids: Vec<_> = self
            .presence
            .guild_user_activities
            .keys()
            .filter_map(|(guild_id, activity_user_id)| {
                (*activity_user_id == user_id).then_some(*guild_id)
            })
            .collect();
        for guild_id in guild_ids {
            self.update_guild_user_activities(guild_id, user_id, activities);
        }
    }

    pub(in crate::discord) fn upsert_guild_member(
        &mut self,
        guild_id: Id<GuildMarker>,
        member: &MemberInfo,
    ) -> bool {
        let was_known = self
            .guild_details
            .members
            .get(&guild_id)
            .is_some_and(|members| members.contains_key(&member.user_id));
        let previous_status = self
            .guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&member.user_id))
            .map(|member| member.status);
        let preserve_current_user_roles = self.session.current_user_id == Some(member.user_id)
            && member.role_ids.is_empty()
            && is_fallback_identity(member.username.as_deref(), &member.display_name);
        let protected_role_ids = preserve_current_user_roles
            .then(|| {
                self.guild_details
                    .current_user_role_ids
                    .get(&guild_id)
                    .cloned()
            })
            .flatten();

        let entry = self.guild_details.members.entry(guild_id).or_default();
        upsert_member(entry, member, previous_status);

        if self.session.current_user_id == Some(member.user_id) {
            if let Some(cached_role_ids) = protected_role_ids {
                if let Some(current_member) = entry.get_mut(&member.user_id) {
                    current_member.role_ids = cached_role_ids.clone();
                }
                self.guild_details
                    .current_user_role_ids
                    .insert(guild_id, cached_role_ids);
            } else if let Some(current_member) = entry.get(&member.user_id) {
                self.guild_details
                    .current_user_role_ids
                    .insert(guild_id, current_member.role_ids.clone());
            }
        }

        was_known
    }

    pub(in crate::discord) fn refresh_current_user_role_cache(&mut self) {
        let Some(current_user_id) = self.session.current_user_id else {
            return;
        };
        for (guild_id, members) in &self.guild_details.members {
            if let Some(member) = members.get(&current_user_id) {
                self.guild_details
                    .current_user_role_ids
                    .insert(*guild_id, member.role_ids.clone());
            }
        }
    }

    pub(in crate::discord) fn current_user_role_ids_for_guild(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Option<&[Id<RoleMarker>]> {
        self.guild_details
            .current_user_role_ids
            .get(&guild_id)
            .map(Vec::as_slice)
            .or_else(|| {
                let current_user_id = self.session.current_user_id?;
                self.guild_details
                    .members
                    .get(&guild_id)
                    .and_then(|members| members.get(&current_user_id))
                    .map(|member| member.role_ids.as_slice())
            })
    }

    pub(in crate::discord) fn record_selected_member_guild(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        if let Some(guild_id) = guild_id {
            self.guild_details
                .member_cache_guild_order
                .retain(|existing| *existing != guild_id);
            self.guild_details
                .member_cache_guild_order
                .push_back(guild_id);
        }
        self.prune_member_cache(guild_id);
    }

    fn prune_member_cache(&mut self, selected_guild_id: Option<Id<GuildMarker>>) {
        let mut keep_guilds: BTreeSet<Id<GuildMarker>> = self
            .guild_details
            .member_cache_guild_order
            .iter()
            .rev()
            .take(MAX_RECENT_MEMBER_GUILDS)
            .copied()
            .collect();
        if let Some(selected_guild_id) = selected_guild_id {
            keep_guilds.insert(selected_guild_id);
        }
        self.guild_details
            .member_cache_guild_order
            .retain(|guild_id| keep_guilds.contains(guild_id));

        let current_user_id = self.session.current_user_id;
        let message_authors = self.message_author_ids_by_guild();
        self.guild_details.members.retain(|guild_id, members| {
            if keep_guilds.contains(guild_id) {
                return true;
            }
            members.retain(|user_id, _| {
                current_user_id == Some(*user_id)
                    || message_authors
                        .get(guild_id)
                        .is_some_and(|authors| authors.contains(user_id))
            });
            !members.is_empty()
        });
        self.prune_presence_activity_cache();
    }

    fn message_author_ids_by_guild(&self) -> BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> {
        let mut authors: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();
        for message in self
            .message_cache
            .messages
            .values()
            .chain(self.message_cache.pinned_messages.values())
            .flat_map(|messages| messages.iter())
        {
            if let Some(guild_id) = message.guild_id {
                authors
                    .entry(guild_id)
                    .or_default()
                    .insert(message.author_id);
            }
            collect_nested_message_authors(&mut authors, message.guild_id, &message.reply);
        }
        authors
    }

    fn prune_presence_activity_cache(&mut self) {
        let retained_pairs = self.retained_guild_presence_keys();
        self.presence
            .guild_user_presences
            .retain(|key, _| retained_pairs.contains(key));
        self.presence
            .guild_user_activities
            .retain(|key, _| retained_pairs.contains(key));

        let retained_users = self.retained_presence_user_ids();
        self.presence
            .user_presences
            .retain(|user_id, _| retained_users.contains(user_id));
        self.presence
            .user_activities
            .retain(|user_id, _| retained_users.contains(user_id));
    }

    fn retained_presence_user_ids(&self) -> BTreeSet<Id<UserMarker>> {
        let mut retained = BTreeSet::new();
        if let Some(current_user_id) = self.session.current_user_id {
            retained.insert(current_user_id);
        }
        for members in self.guild_details.members.values() {
            retained.extend(members.keys().copied());
        }
        for channel in self
            .navigation
            .channels
            .values()
            .filter(|channel| channel.guild_id.is_none())
        {
            retained.extend(channel.recipients.iter().map(|recipient| recipient.user_id));
        }
        for profile_key in self.profiles.user_profiles.keys() {
            retained.insert(profile_key.user_id);
        }
        retained
    }

    fn retained_guild_presence_keys(&self) -> BTreeSet<(Id<GuildMarker>, Id<UserMarker>)> {
        let mut retained = BTreeSet::new();
        for (guild_id, members) in &self.guild_details.members {
            retained.extend(members.keys().map(|user_id| (*guild_id, *user_id)));
        }
        retained
    }
}

fn collect_nested_message_authors(
    authors: &mut BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>>,
    guild_id: Option<Id<GuildMarker>>,
    reply: &Option<crate::discord::ReplyInfo>,
) {
    let (Some(guild_id), Some(reply)) = (guild_id, reply) else {
        return;
    };
    if let Some(author_id) = reply.author_id {
        authors.entry(guild_id).or_default().insert(author_id);
    }
}

pub(in crate::discord) fn upsert_member(
    map: &mut BTreeMap<Id<UserMarker>, GuildMemberState>,
    member: &MemberInfo,
    previous_status: Option<PresenceStatus>,
) {
    let status = previous_status.unwrap_or(PresenceStatus::Unknown);

    let is_fallback = is_fallback_identity(member.username.as_deref(), &member.display_name);
    let existing_complete = is_fallback
        .then(|| map.get(&member.user_id))
        .flatten()
        .filter(|e| e.username.is_some());
    let (display_name, username, avatar_url) = match existing_complete {
        Some(existing) => (
            existing.display_name.clone(),
            existing.username.clone(),
            existing.avatar_url.clone(),
        ),
        None => (
            member.display_name.clone(),
            member.username.clone(),
            member.avatar_url.clone(),
        ),
    };

    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name,
            username,
            is_bot: member.is_bot,
            avatar_url,
            role_ids: member.role_ids.clone(),
            status,
        },
    );
}

pub(in crate::discord) fn role_map(roles: &[RoleInfo]) -> BTreeMap<Id<RoleMarker>, RoleState> {
    roles
        .iter()
        .map(|role| (role.id, role_state(role)))
        .collect()
}

pub(in crate::discord) fn role_state(role: &RoleInfo) -> RoleState {
    RoleState {
        id: role.id,
        name: role.name.clone(),
        color: role.color,
        position: role.position,
        hoist: role.hoist,
        permissions: role.permissions,
    }
}

pub(in crate::discord) fn selected_member_role_color(
    member: &GuildMemberState,
    roles: &BTreeMap<Id<RoleMarker>, RoleState>,
) -> Option<u32> {
    selected_role_ids_color(&member.role_ids, roles)
}

pub(in crate::discord) fn selected_role_ids_color(
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
