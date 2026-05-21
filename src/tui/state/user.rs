use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    layout::Alignment,
    style::{Color, Style},
    text::{Line, Span},
};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{
    ActivityInfo, AppCommand, MessageInfo, MessageState, PresenceStatus, UserProfileInfo,
};

use super::{ActiveGuildScope, DashboardState};
use super::{
    member_grouping::{
        MemberEntry, MemberGroup, channel_recipient_group, flatten_member_groups,
        guild_member_groups,
    },
    model::{FocusPane, MemberActionItem, MemberActionKind},
    popups::{MemberLeaderActionState, UserProfilePopupState},
    scroll::clamp_selected_index,
};

/// Holds `popup.scroll` inside `[0, max(0, total_lines - view_height)]` so
/// the renderer never asks for rows past the laid-out content. Re-applied
/// on every render hook because mutual-server data and bio paragraphs can
/// change between frames as the profile loads.
fn clamp_user_profile_popup_scroll(popup: &mut UserProfilePopupState) {
    let max_scroll = popup.total_lines.saturating_sub(popup.view_height);
    popup.scroll = popup.scroll.min(max_scroll);
}

const MAX_GUILD_MEMBER_BY_ID_REQUEST_USERS: usize = 100;

impl DashboardState {
    pub fn is_user_profile_popup_open(&self) -> bool {
        self.popups.user_profile_popup.is_some()
    }

    pub fn is_member_leader_action_active(&self) -> bool {
        self.popups.member_leader_action.is_some()
    }

    /// Direct shortcut from the member pane: open the profile popup for the
    /// currently selected member without going through Leader Actions.
    pub fn show_selected_member_profile(&mut self) -> Option<AppCommand> {
        if self.navigation.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        let guild_id = match self.navigation.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_selected_member_actions(&mut self) {
        if self.navigation.focus != FocusPane::Members {
            return;
        }
        let entries = self.flattened_members();
        let Some(entry) = entries.get(self.selected_member()) else {
            return;
        };
        let user_id = entry.user_id();
        // For DM/group-DM panes there is no guild context. Pass it through so
        // the profile fetch can omit `guild_id` and skip the guild_member
        // section gracefully.
        let guild_id = match self.navigation.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.popups.member_leader_action = Some(MemberLeaderActionState {
            user_id,
            guild_id,
            selected: 0,
        });
    }

    pub fn close_member_leader_action(&mut self) {
        self.popups.member_leader_action = None;
    }

    pub fn selected_member_action_items(&self) -> Vec<MemberActionItem> {
        if self.popups.member_leader_action.is_none() {
            return Vec::new();
        }
        vec![MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }]
    }

