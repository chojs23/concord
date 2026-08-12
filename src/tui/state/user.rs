use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    layout::Alignment,
    text::{Line, Span},
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{ActivityInfo, AppCommand, ChannelInfo, MessageInfo, MessageState};
use crate::tui::theme;

use super::DashboardState;
use super::member_grouping::{
    MemberEntry, MemberGroup, channel_recipient_group, flatten_member_groups, guild_member_groups,
};

const MAX_GUILD_MEMBER_BY_ID_REQUEST_USERS: usize = 100;
type OrderedUserIds = (BTreeSet<Id<UserMarker>>, Vec<Id<UserMarker>>);

impl DashboardState {
    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.discord
            .cache
            .user_activities_for_guild(self.selected_guild_id(), user_id)
    }

    pub(in crate::tui) fn channel_user_display_name(
        &self,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        fallback: &str,
    ) -> String {
        self.discord
            .cache
            .user_display_name_for_channel(channel_id, user_id)
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let entries = self.discord.cache.member_list_entries_for_guild(guild_id);
        guild_member_groups(
            entries,
            |user_id| self.discord.cache.member_for_guild(guild_id, user_id),
            |role_id| self.discord.cache.role_for_guild(guild_id, role_id),
        )
    }

    pub fn is_member_list_loading(&self) -> bool {
        let Some((guild_id, _)) = self.member_list_subscription_target() else {
            return false;
        };
        !self
            .discord
            .cache
            .member_list_has_ranges(guild_id, &self.member_subscription_ranges())
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

    pub(in crate::tui) fn missing_message_author_member_requests(
        &self,
        messages: &[MessageInfo],
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let guild_ids = messages
            .iter()
            .map(|message| {
                message.guild_id.or_else(|| {
                    self.discord
                        .cache
                        .channel(message.channel_id)
                        .and_then(|channel| channel.guild_id)
                })
            })
            .collect::<Vec<_>>();
        let mut users = Vec::new();
        for (message, guild_id) in messages.iter().zip(&guild_ids) {
            let Some(guild_id) = guild_id else {
                continue;
            };
            if message.webhook_id.is_none() {
                users.push((*guild_id, message.author_id));
            }
        }

        // The first Gateway batch must contain the names painted in the
        // message rows. Related users still need hydration, but they cannot
        // displace authors when one history page contains over 100 user IDs.
        for (message, guild_id) in messages.iter().zip(guild_ids) {
            let Some(guild_id) = guild_id else {
                continue;
            };
            users.extend(
                message
                    .interaction
                    .as_ref()
                    .and_then(|interaction| interaction.user_id)
                    .map(|user_id| (guild_id, user_id)),
            );
            users.extend(
                message
                    .mentions
                    .iter()
                    .map(|mention| (guild_id, mention.user_id)),
            );
            if let Some(reply) = message.reply.as_ref() {
                users.extend(reply.author_id.map(|user_id| (guild_id, user_id)));
                users.extend(
                    reply
                        .mentions
                        .iter()
                        .map(|mention| (guild_id, mention.user_id)),
                );
            }
            for snapshot in &message.forwarded_snapshots {
                let Some(snapshot_guild_id) = snapshot
                    .source_channel_id
                    .and_then(|channel_id| self.discord.cache.channel(channel_id))
                    .and_then(|channel| channel.guild_id)
                else {
                    continue;
                };
                users.extend(
                    snapshot
                        .mentions
                        .iter()
                        .map(|mention| (snapshot_guild_id, mention.user_id)),
                );
            }
        }
        self.missing_guild_member_requests(users)
    }

    pub(in crate::tui) fn missing_thread_owner_member_requests(
        &self,
        threads: &[ChannelInfo],
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let users = threads.iter().filter_map(|thread| {
            let user_id = thread.owner_id?;
            let guild_id = thread.guild_id.or_else(|| {
                self.discord
                    .cache
                    .channel(thread.channel_id)
                    .and_then(|channel| channel.guild_id)
            })?;
            Some((guild_id, user_id))
        });
        self.missing_guild_member_requests(users)
    }

    pub(in crate::tui) fn missing_channel_user_member_requests(
        &self,
        channel_id: Id<ChannelMarker>,
        guild_id: Option<Id<GuildMarker>>,
        user_ids: impl IntoIterator<Item = Id<UserMarker>>,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let Some(guild_id) = guild_id.or_else(|| {
            self.discord
                .cache
                .channel(channel_id)
                .and_then(|channel| channel.guild_id)
        }) else {
            return Vec::new();
        };
        self.missing_guild_member_requests(user_ids.into_iter().map(|user_id| (guild_id, user_id)))
    }

    pub(in crate::tui) fn observed_member_hydration_requests(
        &self,
        now: std::time::Instant,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        self.discord
            .cache
            .missing_member_hydration_requests(self.selected_guild_id(), now)
    }

    pub(in crate::tui) fn enqueue_member_hydration_requests(
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

    fn missing_guild_member_requests(
        &self,
        users: impl IntoIterator<Item = (Id<GuildMarker>, Id<UserMarker>)>,
    ) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
        let mut by_guild: BTreeMap<Id<GuildMarker>, OrderedUserIds> = BTreeMap::new();
        for (guild_id, user_id) in users {
            if self.discord.cache.member_needs_hydration(guild_id, user_id) {
                let (seen, ordered) = by_guild.entry(guild_id).or_default();
                if seen.insert(user_id) {
                    ordered.push(user_id);
                }
            }
        }
        by_guild
            .into_iter()
            .map(|(guild_id, (_, user_ids))| (guild_id, user_ids))
            .collect()
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
            Span::styled(
                "●",
                theme::current().style(theme::HighlightGroup::PresenceOnline),
            ),
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

    /// Members confirmed by the current guild snapshot, including explicit
    /// Opcode 8 results that are not part of the streamed sidebar ranges.
    pub(in crate::tui) fn searchable_members(&self) -> Vec<MemberEntry<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return flatten_member_groups(self.selected_channel_recipient_group());
        };
        self.discord
            .cache
            .searchable_members_for_guild(guild_id)
            .into_iter()
            .map(MemberEntry::Guild)
            .collect()
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
