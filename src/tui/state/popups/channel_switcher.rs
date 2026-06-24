use std::collections::HashSet;

use crate::discord::{ChannelState, ChannelUnreadState};
use crate::tui::text_input::TextInputState;
use crate::{
    discord::ids::{Id, marker::GuildMarker},
    tui::fuzzy::{FuzzyMatchQuality, FuzzyScore, fuzzy_name_match_score},
};

use crate::tui::fuzzy::fuzzy_text_score;
use crate::tui::keybindings::SelectionAction;

use super::super::{
    ActiveGuildScope, DashboardState, channel_tree,
    model::{ChannelSwitcherItem, GuildPaneEntry},
    presentation::{is_direct_message_channel, sort_direct_message_channels},
};
use crate::discord::AppCommand;
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup, SelectablePopupState};

#[derive(Debug)]
pub(in crate::tui::state) struct ChannelSwitcherState {
    query: TextInputState,
    selection: SelectablePopupState,
    base_items: Vec<ChannelSwitcherItem>,
    query_items: Option<Vec<ChannelSwitcherItem>>,
}

impl ChannelSwitcherState {
    fn new(base_items: Vec<ChannelSwitcherItem>) -> Self {
        Self {
            query: TextInputState::default(),
            selection: SelectablePopupState::default(),
            base_items,
            query_items: None,
        }
    }

    fn visible_items(&self) -> &[ChannelSwitcherItem] {
        self.query_items.as_deref().unwrap_or(&self.base_items)
    }

    fn visible_len(&self) -> usize {
        self.visible_items().len()
    }

    fn refresh_query_items(&mut self) {
        let query = self.query.value().trim();
        self.query_items =
            (!query.is_empty()).then(|| filter_channel_switcher_items(&self.base_items, query));
    }
}

impl DashboardState {
    pub fn open_channel_switcher(&mut self) {
        let items = self.all_channel_switcher_items();
        self.popups.modal = Some(ModalPopup::ChannelSwitcher(ChannelSwitcherState::new(
            items,
        )));
    }

