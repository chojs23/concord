use std::collections::HashSet;

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{ActivityInfo, AppCommand, MessageState, PresenceStatus, UserProfileInfo};

use super::{ActiveGuildScope, DashboardState};
use super::{
    member_grouping::{
        MemberEntry, MemberGroup, channel_recipient_group, flatten_member_groups,
        guild_member_groups,
    },
    model::{FocusPane, MemberActionItem, MemberActionKind, member_action_shortcut},
    popups::{MemberActionMenuState, UserProfilePopupState},
    scroll::{clamp_selected_index, move_index_down, move_index_up},
};

/// Holds `popup.scroll` inside `[0, max(0, total_lines - view_height)]` so
/// the renderer never asks for rows past the laid-out content. Re-applied
/// on every render hook because mutual-server data and bio paragraphs can
/// change between frames as the profile loads.
fn clamp_user_profile_popup_scroll(popup: &mut UserProfilePopupState) {
    let max_scroll = popup.total_lines.saturating_sub(popup.view_height);
    popup.scroll = popup.scroll.min(max_scroll);
}

impl DashboardState {
    pub fn is_user_profile_popup_open(&self) -> bool {
        self.user_profile_popup.is_some()
    }

    pub fn is_member_action_menu_open(&self) -> bool {
        self.member_action_menu.is_some()
    }

    pub fn member_action_menu_title(&self) -> Option<String> {
        let menu = self.member_action_menu.as_ref()?;
        let entries = self.flattened_members();
        let entry = entries.iter().find(|m| m.user_id() == menu.user_id)?;
        Some(entry.display_name())
    }

    /// Direct shortcut from the member pane: open the profile popup for the
    /// currently selected member without going through the action menu.
    pub fn show_selected_member_profile(&mut self) -> Option<AppCommand> {
        if self.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_selected_member_actions(&mut self) {
        if self.focus != FocusPane::Members {
            return;
        }
        let entries = self.flattened_members();
        let Some(entry) = entries.get(self.selected_member()) else {
            return;
        };
        let user_id = entry.user_id();
        // For DM/group-DM panes there is no guild context; pass it through so
        // the profile fetch can omit `guild_id` and skip the guild_member
        // section gracefully.
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.member_action_menu = Some(MemberActionMenuState {
            user_id,
            guild_id,
            selected: 0,
        });
    }

    pub fn close_member_action_menu(&mut self) {
        self.member_action_menu = None;
    }

    pub fn selected_member_action_items(&self) -> Vec<MemberActionItem> {
        if self.member_action_menu.is_none() {
            return Vec::new();
        }
        vec![MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }]
    }

    pub fn selected_member_action_index(&self) -> Option<usize> {
        let menu = self.member_action_menu.as_ref()?;
        Some(clamp_selected_index(
            menu.selected,
            self.selected_member_action_items().len(),
        ))
    }

