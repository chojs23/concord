use std::collections::{HashMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};
use crate::discord::{AppCommand, AppEvent, GuildFolder, GuildState};

use super::{ActiveGuildScope, DashboardState, FolderKey};
use super::{
    model::{
        FocusPane, GuildActionItem, GuildActionKind, GuildBranch, GuildPaneEntry,
        MUTE_ACTION_DURATIONS, guild_action_shortcut, indexed_shortcut,
    },
    popups::GuildActionMenuState,
    scroll::{
        clamp_list_viewport, clamp_selected_index, close_collapsed_key, move_index_down,
        move_index_up, open_collapsed_key, pane_content_height, toggle_collapsed_key,
    },
};

impl DashboardState {
    pub fn guild_name(&self, guild_id: Id<GuildMarker>) -> Option<&str> {
        self.discord
            .guild(guild_id)
            .map(|state| state.name.as_str())
    }

    /// Builds the guild pane in display order: a virtual "Direct Messages"
    /// row, then each `guild_folders` entry expanded into either a single
    /// guild row (`id == None`, one member) or a folder header followed by
    /// indented children. Collapsed folders hide their children. Guilds that
    /// the user is in but the folder list omits get appended at the bottom.
    pub fn guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        let mut entries: Vec<GuildPaneEntry<'_>> = vec![GuildPaneEntry::DirectMessages];
        let by_id: HashMap<Id<GuildMarker>, &GuildState> = self
            .discord
            .guilds()
            .into_iter()
            .map(|guild| (guild.id, guild))
            .collect();
        let mut placed: HashSet<Id<GuildMarker>> = HashSet::new();
        let folders = self.discord.guild_folders();

