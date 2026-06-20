use std::collections::{HashSet, VecDeque};

use crate::config::{DEFAULT_CHANNEL_LIST_WIDTH, DEFAULT_MEMBER_LIST_WIDTH, DEFAULT_SERVER_WIDTH};
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
use crate::tui::text_input::TextInputState;

const MIN_PANE_WIDTH: u16 = 8;
const MAX_PANE_WIDTH: u16 = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusedNavigationAction {
    MoveDown,
    MoveUp,
    JumpTop,
    JumpBottom,
    HalfPageDown,
    HalfPageUp,
    ScrollViewportDown,
    ScrollViewportUp,
    ScrollHorizontalRight,
    ScrollHorizontalLeft,
}

#[derive(Debug)]
pub(super) struct NavigationState {
    pub(super) focus: FocusPane,
    pub(super) guilds: GuildPaneNavigationState,
    pub(super) channels: ChannelPaneNavigationState,
    pub(super) members: MemberPaneNavigationState,
}

#[derive(Debug)]
pub(super) struct GuildPaneNavigationState {
    pub(super) active: ActiveGuildScope,
    pub(super) list: PaneListState,
    pub(super) filter: Option<PaneFilterState>,
    pub(super) visible: bool,
    pub(super) width: u16,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    pub(super) collapsed_folders: HashSet<FolderKey>,
    pub(super) folder_settings: Option<FolderSettingsState>,
}

#[derive(Debug)]
pub(super) struct ChannelPaneNavigationState {
    pub(super) active_channel_id: Option<Id<ChannelMarker>>,
    pub(super) list: PaneListState,
    pub(super) recent_channel_ids: VecDeque<Id<ChannelMarker>>,
    pub(super) filter: Option<PaneFilterState>,
    pub(super) visible: bool,
    pub(super) width: u16,
    pub(super) collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
}

#[derive(Debug)]
pub(super) struct MemberPaneNavigationState {
    pub(super) list: PaneListState,
    pub(super) visible: bool,
    pub(super) width: u16,
}

#[derive(Debug)]
pub(super) struct FolderSettingsState {
    pub(super) folder_id: u64,
    pub(super) active_field: FolderSettingsField,
    pub(super) editing_field: Option<FolderSettingsField>,
    pub(super) edit_input: TextInputState,
    pub(super) name_input: TextInputState,
    pub(super) color_input: TextInputState,
    pub(super) color_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum FolderSettingsField {
    #[default]
    Name,
    Color,
}

#[derive(Debug)]
pub(super) struct PaneListState {
    pub(super) selected: usize,
    pub(super) scroll: usize,
    pub(super) horizontal_scroll: usize,
    pub(super) keep_selection_visible: bool,
    pub(super) view_height: usize,
}

impl PaneListState {
    fn new(selected: usize) -> Self {
        Self {
            selected,
            scroll: 0,
            horizontal_scroll: 0,
            keep_selection_visible: true,
            view_height: 1,
        }
    }

    pub(super) fn content_height(&self) -> usize {
        pane_content_height(self.view_height)
    }

    pub(super) fn keep_selection_visible(&mut self) {
        self.keep_selection_visible = true;
    }

    pub(super) fn allow_detached_scroll(&mut self) {
        self.keep_selection_visible = false;
    }

    pub(super) fn reset_selection_and_scroll(&mut self) {
        self.selected = 0;
        self.scroll = 0;
        self.keep_selection_visible();
    }

    pub(super) fn set_view_height_and_clamp(&mut self, height: usize, cursor: usize, len: usize) {
        self.view_height = height;
        self.clamp_viewport(cursor, len);
    }

    pub(super) fn clamp_selected(&mut self, len: usize) {
        self.selected = clamp_selected_index(self.selected, len);
    }

    pub(super) fn clamp_viewport(&mut self, cursor: usize, len: usize) {
        let height = self.content_height();
        clamp_list_viewport(
            cursor,
            &mut self.scroll,
            height,
            len,
            self.keep_selection_visible,
        );
    }

    fn move_down(&mut self, len: usize) {
        move_index_down(&mut self.selected, len);
        self.keep_selection_visible();
    }

