use std::collections::{HashSet, VecDeque};

use crate::discord::PresenceStatus;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use unicode_width::UnicodeWidthStr;

use super::scroll::{
    clamp_list_viewport, clamp_selected_index, last_index, move_index_down, move_index_down_by,
    move_index_up, move_index_up_by, pane_content_height, scroll_list_down, scroll_list_up,
};
use super::{
    ChannelPaneEntry, DashboardState, FocusPane, MemberEntry, MemberGroup, PaneFilterState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Debug)]
pub(super) struct NavigationState {
    pub(super) focus: FocusPane,
    pub(super) active_guild: ActiveGuildScope,
    pub(super) active_channel_id: Option<Id<ChannelMarker>>,
    pub(super) selected_guild: usize,
    pub(super) guild_scroll: usize,
    pub(super) guild_horizontal_scroll: usize,
    pub(super) guild_keep_selection_visible: bool,
    pub(super) guild_view_height: usize,
    pub(super) selected_channel: usize,
    pub(super) channel_scroll: usize,
    pub(super) channel_horizontal_scroll: usize,
    pub(super) channel_keep_selection_visible: bool,
    pub(super) channel_view_height: usize,
    pub(super) selected_member: usize,
    pub(super) member_scroll: usize,
    pub(super) member_horizontal_scroll: usize,
    pub(super) member_keep_selection_visible: bool,
    pub(super) member_view_height: usize,
    pub(super) recent_channel_ids: VecDeque<Id<ChannelMarker>>,
    pub(super) guild_pane_filter: Option<PaneFilterState>,
    pub(super) channel_pane_filter: Option<PaneFilterState>,
    pub(super) guild_pane_visible: bool,
    pub(super) channel_pane_visible: bool,
    pub(super) member_pane_visible: bool,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    pub(super) collapsed_folders: HashSet<FolderKey>,
    pub(super) collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            focus: FocusPane::Guilds,
            active_guild: ActiveGuildScope::Unset,
            active_channel_id: None,
            // Index 0 is the virtual "Direct Messages" entry. Start on the
            // first real guild when one exists. The bounds clamp inside
            // `selected_guild()` falls back to the DM entry while the guild
            // list is still empty.
            selected_guild: 1,
            guild_scroll: 0,
            guild_horizontal_scroll: 0,
            guild_keep_selection_visible: true,
            guild_view_height: 1,
            selected_channel: 0,
            channel_scroll: 0,
            channel_horizontal_scroll: 0,
            channel_keep_selection_visible: true,
            channel_view_height: 1,
            selected_member: 0,
            member_scroll: 0,
            member_horizontal_scroll: 0,
            member_keep_selection_visible: true,
            member_view_height: 1,
            recent_channel_ids: VecDeque::new(),
            guild_pane_filter: None,
            channel_pane_filter: None,
            guild_pane_visible: true,
            channel_pane_visible: true,
            member_pane_visible: true,
            collapsed_folders: HashSet::new(),
            collapsed_channel_categories: HashSet::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum FolderKey {
    Id(u64),
    Guilds(Vec<Id<GuildMarker>>),
}

impl DashboardState {
    pub fn focus(&self) -> FocusPane {
        self.navigation.focus
    }
}

impl DashboardState {
    pub fn is_pane_visible(&self, pane: FocusPane) -> bool {
        match pane {
            FocusPane::Guilds => self.navigation.guild_pane_visible,
            FocusPane::Channels => self.navigation.channel_pane_visible,
            FocusPane::Messages => true,
            FocusPane::Members => self.navigation.member_pane_visible,
        }
    }

    pub fn toggle_pane_visibility(&mut self, pane: FocusPane) {
        match pane {
            FocusPane::Guilds => {
                self.navigation.guild_pane_visible = !self.navigation.guild_pane_visible
            }
            FocusPane::Channels => {
                self.navigation.channel_pane_visible = !self.navigation.channel_pane_visible
            }
            FocusPane::Members => {
                self.navigation.member_pane_visible = !self.navigation.member_pane_visible
            }
            FocusPane::Messages => return,
        }
        if !self.is_pane_visible(self.navigation.focus) {
            self.navigation.focus = FocusPane::Messages;
        }
    }
}

impl DashboardState {
    pub fn selected_member(&self) -> usize {
        clamp_selected_index(
            self.navigation.selected_member,
            self.flattened_members().len(),
        )
    }

