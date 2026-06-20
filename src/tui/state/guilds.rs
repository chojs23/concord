use std::collections::{HashMap, HashSet};

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{GuildFolder, GuildState, MuteDuration};

use super::navigation::FolderSettingsField;
use super::{ActiveGuildScope, DashboardState, FolderKey, FolderSettingsState};
use super::{
    model::{FocusPane, GuildBranch, GuildPaneEntry},
    scroll::{clamp_selected_index, toggle_collapsed_key},
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
                .is_some_and(|key| self.navigation.guilds.collapsed_folders.contains(key));
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
                    self.navigation.guilds.active == ActiveGuildScope::Guild(guild.id)
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
            .guilds
            .filter
            .as_ref()
            .map(|f| f.query().trim().to_owned())
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
            self.navigation.guilds.list.keep_selection_visible();
            return true;
        }
        false
    }

    pub fn selected_guild(&self) -> usize {
        clamp_selected_index(
            self.navigation.guilds.list.selected,
            self.guild_pane_filtered_entries().len(),
        )
    }

    pub fn guild_scroll(&self) -> usize {
        self.navigation.guilds.list.scroll
    }

    pub fn visible_guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        self.guild_pane_filtered_entries()
            .into_iter()
            .skip(self.navigation.guilds.list.scroll)
            .take(self.navigation.guilds.list.content_height())
            .collect()
    }

    pub fn focused_guild_selection(&self) -> Option<usize> {
        if self.navigation.focus == FocusPane::Guilds
            && !self.guild_pane_filtered_entries().is_empty()
        {
            let selected = self.selected_guild();
            let visible_len = self.visible_guild_pane_entries().len();
            if selected >= self.navigation.guilds.list.scroll
                && selected < self.navigation.guilds.list.scroll + visible_len
            {
                Some(selected - self.navigation.guilds.list.scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        let len = self.guild_pane_filtered_entries().len();
        let selected = self.navigation.guilds.list.selected;
        self.navigation
            .guilds
            .list
            .set_view_height_and_clamp(height, selected, len);
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.navigation.guilds.active {
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
        match (self.navigation.guilds.active, entry) {
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
            toggle_collapsed_key(&mut self.navigation.guilds.collapsed_folders, key);
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

    pub fn open_selected_folder_settings(&mut self) -> bool {
        let Some((folder_id, name, color)) = self.selected_configurable_folder() else {
            return false;
        };
        let mut name_input = crate::tui::text_input::TextInputState::default();
        name_input.set_value(name.unwrap_or_default());
        let mut color_input = crate::tui::text_input::TextInputState::default();
        color_input.set_value(format_folder_color_code(color));
        self.navigation.guilds.folder_settings = Some(FolderSettingsState {
            folder_id,
            active_field: Default::default(),
            editing_field: None,
            edit_input: Default::default(),
            name_input,
            color_input,
            color_error: None,
        });
        true
    }

    pub fn close_folder_settings(&mut self) {
        self.navigation.guilds.folder_settings = None;
    }

    pub fn is_folder_settings_open(&self) -> bool {
        self.navigation.guilds.folder_settings.is_some()
    }

    pub(in crate::tui) fn folder_settings_name_value(&self) -> Option<&str> {
        let settings = self.navigation.guilds.folder_settings.as_ref()?;
        Some(
            if settings.editing_field == Some(FolderSettingsField::Name) {
                settings.edit_input.value()
            } else {
                settings.name_input.value()
            },
        )
    }

    pub(in crate::tui) fn folder_settings_color_value(&self) -> Option<&str> {
        let settings = self.navigation.guilds.folder_settings.as_ref()?;
        Some(
            if settings.editing_field == Some(FolderSettingsField::Color) {
                settings.edit_input.value()
            } else {
                settings.color_input.value()
            },
        )
    }

    pub(in crate::tui) fn folder_settings_name_active(&self) -> bool {
        self.navigation
            .guilds
            .folder_settings
            .as_ref()
            .is_some_and(|settings| matches!(settings.active_field, FolderSettingsField::Name))
    }

    pub(in crate::tui) fn folder_settings_color_active(&self) -> bool {
        self.navigation
            .guilds
            .folder_settings
            .as_ref()
            .is_some_and(|settings| matches!(settings.active_field, FolderSettingsField::Color))
    }

    pub(in crate::tui) fn is_folder_settings_editing(&self) -> bool {
        self.navigation
            .guilds
            .folder_settings
            .as_ref()
            .is_some_and(|settings| settings.editing_field.is_some())
    }

    pub(in crate::tui) fn folder_settings_color_error(&self) -> Option<&str> {
        self.navigation
            .guilds
            .folder_settings
            .as_ref()
            .and_then(|settings| settings.color_error.as_deref())
    }

    pub(in crate::tui) fn folder_settings_cursor_byte_index(&self) -> Option<usize> {
        let settings = self.navigation.guilds.folder_settings.as_ref()?;
        settings.editing_field?;
        Some(settings.edit_input.cursor_byte_index())
    }

    pub fn next_folder_settings_field(&mut self) {
        if let Some(settings) = self.navigation.guilds.folder_settings.as_mut() {
            if settings.editing_field.is_some() {
                return;
            }
            settings.active_field = match settings.active_field {
                FolderSettingsField::Name => FolderSettingsField::Color,
                FolderSettingsField::Color => FolderSettingsField::Name,
            };
        }
    }

    pub fn previous_folder_settings_field(&mut self) {
        self.next_folder_settings_field();
    }

    pub fn start_or_commit_folder_settings_edit(&mut self) {
        if let Some(settings) = self.navigation.guilds.folder_settings.as_mut() {
            if settings.editing_field == Some(settings.active_field) {
                let value = settings.edit_input.value().to_owned();
                match settings.active_field {
                    FolderSettingsField::Name => settings.name_input.set_value(value),
                    FolderSettingsField::Color => settings.color_input.set_value(value),
                }
                settings.editing_field = None;
                settings.edit_input.clear();
            } else {
                let value = match settings.active_field {
                    FolderSettingsField::Name => settings.name_input.value().to_owned(),
                    FolderSettingsField::Color => settings.color_input.value().to_owned(),
                };
                settings.editing_field = Some(settings.active_field);
                settings.edit_input.set_value(value);
            }
        }
    }

    pub fn cancel_folder_settings_edit(&mut self) -> bool {
        if let Some(settings) = self.navigation.guilds.folder_settings.as_mut()
            && settings.editing_field.take().is_some()
        {
            settings.edit_input.clear();
            return true;
        }
        false
    }

    pub fn push_folder_settings_char(&mut self, value: char) {
        self.update_active_folder_settings_input(|input| input.insert_char(value));
        self.clear_folder_settings_color_error_if_editing_color();
    }

    pub fn pop_folder_settings_char(&mut self) {
        self.update_active_folder_settings_input(|input| {
            input.delete_previous_grapheme();
        });
        self.clear_folder_settings_color_error_if_editing_color();
    }

    pub fn delete_previous_folder_settings_word(&mut self) {
        self.update_active_folder_settings_input(|input| {
            input.delete_previous_word();
        });
        self.clear_folder_settings_color_error_if_editing_color();
    }

    fn clear_folder_settings_color_error_if_editing_color(&mut self) {
        if let Some(settings) = self.navigation.guilds.folder_settings.as_mut()
            && matches!(settings.active_field, FolderSettingsField::Color)
        {
            settings.color_error = None;
        }
    }

    pub fn move_folder_settings_cursor_left(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_left());
    }

    pub fn move_folder_settings_cursor_right(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_right());
    }

    pub fn move_folder_settings_cursor_word_left(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_word_left());
    }

    pub fn move_folder_settings_cursor_word_right(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_word_right());
    }

    pub fn move_folder_settings_cursor_home(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_home());
    }

    pub fn move_folder_settings_cursor_end(&mut self) {
        self.update_active_folder_settings_input(|input| input.move_end());
    }

    fn update_active_folder_settings_input(
        &mut self,
        update: impl FnOnce(&mut crate::tui::text_input::TextInputState),
    ) {
        if let Some(settings) = self.navigation.guilds.folder_settings.as_mut() {
            if settings.editing_field != Some(settings.active_field) {
                return;
            }
            update(&mut settings.edit_input);
        }
    }

    pub fn commit_folder_settings_command(&mut self) -> Option<AppCommand> {
        let settings = self.navigation.guilds.folder_settings.as_ref()?;
        let Some(color) = parse_folder_color_code(settings.color_input.value()) else {
            if let Some(settings) = self.navigation.guilds.folder_settings.as_mut() {
                settings.color_error = Some("Use #RRGGBB or leave blank".to_owned());
            }
            return None;
        };
        let folder_id = settings.folder_id;
        let name = settings.name_input.value().trim().to_owned();
        let name = (!name.is_empty()).then_some(name);
        self.navigation.guilds.folder_settings = None;
        Some(AppCommand::UpdateGuildFolderSettings {
            folder_id,
            name,
            color,
        })
    }

    pub(super) fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.navigation.guilds.active = scope;
        self.navigation.channels.list.reset_selection_and_scroll();
        self.navigation.channels.active_channel_id = None;
        self.messages.pinned_message_view_channel_id = None;
        self.messages.pinned_message_view_return_target = None;
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_keep_selection_visible = true;
        self.messages.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.navigation.members.list.reset_selection_and_scroll();

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

    fn selected_configurable_folder(&self) -> Option<(u64, Option<String>, Option<u32>)> {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::FolderHeader { folder, .. }) => {
                folder.id.map(|id| (id, folder.name.clone(), folder.color))
            }
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

fn format_folder_color_code(color: Option<u32>) -> String {
    color
        .map(|color| format!("#{:06X}", color & 0x00ff_ffff))
        .unwrap_or_default()
}

fn parse_folder_color_code(value: &str) -> Option<Option<u32>> {
    let value = value.trim();
    if value.is_empty() {
        return Some(None);
    }
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(value, 16).ok().map(Some)
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