    fn move_up(&mut self) {
        move_index_up(&mut self.selected);
        self.keep_selection_visible();
    }

    fn move_down_by(&mut self, len: usize, distance: usize) {
        move_index_down_by(&mut self.selected, len, distance);
        self.keep_selection_visible();
    }

    fn move_up_by(&mut self, distance: usize) {
        move_index_up_by(&mut self.selected, distance);
        self.keep_selection_visible();
    }

    fn jump_top(&mut self) {
        self.selected = 0;
        self.keep_selection_visible();
    }

    fn jump_bottom(&mut self, len: usize) {
        self.selected = last_index(len);
        self.keep_selection_visible();
    }

    fn scroll_down(&mut self, len: usize) {
        self.allow_detached_scroll();
        let height = self.content_height();
        scroll_list_down(&mut self.scroll, height, len);
    }

    fn scroll_up(&mut self) {
        self.allow_detached_scroll();
        scroll_list_up(&mut self.scroll);
    }

    fn scroll_horizontal_right(&mut self, max: usize) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_add(1).min(max);
    }

    fn scroll_horizontal_left(&mut self) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(1);
    }
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            focus: FocusPane::Guilds,
            guilds: GuildPaneNavigationState::default(),
            channels: ChannelPaneNavigationState::default(),
            members: MemberPaneNavigationState::default(),
        }
    }
}

impl Default for GuildPaneNavigationState {
    fn default() -> Self {
        Self {
            active: ActiveGuildScope::Unset,
            // Index 0 is the virtual "Direct Messages" entry. Start on the
            // first real guild when one exists. The bounds clamp inside
            // `selected_guild()` falls back to the DM entry while the guild
            // list is still empty.
            list: PaneListState::new(1),
            filter: None,
            visible: true,
            width: DEFAULT_SERVER_WIDTH,
            collapsed_folders: HashSet::new(),
            folder_settings: None,
        }
    }
}

impl Default for ChannelPaneNavigationState {
    fn default() -> Self {
        Self {
            active_channel_id: None,
            list: PaneListState::new(0),
            recent_channel_ids: VecDeque::new(),
            filter: None,
            visible: true,
            width: DEFAULT_CHANNEL_LIST_WIDTH,
            collapsed_channel_categories: HashSet::new(),
        }
    }
}

impl Default for MemberPaneNavigationState {
    fn default() -> Self {
        Self {
            list: PaneListState::new(0),
            visible: true,
            width: DEFAULT_MEMBER_LIST_WIDTH,
        }
    }
}

impl NavigationState {
    fn pane_visible(&self, pane: FocusPane) -> bool {
        match pane {
            FocusPane::Guilds => self.guilds.visible,
            FocusPane::Channels => self.channels.visible,
            FocusPane::Messages => true,
            FocusPane::Members => self.members.visible,
        }
    }

    fn pane_visible_mut(&mut self, pane: FocusPane) -> Option<&mut bool> {
        match pane {
            FocusPane::Guilds => Some(&mut self.guilds.visible),
            FocusPane::Channels => Some(&mut self.channels.visible),
            FocusPane::Members => Some(&mut self.members.visible),
            FocusPane::Messages => None,
        }
    }

    fn pane_width(&self, pane: FocusPane) -> u16 {
        match pane {
            FocusPane::Guilds => self.guilds.width,
            FocusPane::Channels => self.channels.width,
            FocusPane::Members => self.members.width,
            FocusPane::Messages => 0,
        }
    }

    fn pane_width_mut(&mut self, pane: FocusPane) -> Option<&mut u16> {
        match pane {
            FocusPane::Guilds => Some(&mut self.guilds.width),
            FocusPane::Channels => Some(&mut self.channels.width),
            FocusPane::Members => Some(&mut self.members.width),
            FocusPane::Messages => None,
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
        self.navigation.pane_visible(pane)
    }

    pub fn toggle_pane_visibility(&mut self, pane: FocusPane) {
        let Some(visible) = self.navigation.pane_visible_mut(pane) else {
            return;
        };
        *visible = !*visible;
        self.options.ui_state_save_pending = true;
        if !self.is_pane_visible(self.navigation.focus) {
            self.navigation.focus = FocusPane::Messages;
        }
    }

    pub fn pane_width(&self, pane: FocusPane) -> u16 {
        self.navigation.pane_width(pane)
    }

    pub fn adjust_focused_pane_width(&mut self, delta: i16) {
        let Some(width) = self.navigation.pane_width_mut(self.navigation.focus) else {
            return;
        };

        let adjusted = if delta.is_negative() {
            width.saturating_sub(delta.unsigned_abs())
        } else {
            width.saturating_add(delta as u16)
        };
        let adjusted = adjusted.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH);
        if adjusted != *width {
            *width = adjusted;
            self.options.ui_state_save_pending = true;
        }
    }
}