    #[cfg(test)]
    pub fn focused_member_selection_line(&self) -> Option<usize> {
        let groups = self.members_grouped();
        self.focused_member_selection_line_in_groups(&groups)
    }

    pub fn focused_member_selection_line_in_groups(
        &self,
        groups: &[MemberGroup<'_>],
    ) -> Option<usize> {
        if self.navigation.focus != FocusPane::Members {
            return None;
        }
        let selected_line = self.selected_member_line_in_groups(groups)?;
        if selected_line >= self.navigation.member_scroll
            && selected_line < self.navigation.member_scroll + self.member_content_height()
        {
            Some(selected_line - self.navigation.member_scroll)
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.navigation.member_scroll
    }

    pub fn guild_horizontal_scroll(&self) -> usize {
        self.navigation.guild_horizontal_scroll
    }

    pub fn channel_horizontal_scroll(&self) -> usize {
        self.navigation.channel_horizontal_scroll
    }

    pub fn member_horizontal_scroll(&self) -> usize {
        self.navigation.member_horizontal_scroll
    }

    pub fn member_content_height(&self) -> usize {
        pane_content_height(self.navigation.member_view_height)
    }

    #[cfg(test)]
    pub fn member_line_count(&self) -> usize {
        self.count_member_lines()
    }

    pub fn member_line_count_in_groups(&self, groups: &[MemberGroup<'_>]) -> usize {
        self.count_member_lines_in_groups(groups)
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        self.navigation.member_view_height = height;
        let selected_line = self.selected_member_line();
        let height = pane_content_height(self.navigation.member_view_height);
        let len = self.count_member_lines();
        clamp_list_viewport(
            selected_line,
            &mut self.navigation.member_scroll,
            height,
            len,
            self.navigation.member_keep_selection_visible,
        );
    }

    pub fn move_down(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                let len = self.guild_pane_filtered_entries().len();
                move_index_down(&mut self.navigation.selected_guild, len);
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.move_channel_selection_down();
            }
            FocusPane::Messages => {
                let len = self.message_pane_item_count();
                move_index_down(&mut self.messages.selected_message, len);
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                let len = self.flattened_members().len();
                move_index_down(&mut self.navigation.selected_member, len);
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                move_index_up(&mut self.navigation.selected_guild);
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.move_channel_selection_up();
            }
            FocusPane::Messages => {
                move_index_up(&mut self.messages.selected_message);
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                move_index_up(&mut self.navigation.selected_member);
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_top(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                self.navigation.selected_guild = 0;
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.jump_channel_selection_top();
            }
            FocusPane::Messages => {
                self.messages.selected_message = 0;
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                self.navigation.selected_member = 0;
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                self.navigation.selected_guild =
                    last_index(self.guild_pane_filtered_entries().len());
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.jump_channel_selection_bottom();
            }
            FocusPane::Messages => {
                self.messages.selected_message = last_index(self.message_pane_item_count());
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                self.navigation.selected_member = last_index(self.flattened_members().len());
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_down(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.navigation.guild_view_height) / 2;
                let len = self.guild_pane_filtered_entries().len();
                move_index_down_by(&mut self.navigation.selected_guild, len, distance.max(1));
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.navigation.channel_view_height) / 2;
                self.move_channel_selection_down_by(distance.max(1));
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                self.half_page_message_down(distance);
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.navigation.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_up(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.navigation.guild_view_height) / 2;
                move_index_up_by(&mut self.navigation.selected_guild, distance.max(1));
                self.navigation.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.navigation.channel_view_height) / 2;
                self.move_channel_selection_up_by(distance.max(1));
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                self.half_page_message_up(distance);
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.navigation.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.navigation.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn scroll_focused_pane_viewport_down(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                let height = pane_content_height(self.navigation.guild_view_height);
                let len = self.guild_pane_filtered_entries().len();
                self.navigation.guild_keep_selection_visible = false;
                scroll_list_down(&mut self.navigation.guild_scroll, height, len);
            }
            FocusPane::Channels => {
                let height = pane_content_height(self.navigation.channel_view_height);
                let len = self.channel_pane_filtered_entries().len();
                self.navigation.channel_keep_selection_visible = false;
                scroll_list_down(&mut self.navigation.channel_scroll, height, len);
            }
            FocusPane::Messages => self.scroll_message_viewport_down(),
            FocusPane::Members => {
                let height = pane_content_height(self.navigation.member_view_height);
                let len = self.count_member_lines();
                self.navigation.member_keep_selection_visible = false;
                scroll_list_down(&mut self.navigation.member_scroll, height, len);
            }
        }
    }

    pub fn scroll_focused_pane_viewport_up(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                self.navigation.guild_keep_selection_visible = false;
                scroll_list_up(&mut self.navigation.guild_scroll);
            }
            FocusPane::Channels => {
                self.navigation.channel_keep_selection_visible = false;
                scroll_list_up(&mut self.navigation.channel_scroll);
            }
            FocusPane::Messages => self.scroll_message_viewport_up(),
            FocusPane::Members => {
                self.navigation.member_keep_selection_visible = false;
                scroll_list_up(&mut self.navigation.member_scroll);
            }
        }
    }