    pub fn close_channel_switcher(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ChannelSwitcher) {
            self.popups.clear_modal();
        }
    }

    pub fn channel_switcher_query(&self) -> Option<&str> {
        self.popups
            .channel_switcher()
            .map(|switcher| switcher.query.value())
    }

    pub fn channel_switcher_query_cursor_byte_index(&self) -> Option<usize> {
        let switcher = self.popups.channel_switcher()?;
        Some(switcher.query.cursor_byte_index())
    }

    pub fn selected_channel_switcher_index(&self) -> Option<usize> {
        let switcher = self.popups.channel_switcher()?;
        Some(switcher.selection.selected_for_len(switcher.visible_len()))
    }

    pub fn channel_switcher_items(&self) -> Vec<ChannelSwitcherItem> {
        self.popups
            .channel_switcher()
            .map(|switcher| switcher.visible_items().to_vec())
            .unwrap_or_default()
    }

    pub fn move_channel_switcher_down(&mut self) {
        let len = self
            .popups
            .channel_switcher()
            .map(ChannelSwitcherState::visible_len)
            .unwrap_or_default();
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.selection.move_down(len);
        }
    }

    pub fn move_channel_switcher_up(&mut self) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.selection.move_up();
        }
    }

    pub(super) fn page_channel_switcher_selection(&mut self, action: SelectionAction) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.selection.page(switcher.visible_len(), action);
        }
    }

    pub fn select_channel_switcher_item(&mut self, row: usize) -> bool {
        let Some(switcher) = self.popups.channel_switcher_mut() else {
            return false;
        };
        if row >= switcher.visible_len() {
            return false;
        }
        switcher.selection.select(row);
        true
    }

    pub fn push_channel_switcher_char(&mut self, value: char) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.query.insert_char(value);
            switcher.selection.select(0);
            switcher.refresh_query_items();
        }
    }

    pub fn pop_channel_switcher_char(&mut self) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            if switcher.query.delete_previous_grapheme() {
                switcher.selection.select(0);
                switcher.refresh_query_items();
            }
        }
    }

    pub fn move_channel_switcher_query_cursor_left(&mut self) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.query.move_left();
        }
    }

    pub fn move_channel_switcher_query_cursor_right(&mut self) {
        if let Some(switcher) = self.popups.channel_switcher_mut() {
            switcher.query.move_right();
        }
    }

    pub fn activate_selected_channel_switcher_item(&mut self) -> Option<AppCommand> {
        let item = {
            let switcher = self.popups.channel_switcher()?;
            let selected = switcher.selection.selected_for_len(switcher.visible_len());
            switcher.visible_items().get(selected)?.clone()
        };

        let Some(channel) = self.discord.cache.channel(item.channel_id) else {
            self.close_channel_switcher();
            return None;
        };
        let guild_id = channel.guild_id;
        let parent_id = channel.parent_id;
        self.close_channel_switcher();

        match guild_id {
            Some(guild_id) => {
                self.activate_guild(ActiveGuildScope::Guild(guild_id));
                if let Some(parent_id) = parent_id {
                    self.navigation
                        .channels
                        .collapsed_channel_categories
                        .remove(&parent_id);
                }
                self.restore_channel_cursor(Some(item.channel_id));
                self.activate_channel(item.channel_id);
                Some(AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id: item.channel_id,
                })
            }
            None => {
                self.activate_guild(ActiveGuildScope::DirectMessages);
                self.restore_channel_cursor(Some(item.channel_id));
                self.activate_channel(item.channel_id);
                Some(AppCommand::SubscribeDirectMessage {
                    channel_id: item.channel_id,
                })
            }
        }
    }

    fn all_channel_switcher_items(&self) -> Vec<ChannelSwitcherItem> {
        let mut base = Vec::new();
        self.push_direct_message_switcher_items(&mut base);

        let mut seen = HashSet::new();
        for entry in self.guild_pane_entries() {
            let GuildPaneEntry::Guild { state: guild, .. } = entry else {
                continue;
            };
            if seen.insert(guild.id) {
                self.push_guild_channel_switcher_items(&mut base, guild.id, &guild.name);
            }
        }

        let recent = self.recent_channel_switcher_items(&base);
        if !recent.is_empty() {
            for item in base.iter_mut() {
                item.group_order = item.group_order.saturating_add(1);
            }
        }

        let mut items = recent;
        items.extend(base);
        for (index, item) in items.iter_mut().enumerate() {
            item.original_index = index;
        }
        items
    }

    fn recent_channel_switcher_items(
        &self,
        base: &[ChannelSwitcherItem],
    ) -> Vec<ChannelSwitcherItem> {
        let mut recent = Vec::new();
        let mut seen = HashSet::new();
        for channel_id in &self.navigation.channels.recent_channel_ids {
            if Some(*channel_id) == self.navigation.channels.active_channel_id {
                continue;
            }
            if !seen.insert(*channel_id) {
                continue;
            }
            let Some(item) = base.iter().find(|item| item.channel_id == *channel_id) else {
                continue;
            };
            let mut item = item.clone();
            item.group_label = "Recent Channels".to_owned();
            item.parent_label = item.guild_name.clone();
            item.depth = 0;
            item.group_order = 0;
            recent.push(item);
        }
        recent
    }

    fn push_direct_message_switcher_items(&self, items: &mut Vec<ChannelSwitcherItem>) {
        let mut channels = self.discord.cache.channels_for_guild(None);
        channels.retain(|channel| !channel.is_category() && !channel.is_thread());
        sort_direct_message_channels(&mut channels);
        let group_order = items.len();
        for channel in channels {
            push_channel_switcher_item(
                items,
                ChannelSwitcherItemInput {
                    guild_id: None,
                    guild_name: None,
                    group_label: "Direct Messages",
                    parent_label: None,
                    channel,
                    depth: 0,
                    group_order,
                    unread: self.channel_unread(channel.id),
                    unread_message_count: self.channel_unread_message_count(channel.id),
                },
            );
        }
    }

    fn push_guild_channel_switcher_items(
        &self,
        items: &mut Vec<ChannelSwitcherItem>,
        guild_id: Id<GuildMarker>,
        guild_name: &str,
    ) {
        // Threads stay in the list: the tree helpers skip them, so they only
        // surface as nested entries under their parent in the loop below.
        let channels = self
            .discord
            .cache
            .viewable_channels_for_guild(Some(guild_id));
        let group_order = items.len();
        for root in channel_tree::sorted_channel_tree_roots(&channels) {
            if !root.is_category() {
                self.push_channel_and_child_threads(
                    items,
                    &channels,
                    guild_id,
                    guild_name,
                    root,
                    None,
                    0,
                    group_order,
                );
                continue;
            }

            for child in channel_tree::sorted_category_children(&channels, root.id) {
                self.push_channel_and_child_threads(
                    items,
                    &channels,
                    guild_id,
                    guild_name,
                    child,
                    Some(root.name.as_str()),
                    1,
                    group_order,
                );
            }
        }
    }

    /// Push a channel followed by its joined, non-archived threads. Forum posts
    /// are threads parented to a forum; the switcher lists forum channels but
    /// not their posts, so we stop before descending into forums.
    #[allow(clippy::too_many_arguments)]
    fn push_channel_and_child_threads(
        &self,
        items: &mut Vec<ChannelSwitcherItem>,
        channels: &[&ChannelState],
        guild_id: Id<GuildMarker>,
        guild_name: &str,
        channel: &ChannelState,
        parent_label: Option<&str>,
        depth: usize,
        group_order: usize,
    ) {
        push_channel_switcher_item(
            items,
            ChannelSwitcherItemInput {
                guild_id: Some(guild_id),
                guild_name: Some(guild_name),
                group_label: guild_name,
                parent_label,
                channel,
                depth,
                group_order,
                unread: self.channel_unread(channel.id),
                unread_message_count: self.channel_unread_message_count(channel.id),
            },
        );

        if channel.is_forum() {
            return;
        }

        let thread_parent_label = match parent_label {
            Some(category) => format!("{category} / {}", channel.name),
            None => channel.name.clone(),
        };
        for thread in channel_tree::sorted_child_threads(channels.iter().copied(), channel.id) {
            // Match the channel pane: only joined, non-archived threads appear.
            if !thread.current_user_joined_thread || thread.thread_archived().unwrap_or(false) {
                continue;
            }
            push_channel_switcher_item(
                items,
                ChannelSwitcherItemInput {
                    guild_id: Some(guild_id),
                    guild_name: Some(guild_name),
                    group_label: guild_name,
                    parent_label: Some(thread_parent_label.as_str()),
                    channel: thread,
                    depth: depth.saturating_add(1),
                    group_order,
                    unread: self.channel_unread(thread.id),
                    unread_message_count: self.channel_unread_message_count(thread.id),
                },
            );
        }
    }
}

