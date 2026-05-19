use std::collections::HashSet;

use crate::discord::{AppCommand, ChannelState, ChannelUnreadState};
use crate::{
    discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker},
    },
    tui::fuzzy::FuzzyScore,
};
use unicode_segmentation::UnicodeSegmentation;

use super::super::fuzzy::fuzzy_text_score;
use super::{ActiveGuildScope, DashboardState};
use super::{
    model::{ChannelBranch, ChannelSwitcherItem, GuildPaneEntry},
    presentation::{is_direct_message_channel, sort_channels, sort_direct_message_channels},
    scroll::{clamp_selected_index, move_index_down, move_index_up},
};

#[derive(Debug)]
pub(super) struct ChannelSwitcherState {
    query: String,
    query_cursor_byte_index: usize,
    selected: usize,
    base_items: Vec<ChannelSwitcherItem>,
    query_items: Option<Vec<ChannelSwitcherItem>>,
}

impl ChannelSwitcherState {
    fn new(base_items: Vec<ChannelSwitcherItem>) -> Self {
        Self {
            query: String::new(),
            query_cursor_byte_index: 0,
            selected: 0,
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
        let query = self.query.trim();
        self.query_items =
            (!query.is_empty()).then(|| filter_channel_switcher_items(&self.base_items, query));
    }
}

impl DashboardState {
    pub fn is_channel_switcher_open(&self) -> bool {
        self.channel_switcher.is_some()
    }

    pub fn open_channel_switcher(&mut self) {
        self.close_all_action_contexts();
        self.close_leader();
        let items = self.all_channel_switcher_items();
        self.channel_switcher = Some(ChannelSwitcherState::new(items));
    }

    pub fn close_channel_switcher(&mut self) {
        self.channel_switcher = None;
    }

    pub fn channel_switcher_query(&self) -> Option<&str> {
        self.channel_switcher
            .as_ref()
            .map(|switcher| switcher.query.as_str())
    }

    pub fn channel_switcher_query_cursor_byte_index(&self) -> Option<usize> {
        let switcher = self.channel_switcher.as_ref()?;
        Some(clamp_cursor_index(
            &switcher.query,
            switcher.query_cursor_byte_index,
        ))
    }

    pub fn selected_channel_switcher_index(&self) -> Option<usize> {
        let switcher = self.channel_switcher.as_ref()?;
        Some(clamp_selected_index(
            switcher.selected,
            switcher.visible_len(),
        ))
    }

    pub fn channel_switcher_items(&self) -> Vec<ChannelSwitcherItem> {
        self.channel_switcher
            .as_ref()
            .map(|switcher| switcher.visible_items().to_vec())
            .unwrap_or_default()
    }

    pub fn move_channel_switcher_down(&mut self) {
        let len = self
            .channel_switcher
            .as_ref()
            .map(ChannelSwitcherState::visible_len)
            .unwrap_or_default();
        if let Some(switcher) = self.channel_switcher.as_mut() {
            move_index_down(&mut switcher.selected, len);
        }
    }

    pub fn move_channel_switcher_up(&mut self) {
        if let Some(switcher) = self.channel_switcher.as_mut() {
            move_index_up(&mut switcher.selected);
        }
    }

    pub fn select_channel_switcher_item(&mut self, row: usize) -> bool {
        let Some(switcher) = self.channel_switcher.as_mut() else {
            return false;
        };
        if row >= switcher.visible_len() {
            return false;
        }
        switcher.selected = row;
        true
    }

    pub fn push_channel_switcher_char(&mut self, value: char) {
        if let Some(switcher) = self.channel_switcher.as_mut() {
            let cursor = clamp_cursor_index(&switcher.query, switcher.query_cursor_byte_index);
            switcher.query.insert(cursor, value);
            switcher.query_cursor_byte_index = cursor + value.len_utf8();
            switcher.selected = 0;
            switcher.refresh_query_items();
        }
    }

    pub fn pop_channel_switcher_char(&mut self) {
        if let Some(switcher) = self.channel_switcher.as_mut() {
            let cursor = clamp_cursor_index(&switcher.query, switcher.query_cursor_byte_index);
            if cursor == 0 {
                return;
            }
            let start = previous_char_boundary(&switcher.query, cursor);
            switcher.query.replace_range(start..cursor, "");
            switcher.query_cursor_byte_index = start;
            switcher.selected = 0;
            switcher.refresh_query_items();
        }
    }

    pub fn move_channel_switcher_query_cursor_left(&mut self) {
        if let Some(switcher) = self.channel_switcher.as_mut() {
            let cursor = clamp_cursor_index(&switcher.query, switcher.query_cursor_byte_index);
            switcher.query_cursor_byte_index = previous_char_boundary(&switcher.query, cursor);
        }
    }

