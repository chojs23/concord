use std::collections::{HashMap, HashSet};

use ratatui::style::Color;
use twilight_model::id::{Id, marker::ChannelMarker, marker::GuildMarker, marker::MessageMarker};

use crate::discord::{
    AppCommand, AppEvent, ChannelState, DiscordState, GuildFolder, GuildMemberState, GuildState,
    MessageState, PresenceStatus,
};
use crate::logging;

const SCROLL_OFF: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Members,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Debug)]
pub struct DashboardState {
    discord: DiscordState,
    focus: FocusPane,
    active_guild: ActiveGuildScope,
    active_channel_id: Option<Id<ChannelMarker>>,
    selected_guild: usize,
    guild_scroll: usize,
    guild_view_height: usize,
    selected_channel: usize,
    channel_scroll: usize,
    channel_view_height: usize,
    selected_message: usize,
    message_scroll: usize,
    message_auto_follow: bool,
    message_view_height: usize,
    selected_member: usize,
    member_scroll: usize,
    member_view_height: usize,
    composer_input: String,
    composer_active: bool,
    current_user: Option<String>,
    last_error: Option<String>,
    skipped_events: u64,
    should_quit: bool,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    collapsed_folders: HashSet<FolderKey>,
    collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum FolderKey {
    Id(u64),
    Guilds(Vec<Id<GuildMarker>>),
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            discord: DiscordState::default(),
            focus: FocusPane::Guilds,
            active_guild: ActiveGuildScope::Unset,
            active_channel_id: None,
            // Index 0 is the virtual "Direct Messages" entry. Start on the
            // first real guild when one exists; the bounds clamp inside
            // `selected_guild()` falls back to the DM entry while the guild
            // list is still empty.
            selected_guild: 1,
            guild_scroll: 0,
            guild_view_height: 1,
            selected_channel: 0,
            channel_scroll: 0,
            channel_view_height: 1,
            selected_message: 0,
            message_scroll: 0,
            message_auto_follow: true,
            message_view_height: 1,
            selected_member: 0,
            member_scroll: 0,
            member_view_height: 1,
            composer_input: String::new(),
            composer_active: false,
            current_user: None,
            last_error: None,
            skipped_events: 0,
            should_quit: false,
            collapsed_folders: HashSet::new(),
            collapsed_channel_categories: HashSet::new(),
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        let selected_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.selected_message())
                    .map(|message| message.id)
            })
            .flatten();
        let scroll_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let channel_cursor_id = self.selected_channel_cursor_id();

        match &event {
            AppEvent::Ready { user } => self.current_user = Some(user.clone()),
            AppEvent::GatewayError { message } => {
                logging::error("app_event", message);
                self.last_error = Some(message.clone());
            }
            AppEvent::MessageHistoryLoadFailed { message, .. } => {
                logging::error("history", message);
                self.last_error = Some(message.clone());
            }
            AppEvent::GatewayClosed => {
                self.last_error = Some("gateway closed".to_owned());
            }
            _ => {}
        }
        self.discord.apply_event(&event);
        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.clamp_selection_indices();
        if self.message_auto_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    pub fn record_lag(&mut self, skipped: u64) {
        self.skipped_events += skipped;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn focus(&self) -> FocusPane {
        self.focus
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn skipped_events(&self) -> u64 {
        self.skipped_events
    }

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
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
            for guild in by_id.values() {
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

        for guild in by_id.values() {
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
        self.selected_guild
            .min(self.guild_pane_entries().len().saturating_sub(1))
    }

    #[cfg(test)]
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
        if let Some(key) = folder_key
            && !self.collapsed_folders.insert(key.clone())
        {
            self.collapsed_folders.remove(&key);
        }
    }

    pub fn open_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            self.collapsed_folders.remove(&key);
        }
    }

    pub fn close_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            self.collapsed_folders.insert(key);
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

    fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.active_guild = scope;
        self.selected_channel = 0;
        self.channel_scroll = 0;
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
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

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            ActiveGuildScope::DirectMessages => self.discord.channels_for_guild(None),
            ActiveGuildScope::Guild(guild_id) => self.discord.channels_for_guild(Some(guild_id)),
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        if self.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels
                .into_iter()
                .map(|state| ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                })
                .collect();
        }

        let category_ids: HashSet<Id<ChannelMarker>> = channels
            .iter()
            .filter(|channel| channel.is_category())
            .map(|channel| channel.id)
            .collect();

        let mut roots: Vec<&ChannelState> = channels
            .iter()
            .copied()
            .filter(|channel| {
                channel.is_category()
                    || channel
                        .parent_id
                        .is_none_or(|parent_id| !category_ids.contains(&parent_id))
            })
            .collect();
        sort_channels(&mut roots);

        let mut entries = Vec::new();
        for root in roots {
            if !root.is_category() {
                entries.push(ChannelPaneEntry::Channel {
                    state: root,
                    branch: ChannelBranch::None,
                });
                continue;
            }

            let collapsed = self.collapsed_channel_categories.contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });
            if collapsed {
                continue;
            }

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            let last_child_index = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    ChannelBranch::Last
                } else {
                    ChannelBranch::Middle
                };
                entries.push(ChannelPaneEntry::Channel {
                    state: child,
                    branch,
                });
            }
        }

        entries
    }

    pub fn selected_channel(&self) -> usize {
        self.selected_channel
            .min(self.channel_pane_entries().len().saturating_sub(1))
    }

    fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::CategoryHeader { .. }) | None => None,
        }
    }

    #[cfg(test)]
    pub fn channel_scroll(&self) -> usize {
        self.channel_scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_entries()
            .into_iter()
            .skip(self.channel_scroll)
            .take(pane_content_height(self.channel_view_height))
            .collect()
    }

    pub fn focused_channel_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Channels && !self.channel_pane_entries().is_empty() {
            Some(self.selected_channel().saturating_sub(self.channel_scroll))
        } else {
            None
        }
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        self.channel_view_height = height;
        self.clamp_channel_viewport();
    }

    fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self.channel_pane_entries().iter().position(|entry| {
            matches!(entry, ChannelPaneEntry::Channel { state, .. } if state.id == channel_id)
        }) {
            self.selected_channel = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } if Some(state.id) == self.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        if !self.collapsed_channel_categories.insert(category_id) {
            self.collapsed_channel_categories.remove(&category_id);
        }
    }

    pub fn open_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            self.collapsed_channel_categories.remove(&category_id);
        }
    }

    pub fn close_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            self.collapsed_channel_categories.insert(category_id);
        }
    }

    pub fn confirm_selected_channel(&mut self) {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { .. }) => {
                self.toggle_selected_channel_category()
            }
            Some(ChannelPaneEntry::Channel { state, .. }) => self.activate_channel(state.id),
            None => {}
        }
    }

    fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.active_channel_id = Some(channel_id);
        self.message_auto_follow = true;
        self.selected_message = self.messages().len().saturating_sub(1);
        self.clamp_message_viewport();
    }

    fn selected_channel_category_id(&self) -> Option<Id<ChannelMarker>> {
        let entries = self.channel_pane_entries();
        let selected = self.selected_channel();
        match entries.get(selected) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { branch, .. }) if branch.is_category_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
                    _ => None,
                }),
            _ => None,
        }
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        self.selected_channel_id()
            .map(|channel_id| self.discord.messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    pub fn selected_message(&self) -> usize {
        self.selected_message
            .min(self.messages().len().saturating_sub(1))
    }

    #[cfg(test)]
    pub fn message_scroll(&self) -> usize {
        self.message_scroll
    }

    #[cfg(test)]
    pub fn message_auto_follow(&self) -> bool {
        self.message_auto_follow
    }

    #[cfg(test)]
    pub fn message_view_height(&self) -> usize {
        self.message_view_height
    }

    pub fn visible_messages(&self) -> Vec<&MessageState> {
        self.messages()
            .into_iter()
            .skip(self.message_scroll)
            .take(self.message_content_height())
            .collect()
    }

    pub fn set_message_view_height(&mut self, height: usize) {
        self.message_view_height = height;
        self.clamp_message_viewport();
    }

    pub fn clamp_message_viewport_for_image_previews(
        &mut self,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        self.clamp_message_viewport();
        if preview_width == 0 || max_preview_height == 0 || self.messages().is_empty() {
            return;
        }

        let height = self.message_content_height();
        let upper_scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);

        for _ in 0..self.messages().len() {
            let lower_scrolloff = self
                .following_message_rendered_rows(preview_width, max_preview_height, SCROLL_OFF)
                .min(height.saturating_sub(1));
            let lower_bound = height.saturating_sub(1).saturating_sub(lower_scrolloff);
            let selected_row =
                self.selected_message_rendered_row(preview_width, max_preview_height);
            let selected_bottom = selected_row.saturating_add(
                self.selected_message_rendered_height(preview_width, max_preview_height)
                    .saturating_sub(1),
            );
            if selected_bottom > lower_bound && self.message_scroll < self.selected_message {
                self.message_scroll = self.message_scroll.saturating_add(1);
                continue;
            }

            if selected_row < upper_scrolloff && self.message_scroll > 0 {
                self.message_scroll = self.message_scroll.saturating_sub(1);
                continue;
            }

            break;
        }
    }

    pub fn focused_message_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Messages && !self.messages().is_empty() {
            Some(self.selected_message().saturating_sub(self.message_scroll))
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn selected_message_attachment_url(&self) -> Option<&str> {
        self.selected_message_attachment()
            .and_then(|attachment| attachment.preferred_url())
    }

    #[allow(dead_code)]
    fn selected_message_attachment(&self) -> Option<&crate::discord::AttachmentInfo> {
        self.selected_message_attachment_matching(|_| true)
    }

    fn selected_message_attachment_matching(
        &self,
        predicate: impl Fn(&crate::discord::AttachmentInfo) -> bool,
    ) -> Option<&crate::discord::AttachmentInfo> {
        let channel_id = self.selected_channel_id()?;
        let messages = self.discord.messages_for_channel(channel_id);
        let message = messages.get(self.selected_message())?;
        message
            .attachments_in_display_order()
            .find(|attachment| predicate(attachment))
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return Vec::new();
        };
        let members = self.discord.members_for_guild(guild_id);
        let mut groups: Vec<MemberGroup<'_>> = Vec::new();

        for status in [
            PresenceStatus::Online,
            PresenceStatus::Idle,
            PresenceStatus::DoNotDisturb,
            PresenceStatus::Offline,
        ] {
            let mut entries: Vec<&GuildMemberState> = members
                .iter()
                .filter(|member| member.status == status)
                .copied()
                .collect();
            if entries.is_empty() {
                continue;
            }
            entries.sort_by(|a, b| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            });
            groups.push(MemberGroup { status, entries });
        }

        groups
    }

    pub fn flattened_members(&self) -> Vec<&GuildMemberState> {
        self.members_grouped()
            .into_iter()
            .flat_map(|group| group.entries)
            .collect()
    }

    pub fn selected_member(&self) -> usize {
        self.selected_member
            .min(self.flattened_members().len().saturating_sub(1))
    }

    pub fn focused_member_selection_line(&self) -> Option<usize> {
        if self.focus == FocusPane::Members && !self.flattened_members().is_empty() {
            Some(
                self.selected_member_line()
                    .saturating_sub(self.member_scroll),
            )
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.member_scroll
    }

    pub fn member_content_height(&self) -> usize {
        pane_content_height(self.member_view_height)
    }

    #[cfg(test)]
    pub fn selected_member_line_for_test(&self) -> usize {
        self.selected_member_line()
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        self.member_view_height = height;
        self.clamp_member_viewport();
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self
                    .selected_guild
                    .saturating_add(1)
                    .min(self.guild_pane_entries().len().saturating_sub(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self
                    .selected_channel
                    .saturating_add(1)
                    .min(self.channel_pane_entries().len().saturating_sub(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = self
                    .selected_message
                    .saturating_add(1)
                    .min(self.messages().len().saturating_sub(1));
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self
                    .selected_member
                    .saturating_add(1)
                    .min(self.flattened_members().len().saturating_sub(1));
                self.clamp_member_viewport();
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.selected_guild.saturating_sub(1);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self.selected_channel.saturating_sub(1);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                self.selected_message = self.selected_message.saturating_sub(1);
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self.selected_member.saturating_sub(1);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = 0;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = 0;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                self.selected_message = 0;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = 0;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.guild_pane_entries().len().saturating_sub(1);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self.channel_pane_entries().len().saturating_sub(1);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = self.messages().len().saturating_sub(1);
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self.flattened_members().len().saturating_sub(1);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                self.selected_guild = self
                    .selected_guild
                    .saturating_add(distance.max(1))
                    .min(self.guild_pane_entries().len().saturating_sub(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                self.selected_channel = self
                    .selected_channel
                    .saturating_add(distance.max(1))
                    .min(self.channel_pane_entries().len().saturating_sub(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                self.selected_message = self
                    .selected_message
                    .saturating_add(distance.max(1))
                    .min(self.messages().len().saturating_sub(1));
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                self.selected_guild = self.selected_guild.saturating_sub(distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                self.selected_channel = self.selected_channel.saturating_sub(distance.max(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                let distance = self.message_content_height() / 2;
                self.selected_message = self.selected_message.saturating_sub(distance.max(1));
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn toggle_message_auto_follow(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }

        self.message_auto_follow = !self.message_auto_follow;
        if self.message_auto_follow {
            self.follow_latest_message();
        }
        self.clamp_message_viewport();
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Members,
            FocusPane::Members => FocusPane::Guilds,
        };
    }

    pub fn focus_pane(&mut self, pane: FocusPane) {
        self.focus = pane;
    }

    pub fn start_composer(&mut self) {
        if self.selected_channel_id().is_none() {
            return;
        }
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn cancel_composer(&mut self) {
        self.composer_active = false;
        self.composer_input.clear();
    }

    pub fn push_composer_char(&mut self, value: char) {
        self.composer_input.push(value);
    }

    pub fn pop_composer_char(&mut self) {
        self.composer_input.pop();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let content = self.composer_input.trim().to_owned();
        if content.is_empty() {
            return None;
        }

        self.composer_input.clear();
        self.composer_active = false;
        Some(AppCommand::SendMessage {
            channel_id,
            content,
        })
    }

    fn clamp_selection_indices(&mut self) {
        self.selected_guild = self.selected_guild();
        self.selected_channel = self.selected_channel();
        self.selected_message = self.selected_message();
        self.selected_member = self.selected_member();
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    fn clamp_active_selection(&mut self) {
        if let ActiveGuildScope::Guild(guild_id) = self.active_guild
            && !self
                .discord
                .guilds()
                .iter()
                .any(|guild| guild.id == guild_id)
        {
            self.active_guild = ActiveGuildScope::Unset;
        }

        let active_channel_is_valid = self
            .active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
            .is_some_and(|channel| match self.active_guild {
                ActiveGuildScope::Unset => false,
                ActiveGuildScope::DirectMessages => {
                    channel.guild_id.is_none() && !channel.is_category()
                }
                ActiveGuildScope::Guild(guild_id) => {
                    channel.guild_id == Some(guild_id) && !channel.is_category()
                }
            });
        if self.active_channel_id.is_some() && !active_channel_is_valid {
            self.active_channel_id = None;
        }
    }

    fn clamp_list_viewports(&mut self) {
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    fn clamp_guild_viewport(&mut self) {
        let entries_len = self.guild_pane_entries().len();
        self.selected_guild = self.selected_guild.min(entries_len.saturating_sub(1));
        self.guild_scroll = clamp_list_scroll(
            self.selected_guild,
            self.guild_scroll,
            pane_content_height(self.guild_view_height),
            entries_len,
        );
    }

    fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_entries().len();
        self.selected_channel = self.selected_channel.min(entries_len.saturating_sub(1));
        self.channel_scroll = clamp_list_scroll(
            self.selected_channel,
            self.channel_scroll,
            pane_content_height(self.channel_view_height),
            entries_len,
        );
    }

    fn clamp_member_viewport(&mut self) {
        let members_len = self.flattened_members().len();
        if members_len == 0 {
            self.selected_member = 0;
            self.member_scroll = 0;
            return;
        }

        self.selected_member = self.selected_member.min(members_len - 1);
        self.member_scroll = clamp_list_scroll(
            self.selected_member_line(),
            self.member_scroll,
            pane_content_height(self.member_view_height),
            self.member_line_count(),
        );
    }

    fn selected_member_line(&self) -> usize {
        let selected_member = self.selected_member();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for _member in group.entries {
                if member_index == selected_member {
                    return line_index;
                }
                member_index += 1;
                line_index += 1;
            }
        }
        0
    }

    fn select_member_near_line(&mut self, target_line: usize) {
        let mut last_member = None;
        for (member_index, line_index) in self.member_line_indices() {
            if line_index >= target_line {
                self.selected_member = member_index;
                return;
            }
            last_member = Some(member_index);
        }

        if let Some(member_index) = last_member {
            self.selected_member = member_index;
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
            for _member in group.entries {
                indices.push((member_index, line_index));
                member_index += 1;
                line_index += 1;
            }
        }
        indices
    }

    fn member_line_count(&self) -> usize {
        let mut lines = 0usize;
        for group in self.members_grouped() {
            if lines > 0 {
                lines += 1;
            }
            lines += 1 + group.entries.len();
        }
        lines
    }

    fn follow_latest_message(&mut self) {
        self.selected_message = self.messages().len().saturating_sub(1);
    }

    fn restore_message_position(
        &mut self,
        selected_message_id: Option<Id<MessageMarker>>,
        scroll_message_id: Option<Id<MessageMarker>>,
    ) {
        let message_ids: Vec<_> = self
            .messages()
            .into_iter()
            .map(|message| message.id)
            .collect();
        if let Some(message_id) = selected_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.selected_message = index;
        }
        if let Some(message_id) = scroll_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.message_scroll = index;
        }
    }

    fn clamp_message_viewport(&mut self) {
        let messages_len = self.messages().len();
        if messages_len == 0 {
            self.selected_message = 0;
            self.message_scroll = 0;
            return;
        }

        self.selected_message = self.selected_message.min(messages_len - 1);
        self.message_scroll = clamp_list_scroll(
            self.selected_message,
            self.message_scroll,
            self.message_content_height(),
            messages_len,
        );
    }

    fn message_content_height(&self) -> usize {
        pane_content_height(self.message_view_height)
    }

    fn selected_message_rendered_row(&self, preview_width: u16, max_preview_height: u16) -> usize {
        let messages = self.messages();
        messages
            .iter()
            .skip(self.message_scroll)
            .take(self.selected_message.saturating_sub(self.message_scroll))
            .map(|message| message_rendered_height(message, preview_width, max_preview_height))
            .sum()
    }

    fn selected_message_rendered_height(
        &self,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        self.messages()
            .get(self.selected_message)
            .map(|message| message_rendered_height(message, preview_width, max_preview_height))
            .unwrap_or(1)
    }

    fn following_message_rendered_rows(
        &self,
        preview_width: u16,
        max_preview_height: u16,
        count: usize,
    ) -> usize {
        self.messages()
            .iter()
            .skip(self.selected_message.saturating_add(1))
            .take(count)
            .map(|message| message_rendered_height(message, preview_width, max_preview_height))
            .sum()
    }
}

fn message_rendered_height(
    message: &MessageState,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    let preview_height = message
        .attachments_in_display_order()
        .find(|attachment| attachment.inline_preview_url().is_some())
        .map(|attachment| {
            super::image_preview_height_for_dimensions(
                preview_width,
                max_preview_height,
                attachment.width,
                attachment.height,
            )
        })
        .unwrap_or(0);
    1 + usize::from(preview_height)
}

fn pane_content_height(height: usize) -> usize {
    height.max(1)
}

fn clamp_list_scroll(cursor: usize, mut scroll: usize, height: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let max_scroll = len.saturating_sub(height);
    scroll = scroll.min(max_scroll);
    let scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);

    let lower_bound = scroll
        .saturating_add(height)
        .saturating_sub(1)
        .saturating_sub(scrolloff);
    if cursor > lower_bound {
        scroll = cursor
            .saturating_add(1)
            .saturating_add(scrolloff)
            .saturating_sub(height);
    }

    let upper_bound = scroll.saturating_add(scrolloff);
    if cursor < upper_bound {
        scroll = cursor.saturating_sub(scrolloff);
    }

    scroll.min(max_scroll)
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct MemberGroup<'a> {
    pub status: PresenceStatus,
    pub entries: Vec<&'a GuildMemberState>,
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelPaneEntry<'a> {
    CategoryHeader {
        state: &'a ChannelState,
        collapsed: bool,
    },
    Channel {
        state: &'a ChannelState,
        branch: ChannelBranch,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChannelBranch {
    None,
    Middle,
    Last,
}

impl ChannelBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    fn is_category_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GuildPaneEntry<'a> {
    DirectMessages,
    FolderHeader {
        folder: &'a GuildFolder,
        collapsed: bool,
    },
    Guild {
        state: &'a GuildState,
        branch: GuildBranch,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GuildBranch {
    None,
    Middle,
    Last,
}

impl GuildBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    fn is_folder_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl GuildPaneEntry<'_> {
    pub fn label(&self) -> &str {
        match self {
            Self::DirectMessages => "Direct Messages",
            Self::FolderHeader { folder, .. } => folder.name.as_deref().unwrap_or("Folder"),
            Self::Guild { state, .. } => state.name.as_str(),
        }
    }
}

/// Convert a Discord folder color (24-bit RGB integer) to a ratatui color.
/// Falls back to a neutral cyan when the color is missing or zero so
/// uncolored folders still read as folder headers.
pub fn folder_color(color: Option<u32>) -> Color {
    match color {
        Some(value) if value != 0 => {
            let r = ((value >> 16) & 0xFF) as u8;
            let g = ((value >> 8) & 0xFF) as u8;
            let b = (value & 0xFF) as u8;
            Color::Rgb(r, g, b)
        }
        _ => Color::Cyan,
    }
}

pub fn presence_color(status: PresenceStatus) -> Color {
    match status {
        PresenceStatus::Online => Color::Green,
        PresenceStatus::Idle => Color::Yellow,
        PresenceStatus::DoNotDisturb => Color::Red,
        PresenceStatus::Offline => Color::DarkGray,
    }
}

pub fn presence_marker(status: PresenceStatus) -> char {
    match status {
        PresenceStatus::Online => '●',
        PresenceStatus::Idle => '◐',
        PresenceStatus::DoNotDisturb => '⊘',
        PresenceStatus::Offline => '○',
    }
}

fn sort_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| (channel.position.unwrap_or(i32::MAX), channel.id));
}

fn sort_direct_message_channels(channels: &mut [&ChannelState]) {
    channels.sort_by(|left, right| {
        right
            .last_message_id
            .cmp(&left.last_message_id)
            .then_with(|| right.id.cmp(&left.id))
    });
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker, marker::UserMarker};

    use super::{
        ChannelBranch, ChannelPaneEntry, DashboardState, FocusPane, GuildBranch, GuildPaneEntry,
        MessageState, message_rendered_height,
    };
    use crate::discord::{
        AppEvent, AttachmentInfo, ChannelInfo, GuildFolder, MemberInfo, MessageInfo,
        MessageSnapshotInfo, PresenceStatus,
    };

    #[test]
    fn tracks_current_user_from_ready() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
        });
        assert_eq!(state.current_user(), Some("neo"));
    }

    #[test]
    fn captures_last_gateway_error() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GatewayError {
            message: "boom".to_owned(),
        });
        assert_eq!(state.last_error(), Some("boom"));
    }

    #[test]
    fn dashboard_starts_without_message_focus() {
        let state = DashboardState::new();

        assert_eq!(state.focus(), FocusPane::Guilds);
        assert_eq!(state.focused_message_selection(), None);
    }

    #[test]
    fn cycle_focus_uses_four_top_level_panes() {
        let mut state = DashboardState::new();

        assert_eq!(state.focus(), FocusPane::Guilds);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Channels);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Messages);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Members);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Guilds);
    }

    #[test]
    fn loaded_messages_are_unselected_until_message_pane_is_focused() {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=2u64 {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("msg {id}")),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        assert_eq!(state.selected_message(), 1);
        assert_eq!(state.focused_message_selection(), None);

        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
        assert_eq!(state.focused_message_selection(), Some(0));
    }

    #[test]
    fn startup_events_do_not_auto_open_direct_messages() {
        let channel_id: Id<ChannelMarker> = Id::new(20);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.selected_channel_id(), None);
        assert_eq!(state.selected_channel_state(), None);
        assert!(state.channel_pane_entries().is_empty());
        assert!(state.messages().is_empty());
    }

    #[test]
    fn member_groups_are_sorted_by_status_then_name() {
        let guild_id = Id::new(1);
        let alice: Id<UserMarker> = Id::new(10);
        let bob: Id<UserMarker> = Id::new(20);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(2),
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: vec![
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                },
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    is_bot: false,
                },
            ],
            presences: vec![
                (alice, PresenceStatus::Online),
                (bob, PresenceStatus::Online),
            ],
        });
        state.confirm_selected_guild();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, PresenceStatus::Online);
        assert_eq!(
            groups[0]
                .entries
                .iter()
                .map(|m| m.display_name.as_str())
                .collect::<Vec<_>>(),
            vec!["alice", "bob"],
        );
    }

    #[test]
    fn message_creation_keeps_viewport_on_latest() {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=3u64 {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("msg {id}")),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        assert_eq!(state.selected_message(), 2);
    }

    #[test]
    fn message_scroll_preserves_position_when_not_following() {
        let mut state = state_with_messages(5);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        assert_eq!(state.selected_message(), 4);
        assert!(state.message_auto_follow());

        state.move_up();
        assert_eq!(state.selected_message(), 3);
        assert!(!state.message_auto_follow());

        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(6),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("msg 6".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.selected_message(), 3);
        assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_auto_follow_can_jump_back_to_latest() {
        let mut state = state_with_messages(5);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        state.move_up();
        assert!(!state.message_auto_follow());

        state.toggle_message_auto_follow();

        assert!(state.message_auto_follow());
        assert_eq!(state.selected_message(), 4);
    }

    #[test]
    fn image_preview_rows_advance_message_scroll_when_selection_would_be_clipped() {
        let mut state = state_with_image_messages(6, &[1]);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        assert_eq!(state.message_scroll(), 0);

        state.clamp_message_viewport_for_image_previews(16, 3);

        assert!(state.message_scroll() > 0);
        let selected_bottom = state.selected_message_rendered_row(16, 3).saturating_add(
            state
                .selected_message_rendered_height(16, 3)
                .saturating_sub(1),
        );
        assert!(selected_bottom < state.message_view_height());
    }

    #[test]
    fn image_preview_scrolloff_reserves_following_image_items() {
        let mut state = state_with_image_messages(8, &[5, 6, 7]);
        focus_messages(&mut state);
        state.set_message_view_height(14);

        while state.selected_message() > 3 {
            state.move_up();
        }
        state.clamp_message_viewport_for_image_previews(16, 3);

        assert_eq!(state.following_message_rendered_rows(16, 3, 3), 12);
        let selected_bottom = state.selected_message_rendered_row(16, 3).saturating_add(
            state
                .selected_message_rendered_height(16, 3)
                .saturating_sub(1),
        );
        assert!(selected_bottom <= 1);
    }

    #[test]
    fn video_attachment_does_not_reserve_image_preview_rows() {
        let message = MessageState {
            id: Id::new(1),
            channel_id: Id::new(2),
            author: "neo".to_owned(),
            content: Some("clip".to_owned()),
            attachments: vec![video_attachment(1)],
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 16, 3), 1);
    }

    #[test]
    fn forwarded_image_attachment_reserves_preview_rows() {
        let message = MessageState {
            id: Id::new(1),
            channel_id: Id::new(2),
            author: "neo".to_owned(),
            content: Some(String::new()),
            attachments: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(1)],
        };

        assert_eq!(message_rendered_height(&message, 16, 3), 4);
    }

    #[test]
    fn selected_message_attachment_url_falls_back_to_forwarded_attachment() {
        let mut state = state_with_image_messages(1, &[]);
        focus_messages(&mut state);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some(String::new()),
            attachments: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(2)],
        });

        assert_eq!(
            state.selected_message_attachment_url(),
            Some("https://cdn.discordapp.com/image-2.png")
        );
    }

    #[test]
    fn selected_message_attachment_url_uses_proxy_when_url_is_empty() {
        let mut state = state_with_proxy_only_attachment_message();
        focus_messages(&mut state);

        assert_eq!(
            state.selected_message_attachment_url(),
            Some("https://media.discordapp.net/cat.png")
        );
    }

    #[test]
    fn message_scroll_uses_scrolloff() {
        let mut state = state_with_messages(12);
        focus_messages(&mut state);
        state.set_message_view_height(7);

        assert_eq!(state.message_scroll(), 5);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_message(), 9);
        assert_eq!(state.message_scroll(), 5);

        state.move_up();
        assert_eq!(state.selected_message(), 8);
        assert_eq!(state.message_scroll(), 5);
    }

    #[test]
    fn shared_scroll_helper_keeps_three_rows_below_cursor_when_scrolling_starts() {
        let height = 10;
        let scroll = super::clamp_list_scroll(7, 0, height, 20);

        assert_eq!(scroll, 1);
        assert_eq!(height - 1 - (7 - scroll), 3);
    }

    #[test]
    fn shared_scroll_helper_moves_one_row_near_bottom() {
        let mut scroll = 0usize;

        for cursor in 0..20 {
            let next_scroll = super::clamp_list_scroll(cursor, scroll, 7, 20);
            assert!(
                next_scroll <= scroll.saturating_add(1),
                "cursor {cursor} moved scroll from {scroll} to {next_scroll}",
            );
            scroll = next_scroll;
        }
    }

    #[test]
    fn guild_scroll_uses_scrolloff() {
        let mut state = state_with_many_guilds(8);
        focus_guilds(&mut state);
        state.set_guild_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_guild(), 8);
        assert_eq!(state.guild_scroll(), 2);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_guild(), 6);
        assert_eq!(state.guild_scroll(), 2);

        state.move_up();
        assert_eq!(state.selected_guild(), 5);
        assert_eq!(state.guild_scroll(), 2);
    }

    #[test]
    fn channel_scroll_uses_scrolloff() {
        let mut state = state_with_many_channels(8);
        focus_channels(&mut state);
        state.set_channel_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_channel(), 7);
        assert_eq!(state.channel_scroll(), 1);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_channel(), 5);
        assert_eq!(state.channel_scroll(), 1);

        state.move_up();
        assert_eq!(state.selected_channel(), 4);
        assert_eq!(state.channel_scroll(), 1);
    }

    #[test]
    fn member_scroll_uses_scrolloff() {
        let mut state = state_with_members(8);
        focus_members(&mut state);
        state.set_member_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_member(), 7);
        assert_eq!(state.member_scroll(), 2);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_member(), 5);
        assert_eq!(state.member_scroll(), 2);

        state.move_up();
        assert_eq!(state.selected_member(), 4);
        assert_eq!(state.member_scroll(), 2);
    }

    #[test]
    fn member_half_page_scrolls_by_rendered_lines() {
        let mut state = state_with_grouped_members();
        focus_members(&mut state);
        state.set_member_view_height(9);

        assert_eq!(state.selected_member(), 0);
        assert_eq!(state.selected_member_line_for_test(), 1);

        state.half_page_down();
        assert_eq!(state.selected_member(), 2);
        assert_eq!(state.selected_member_line_for_test(), 5);

        state.half_page_up();
        assert_eq!(state.selected_member(), 0);
        assert_eq!(state.selected_member_line_for_test(), 1);
    }

    #[test]
    fn half_page_scrolls_all_list_panes() {
        let mut guild_state = state_with_many_guilds(8);
        focus_guilds(&mut guild_state);
        guild_state.set_guild_view_height(9);
        guild_state.half_page_down();
        assert_eq!(guild_state.selected_guild(), 5);

        let mut channel_state = state_with_many_channels(8);
        focus_channels(&mut channel_state);
        channel_state.set_channel_view_height(9);
        channel_state.half_page_down();
        assert_eq!(channel_state.selected_channel(), 4);

        let mut member_state = state_with_members(8);
        focus_members(&mut member_state);
        member_state.set_member_view_height(9);
        member_state.half_page_down();
        assert_eq!(member_state.selected_member(), 4);
    }

    #[test]
    fn message_half_page_up_disables_follow() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.half_page_up();

        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_jump_bottom_does_not_enable_auto_follow() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.move_up();
        assert!(!state.message_auto_follow());

        state.jump_bottom();

        assert_eq!(state.selected_message(), 9);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_half_page_down_keeps_follow_state() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.half_page_down();
        assert!(state.message_auto_follow());

        state.move_up();
        assert!(!state.message_auto_follow());

        state.half_page_down();
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn history_load_preserves_manual_scroll_position_by_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.move_up();
        state.move_up();

        let selected_id = state.messages()[state.selected_message()].id;
        let scroll_id = state.messages()[state.message_scroll()].id;

        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 5)],
        });

        assert_eq!(state.messages()[state.selected_message()].id, selected_id);
        assert_eq!(state.messages()[state.message_scroll()].id, scroll_id);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn direct_messages_are_sorted_by_latest_message_id() {
        let mut state = state_with_direct_messages();
        state.confirm_selected_guild();

        assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);
    }

    #[test]
    fn direct_message_selection_waits_for_channel_confirmation() {
        let mut state = state_with_direct_messages();

        state.confirm_selected_guild();
        assert_eq!(state.selected_channel_id(), None);

        state.confirm_selected_channel();
        assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
    }

    #[test]
    fn direct_message_sorting_uses_channel_id_fallback() {
        let mut state = DashboardState::new();
        for (channel_id, name) in [(Id::new(10), "older-id"), (Id::new(30), "newer-id")] {
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
                guild_id: None,
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: name.to_owned(),
                kind: "dm".to_owned(),
            }));
        }
        state.confirm_selected_guild();

        assert_eq!(channel_entry_names(&state), vec!["newer-id", "older-id"]);
    }

    #[test]
    fn direct_message_cursor_stays_on_same_channel_after_recency_sort() {
        let mut state = state_with_direct_messages();
        state.confirm_selected_guild();
        focus_channels(&mut state);
        state.move_down();

        assert_eq!(state.selected_channel(), 1);
        assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);

        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id: Id::new(30),
            message_id: Id::new(300),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("new empty dm".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(channel_entry_names(&state), vec!["empty", "new", "old"]);
        assert_eq!(state.selected_channel(), 2);
    }

    #[test]
    fn channel_tree_groups_category_children() {
        let state = state_with_channel_tree();
        let entries = state.channel_pane_entries();

        assert!(matches!(
            entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: false,
                ..
            }
        ));
        assert!(matches!(
            entries[1],
            ChannelPaneEntry::Channel {
                branch: ChannelBranch::Middle,
                ..
            }
        ));
        assert!(matches!(
            entries[2],
            ChannelPaneEntry::Channel {
                branch: ChannelBranch::Last,
                ..
            }
        ));
    }

    #[test]
    fn selected_channel_category_can_be_closed_and_opened() {
        let mut state = state_with_channel_tree();

        assert_eq!(state.channel_pane_entries().len(), 3);
        assert_eq!(state.selected_channel_id(), None);

        state.close_selected_channel_category();
        let closed_entries = state.channel_pane_entries();
        assert_eq!(closed_entries.len(), 1);
        assert!(matches!(
            closed_entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: true,
                ..
            }
        ));

        state.open_selected_channel_category();
        assert_eq!(state.channel_pane_entries().len(), 3);
    }

    #[test]
    fn selected_channel_child_can_close_parent_category() {
        let mut state = state_with_channel_tree();
        state.selected_channel = 1;

        state.toggle_selected_channel_category();
        let entries = state.channel_pane_entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn moving_guild_cursor_does_not_activate_guild() {
        let mut state = state_with_two_guilds();
        focus_guilds(&mut state);

        state.confirm_selected_guild();
        let active_guild = state.selected_guild_id();
        assert!(active_guild.is_some());

        state.move_down();
        assert_eq!(state.selected_guild, 2);
        assert_eq!(state.selected_guild_id(), active_guild);

        state.confirm_selected_guild();
        assert_ne!(state.selected_guild_id(), active_guild);
    }

    #[test]
    fn active_guild_entry_tracks_confirmed_guild() {
        let mut state = state_with_two_guilds();
        focus_guilds(&mut state);

        {
            let entries = state.guild_pane_entries();
            assert!(!state.is_active_guild_entry(&entries[0]));
            assert!(!state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.confirm_selected_guild();
        {
            let entries = state.guild_pane_entries();
            assert!(!state.is_active_guild_entry(&entries[0]));
            assert!(state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.move_down();
        {
            let entries = state.guild_pane_entries();
            assert!(state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.confirm_selected_guild();
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[1]));
        assert!(state.is_active_guild_entry(&entries[2]));
    }

    #[test]
    fn moving_channel_cursor_does_not_activate_channel() {
        let mut state = state_with_channel_tree();
        let random_id = Id::new(12);
        focus_channels(&mut state);

        assert_eq!(state.selected_channel_id(), None);

        state.move_down();
        state.move_down();
        assert_eq!(state.selected_channel, 2);
        assert_eq!(state.selected_channel_id(), None);

        state.confirm_selected_channel();
        assert_eq!(state.selected_channel_id(), Some(random_id));
    }

    #[test]
    fn active_channel_entry_tracks_confirmed_channel() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);

        {
            let entries = state.channel_pane_entries();
            assert!(!state.is_active_channel_entry(&entries[0]));
            assert!(!state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.move_down();
        state.confirm_selected_channel();
        {
            let entries = state.channel_pane_entries();
            assert!(!state.is_active_channel_entry(&entries[0]));
            assert!(state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.move_down();
        {
            let entries = state.channel_pane_entries();
            assert!(state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.confirm_selected_channel();
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[1]));
        assert!(state.is_active_channel_entry(&entries[2]));
    }

    #[test]
    fn selected_folder_can_be_closed_and_opened() {
        let mut state = state_with_folder(Some(42));

        assert_eq!(state.guild_pane_entries().len(), 4);
        state.close_selected_folder();
        let closed_entries = state.guild_pane_entries();
        assert_eq!(closed_entries.len(), 2);
        assert!(matches!(
            closed_entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));

        state.open_selected_folder();
        let open_entries = state.guild_pane_entries();
        assert_eq!(open_entries.len(), 4);
        assert!(matches!(
            open_entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: false,
                ..
            }
        ));
    }

    #[test]
    fn folder_children_use_middle_and_last_branches() {
        let state = state_with_folder(Some(42));

        let entries = state.guild_pane_entries();
        assert!(matches!(
            entries[2],
            GuildPaneEntry::Guild {
                branch: GuildBranch::Middle,
                ..
            }
        ));
        assert!(matches!(
            entries[3],
            GuildPaneEntry::Guild {
                branch: GuildBranch::Last,
                ..
            }
        ));
    }

    #[test]
    fn folder_without_id_can_be_closed() {
        let mut state = state_with_folder(None);

        state.close_selected_folder();
        let entries = state.guild_pane_entries();
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn selected_folder_child_can_close_parent() {
        let mut state = state_with_folder(Some(42));
        state.selected_guild = 2;

        state.toggle_selected_folder();
        let entries = state.guild_pane_entries();
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));
    }

    fn state_with_folder(folder_id: Option<u64>) -> DashboardState {
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        let mut state = DashboardState::new();

        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![GuildFolder {
                id: folder_id,
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }],
        });
        state
    }

    fn state_with_many_guilds(count: u64) -> DashboardState {
        let mut state = DashboardState::new();
        for id in 1..=count {
            state.push_event(AppEvent::GuildCreate {
                guild_id: Id::new(id),
                name: format!("guild {id}"),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
            });
        }
        state
    }

    fn state_with_many_channels(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let mut state = DashboardState::new();
        let channels = (1..=count)
            .map(|id| ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(id),
                parent_id: None,
                position: Some(id as i32),
                last_message_id: None,
                name: format!("channel {id}"),
                kind: "text".to_owned(),
            })
            .collect();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels,
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_members(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();
        let members = (1..=count)
            .map(|id| MemberInfo {
                user_id: Id::new(id),
                display_name: format!("member {id}"),
                is_bot: false,
            })
            .collect();
        let presences = (1..=count)
            .map(|id| (Id::new(id), PresenceStatus::Online))
            .collect();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members,
            presences,
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_grouped_members() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();
        let members = (1..=4)
            .map(|id| MemberInfo {
                user_id: Id::new(id),
                display_name: format!("member {id}"),
                is_bot: false,
            })
            .collect();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members,
            presences: vec![
                (Id::new(1), PresenceStatus::Online),
                (Id::new(2), PresenceStatus::Online),
                (Id::new(3), PresenceStatus::Offline),
                (Id::new(4), PresenceStatus::Offline),
            ],
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_channel_tree() -> DashboardState {
        let guild_id = Id::new(1);
        let category_id = Id::new(10);
        let general_id = Id::new(11);
        let random_id = Id::new(12);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: category_id,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "Text Channels".to_owned(),
                    kind: "category".to_owned(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: general_id,
                    parent_id: Some(category_id),
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: random_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: None,
                    name: "random".to_owned(),
                    kind: "text".to_owned(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_direct_messages() -> DashboardState {
        let mut state = DashboardState::new();
        for (channel_id, name, last_message_id) in [
            (Id::new(10), "old", Some(Id::new(100))),
            (Id::new(20), "new", Some(Id::new(200))),
            (Id::new(30), "empty", None),
        ] {
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
                guild_id: None,
                channel_id,
                parent_id: None,
                position: None,
                last_message_id,
                name: name.to_owned(),
                kind: "dm".to_owned(),
            }));
        }
        state
    }

    fn state_with_messages(count: u64) -> DashboardState {
        state_with_message_ids(1..=count)
    }

    fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
        state_with_messages_matching(1..=count, |id| image_message_ids.contains(&id))
    }

    fn state_with_message_ids(message_ids: impl IntoIterator<Item = u64>) -> DashboardState {
        state_with_messages_matching(message_ids, |_| false)
    }

    fn state_with_messages_matching(
        message_ids: impl IntoIterator<Item = u64>,
        has_image: impl Fn(u64) -> bool,
    ) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in message_ids {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("msg {id}")),
                attachments: has_image(id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state
    }

    fn image_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("image-{id}.png"),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }

    fn video_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("clip-{id}.mp4"),
            url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
            proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }

    fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(format!("forwarded {id}")),
            attachments: vec![image_attachment(id)],
        }
    }

    fn state_with_proxy_only_attachment_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some(String::new()),
            attachments: vec![AttachmentInfo {
                id: Id::new(3),
                filename: "cat.png".to_owned(),
                url: String::new(),
                proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
            }],
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64) -> MessageInfo {
        MessageInfo {
            guild_id: Some(Id::new(1)),
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some(format!("msg {message_id}")),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }

    fn channel_entry_names(state: &DashboardState) -> Vec<&str> {
        state
            .channel_pane_entries()
            .into_iter()
            .filter_map(|entry| match entry {
                ChannelPaneEntry::Channel { state, .. } => Some(state.name.as_str()),
                ChannelPaneEntry::CategoryHeader { .. } => None,
            })
            .collect()
    }

    fn state_with_two_guilds() -> DashboardState {
        let mut state = DashboardState::new();
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![first_guild],
                },
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![second_guild],
                },
            ],
        });
        state
    }

    fn focus_guilds(state: &mut DashboardState) {
        while state.focus() != FocusPane::Guilds {
            state.cycle_focus();
        }
    }

    fn focus_channels(state: &mut DashboardState) {
        while state.focus() != FocusPane::Channels {
            state.cycle_focus();
        }
    }

    fn focus_members(state: &mut DashboardState) {
        while state.focus() != FocusPane::Members {
            state.cycle_focus();
        }
    }

    fn focus_messages(state: &mut DashboardState) {
        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
    }
}
