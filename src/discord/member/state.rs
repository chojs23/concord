use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
};
use crate::discord::member::onboarding_status_from_flags;
use crate::discord::{
    ActivityInfo, MemberInfo, MemberOnboardingStatus, PresenceStatus, RoleInfo, VoiceScope,
};

use crate::discord::state::{
    DiscordState, MAX_RECENT_MEMBER_GUILDS, TYPING_INDICATOR_TTL, is_fallback_identity,
    touch_recent,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypingUserState {
    pub user_id: Id<UserMarker>,
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
    /// True when Discord supplied the member's role list, including an
    /// explicitly empty list. Permission checks must not treat an omitted
    /// partial field as "no roles".
    pub role_ids_known: bool,
    pub joined_at: Option<DateTime<Utc>>,
    pub flags: Option<u64>,
    pub pending: Option<bool>,
    pub communication_disabled_until: Option<DateTime<Utc>>,
    pub status: PresenceStatus,
}

impl GuildMemberState {
    /// Returns the progress Discord reports through guild member flags.
    /// `None` means the source payload has not supplied flags yet.
    pub fn onboarding_status(&self) -> Option<MemberOnboardingStatus> {
        onboarding_status_from_flags(self.flags)
    }
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
            role_ids_known: true,
            joined_at: None,
            flags: None,
            pending: None,
            communication_disabled_until: None,
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
        let mut fresh: Vec<(Id<UserMarker>, Instant)> = channel_typers
            .iter()
            .filter(|(_, indicator)| now.duration_since(indicator.started) <= TYPING_INDICATOR_TTL)
            .map(|(user_id, indicator)| (*user_id, indicator.started))
            .collect();
        // Newest typer first so the "X is typing…" label tends to surface the
        // person who just hit a key.
        fresh.sort_by_key(|(_, started)| std::cmp::Reverse(*started));
        fresh
            .into_iter()
            .map(|(user_id, _)| TypingUserState { user_id })
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

    pub fn role_for_guild(
        &self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> Option<&RoleState> {
        self.guild_details.roles.get(&guild_id)?.get(&role_id)
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

    pub(crate) fn role_color_for_ids(
        &self,
        guild_id: Id<GuildMarker>,
        role_ids: &[Id<RoleMarker>],
    ) -> Option<u32> {
        let roles = self.guild_details.roles.get(&guild_id)?;
        selected_role_ids_color(role_ids, roles)
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
            .map(|member| !is_fallback_identity(member.username.as_deref(), &member.display_name))
            .unwrap_or(false)
    }

    pub fn member_needs_hydration(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> bool {
        self.guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            .map(|member| {
                is_fallback_identity(member.username.as_deref(), &member.display_name)
                    || !member.role_ids_known
            })
            .unwrap_or(true)
    }

    /// Collects persistent member demands from shared state. Event-specific
    /// message loads add their own demand immediately, while this sweep makes
    /// voice, typing, thread, and current-user permission data retryable after
    /// reconnects or lost member chunks.
    pub fn missing_member_hydration_requests(
        &self,
        selected_guild_id: Option<Id<GuildMarker>>,
        now: Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let mut by_guild: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();

        let mut require = |guild_id: Id<GuildMarker>, user_id: Id<UserMarker>| {
            if self.member_needs_hydration(guild_id, user_id) {
                by_guild.entry(guild_id).or_default().insert(user_id);
            }
        };

        if let Some(guild_id) = selected_guild_id {
            if let Some(current_user_id) = self.session.current_user_id {
                require(guild_id, current_user_id);
            }
            if let Some(members) = self.guild_details.members.get(&guild_id) {
                for member in members.values().filter(|member| {
                    is_fallback_identity(member.username.as_deref(), &member.display_name)
                        || !member.role_ids_known
                }) {
                    require(guild_id, member.user_id);
                }
            }
        }

        for (scope, user_id) in self.voice.states.keys() {
            if let VoiceScope::Guild(guild_id) = scope {
                require(*guild_id, *user_id);
            }
        }
        for stream in self.voice.streams.values() {
            if let VoiceScope::Guild(guild_id) = stream.scope {
                require(guild_id, stream.owner_id);
                for user_id in &stream.viewer_ids {
                    require(guild_id, *user_id);
                }
            }
        }

        for (channel_id, typers) in &self.presence.typing {
            let Some(guild_id) = self
                .channel(*channel_id)
                .and_then(|channel| channel.guild_id)
            else {
                continue;
            };
            for (user_id, indicator) in typers {
                if now.saturating_duration_since(indicator.started) <= TYPING_INDICATOR_TTL {
                    require(guild_id, *user_id);
                }
            }
        }

        for creator in self.navigation.thread_creators.values() {
            if let Some(guild_id) = creator.guild_id {
                require(guild_id, creator.user_id);
            }
        }

        by_guild
            .into_iter()
            .map(|(guild_id, user_ids)| (guild_id, user_ids.into_iter().collect()))
            .collect()
    }

    pub(in crate::discord) fn update_user_activities(
        &mut self,
        user_id: Id<UserMarker>,
        activities: &[ActivityInfo],
    ) {
        if activities.is_empty() {
            self.presence_mut().user_activities.remove(&user_id);
        } else {
            self.presence_mut()
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
            self.presence_mut().guild_user_activities.remove(&key);
        } else {
            self.presence_mut()
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
        let Self {
            guild_details,
            session,
            ..
        } = self;
        let guild_details = Arc::make_mut(guild_details);
        let entry = guild_details.members.entry(guild_id).or_default();
        upsert_member(entry, member, previous_status);

        if session.current_user_id == Some(member.user_id)
            && let Some(current_member) = entry
                .get(&member.user_id)
                .filter(|member| member.role_ids_known)
        {
            guild_details
                .current_user_role_ids
                .insert(guild_id, current_member.role_ids.clone());
        }

        was_known
    }

    pub(in crate::discord) fn refresh_current_user_role_cache(&mut self) {
        let Some(current_user_id) = self.session.current_user_id else {
            return;
        };
        let guild_details = self.guild_details_mut();
        for (guild_id, members) in &guild_details.members {
            if let Some(member) = members
                .get(&current_user_id)
                .filter(|member| member.role_ids_known)
            {
                guild_details
                    .current_user_role_ids
                    .insert(*guild_id, member.role_ids.clone());
            }
        }
    }

    pub(crate) fn current_user_role_ids_for_guild(
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
                    .filter(|member| member.role_ids_known)
                    .map(|member| member.role_ids.as_slice())
            })
    }

    pub(in crate::discord) fn record_selected_member_guild(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        if let Some(guild_id) = guild_id {
            touch_recent(
                &mut self.guild_details_mut().member_cache_guild_order,
                guild_id,
            );
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
        self.guild_details_mut()
            .member_cache_guild_order
            .retain(|guild_id| keep_guilds.contains(guild_id));

        let current_user_id = self.session.current_user_id;
        let message_authors = self.message_author_ids_by_guild();
        let voice_participants = self.active_voice_member_ids_by_guild();
        self.guild_details_mut()
            .members
            .retain(|guild_id, members| {
                if keep_guilds.contains(guild_id) {
                    return true;
                }
                members.retain(|user_id, _| {
                    current_user_id == Some(*user_id)
                        || message_authors
                            .get(guild_id)
                            .is_some_and(|authors| authors.contains(user_id))
                        || voice_participants
                            .get(guild_id)
                            .is_some_and(|participants| participants.contains(user_id))
                });
                !members.is_empty()
            });
        self.prune_presence_activity_cache();
    }

    fn message_author_ids_by_guild(&self) -> BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> {
        let mut authors: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();
        for message in self
            .message_cache
            .timelines
            .values()
            .flat_map(|timeline| timeline.messages.iter())
            .chain(
                self.message_cache
                    .pinned_messages
                    .values()
                    .flat_map(|messages| messages.iter()),
            )
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

    fn active_voice_member_ids_by_guild(
        &self,
    ) -> BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> {
        let mut participants: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();
        for (scope, user_id) in self.voice.states.keys() {
            if let VoiceScope::Guild(guild_id) = scope {
                participants.entry(*guild_id).or_default().insert(*user_id);
            }
        }
        for stream in self.voice.streams.values() {
            if let VoiceScope::Guild(guild_id) = stream.scope {
                participants.entry(guild_id).or_default().extend(
                    std::iter::once(stream.owner_id).chain(stream.viewer_ids.iter().copied()),
                );
            }
        }
        participants
    }

    fn prune_presence_activity_cache(&mut self) {
        let retained_pairs = self.retained_guild_presence_keys();
        self.presence_mut()
            .guild_user_presences
            .retain(|key, _| retained_pairs.contains(key));
        self.presence_mut()
            .guild_user_activities
            .retain(|key, _| retained_pairs.contains(key));

        let retained_users = self.retained_presence_user_ids();
        self.presence_mut()
            .user_presences
            .retain(|user_id, _| retained_users.contains(user_id));
        self.presence_mut()
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
    let existing = map.get(&member.user_id);
    let display_name = if is_fallback {
        existing
            .map(|member| member.display_name.clone())
            .unwrap_or_else(|| member.display_name.clone())
    } else {
        member.display_name.clone()
    };
    let username = member
        .username
        .clone()
        .or_else(|| existing.and_then(|member| member.username.clone()));
    let is_bot = if member.is_bot_present {
        member.is_bot
    } else {
        existing.is_some_and(|member| member.is_bot)
    };
    let avatar_url = if member.avatar_url_present {
        member.avatar_url.clone()
    } else {
        existing.and_then(|member| member.avatar_url.clone())
    };
    let (role_ids, role_ids_known) = if member.role_ids_present {
        (member.role_ids.clone(), true)
    } else {
        existing
            .map(|member| (member.role_ids.clone(), member.role_ids_known))
            .unwrap_or_default()
    };
    let joined_at = member
        .joined_at
        .or_else(|| existing.and_then(|member| member.joined_at));
    let flags = member
        .flags
        .or_else(|| existing.and_then(|member| member.flags));
    let pending = member
        .pending
        .or_else(|| existing.and_then(|member| member.pending));
    let communication_disabled_until = if member.communication_disabled_until_present {
        member.communication_disabled_until
    } else {
        existing.and_then(|member| member.communication_disabled_until)
    };

    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name,
            username,
            is_bot,
            avatar_url,
            role_ids,
            role_ids_known,
            joined_at,
            flags,
            pending,
            communication_disabled_until,
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
