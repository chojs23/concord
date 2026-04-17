use std::collections::{HashMap, HashSet};

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{GuildFolder, GuildState};

use super::{ActiveGuildScope, DashboardState, FolderKey};
use super::{
    model::{FocusPane, GuildBranch, GuildPaneEntry},
    scroll::{
        clamp_selected_index, close_collapsed_key, open_collapsed_key, pane_content_height,
        toggle_collapsed_key,
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
            Some(self.selected_guild().saturating_sub(self.guild_scroll))
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        self.guild_view_height = height;
        self.clamp_guild_viewport();
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => None,
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
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.selected_member = 0;
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