    pub fn move_member_action_down(&mut self) {
        let len = self.selected_member_action_items().len();
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_down(&mut menu.selected, len);
        }
    }

    pub fn move_member_action_up(&mut self) {
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_up(&mut menu.selected);
        }
    }

    pub fn select_member_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_member_action_items().len() {
            return false;
        }
        if let Some(menu) = self.member_action_menu.as_mut() {
            menu.selected = row;
            return true;
        }
        false
    }

    pub fn activate_selected_member_action(&mut self) -> Option<AppCommand> {
        let menu = self.member_action_menu.clone()?;
        let items = self.selected_member_action_items();
        let item = items
            .get(clamp_selected_index(menu.selected, items.len()))?
            .clone();
        if !item.enabled {
            return None;
        }
        match item.kind {
            MemberActionKind::ShowProfile => {
                self.close_member_action_menu();
                self.open_user_profile_popup(menu.user_id, menu.guild_id)
            }
        }
    }

    pub fn activate_member_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        let actions = self.selected_member_action_items();
        let index = actions.iter().enumerate().position(|(index, action)| {
            action.enabled
                && member_action_shortcut(&actions, index)
                    .is_some_and(|candidate| candidate == shortcut)
        })?;
        self.select_member_action_row(index);
        self.activate_selected_member_action()
    }

    /// Opens the profile popup for `user_id`. Returns
    /// `AppCommand::LoadUserProfile` to fetch fresh data when nothing is
    /// cached yet — the popup itself shows a loading state in the meantime.
    pub fn open_user_profile_popup(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        self.user_profile_popup = Some(UserProfilePopupState {
            user_id,
            guild_id,
            load_error: None,
            scroll: 0,
            view_height: 0,
            total_lines: 0,
        });
        if self.discord.user_profile(user_id, guild_id).is_some() {
            None
        } else {
            Some(AppCommand::LoadUserProfile { user_id, guild_id })
        }
    }

    pub fn close_user_profile_popup(&mut self) {
        self.user_profile_popup = None;
    }

    pub fn user_profile_popup_data(&self) -> Option<&UserProfileInfo> {
        let popup = self.user_profile_popup.as_ref()?;
        self.discord.user_profile(popup.user_id, popup.guild_id)
    }

    pub fn user_profile_popup_load_error(&self) -> Option<&str> {
        self.user_profile_popup
            .as_ref()
            .and_then(|popup| popup.load_error.as_deref())
    }

    pub fn user_profile_popup_status(&self) -> PresenceStatus {
        let Some(popup) = self.user_profile_popup.as_ref() else {
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

        let recipient_status = self
            .discord
            .channels_for_guild(None)
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == popup.user_id)
            .map(|recipient| recipient.status);

        recipient_status
            .filter(|status| *status != PresenceStatus::Unknown)
            .or_else(|| self.discord.user_presence(popup.user_id))
            .unwrap_or(PresenceStatus::Unknown)
    }

    /// URL of the avatar image to render into the open profile popup. None
    /// when the popup is closed, the profile has not loaded yet, or the user
    /// has no avatar attachment.
    pub fn user_profile_popup_avatar_url(&self) -> Option<&str> {
        self.user_profile_popup_data()?.avatar_url.as_deref()
    }

    pub fn user_profile_popup_activities(&self) -> &[ActivityInfo] {
        let Some(popup) = self.user_profile_popup.as_ref() else {
            return &[];
        };
        self.discord.user_activities(popup.user_id)
    }

    pub fn user_activities(&self, user_id: Id<UserMarker>) -> &[ActivityInfo] {
        self.discord.user_activities(user_id)
    }

    /// Top-of-viewport row for the popup body. Used by the renderer.
    pub fn user_profile_popup_scroll(&self) -> usize {
        self.user_profile_popup
            .as_ref()
            .map(|popup| popup.scroll)
            .unwrap_or(0)
    }

    /// Renderer hook: passes the latest viewport height back so scroll
    /// methods can clamp without snapping past the last visible row.
    pub fn set_user_profile_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.view_height = height;
            clamp_user_profile_popup_scroll(popup);
        }
    }

    /// Renderer hook: stash the laid-out content height so scroll
    /// clamping is a constant-time check instead of recomputing layout.
    pub fn set_user_profile_popup_total_lines(&mut self, total_lines: usize) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.total_lines = total_lines;
            clamp_user_profile_popup_scroll(popup);
        }
    }

    pub fn scroll_user_profile_popup_down(&mut self) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.scroll = popup.scroll.saturating_add(1);
            clamp_user_profile_popup_scroll(popup);
        }
    }

    pub fn scroll_user_profile_popup_up(&mut self) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.scroll = popup.scroll.saturating_sub(1);
        }
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let members = self.discord.members_for_guild(guild_id);
        let roles = self.discord.roles_for_guild(guild_id);
        guild_member_groups(members, roles)
    }

    pub fn message_author_role_color(&self, message: &MessageState) -> Option<u32> {
        let channel = self.discord.channel(message.channel_id);
        let guild_id = message
            .guild_id
            .or_else(|| channel.and_then(|channel| channel.guild_id));
        let guild_id = guild_id?;
        self.discord.message_author_role_color(
            guild_id,
            message.channel_id,
            message.id,
            message.author_id,
        )
    }

    pub fn missing_message_author_profile_requests(
        &self,
    ) -> Vec<(Id<UserMarker>, Id<GuildMarker>)> {
        let mut seen = HashSet::new();
        let mut requests = Vec::new();

        for message in self.visible_messages() {
            let guild_id = message.guild_id.or_else(|| {
                self.discord
                    .channel(message.channel_id)
                    .and_then(|channel| channel.guild_id)
            });
            self.push_missing_author_profile_request(
                &mut requests,
                &mut seen,
                message.author_id,
                guild_id,
            );
        }

        for post in self.visible_forum_post_items() {
            let guild_id = self
                .discord
                .channel(post.channel_id)
                .and_then(|channel| channel.guild_id);
            if let Some(author_id) = post.preview_author_id {
                self.push_missing_author_profile_request(
                    &mut requests,
                    &mut seen,
                    author_id,
                    guild_id,
                );
            }
        }

        requests
    }

    fn push_missing_author_profile_request(
        &self,
        requests: &mut Vec<(Id<UserMarker>, Id<GuildMarker>)>,
        seen: &mut HashSet<(Id<UserMarker>, Id<GuildMarker>)>,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        let Some(guild_id) = guild_id else {
            return;
        };
        if self
            .discord
            .member_display_name(guild_id, user_id)
            .is_some()
            || self.discord.user_profile(user_id, Some(guild_id)).is_some()
            || !seen.insert((user_id, guild_id))
        {
            return;
        }
        requests.push((user_id, guild_id));
    }

    pub fn member_role_color(&self, member: MemberEntry<'_>) -> Option<u32> {
        let guild_id = self.selected_guild_id()?;
        self.discord.member_role_color(guild_id, member.user_id())
    }

    pub fn member_panel_title(&self) -> String {
        let Some(guild_id) = self.selected_guild_id() else {
            return "Members".to_owned();
        };
        let Some(member_count) = self
            .discord
            .guild(guild_id)
            .and_then(|guild| guild.member_count)
        else {
            return "Members".to_owned();
        };

        let loaded = self.discord.members_for_guild(guild_id).len();
        format!("Members {loaded}/{member_count} loaded")
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