impl DashboardState {
    pub fn selected_member(&self) -> usize {
        clamp_selected_index(
            self.navigation.members.list.selected,
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
        if selected_line >= self.navigation.members.list.scroll
            && selected_line < self.navigation.members.list.scroll + self.member_content_height()
        {
            Some(selected_line - self.navigation.members.list.scroll)
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.navigation.members.list.scroll
    }

    pub fn guild_horizontal_scroll(&self) -> usize {
        self.navigation.guilds.list.horizontal_scroll
    }

    pub fn channel_horizontal_scroll(&self) -> usize {
        self.navigation.channels.list.horizontal_scroll
    }

    pub fn member_horizontal_scroll(&self) -> usize {
        self.navigation.members.list.horizontal_scroll
    }

    pub fn member_content_height(&self) -> usize {
        self.navigation.members.list.content_height()
    }

    #[cfg(test)]
    pub fn member_line_count(&self) -> usize {
        self.count_member_lines()
    }

    pub fn member_line_count_in_groups(&self, groups: &[MemberGroup<'_>]) -> usize {
        self.count_member_lines_in_groups(groups)
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        let selected_line = self.selected_member_line();
        let len = self.count_member_lines();
        self.navigation
            .members
            .list
            .set_view_height_and_clamp(height, selected_line, len);
    }

    pub fn move_down(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::MoveDown);
    }

    pub fn move_up(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::MoveUp);
    }

    pub fn jump_top(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::JumpTop);
    }

