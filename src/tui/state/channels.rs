use std::collections::HashSet;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{AppCommand, ChannelState};

use super::{ActiveGuildScope, DashboardState};
use super::{
    model::{
        ChannelActionItem, ChannelActionKind, ChannelBranch, ChannelPaneEntry, ChannelThreadItem,
        FocusPane,
    },
    popups::ChannelActionMenuState,
    presentation::{is_direct_message_channel, sort_channels, sort_direct_message_channels},
    scroll::{
        clamp_selected_index, close_collapsed_key, move_index_down, move_index_up,
        open_collapsed_key, pane_content_height, toggle_collapsed_key,
    },
};

impl DashboardState {
    pub fn channel_action_menu_title(&self) -> Option<String> {
        let channel_id = match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { channel_id, .. }
            | ChannelActionMenuState::Threads { channel_id, .. } => *channel_id,
        };
        let channel = self.discord.channel(channel_id)?;
        Some(format!("#{}", channel.name))
    }

    pub fn open_selected_channel_actions(&mut self) {
        if self.focus != FocusPane::Channels {
            return;
        }
        let Some(channel_id) = self.selected_channel_cursor_id() else {
            return;
        };
        let Some(channel) = self.discord.channel(channel_id) else {
            return;
        };
        if channel.is_category() || channel.is_thread() {
            return;
        }
        self.channel_action_menu = Some(ChannelActionMenuState::Actions {
            channel_id,
            selected: 0,
        });
    }

    pub fn close_channel_action_menu(&mut self) {
        self.channel_action_menu = None;
    }

    pub fn back_channel_action_menu(&mut self) {
        if let Some(ChannelActionMenuState::Threads { channel_id, .. }) =
            self.channel_action_menu.as_ref()
        {
            let channel_id = *channel_id;
            self.channel_action_menu = Some(ChannelActionMenuState::Actions {
                channel_id,
                selected: 0,
            });
        } else {
            self.channel_action_menu = None;
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let thread_count = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .count();
        let label = if thread_count == 0 {
            "Show threads (none)".to_owned()
        } else {
            format!("Show threads ({thread_count})")
        };
        vec![ChannelActionItem {
            kind: ChannelActionKind::ShowThreads,
            label,
            enabled: thread_count > 0,
        }]
    }

    pub fn channel_action_thread_items(&self) -> Vec<ChannelThreadItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Threads { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let mut threads: Vec<&ChannelState> = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .collect();
        sort_channels(&mut threads);
        threads
            .into_iter()
            .map(|c| ChannelThreadItem {
                channel_id: c.id,
                label: c.name.clone(),
                archived: c.thread_archived.unwrap_or(false),
                locked: c.thread_locked.unwrap_or(false),
            })
            .collect()
    }

    pub fn selected_channel_action_index(&self) -> Option<usize> {
        match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_action_items().len(),
            )),
            ChannelActionMenuState::Threads { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.channel_action_thread_items().len(),
            )),
        }
    }

    pub fn move_channel_action_down(&mut self) {
        let len = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelActionMenuState::Threads { .. }) => {
                self.channel_action_thread_items().len()
            }
            None => return,
        };
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_down(selected, len);
        }
    }

    pub fn move_channel_action_up(&mut self) {
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_up(selected);
        }
    }

    pub fn activate_selected_channel_action(&mut self) -> Option<AppCommand> {
        let menu = self.channel_action_menu.clone()?;
        match menu {
            ChannelActionMenuState::Actions {
                channel_id,
                selected,
            } => {
                let items = self.selected_channel_action_items();
                let item = items
                    .get(clamp_selected_index(selected, items.len()))?
                    .clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ChannelActionKind::ShowThreads => {
                        self.channel_action_menu = Some(ChannelActionMenuState::Threads {
                            channel_id,
                            selected: 0,
                        });
                        None
                    }
                }
            }
            ChannelActionMenuState::Threads { .. } => {
                let items = self.channel_action_thread_items();
                let index = self.selected_channel_action_index()?;
                let item = items.get(index)?.clone();
                let guild_id = self
                    .discord
                    .channel(item.channel_id)
                    .and_then(|c| c.guild_id);
                self.activate_channel(item.channel_id);
                self.close_channel_action_menu();
                guild_id.map(|guild_id| AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id: item.channel_id,
                })
            }
        }
    }

    pub(super) fn selected_channel_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            // DMs do not carry guild-style permissions; show every channel.
            ActiveGuildScope::DirectMessages => self.discord.channels_for_guild(None),
            // Filter to channels we have VIEW_CHANNEL on, otherwise the
            // sidebar surfaces channels that REST refuses with 403.
            ActiveGuildScope::Guild(guild_id) => {
                self.discord.viewable_channels_for_guild(Some(guild_id))
            }
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        // Threads are reached through the channel action menu instead of
        // appearing as top-level entries; without this filter their parent
        // channel would not be in `category_ids`, so the roots filter below
        // would let them through and render them under the channel list.
        channels.retain(|channel| !channel.is_thread());
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
        clamp_selected_index(self.selected_channel, self.channel_pane_entries().len())
    }

    pub(super) fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::CategoryHeader { .. }) | None => None,
        }
    }

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

    pub(super) fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
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

    /// Builds the "X is typing…" line for the currently selected channel, or
    /// `None` when nobody is typing (or the only typer is us). Resolution
    /// order for each user: cached guild member alias → DM recipient
    /// display name → `user-{id}` fallback. Caps at three names and
    /// collapses to "Several people are typing…" beyond that.
    pub fn typing_footer_for_selected_channel(&self) -> Option<String> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.discord.channel(channel_id)?;
        let guild_id = channel.guild_id;
        let typers: Vec<Id<UserMarker>> = self
            .discord
            .typing_users(channel_id)
            .into_iter()
            .filter(|user_id| Some(*user_id) != self.current_user_id)
            .collect();
        if typers.is_empty() {
            return None;
        }

        let resolve_name = |user_id: Id<UserMarker>| -> String {
            if let Some(name) =
                guild_id.and_then(|guild_id| self.discord.member_display_name(guild_id, user_id))
            {
                return name.to_owned();
            }
            if let Some(recipient) = channel
                .recipients
                .iter()
                .find(|recipient| recipient.user_id == user_id)
            {
                return recipient.display_name.clone();
            }
            format!("user-{}", user_id.get())
        };

        let total = typers.len();
        let names: Vec<String> = typers.iter().take(3).copied().map(resolve_name).collect();
        let footer = match total {
            1 => format!("{} is typing…", names[0]),
            2 => format!("{} and {} are typing…", names[0], names[1]),
            3 => format!("{}, {}, and {} are typing…", names[0], names[1], names[2]),
            _ => "Several people are typing…".to_owned(),
        };
        Some(footer)
    }

    pub fn channel_label(&self, channel_id: Id<ChannelMarker>) -> String {
        self.discord
            .channel(channel_id)
            .map(|channel| match channel.kind.as_str() {
                "dm" | "Private" => format!("@{}", channel.name),
                "group-dm" | "Group" => channel.name.clone(),
                _ => format!("#{}", channel.name),
            })
            .unwrap_or_else(|| format!("#channel-{}", channel_id.get()))
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
        toggle_collapsed_key(&mut self.collapsed_channel_categories, category_id);
    }

    pub fn open_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            open_collapsed_key(&mut self.collapsed_channel_categories, &category_id);
        }
    }

    pub fn close_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            close_collapsed_key(&mut self.collapsed_channel_categories, category_id);
        }
    }

    #[cfg(test)]
    pub fn confirm_selected_channel(&mut self) {
        let _ = self.confirm_selected_channel_command();
    }

    pub fn confirm_selected_channel_command(&mut self) -> Option<AppCommand> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { .. }) => {
                self.toggle_selected_channel_category();
                None
            }
            Some(ChannelPaneEntry::Channel { state, .. }) => {
                let channel_id = state.id;
                let command = if is_direct_message_channel(state) {
                    Some(AppCommand::SubscribeDirectMessage { channel_id })
                } else {
                    state
                        .guild_id
                        .map(|guild_id| AppCommand::SubscribeGuildChannel {
                            guild_id,
                            channel_id,
                        })
                };
                self.activate_channel(channel_id);
                command
            }
            None => None,
        }
    }

    pub(super) fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.active_channel_id = Some(channel_id);
        self.message_auto_follow = true;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
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
}
