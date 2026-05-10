use crate::discord::AppCommand;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::emoji::{custom_emoji_reaction_item, unicode_emoji_reaction_items};
use super::scroll::{clamp_selected_index, move_index_down, move_index_up};
use super::{
    DashboardState, EmojiReactionItem, EmojiReactionPickerState, ReactionUsersPopupState,
    indexed_shortcut,
};

impl DashboardState {
    pub fn is_emoji_reaction_picker_open(&self) -> bool {
        self.emoji_reaction_picker.is_some()
    }

    pub fn is_reaction_users_popup_open(&self) -> bool {
        self.reaction_users_popup.is_some()
    }

    pub fn reaction_users_popup(&self) -> Option<&ReactionUsersPopupState> {
        self.reaction_users_popup.as_ref()
    }

    pub fn emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        if let Some(picker) = &self.emoji_reaction_picker {
            return picker.items.clone();
        }

        self.emoji_reaction_items_for_guild(self.picker_guild_id())
    }

    fn emoji_reaction_items_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Vec<EmojiReactionItem> {
        let mut items = unicode_emoji_reaction_items();

        if let Some(guild_id) = guild_id {
            items.extend(
                self.discord
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .filter(|emoji| emoji.available)
                    .map(custom_emoji_reaction_item),
            );
        }

        items
    }

    pub fn filtered_emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        if let Some(picker) = &self.emoji_reaction_picker {
            return picker.filtered_items.clone();
        }

        let items = self.emoji_reaction_items();
        let Some(filter) = self.emoji_reaction_filter() else {
            return items;
        };

        filter_emoji_reaction_items(items, filter)
    }

    pub fn filtered_emoji_reaction_items_slice(&self) -> Option<&[EmojiReactionItem]> {
        self.emoji_reaction_picker
            .as_ref()
            .map(|picker| picker.filtered_items.as_slice())
    }

    pub fn emoji_reaction_filter(&self) -> Option<&str> {
        self.emoji_reaction_picker
            .as_ref()
            .and_then(|picker| picker.filter.as_deref())
    }

    pub fn is_filtering_emoji_reactions(&self) -> bool {
        self.emoji_reaction_filter().is_some()
    }

    pub fn close_emoji_reaction_picker(&mut self) {
        self.emoji_reaction_picker = None;
    }

    pub fn close_reaction_users_popup(&mut self) {
        self.reaction_users_popup = None;
    }

    pub fn scroll_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(1);
            popup.clamp_scroll();
        }
    }

    pub fn scroll_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(1);
        }
    }

    pub fn page_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(10);
            popup.clamp_scroll();
        }
    }

    pub fn page_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(10);
        }
    }

    pub fn set_reaction_users_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.view_height = height;
            popup.clamp_scroll();
        }
    }

    pub fn move_emoji_reaction_down(&mut self) {
        let reactions_len = self.filtered_emoji_reaction_items().len();
        if let Some(picker) = &mut self.emoji_reaction_picker {
            move_index_down(&mut picker.selected, reactions_len);
        }
    }

    pub fn move_emoji_reaction_up(&mut self) {
        if let Some(picker) = &mut self.emoji_reaction_picker {
            move_index_up(&mut picker.selected);
        }
    }

    pub fn selected_emoji_reaction_index_for_len(&self, len: usize) -> Option<usize> {
        self.emoji_reaction_picker
            .as_ref()
            .map(|picker| clamp_selected_index(picker.selected, len))
    }

    pub fn selected_emoji_reaction(&self) -> Option<EmojiReactionItem> {
        let items = self.filtered_emoji_reaction_items();
        let index = self.selected_emoji_reaction_index_for_len(items.len())?;
        items.get(index).cloned()
    }

    pub fn activate_selected_emoji_reaction(&mut self) -> Option<AppCommand> {
        let picker = self.emoji_reaction_picker.clone()?;
        let reaction = self.selected_emoji_reaction()?;
        let already_reacted = self.selected_message_state().is_some_and(|message| {
            message.channel_id == picker.channel_id
                && message.id == picker.message_id
                && message
                    .reactions
                    .iter()
                    .any(|existing| existing.me && existing.emoji == reaction.emoji)
        });
        let command = if already_reacted {
            AppCommand::RemoveReaction {
                channel_id: picker.channel_id,
                message_id: picker.message_id,
                emoji: reaction.emoji,
            }
        } else {
            AppCommand::AddReaction {
                channel_id: picker.channel_id,
                message_id: picker.message_id,
                emoji: reaction.emoji,
            }
        };
        self.close_emoji_reaction_picker();
        Some(command)
    }

    pub fn activate_emoji_reaction_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        let index = self
            .filtered_emoji_reaction_items()
            .iter()
            .enumerate()
            .position(|(index, _)| indexed_shortcut(index) == Some(shortcut))?;
        if let Some(picker) = &mut self.emoji_reaction_picker {
            picker.selected = index;
        }
        self.activate_selected_emoji_reaction()
    }

    pub fn start_emoji_reaction_filter(&mut self) {
        if let Some(picker) = &mut self.emoji_reaction_picker {
            picker.filter = Some(String::new());
            picker.filtered_items = picker.items.clone();
            picker.selected = 0;
        }
    }

    pub fn push_emoji_reaction_filter_char(&mut self, value: char) {
        if let Some(picker) = &mut self.emoji_reaction_picker
            && let Some(filter) = &mut picker.filter
        {
            filter.push(value);
            picker.filtered_items = filter_emoji_reaction_items_from_slice(&picker.items, filter);
            picker.selected = 0;
        }
    }

    pub fn pop_emoji_reaction_filter_char(&mut self) {
        if let Some(picker) = &mut self.emoji_reaction_picker
            && let Some(filter) = &mut picker.filter
        {
            filter.pop();
            picker.filtered_items = filter_emoji_reaction_items_from_slice(&picker.items, filter);
            picker.selected = 0;
        }
    }

    pub(super) fn open_emoji_reaction_picker(&mut self) {
        if let Some(message) = self.selected_message_state() {
            let guild_id = message
                .guild_id
                .or_else(|| self.selected_channel_guild_id());
            let items = self.emoji_reaction_items_for_guild(guild_id);
            self.emoji_reaction_picker = Some(EmojiReactionPickerState {
                selected: 0,
                filter: None,
                filtered_items: items.clone(),
                items,
                guild_id,
                channel_id: message.channel_id,
                message_id: message.id,
            });
        }
    }

    fn picker_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.emoji_reaction_picker
            .as_ref()
            .and_then(|picker| picker.guild_id)
            .or_else(|| {
                self.selected_message_state()
                    .and_then(|message| message.guild_id)
            })
            .or_else(|| self.selected_channel_guild_id())
    }
}

fn emoji_reaction_matches_filter(item: &EmojiReactionItem, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }

    item.label.to_lowercase().contains(filter)
        || item.emoji.status_label().to_lowercase().contains(filter)
}

fn filter_emoji_reaction_items(
    items: Vec<EmojiReactionItem>,
    filter: &str,
) -> Vec<EmojiReactionItem> {
    filter_emoji_reaction_items_from_slice(&items, filter)
}

fn filter_emoji_reaction_items_from_slice(
    items: &[EmojiReactionItem],
    filter: &str,
) -> Vec<EmojiReactionItem> {
    let filter = filter.to_lowercase();

    items
        .iter()
        .filter(|item| emoji_reaction_matches_filter(item, &filter))
        .cloned()
        .collect()
}