    pub fn move_channel_switcher_query_cursor_right(&mut self) {
        if let Some(switcher) = self.channel_switcher.as_mut() {
            let cursor = clamp_cursor_index(&switcher.query, switcher.query_cursor_byte_index);
            switcher.query_cursor_byte_index = next_char_boundary(&switcher.query, cursor);
        }
    }

    pub fn activate_selected_channel_switcher_item(&mut self) -> Option<AppCommand> {
        let item = {
            let switcher = self.channel_switcher.as_ref()?;
            let selected = clamp_selected_index(switcher.selected, switcher.visible_len());
            switcher.visible_items().get(selected)?.clone()
        };

        let Some(channel) = self.discord.channel(item.channel_id) else {
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
                    self.collapsed_channel_categories.remove(&parent_id);
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

        // Collect channels with active notifications into a dedicated top section.
        // Muted channels are intentionally excluded so the section reflects what
        // would actually ping the user.
        let mut notifications: Vec<ChannelSwitcherItem> = base
            .iter()
            .filter(|item| {
                item.guild_id
                    .is_none_or(|guild_id| !self.guild_notification_muted(guild_id))
                    && self.sidebar_channel_unread(item.channel_id) != ChannelUnreadState::Seen
            })
            .cloned()
            .collect();
        for item in notifications.iter_mut() {
            item.group_label = "Notifications".to_owned();
            item.parent_label = item.guild_name.clone();
            item.depth = 0;
        }
        notifications.sort_by_key(|item| match item.unread {
            ChannelUnreadState::Mentioned(_) => 0,
            ChannelUnreadState::Notified(_) => 1,
            ChannelUnreadState::Unread => 2,
            ChannelUnreadState::Seen => 3,
        });

        // Shift base group_order so the notifications section sorts above them
        // in filtered (scored) mode.
        for item in base.iter_mut() {
            item.group_order = item.group_order.saturating_add(1);
        }
        for item in notifications.iter_mut() {
            item.group_order = 0;
        }

        let mut items = notifications;
        items.extend(base);
        for (index, item) in items.iter_mut().enumerate() {
            item.original_index = index;
        }
        items
    }

    fn push_direct_message_switcher_items(&self, items: &mut Vec<ChannelSwitcherItem>) {
        let mut channels = self.discord.channels_for_guild(None);
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
                    branch: ChannelBranch::None,
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
        let mut channels = self.discord.viewable_channels_for_guild(Some(guild_id));
        channels.retain(|channel| !channel.is_thread());
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

        let group_order = items.len();
        for root in roots {
            if !root.is_category() {
                push_channel_switcher_item(
                    items,
                    ChannelSwitcherItemInput {
                        guild_id: Some(guild_id),
                        guild_name: Some(guild_name),
                        group_label: guild_name,
                        parent_label: None,
                        channel: root,
                        branch: ChannelBranch::None,
                        group_order,
                        unread: self.channel_unread(root.id),
                        unread_message_count: self.channel_unread_message_count(root.id),
                    },
                );
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
                push_channel_switcher_item(
                    items,
                    ChannelSwitcherItemInput {
                        guild_id: Some(guild_id),
                        guild_name: Some(guild_name),
                        group_label: guild_name,
                        parent_label: Some(root.name.as_str()),
                        channel: child,
                        branch,
                        group_order,
                        unread: self.channel_unread(child.id),
                        unread_message_count: self.channel_unread_message_count(child.id),
                    },
                );
            }
        }
    }
}

struct ChannelSwitcherItemInput<'a> {
    guild_id: Option<Id<GuildMarker>>,
    guild_name: Option<&'a str>,
    group_label: &'a str,
    parent_label: Option<&'a str>,
    channel: &'a ChannelState,
    branch: ChannelBranch,
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
        branch,
        group_order,
        unread,
        unread_message_count,
    } = input;
    if channel.is_category() || channel.is_thread() {
        return;
    }
    let original_index = items.len();
    let depth = usize::from(parent_label.is_some() || branch != ChannelBranch::None);
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

fn channel_switcher_match_score(item: &ChannelSwitcherItem, query: &str) -> Option<FuzzyScore> {
    fuzzy_text_score(&item.search_name, query)
}

fn filter_channel_switcher_items(
    items: &[ChannelSwitcherItem],
    query: &str,
) -> Vec<ChannelSwitcherItem> {
    let mut scored: Vec<(FuzzyScore, ChannelSwitcherItem)> = items
        .iter()
        .filter_map(|item| channel_switcher_match_score(item, query).map(|score| (score, item)))
        .map(|(score, item)| (score, item.clone()))
        .collect();
    scored.sort_by_key(|(score, item)| (item.group_order, *score, item.original_index));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn channel_switcher_channel_label(channel: &ChannelState) -> String {
    if is_direct_message_channel(channel) {
        match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            _ => channel.name.clone(),
        }
    } else {
        format!("#{}", channel.name)
    }
}

fn clamp_cursor_index(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn previous_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[..index]
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[index..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(value.len())
}