        if folders.is_empty() {
            // Iterating `by_id.values()` here is non-deterministic because
            // it's a HashMap, which makes the sidebar shuffle on every render.
            // Fall back to the discord state's own (insertion-ordered) guild
            // list so the order stays stable until folder data arrives.
            for guild in self.discord.guilds() {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
            return entries;
        }

        for folder in folders {
            let is_single_container = folder.id.is_none() && folder.guild_ids.len() == 1;
            if is_single_container {
                if let Some(guild) = by_id.get(&folder.guild_ids[0]) {
                    entries.push(GuildPaneEntry::Guild {
                        state: guild,
                        branch: GuildBranch::None,
                    });
                    placed.insert(folder.guild_ids[0]);
                }
                continue;
            }

            let folder_key = Self::folder_key(folder);
            let collapsed = folder_key
                .as_ref()
                .is_some_and(|key| self.collapsed_folders.contains(key));
            entries.push(GuildPaneEntry::FolderHeader { folder, collapsed });

            // Always mark children as placed even when collapsed so we don't
            // duplicate them in the trailing "ungrouped" loop.
            for guild_id in &folder.guild_ids {
                placed.insert(*guild_id);
            }

            if collapsed {
                continue;
            }

            let child_guilds: Vec<&GuildState> = folder
                .guild_ids
                .iter()
                .filter_map(|guild_id| by_id.get(guild_id).copied())
                .collect();
            let last_child_index = child_guilds.len().saturating_sub(1);
            for (index, guild) in child_guilds.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    GuildBranch::Last
                } else {
                    GuildBranch::Middle
                };
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch,
                });
            }
        }

        // Same reasoning as the folder-empty branch above: walk the discord
        // state's BTreeMap-backed list so the trailing "ungrouped" guilds
        // appear in a stable, deterministic order.
        for guild in self.discord.guilds() {
            if !placed.contains(&guild.id) {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
        }

        entries
    }

    pub fn selected_guild(&self) -> usize {
        clamp_selected_index(self.selected_guild, self.guild_pane_entries().len())
    }

    pub fn guild_scroll(&self) -> usize {
        self.guild_scroll
    }

    pub fn visible_guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        self.guild_pane_entries()
            .into_iter()
            .skip(self.guild_scroll)
            .take(pane_content_height(self.guild_view_height))
            .collect()
    }

    pub fn focused_guild_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Guilds && !self.guild_pane_entries().is_empty() {
            let selected = self.selected_guild();
            let visible_len = self.visible_guild_pane_entries().len();
            if selected >= self.guild_scroll && selected < self.guild_scroll + visible_len {
                Some(selected - self.guild_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        self.guild_view_height = height;
        let height = pane_content_height(self.guild_view_height);
        let len = self.guild_pane_entries().len();
        clamp_list_viewport(
            self.selected_guild,
            &mut self.guild_scroll,
            height,
            len,
            self.guild_keep_selection_visible,
        );
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => None,
        }
    }

    pub fn selected_guild_cursor_id(&self) -> Option<Id<GuildMarker>> {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::Guild { state, .. }) => Some(state.id),
            Some(GuildPaneEntry::DirectMessages | GuildPaneEntry::FolderHeader { .. }) | None => {
                None
            }
        }
    }

    pub fn is_active_guild_entry(&self, entry: &GuildPaneEntry<'_>) -> bool {
        match (self.active_guild, entry) {
            (ActiveGuildScope::DirectMessages, GuildPaneEntry::DirectMessages) => true,
            (ActiveGuildScope::Guild(active_id), GuildPaneEntry::Guild { state, .. }) => {
                state.id == active_id
            }
            (ActiveGuildScope::Unset, _)
            | (ActiveGuildScope::DirectMessages, _)
            | (ActiveGuildScope::Guild(_), _) => false,
        }
    }

    pub fn open_selected_guild_actions(&mut self) {
        if self.focus != FocusPane::Guilds {
            return;
        }
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::DirectMessages | GuildPaneEntry::Guild { .. }) => {
                self.guild_action_menu = Some(GuildActionMenuState::Actions { selected: 0 });
            }
            Some(GuildPaneEntry::FolderHeader { .. }) | None => {}
        }
    }

    pub fn close_guild_action_menu(&mut self) {
        self.guild_action_menu = None;
    }

    pub fn guild_action_menu_title(&self) -> Option<String> {
        match self.guild_pane_entries().get(self.selected_guild())? {
            GuildPaneEntry::DirectMessages => Some("Direct Messages".to_owned()),
            GuildPaneEntry::Guild { state, .. } => Some(state.name.clone()),
            GuildPaneEntry::FolderHeader { .. } => None,
        }
    }

    pub fn selected_guild_action_items(&self) -> Vec<GuildActionItem> {
        if self.guild_action_menu.is_none() {
            return Vec::new();
        }
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::Guild { state, .. }) => vec![
                GuildActionItem {
                    kind: GuildActionKind::MarkAsRead,
                    label: "Mark server as read".to_owned(),
                    enabled: self.guild_ack_targets(state.id).next().is_some(),
                },
                GuildActionItem {
                    kind: GuildActionKind::ToggleMute,
                    label: if self.discord.guild_notification_muted(state.id) {
                        "Unmute server".to_owned()
                    } else {
                        "Mute server".to_owned()
                    },
                    enabled: true,
                },
            ],
            Some(GuildPaneEntry::DirectMessages) => vec![GuildActionItem {
                kind: GuildActionKind::NoActionsYet,
                label: "No server actions yet".to_owned(),
                enabled: false,
            }],
            Some(GuildPaneEntry::FolderHeader { .. }) | None => Vec::new(),
        }
    }

    pub fn selected_guild_mute_duration_items(&self) -> &'static [super::MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn selected_guild_action_index(&self) -> Option<usize> {
        match self.guild_action_menu.as_ref()? {
            GuildActionMenuState::Actions { selected } => Some(clamp_selected_index(
                *selected,
                self.selected_guild_action_items().len(),
            )),
            GuildActionMenuState::MuteDuration { selected } => Some(clamp_selected_index(
                *selected,
                self.selected_guild_mute_duration_items().len(),
            )),
        }
    }

    pub fn move_guild_action_down(&mut self) {
        let len = match self.guild_action_menu.as_ref() {
            Some(GuildActionMenuState::Actions { .. }) => self.selected_guild_action_items().len(),
            Some(GuildActionMenuState::MuteDuration { .. }) => {
                self.selected_guild_mute_duration_items().len()
            }
            None => return,
        };
        if let Some(menu) = self.guild_action_menu.as_mut() {
            match menu {
                GuildActionMenuState::Actions { selected }
                | GuildActionMenuState::MuteDuration { selected } => move_index_down(selected, len),
            }
        }
    }

    pub fn move_guild_action_up(&mut self) {
        if let Some(menu) = self.guild_action_menu.as_mut() {
            match menu {
                GuildActionMenuState::Actions { selected }
                | GuildActionMenuState::MuteDuration { selected } => move_index_up(selected),
            }
        }
    }

    pub fn select_guild_action_row(&mut self, row: usize) -> bool {
        let len = match self.guild_action_menu.as_ref() {
            Some(GuildActionMenuState::Actions { .. }) => self.selected_guild_action_items().len(),
            Some(GuildActionMenuState::MuteDuration { .. }) => {
                self.selected_guild_mute_duration_items().len()
            }
            None => return false,
        };
        if row >= len {
            return false;
        }
        if let Some(menu) = self.guild_action_menu.as_mut() {
            match menu {
                GuildActionMenuState::Actions { selected }
                | GuildActionMenuState::MuteDuration { selected } => *selected = row,
            }
            return true;
        }
        false
    }

    pub fn activate_selected_guild_action(&mut self) -> Option<AppCommand> {
        let menu = self.guild_action_menu.clone()?;
        match menu {
            GuildActionMenuState::Actions { selected } => {
                let items = self.selected_guild_action_items();
                let item = items.get(clamp_selected_index(selected, items.len()))?;
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    GuildActionKind::MarkAsRead => self.mark_selected_guild_as_read(),
                    GuildActionKind::ToggleMute => {
                        let guild_id = self.selected_guild_cursor_id()?;
                        if self.discord.guild_notification_muted(guild_id) {
                            self.close_guild_action_menu();
                            self.toggle_selected_guild_mute(None)
                        } else {
                            self.guild_action_menu =
                                Some(GuildActionMenuState::MuteDuration { selected: 0 });
                            None
                        }
                    }
                    GuildActionKind::NoActionsYet => None,
                }
            }
            GuildActionMenuState::MuteDuration { selected } => {
                let item = self
                    .selected_guild_mute_duration_items()
                    .get(clamp_selected_index(
                        selected,
                        self.selected_guild_mute_duration_items().len(),
                    ))?;
                self.close_guild_action_menu();
                self.toggle_selected_guild_mute(Some(item.duration))
            }
        }
    }

    pub fn activate_guild_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        match self.guild_action_menu.as_ref()? {
            GuildActionMenuState::Actions { .. } => {
                let actions = self.selected_guild_action_items();
                let index = actions.iter().enumerate().position(|(index, action)| {
                    action.enabled
                        && guild_action_shortcut(&actions, index)
                            .is_some_and(|candidate| candidate == shortcut)
                })?;
                self.select_guild_action_row(index);
                self.activate_selected_guild_action()
            }
            GuildActionMenuState::MuteDuration { .. } => {
                let index = self
                    .selected_guild_mute_duration_items()
                    .iter()
                    .enumerate()
                    .position(|(index, _)| indexed_shortcut(index) == Some(shortcut))?;
                self.select_guild_action_row(index);
                self.activate_selected_guild_action()
            }
        }
    }

    pub fn back_guild_action_menu(&mut self) {
        if matches!(
            self.guild_action_menu,
            Some(GuildActionMenuState::MuteDuration { .. })
        ) {
            self.guild_action_menu = Some(GuildActionMenuState::Actions { selected: 0 });
        } else {
            self.guild_action_menu = None;
        }
    }

    fn mark_selected_guild_as_read(&mut self) -> Option<AppCommand> {
        let guild_id = match self.guild_pane_entries().get(self.selected_guild())? {
            GuildPaneEntry::Guild { state, .. } => state.id,
            GuildPaneEntry::DirectMessages | GuildPaneEntry::FolderHeader { .. } => return None,
        };
        let targets: Vec<_> = self.guild_ack_targets(guild_id).collect();
        if targets.is_empty() {
            return None;
        }

        for (channel_id, message_id) in targets.iter().copied() {
            self.pending_read_acks.remove(&channel_id);
            self.discord.apply_event(&AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count: 0,
            });
            if self.active_channel_id == Some(channel_id) {
                self.unread_divider_last_acked_id = None;
                self.pending_unread_anchor_scroll = false;
                self.clear_new_messages_marker();
            }
        }
        self.close_guild_action_menu();
        Some(AppCommand::AckChannels { targets })
    }

    fn guild_ack_targets(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> impl Iterator<Item = (Id<ChannelMarker>, Id<MessageMarker>)> + '_ {
        self.discord
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .filter_map(|channel| {
                self.discord
                    .channel_ack_target(channel.id)
                    .map(|message_id| (channel.id, message_id))
            })
    }

    /// Toggles the collapse state of the folder under the selection. Does
    /// nothing if the cursor isn't on a folder header.
    pub fn toggle_selected_folder(&mut self) {
        let folder_key = self.selected_folder_key();
        if let Some(key) = folder_key {
            toggle_collapsed_key(&mut self.collapsed_folders, key);
        }
    }

    pub fn open_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            open_collapsed_key(&mut self.collapsed_folders, &key);
        }
    }

    pub fn close_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            close_collapsed_key(&mut self.collapsed_folders, key);
        }
    }

    pub fn confirm_selected_guild(&mut self) {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::DirectMessages) => {
                self.activate_guild(ActiveGuildScope::DirectMessages)
            }
            Some(GuildPaneEntry::Guild { state, .. }) => {
                self.activate_guild(ActiveGuildScope::Guild(state.id))
            }
            Some(GuildPaneEntry::FolderHeader { .. }) => self.toggle_selected_folder(),
            None => {}
        }
    }

    pub(super) fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.active_guild = scope;
        self.selected_channel = 0;
        self.channel_scroll = 0;
        self.channel_keep_selection_visible = true;
        self.active_channel_id = None;
        self.pinned_message_view_channel_id = None;
        self.pinned_message_view_return_target = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.selected_member = 0;
        self.member_scroll = 0;
        self.member_keep_selection_visible = true;

        self.cache.set_last_guild(scope.into());
    }

    fn selected_folder_key(&self) -> Option<FolderKey> {
        let entries = self.guild_pane_entries();
        let selected = self.selected_guild();
        match entries.get(selected) {
            Some(GuildPaneEntry::FolderHeader { folder, .. }) => Self::folder_key(folder),
            Some(GuildPaneEntry::Guild { branch, .. }) if branch.is_folder_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    GuildPaneEntry::FolderHeader { folder, .. } => Self::folder_key(folder),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn folder_key(folder: &GuildFolder) -> Option<FolderKey> {
        if let Some(id) = folder.id {
            Some(FolderKey::Id(id))
        } else if folder.guild_ids.len() > 1 {
            Some(FolderKey::Guilds(folder.guild_ids.clone()))
        } else {
            None
        }
    }
}