struct ChannelSwitcherItemInput<'a> {
    guild_id: Option<Id<GuildMarker>>,
    guild_name: Option<&'a str>,
    group_label: &'a str,
    parent_label: Option<&'a str>,
    channel: &'a ChannelState,
    depth: usize,
    group_order: usize,
    unread: ChannelUnreadState,
    unread_message_count: usize,
}

fn push_channel_switcher_item(
    items: &mut Vec<ChannelSwitcherItem>,
    input: ChannelSwitcherItemInput<'_>,
) {
    let ChannelSwitcherItemInput {
        guild_id,
        guild_name,
        group_label,
        parent_label,
        channel,
        depth,
        group_order,
        unread,
        unread_message_count,
    } = input;
    if channel.is_category() {
        return;
    }
    let original_index = items.len();
    items.push(ChannelSwitcherItem {
        channel_id: channel.id,
        guild_id,
        guild_name: guild_name.map(str::to_owned),
        group_label: group_label.to_owned(),
        parent_label: parent_label.map(str::to_owned),
        channel_label: channel_switcher_channel_label(channel),
        unread,
        unread_message_count,
        search_name: format!("{} / {}", group_label, channel.name),
        depth,
        group_order,
        original_index,
    });
}

fn channel_switcher_match_score(
    item: &ChannelSwitcherItem,
    query: &str,
) -> Option<(FuzzyMatchQuality, FuzzyScore)> {
    let query = query.trim();
    if let Some(prefix) = channel_switcher_query_label_prefix(query)
        && !item.channel_label.starts_with(prefix)
    {
        return None;
    }
    let channel_query = channel_switcher_search_channel_name(query);
    let channel_name = channel_switcher_search_channel_name(&item.channel_label);
    if let Some(score) = fuzzy_name_match_score(channel_name, channel_query) {
        return Some(score);
    }

    fuzzy_text_score(&item.search_name, query).map(|score| (FuzzyMatchQuality::Context, score))
}

fn filter_channel_switcher_items(
    items: &[ChannelSwitcherItem],
    query: &str,
) -> Vec<ChannelSwitcherItem> {
    let mut scored: Vec<(FuzzyMatchQuality, FuzzyScore, ChannelSwitcherItem)> = items
        .iter()
        .filter_map(|item| {
            channel_switcher_match_score(item, query)
                .map(|(quality, score)| (quality, score, item.clone()))
        })
        .collect();
    scored.sort_by_key(|(quality, score, item)| {
        (*quality, *score, item.group_order, item.original_index)
    });
    scored.into_iter().map(|(_, _, item)| item).collect()
}

fn channel_switcher_search_channel_name(channel_label: &str) -> &str {
    // Match known icon tokens rather than splitting on the first space, since
    // thread and forum names can themselves contain spaces.
    for prefix in CHANNEL_SWITCHER_ICON_PREFIXES {
        if let Some(rest) = channel_label.strip_prefix(prefix) {
            return rest;
        }
    }
    // A typed query has no trailing space (e.g. "#general", "@alice").
    channel_label
        .strip_prefix('#')
        .or_else(|| channel_label.strip_prefix('@'))
        .unwrap_or(channel_label)
}

fn channel_switcher_query_label_prefix(query: &str) -> Option<char> {
    query
        .chars()
        .next()
        .filter(|prefix| matches!(prefix, '#' | '@'))
}

/// Must stay in sync with the prefixes emitted by
/// `channel_switcher_channel_label` so the search helper can strip them back off.
const CHANNEL_SWITCHER_ICON_PREFIXES: [&str; 4] = ["# ", "@ ", "🧵 ", "📝 "];

fn channel_switcher_channel_label(channel: &ChannelState) -> String {
    if is_direct_message_channel(channel) {
        match channel.kind.as_str() {
            "dm" | "Private" => format!("@ {}", channel.name),
            _ => channel.name.clone(),
        }
    } else if channel.is_thread() {
        format!("🧵 {}", channel.name)
    } else if channel.is_forum() {
        format!("📝 {}", channel.name)
    } else {
        format!("# {}", channel.name)
    }
}