    pub fn jump_bottom(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::JumpBottom);
    }

    pub fn half_page_down(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::HalfPageDown);
    }

    pub fn half_page_up(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::HalfPageUp);
    }

    pub fn scroll_focused_pane_viewport_down(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::ScrollViewportDown);
    }

    pub fn scroll_focused_pane_viewport_up(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::ScrollViewportUp);
    }

    pub fn scroll_focused_pane_horizontal_right(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::ScrollHorizontalRight);
    }

    pub fn scroll_focused_pane_horizontal_left(&mut self) {
        self.apply_focused_navigation(FocusedNavigationAction::ScrollHorizontalLeft);
    }

    fn apply_focused_navigation(&mut self, action: FocusedNavigationAction) {
        match self.navigation.focus {
            FocusPane::Guilds => self.apply_guild_navigation(action),
            FocusPane::Channels => self.apply_channel_navigation(action),
            FocusPane::Messages => self.apply_message_navigation(action),
            FocusPane::Members => self.apply_member_navigation(action),
        }
    }

    fn apply_guild_navigation(&mut self, action: FocusedNavigationAction) {
        match action {
            FocusedNavigationAction::MoveDown => {
                let len = self.guild_pane_filtered_entries().len();
                self.navigation.guilds.list.move_down(len);
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::MoveUp => {
                self.navigation.guilds.list.move_up();
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::JumpTop => {
                self.navigation.guilds.list.jump_top();
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::JumpBottom => {
                let len = self.guild_pane_filtered_entries().len();
                self.navigation.guilds.list.jump_bottom(len);
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::HalfPageDown => {
                let distance = self.navigation.guilds.list.content_height() / 2;
                let len = self.guild_pane_filtered_entries().len();
                self.navigation
                    .guilds
                    .list
                    .move_down_by(len, distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::HalfPageUp => {
                let distance = self.navigation.guilds.list.content_height() / 2;
                self.navigation.guilds.list.move_up_by(distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusedNavigationAction::ScrollViewportDown => {
                let len = self.guild_pane_filtered_entries().len();
                self.navigation.guilds.list.scroll_down(len);
            }
            FocusedNavigationAction::ScrollViewportUp => self.navigation.guilds.list.scroll_up(),
            FocusedNavigationAction::ScrollHorizontalRight => {
                let max = self.max_guild_horizontal_scroll();
                self.navigation.guilds.list.scroll_horizontal_right(max);
            }
            FocusedNavigationAction::ScrollHorizontalLeft => {
                self.navigation.guilds.list.scroll_horizontal_left();
            }
        }
    }

    fn apply_channel_navigation(&mut self, action: FocusedNavigationAction) {
        match action {
            FocusedNavigationAction::MoveDown => self.move_channel_selection_down(),
            FocusedNavigationAction::MoveUp => self.move_channel_selection_up(),
            FocusedNavigationAction::JumpTop => self.jump_channel_selection_top(),
            FocusedNavigationAction::JumpBottom => self.jump_channel_selection_bottom(),
            FocusedNavigationAction::HalfPageDown => {
                let distance = self.navigation.channels.list.content_height() / 2;
                self.move_channel_selection_down_by(distance.max(1));
            }
            FocusedNavigationAction::HalfPageUp => {
                let distance = self.navigation.channels.list.content_height() / 2;
                self.move_channel_selection_up_by(distance.max(1));
            }
            FocusedNavigationAction::ScrollViewportDown => {
                let len = self.channel_pane_filtered_entries().len();
                self.navigation.channels.list.scroll_down(len);
            }
            FocusedNavigationAction::ScrollViewportUp => self.navigation.channels.list.scroll_up(),
            FocusedNavigationAction::ScrollHorizontalRight => {
                let max = self.max_channel_horizontal_scroll();
                self.navigation.channels.list.scroll_horizontal_right(max);
            }
            FocusedNavigationAction::ScrollHorizontalLeft => {
                self.navigation.channels.list.scroll_horizontal_left();
            }
        }
    }

    fn apply_message_navigation(&mut self, action: FocusedNavigationAction) {
        match action {
            FocusedNavigationAction::MoveDown => {
                let len = self.message_pane_item_count();
                move_index_down(&mut self.messages.selected_message, len);
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusedNavigationAction::MoveUp => {
                move_index_up(&mut self.messages.selected_message);
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusedNavigationAction::JumpTop => {
                self.messages.selected_message = 0;
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusedNavigationAction::JumpBottom => {
                self.messages.selected_message = last_index(self.message_pane_item_count());
                self.messages.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusedNavigationAction::HalfPageDown => {
                let distance = self.message_content_height() / 2;
                self.half_page_message_down(distance);
            }
            FocusedNavigationAction::HalfPageUp => {
                let distance = self.message_content_height() / 2;
                self.half_page_message_up(distance);
            }
            FocusedNavigationAction::ScrollViewportDown => self.scroll_message_viewport_down(),
            FocusedNavigationAction::ScrollViewportUp => self.scroll_message_viewport_up(),
            FocusedNavigationAction::ScrollHorizontalRight
            | FocusedNavigationAction::ScrollHorizontalLeft => {}
        }
    }

    fn apply_member_navigation(&mut self, action: FocusedNavigationAction) {
        match action {
            FocusedNavigationAction::MoveDown => {
                let len = self.flattened_members().len();
                self.navigation.members.list.move_down(len);
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::MoveUp => {
                self.navigation.members.list.move_up();
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::JumpTop => {
                self.navigation.members.list.jump_top();
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::JumpBottom => {
                let len = self.flattened_members().len();
                self.navigation.members.list.jump_bottom(len);
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::HalfPageDown => {
                let distance = self.navigation.members.list.content_height() / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.navigation.members.list.keep_selection_visible();
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::HalfPageUp => {
                let distance = self.navigation.members.list.content_height() / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.navigation.members.list.keep_selection_visible();
                self.clamp_member_viewport();
            }
            FocusedNavigationAction::ScrollViewportDown => {
                let len = self.count_member_lines();
                self.navigation.members.list.scroll_down(len);
            }
            FocusedNavigationAction::ScrollViewportUp => self.navigation.members.list.scroll_up(),
            FocusedNavigationAction::ScrollHorizontalRight => {
                let max = self.max_member_horizontal_scroll();
                self.navigation.members.list.scroll_horizontal_right(max);
            }
            FocusedNavigationAction::ScrollHorizontalLeft => {
                self.navigation.members.list.scroll_horizontal_left();
            }
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
        let was_visible = self.is_pane_visible(pane);
        if let Some(visible) = self.navigation.pane_visible_mut(pane) {
            *visible = true;
        }
        if !was_visible && pane != FocusPane::Messages {
            self.options.ui_state_save_pending = true;
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
        let index = self.navigation.guilds.list.scroll.saturating_add(row);
        if index >= self.guild_pane_filtered_entries().len() {
            return false;
        }
        self.navigation.guilds.list.selected = index;
        self.navigation.guilds.list.keep_selection_visible();
        true
    }

    fn select_visible_channel_row(&mut self, row: usize) -> bool {
        let index = self.navigation.channels.list.scroll.saturating_add(row);
        let entries = self.channel_pane_filtered_entries();
        if !entries
            .get(index)
            .is_some_and(ChannelPaneEntry::is_selectable)
        {
            return false;
        }
        self.navigation.channels.list.selected = index;
        self.navigation.channels.list.keep_selection_visible();
        true
    }

    fn select_visible_member_line(&mut self, row: usize) -> bool {
        let target_line = self.navigation.members.list.scroll.saturating_add(row);
        for (member_index, line_index) in self.member_line_indices() {
            if line_index == target_line {
                self.navigation.members.list.selected = member_index;
                self.navigation.members.list.keep_selection_visible();
                return true;
            }
        }
        false
    }

    pub(super) fn clamp_selection_indices(&mut self) {
        self.navigation.guilds.list.selected = self.selected_guild();
        self.navigation.channels.list.selected = self.selected_channel();
        self.messages.selected_message = self.selected_message();
        self.navigation.members.list.selected = self.selected_member();
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    pub(super) fn clamp_active_selection(&mut self) {
        if let ActiveGuildScope::Guild(guild_id) = self.navigation.guilds.active
            && !self
                .discord
                .guilds()
                .iter()
                .any(|guild| guild.id == guild_id)
        {
            self.navigation.guilds.active = ActiveGuildScope::Unset;
        }

        let active_channel_is_valid = self
            .navigation
            .channels
            .active_channel_id
            .and_then(|channel_id| self.discord.cache.channel(channel_id))
            .is_some_and(|channel| match self.navigation.guilds.active {
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
        if self.navigation.channels.active_channel_id.is_some() && !active_channel_is_valid {
            self.clear_active_channel();
        }
    }

    fn clear_active_channel(&mut self) {
        self.navigation.channels.active_channel_id = None;
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_keep_selection_visible = true;
        self.messages.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.navigation.channels.list.keep_selection_visible();
        self.navigation.members.list.keep_selection_visible();
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
        self.navigation.guilds.list.clamp_selected(entries_len);
        let selected = self.navigation.guilds.list.selected;
        self.navigation
            .guilds
            .list
            .clamp_viewport(selected, entries_len);
    }

    pub(super) fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_filtered_entries().len();
        self.navigation.channels.list.clamp_selected(entries_len);
        let selected = self.navigation.channels.list.selected;
        self.navigation
            .channels
            .list
            .clamp_viewport(selected, entries_len);
    }

    pub(super) fn clamp_member_viewport(&mut self) {
        let members_len = self.flattened_members().len();
        if members_len == 0 {
            self.navigation.members.list.selected = 0;
            self.navigation.members.list.scroll = 0;
            return;
        }

        self.navigation.members.list.selected =
            self.navigation.members.list.selected.min(members_len - 1);
        let selected_line = self.selected_member_line();
        let len = self.count_member_lines();
        self.navigation
            .members
            .list
            .clamp_viewport(selected_line, len);
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
        let selected_member = self.navigation.members.list.selected.min(members_len - 1);
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
                self.navigation.members.list.selected = member_index;
                return;
            }
            last_member = Some(member_index);
        }

        if let Some(member_index) = last_member {
            self.navigation.members.list.selected = member_index;
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