    pub fn select_member_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_member_action_items().len() {
            return false;
        }
        if let Some(action) = self.popups.member_leader_action.as_mut() {
            action.selected = row;
            return true;
        }
        false
    }

    pub fn activate_selected_member_action(&mut self) -> Option<AppCommand> {
        let action = self.popups.member_leader_action.clone()?;
        let items = self.selected_member_action_items();
        let item = items
            .get(clamp_selected_index(action.selected, items.len()))?
            .clone();
        if !item.enabled {
            return None;
        }
        match item.kind {
            MemberActionKind::ShowProfile => {
                self.close_member_leader_action();
                self.open_user_profile_popup(action.user_id, action.guild_id)
            }
        }
    }

    pub fn activate_member_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        let actions = self.selected_member_action_items();
        let index = actions.iter().enumerate().position(|(index, action)| {
            action.enabled
                && self
                    .options
                    .key_bindings()
                    .member_action_shortcut(&actions, index)
                    .is_some_and(|candidate| candidate == shortcut)
        })?;
        self.select_member_action_row(index);
        self.activate_selected_member_action()
    }

    /// Opens the profile popup for `user_id`. Returns
    /// `AppCommand::LoadUserProfile` to fetch fresh data when nothing is
    /// cached yet. The popup itself shows a loading state in the meantime.
    pub fn open_user_profile_popup(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        self.popups.user_profile_popup = Some(UserProfilePopupState {
            user_id,
            guild_id,
            load_error: None,
            scroll: 0,
            view_height: 0,
            total_lines: 0,
        });
        if !self.discord.cache.is_note_fetched(user_id) {
            self.requests
                .pending_commands
                .push_back(AppCommand::LoadUserNote { user_id });
        }
        if self.discord.cache.user_profile(user_id, guild_id).is_some() {
            None
        } else {
            Some(AppCommand::LoadUserProfile { user_id, guild_id })
        }
    }

    pub fn close_user_profile_popup(&mut self) {
        self.popups.user_profile_popup = None;
    }

    pub fn user_profile_popup_data(&self) -> Option<&UserProfileInfo> {
        let popup = self.popups.user_profile_popup.as_ref()?;
        self.discord
            .cache
            .user_profile(popup.user_id, popup.guild_id)
    }

    pub fn user_profile_popup_load_error(&self) -> Option<&str> {
        self.popups
            .user_profile_popup
            .as_ref()
            .and_then(|popup| popup.load_error.as_deref())
    }

    pub fn user_profile_popup_status(&self) -> PresenceStatus {
        let Some(popup) = self.popups.user_profile_popup.as_ref() else {
            return PresenceStatus::Unknown;
        };

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .members_for_guild(guild_id)
                .into_iter()
                .find(|member| member.user_id == popup.user_id)
                .map(|member| member.status)
        {
            return status;
        }

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .user_presence_for_guild(Some(guild_id), popup.user_id)
        {
            return status;
        }

        let recipient_status = self
            .discord
            .channels_for_guild(None)
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == popup.user_id)
            .map(|recipient| recipient.status);

        recipient_status
            .filter(|status| *status != PresenceStatus::Unknown)
            .or_else(|| self.discord.cache.user_presence(popup.user_id))
            .unwrap_or(PresenceStatus::Unknown)
    }

    /// URL of the avatar image to render into the open profile popup. None
    /// when the popup is closed, the profile has not loaded yet, or the user
    /// has no avatar attachment.
    pub fn user_profile_popup_avatar_url(&self) -> Option<&str> {
        self.user_profile_popup_data()?.avatar_url.as_deref()
    }

    pub fn user_profile_popup_activities(&self) -> &[ActivityInfo] {
        let Some(popup) = self.popups.user_profile_popup.as_ref() else {
            return &[];
        };
        self.discord
            .cache
            .user_activities_for_guild(popup.guild_id, popup.user_id)
    }

    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.discord
            .cache
            .user_activities_for_guild(self.selected_guild_id(), user_id)
    }

    /// Top-of-viewport row for the popup body. Used by the renderer.
    pub fn user_profile_popup_scroll(&self) -> usize {
        self.popups
            .user_profile_popup
            .as_ref()
            .map(|popup| popup.scroll)
            .unwrap_or(0)
    }

    /// Renderer hook: passes the latest viewport height back so scroll
    /// methods can clamp without snapping past the last visible row.
    pub fn set_user_profile_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = self.popups.user_profile_popup.as_mut() {
            popup.view_height = height;
            clamp_user_profile_popup_scroll(popup);
        }
    }

    /// Renderer hook: stash the laid-out content height so scroll
    /// clamping is a constant-time check instead of recomputing layout.
    pub fn set_user_profile_popup_total_lines(&mut self, total_lines: usize) {
        if let Some(popup) = self.popups.user_profile_popup.as_mut() {
            popup.total_lines = total_lines;
            clamp_user_profile_popup_scroll(popup);
        }
    }

    pub fn scroll_user_profile_popup_down(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup.as_mut() {
            popup.scroll = popup.scroll.saturating_add(1);
            clamp_user_profile_popup_scroll(popup);
        }
    }

    pub fn scroll_user_profile_popup_up(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup.as_mut() {
            popup.scroll = popup.scroll.saturating_sub(1);
        }
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
                self.requests
                    .pending_commands
                    .push_back(AppCommand::LoadGuildMembersByIds {
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
        if entry.has_fallback_identity() {
            if let Some(guild_id) = self.selected_guild_id() {
                if let Some(profile) = self
                    .discord
                    .cache
                    .user_profile(entry.user_id(), Some(guild_id))
                {
                    return profile.display_name().to_owned();
                }
            }
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
            Span::styled("●", Style::default().fg(Color::Green)),
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
