use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    layout::Alignment,
    style::Style,
    text::{Line, Span},
};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{ActivityInfo, AppCommand, ChannelInfo, MessageInfo, MessageState};
use crate::tui::theme;

use super::DashboardState;
use super::member_grouping::{
    MemberEntry, MemberGroup, channel_recipient_group, flatten_member_groups, guild_member_groups,
};

const MAX_GUILD_MEMBER_BY_ID_REQUEST_USERS: usize = 100;

impl DashboardState {
    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.discord
            .cache
            .user_activities_for_guild(self.selected_guild_id(), user_id)
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let members = self.discord.cache.members_for_guild(guild_id);
        let roles = self.discord.cache.roles_for_guild(guild_id);
        guild_member_groups(members, roles)
    }

    pub fn is_member_list_loading(&self) -> bool {
        let Some(guild_id) = self.selected_guild_id() else {
            return false;
        };
        self.discord
            .cache
            .guild(guild_id)
            .is_some_and(|guild| guild.online_count.is_none())
    }

    pub fn message_author_role_color(&self, message: &MessageState) -> Option<u32> {
        self.message_user_role_color(message, message.author_id)
    }

    pub fn message_user_role_color(
        &self,
        message: &MessageState,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let channel = self.discord.cache.channel(message.channel_id);
        let guild_id = message
            .guild_id
            .or_else(|| channel.and_then(|channel| channel.guild_id));
        let guild_id = guild_id?;
        if user_id != message.author_id {
            return self.discord.cache.user_role_color(guild_id, user_id);
        }
        self.discord.cache.message_author_role_color(
            guild_id,
            message.channel_id,
            message.id,
            user_id,
        )
    }

    pub fn missing_message_author_member_requests(
        &self,
        messages: &[MessageInfo],
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let mut by_guild: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();

        for message in messages {
            if !message.author_role_ids.is_empty() {
                continue;
            }

            let channel = self.discord.cache.channel(message.channel_id);
            let Some(guild_id) = message
                .guild_id
                .or_else(|| channel.and_then(|channel| channel.guild_id))
            else {
                continue;
            };

            if !self.discord.cache.message_author_role_ids_known(
                guild_id,
                message.channel_id,
                message.message_id,
                message.author_id,
            ) {
                by_guild
                    .entry(guild_id)
                    .or_default()
                    .insert(message.author_id);
            }
        }

        by_guild
            .into_iter()
            .map(|(guild_id, user_ids)| (guild_id, user_ids.into_iter().collect()))
            .collect()
    }

    pub fn missing_thread_owner_member_requests(
        &self,
        threads: &[ChannelInfo],
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let mut by_guild: BTreeMap<Id<GuildMarker>, BTreeSet<Id<UserMarker>>> = BTreeMap::new();

        for thread in threads {
            let Some(user_id) = thread.owner_id else {
                continue;
            };
            let guild_id = thread.guild_id.or_else(|| {
                self.discord
                    .cache
                    .channel(thread.channel_id)
                    .and_then(|channel| channel.guild_id)
            });
            let Some(guild_id) = guild_id else {
                continue;
            };
            if !self.discord.cache.member_has_known_name(guild_id, user_id) {
                by_guild.entry(guild_id).or_default().insert(user_id);
            }
        }

        by_guild
            .into_iter()
            .map(|(guild_id, user_ids)| (guild_id, user_ids.into_iter().collect()))
            .collect()
    }

    pub fn initial_unknown_member_requests(&self) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let Some(guild_id) = self.selected_guild_id() else {
            return Vec::new();
        };
        if !self.is_member_list_loading() {
            return Vec::new();
        }

        let user_ids = self
            .discord
            .members_for_guild(guild_id)
            .into_iter()
            .filter(|member| member.username.is_none() && member.display_name == "unknown")
            .map(|member| member.user_id)
            .take(MAX_GUILD_MEMBER_BY_ID_REQUEST_USERS)
            .collect::<Vec<_>>();

        if user_ids.is_empty() {
            Vec::new()
        } else {
            vec![(guild_id, user_ids)]
        }
    }

    pub fn enqueue_message_author_member_requests(
        &mut self,
        requests: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
    ) {
        self.enqueue_guild_member_by_id_requests(requests);
    }

    pub fn enqueue_guild_member_by_id_requests(
        &mut self,
        requests: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
    ) -> bool {
        let mut enqueued = false;
        for (guild_id, user_ids) in requests {
            for chunk in user_ids.chunks(MAX_GUILD_MEMBER_BY_ID_REQUEST_USERS) {
                self.enqueue_pending_command(AppCommand::LoadGuildMembersByIds {
                    guild_id,
                    user_ids: chunk.to_vec(),
                });
                enqueued = true;
            }
        }
        enqueued
    }

    pub fn member_role_color(&self, member: MemberEntry<'_>) -> Option<u32> {
        let guild_id = self.selected_guild_id()?;
        self.discord
            .cache
            .member_role_color(guild_id, member.user_id())
    }

    /// Resolved display name for a member panel entry. Falls through to the
    /// profile cache when the guild member entry only has fallback data.
    pub fn member_display_name(&self, entry: MemberEntry<'_>) -> String {
        let name = entry.display_name();
        if entry.has_fallback_identity()
            && let Some(guild_id) = self.selected_guild_id()
            && let Some(profile) = self
                .discord
                .cache
                .user_profile(entry.user_id(), Some(guild_id))
        {
            return profile.display_name().to_owned();
        }
        name
    }

    pub fn member_panel_title(&self) -> Line<'static> {
        let Some(guild_id) = self.selected_guild_id() else {
            return Line::from(" Members ");
        };
        let guild = self.discord.cache.guild(guild_id);
        let Some(online) = guild.and_then(|g| g.online_count) else {
            return Line::from(" Members ");
        };
        let total = guild.and_then(|g| g.member_count).unwrap_or(0);
        Line::from(vec![
            Span::styled("●", Style::default().fg(theme::current().success)),
            Span::raw(format!(
                " {}  ○ {}",
                fmt_with_separators(online as u64),
                fmt_with_separators(total)
            )),
        ])
        .alignment(Alignment::Center)
    }

    fn selected_channel_recipient_group(&self) -> Vec<MemberGroup<'_>> {
        let Some(channel) = self.selected_channel_state() else {
            return Vec::new();
        };
        channel_recipient_group(channel)
    }

    pub fn flattened_members(&self) -> Vec<MemberEntry<'_>> {
        flatten_member_groups(self.members_grouped())
    }
}

fn fmt_with_separators(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
