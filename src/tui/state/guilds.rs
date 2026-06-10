use std::collections::{HashMap, HashSet};

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{GuildFolder, GuildState, MuteDuration};

use super::{ActiveGuildScope, DashboardState, FolderKey};
use super::{
    model::{FocusPane, GuildBranch, GuildPaneEntry},
    scroll::{
        clamp_list_viewport, clamp_selected_index, pane_content_height, toggle_collapsed_key,
    },
};
use crate::discord::AppCommand;
use crate::tui::fuzzy::{FuzzyMatchQuality, FuzzyScore, best_fuzzy_name_match_score};

impl DashboardState {
    pub fn guild_name(&self, guild_id: Id<GuildMarker>) -> Option<&str> {
        self.discord
            .cache
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
        let folders = self.discord.cache.guild_folders();

        if folders.is_empty() {
            // Iterating `by_id.values()` here is non-deterministic because
            // it's a HashMap, which makes the sidebar shuffle on every render.
            // Fall back to the discord state's own (insertion-ordered) guild
            // list so the order stays stable until folder data arrives.
            for guild in self.discord.cache.guilds() {
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
                .is_some_and(|key| self.navigation.collapsed_folders.contains(key));
            entries.push(GuildPaneEntry::FolderHeader { folder, collapsed });

            // Always mark children as placed even when collapsed so we don't
            // duplicate them in the trailing "ungrouped" loop.
            for guild_id in &folder.guild_ids {
                placed.insert(*guild_id);
            }

            let mut child_guilds: Vec<&GuildState> = folder
                .guild_ids
                .iter()
                .filter_map(|guild_id| by_id.get(guild_id).copied())
                .collect();
            if collapsed {
                child_guilds.retain(|guild| {
                    self.navigation.active_guild == ActiveGuildScope::Guild(guild.id)
                });
            }
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
        for guild in self.discord.cache.guilds() {
            if !placed.contains(&guild.id) {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
        }

        entries
    }

    /// Returns guild pane entries filtered by the active pane filter query, or
    /// all entries if no filter is active. Folder headers are omitted when a
    /// query is present so results appear as a flat, scored list.
    pub fn guild_pane_filtered_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        let query = self
            .navigation
            .guild_pane_filter
            .as_ref()
            .map(|f| f.query.trim().to_owned())
            .filter(|q| !q.is_empty());
        let Some(query) = query else {
            return self.guild_pane_entries();
        };
        // Search directly over discord.guilds() so servers inside collapsed
        // folders appear in results even when they're not normally visible.
        let mut scored: Vec<(FuzzyMatchQuality, FuzzyScore, usize, GuildPaneEntry<'_>)> =
            Vec::new();
        if let Some((quality, score)) =
            best_fuzzy_name_match_score(&["direct messages", "dm"], &query)
        {
            scored.push((quality, score, 0, GuildPaneEntry::DirectMessages));
        }
        for (index, guild) in self.guild_pane_search_guilds().into_iter().enumerate() {
            if let Some((quality, score)) = best_fuzzy_name_match_score(&[&guild.name], &query) {
                scored.push((
                    quality,
                    score,
                    index + 1,
                    GuildPaneEntry::Guild {
                        state: guild,
                        branch: GuildBranch::None,
                    },
                ));
            }
        }
        scored
            .sort_by_key(|(quality, score, original_index, _)| (*quality, *score, *original_index));
        scored.into_iter().map(|(_, _, _, entry)| entry).collect()
    }

    fn guild_pane_search_guilds(&self) -> Vec<&GuildState> {
        let by_id: HashMap<Id<GuildMarker>, &GuildState> = self
            .discord
            .guilds()
            .into_iter()
            .map(|guild| (guild.id, guild))
            .collect();
        let mut placed: HashSet<Id<GuildMarker>> = HashSet::new();
        let folders = self.discord.cache.guild_folders();

        if folders.is_empty() {
            return self.discord.cache.guilds();
        }

        let mut guilds = Vec::new();
        for folder in folders {
            for guild_id in &folder.guild_ids {
                placed.insert(*guild_id);
                if let Some(guild) = by_id.get(guild_id) {
                    guilds.push(*guild);
                }
            }
        }
        for guild in self.discord.cache.guilds() {
            if !placed.contains(&guild.id) {
                guilds.push(guild);
            }
        }
        guilds
    }

    pub fn confirm_guild_pane_filter(&mut self) -> bool {
        let selected = self.selected_guild();
        let action = {
            let entries = self.guild_pane_filtered_entries();
            match entries.get(selected) {
                Some(GuildPaneEntry::DirectMessages) => Some(ActiveGuildScope::DirectMessages),
                Some(entry) => entry.guild_id().map(ActiveGuildScope::Guild),
                _ => None,
            }
        };
        if let Some(scope) = action {
            self.activate_guild(scope);
            self.navigation.guild_keep_selection_visible = true;
            return true;
        }
        false
    }

    pub fn selected_guild(&self) -> usize {
        clamp_selected_index(
            self.navigation.selected_guild,
            self.guild_pane_filtered_entries().len(),
        )
    }

    pub fn guild_scroll(&self) -> usize {
        self.navigation.guild_scroll
    }

    pub fn visible_guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        self.guild_pane_filtered_entries()
            .into_iter()
            .skip(self.navigation.guild_scroll)
            .take(pane_content_height(self.navigation.guild_view_height))
            .collect()
    }

    pub fn focused_guild_selection(&self) -> Option<usize> {
        if self.navigation.focus == FocusPane::Guilds
            && !self.guild_pane_filtered_entries().is_empty()
        {
            let selected = self.selected_guild();
            let visible_len = self.visible_guild_pane_entries().len();
            if selected >= self.navigation.guild_scroll
                && selected < self.navigation.guild_scroll + visible_len
            {
                Some(selected - self.navigation.guild_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        self.navigation.guild_view_height = height;
        let height = pane_content_height(self.navigation.guild_view_height);
        let len = self.guild_pane_filtered_entries().len();
        clamp_list_viewport(
            self.navigation.selected_guild,
            &mut self.navigation.guild_scroll,
            height,
            len,
            self.navigation.guild_keep_selection_visible,
        );
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.navigation.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => None,
        }
    }

    pub fn selected_guild_cursor_id(&self) -> Option<Id<GuildMarker>> {
        self.guild_pane_entries()
            .get(self.selected_guild())
            .and_then(GuildPaneEntry::guild_id)
    }

    pub fn is_active_guild_entry(&self, entry: &GuildPaneEntry<'_>) -> bool {
        match (self.navigation.active_guild, entry) {
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
            toggle_collapsed_key(&mut self.navigation.collapsed_folders, key);
            self.options.ui_state_save_pending = true;
        }
    }

    pub fn confirm_selected_guild(&mut self) -> bool {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::DirectMessages) => {
                self.activate_guild(ActiveGuildScope::DirectMessages);
                true
            }
            Some(GuildPaneEntry::Guild { state, .. }) => {
                self.activate_guild(ActiveGuildScope::Guild(state.id));
                true
            }
            Some(GuildPaneEntry::FolderHeader { .. }) => {
                self.toggle_selected_folder();
                false
            }
            None => false,
        }
    }

    pub(super) fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.navigation.active_guild = scope;
        self.navigation.selected_channel = 0;
        self.navigation.channel_scroll = 0;
        self.navigation.channel_keep_selection_visible = true;
        self.navigation.active_channel_id = None;
        self.messages.pinned_message_view_channel_id = None;
        self.messages.pinned_message_view_return_target = None;
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_keep_selection_visible = true;
        self.messages.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.navigation.selected_member = 0;
        self.navigation.member_scroll = 0;
        self.navigation.member_keep_selection_visible = true;

        self.refresh_composer_emoji_candidates_for_current_query();
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

impl DashboardState {
    pub fn toggle_selected_guild_mute(
        &mut self,
        duration: Option<MuteDuration>,
    ) -> Option<AppCommand> {
        let guild_id = self.selected_guild_cursor_id()?;
        let label = self
            .discord
            .guild(guild_id)
            .map(|guild| guild.name.clone())
            .unwrap_or_else(|| format!("server-{}", guild_id.get()));
        let muted = !self.discord.cache.guild_notification_muted(guild_id);
        Some(AppCommand::SetGuildMuted {
            guild_id,
            muted,
            duration,
            label,
        })
    }
}