    pub fn scroll_focused_pane_horizontal_right(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                self.navigation.guild_horizontal_scroll = self
                    .navigation
                    .guild_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_guild_horizontal_scroll());
            }
            FocusPane::Channels => {
                self.navigation.channel_horizontal_scroll = self
                    .navigation
                    .channel_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_channel_horizontal_scroll());
            }
            FocusPane::Members => {
                self.navigation.member_horizontal_scroll = self
                    .navigation
                    .member_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_member_horizontal_scroll());
            }
            FocusPane::Messages => {}
        }
    }

    fn max_guild_horizontal_scroll(&self) -> usize {
        self.guild_pane_filtered_entries()
            .into_iter()
            .map(|entry| entry.label().width().saturating_sub(1))
            .max()
            .unwrap_or_default()
    }

    fn max_channel_horizontal_scroll(&self) -> usize {
        self.channel_pane_filtered_entries()
            .into_iter()
            .map(|entry| entry.display_name().width().saturating_sub(1))
            .max()
            .unwrap_or_default()
    }

    fn max_member_horizontal_scroll(&self) -> usize {
        self.flattened_members()
            .into_iter()
            .map(|member| member.display_name().width().saturating_sub(1))
            .max()
            .unwrap_or_default()
    }

    pub fn scroll_focused_pane_horizontal_left(&mut self) {
        match self.navigation.focus {
            FocusPane::Guilds => {
                self.navigation.guild_horizontal_scroll =
                    self.navigation.guild_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Channels => {
                self.navigation.channel_horizontal_scroll =
                    self.navigation.channel_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Members => {
                self.navigation.member_horizontal_scroll =
                    self.navigation.member_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Messages => {}
        }
    }

    pub fn cycle_focus(&mut self) {
        self.navigation.focus = self.next_visible_focus(false);
    }

    pub fn cycle_focus_backward(&mut self) {
        self.navigation.focus = self.next_visible_focus(true);
    }

    pub fn focus_pane(&mut self, pane: FocusPane) {
        if self.is_pane_visible(pane) {
            self.navigation.focus = pane;
        }
    }

    pub fn show_and_focus_pane(&mut self, pane: FocusPane) {
        match pane {
            FocusPane::Guilds => self.navigation.guild_pane_visible = true,
            FocusPane::Channels => self.navigation.channel_pane_visible = true,
            FocusPane::Members => self.navigation.member_pane_visible = true,
            FocusPane::Messages => {}
        }
        self.navigation.focus = pane;
    }

    fn next_visible_focus(&self, backward: bool) -> FocusPane {
        let panes = [
            FocusPane::Guilds,
            FocusPane::Channels,
            FocusPane::Messages,
            FocusPane::Members,
        ];
        let current = panes
            .iter()
            .position(|pane| *pane == self.navigation.focus)
            .unwrap_or(2);
        for step in 1..=panes.len() {
            let index = if backward {
                (current + panes.len() - step) % panes.len()
            } else {
                (current + step) % panes.len()
            };
            if self.is_pane_visible(panes[index]) {
                return panes[index];
            }
        }
        FocusPane::Messages
    }

    pub fn select_visible_pane_row(&mut self, pane: FocusPane, row: usize) -> bool {
        match pane {
            FocusPane::Guilds => self.select_visible_guild_row(row),
            FocusPane::Channels => self.select_visible_channel_row(row),
            FocusPane::Messages => self.select_visible_message_row(row),
            FocusPane::Members => self.select_visible_member_line(row),
        }
    }

    fn select_visible_guild_row(&mut self, row: usize) -> bool {
        let index = self.navigation.guild_scroll.saturating_add(row);
        if index >= self.guild_pane_filtered_entries().len() {
            return false;
        }
        self.navigation.selected_guild = index;
        self.navigation.guild_keep_selection_visible = true;
        true
    }

    fn select_visible_channel_row(&mut self, row: usize) -> bool {
        let index = self.navigation.channel_scroll.saturating_add(row);
        let entries = self.channel_pane_filtered_entries();
        if !entries
            .get(index)
            .is_some_and(ChannelPaneEntry::is_selectable)
        {
            return false;
        }
        self.navigation.selected_channel = index;
        self.navigation.channel_keep_selection_visible = true;
        true
    }

    fn select_visible_member_line(&mut self, row: usize) -> bool {
        let target_line = self.navigation.member_scroll.saturating_add(row);
        for (member_index, line_index) in self.member_line_indices() {
            if line_index == target_line {
                self.navigation.selected_member = member_index;
                self.navigation.member_keep_selection_visible = true;
                return true;
            }
        }
        false
    }

    pub(super) fn clamp_selection_indices(&mut self) {
        self.navigation.selected_guild = self.selected_guild();
        self.navigation.selected_channel = self.selected_channel();
        self.messages.selected_message = self.selected_message();
        self.navigation.selected_member = self.selected_member();
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    pub(super) fn clamp_active_selection(&mut self) {
        if let ActiveGuildScope::Guild(guild_id) = self.navigation.active_guild
            && !self
                .discord
                .guilds()
                .iter()
                .any(|guild| guild.id == guild_id)
        {
            self.navigation.active_guild = ActiveGuildScope::Unset;
        }

        let active_channel_is_valid = self
            .navigation
            .active_channel_id
            .and_then(|channel_id| self.discord.cache.channel(channel_id))
            .is_some_and(|channel| match self.navigation.active_guild {
                ActiveGuildScope::Unset => false,
                ActiveGuildScope::DirectMessages => {
                    channel.guild_id.is_none() && !channel.is_category()
                }
                ActiveGuildScope::Guild(guild_id) => {
                    channel.guild_id == Some(guild_id)
                        && !channel.is_category()
                        && self.discord.cache.can_view_channel(channel)
                }
            });
        if self.navigation.active_channel_id.is_some() && !active_channel_is_valid {
            self.clear_active_channel();
        }
    }

    fn clear_active_channel(&mut self) {
        self.navigation.active_channel_id = None;
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_keep_selection_visible = true;
        self.messages.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.navigation.channel_keep_selection_visible = true;
        self.navigation.member_keep_selection_visible = true;
        self.cancel_composer();
        self.close_message_action_menu();
        self.close_channel_leader_action();
        self.close_emoji_reaction_picker();
        self.close_poll_vote_picker();
        self.close_reaction_users_popup();
        self.messages.thread_return_target = None;
    }

    pub(super) fn clamp_list_viewports(&mut self) {
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    pub(super) fn clamp_guild_viewport(&mut self) {
        let entries_len = self.guild_pane_filtered_entries().len();
        self.navigation.selected_guild =
            clamp_selected_index(self.navigation.selected_guild, entries_len);
        clamp_list_viewport(
            self.navigation.selected_guild,
            &mut self.navigation.guild_scroll,
            pane_content_height(self.navigation.guild_view_height),
            entries_len,
            self.navigation.guild_keep_selection_visible,
        );
    }

    pub(super) fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_filtered_entries().len();
        self.navigation.selected_channel =
            clamp_selected_index(self.navigation.selected_channel, entries_len);
        clamp_list_viewport(
            self.navigation.selected_channel,
            &mut self.navigation.channel_scroll,
            pane_content_height(self.navigation.channel_view_height),
            entries_len,
            self.navigation.channel_keep_selection_visible,
        );
    }

    pub(super) fn clamp_member_viewport(&mut self) {
        let members_len = self.flattened_members().len();
        if members_len == 0 {
            self.navigation.selected_member = 0;
            self.navigation.member_scroll = 0;
            return;
        }

        self.navigation.selected_member = self.navigation.selected_member.min(members_len - 1);
        let selected_line = self.selected_member_line();
        let height = pane_content_height(self.navigation.member_view_height);
        let len = self.count_member_lines();
        clamp_list_viewport(
            selected_line,
            &mut self.navigation.member_scroll,
            height,
            len,
            self.navigation.member_keep_selection_visible,
        );
    }

    pub(super) fn selected_member_line(&self) -> usize {
        let groups = self.members_grouped();
        self.selected_member_line_in_groups(&groups)
            .unwrap_or_default()
    }

    fn selected_member_line_in_groups(&self, groups: &[MemberGroup<'_>]) -> Option<usize> {
        let members_len: usize = groups.iter().map(|group| group.entries.len()).sum();
        if members_len == 0 {
            return None;
        }
        let selected_member = self.navigation.selected_member.min(members_len - 1);
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in groups {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for member in &group.entries {
                if member_index == selected_member {
                    return Some(line_index);
                }
                member_index += 1;
                line_index += 1;
                if self.member_has_activity_row(*member) {
                    line_index += 1;
                }
            }
        }
        None
    }

    fn select_member_near_line(&mut self, target_line: usize) {
        let mut last_member = None;
        for (member_index, line_index) in self.member_line_indices() {
            if line_index >= target_line {
                self.navigation.selected_member = member_index;
                return;
            }
            last_member = Some(member_index);
        }

        if let Some(member_index) = last_member {
            self.navigation.selected_member = member_index;
        }
    }

    fn member_line_indices(&self) -> Vec<(usize, usize)> {
        let mut indices = Vec::new();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for member in group.entries {
                indices.push((member_index, line_index));
                member_index += 1;
                line_index += 1;
                if self.member_has_activity_row(member) {
                    line_index += 1;
                }
            }
        }
        indices
    }

    fn count_member_lines(&self) -> usize {
        let groups = self.members_grouped();
        self.count_member_lines_in_groups(&groups)
    }

    fn count_member_lines_in_groups(&self, groups: &[MemberGroup<'_>]) -> usize {
        let mut lines = 0usize;
        for group in groups {
            if lines > 0 {
                lines += 1;
            }
            lines += 1;
            for member in &group.entries {
                lines += 1;
                if self.member_has_activity_row(*member) {
                    lines += 1;
                }
            }
        }
        lines
    }

    /// Must mirror `tui::ui::panes::render_members`. Line counting and
    /// selection drift apart silently if the predicates diverge.
    fn member_has_activity_row(&self, member: MemberEntry<'_>) -> bool {
        if matches!(
            member.status(),
            PresenceStatus::Offline | PresenceStatus::Unknown
        ) {
            return false;
        }
        !self.user_activities(member.user_id()).is_empty()
    }
}
