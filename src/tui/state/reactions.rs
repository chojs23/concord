use crate::discord::AppCommand;
use crate::discord::ids::{Id, marker::GuildMarker};
use crate::tui::fuzzy::FuzzyScore;

use super::super::fuzzy::fuzzy_text_score;
use super::emoji::{
    custom_emoji_reaction_item, is_quick_unicode_emoji, quick_unicode_emoji_reaction_items,
    remaining_unicode_emoji_reaction_items,
};
use super::scroll::{clamp_selected_index, move_index_down, move_index_up};
use super::{
    DashboardState, EmojiReactionItem, EmojiReactionPickerState, ReactionUsersPopupState,
    emoji_reaction_shortcut,
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
        let mut items = quick_unicode_emoji_reaction_items();

        if let Some(guild_id) = guild_id {
            items.extend(
                self.discord
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .filter(|emoji| emoji.available)
                    .map(custom_emoji_reaction_item),
            );
        }

        items.extend(remaining_unicode_emoji_reaction_items());

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

    pub fn existing_emoji_reactions(&self) -> &[crate::discord::ReactionEmoji] {
        self.emoji_reaction_picker
            .as_ref()
            .map(|picker| picker.existing_reactions.as_slice())
            .unwrap_or(&[])
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
        let selected_message = self.selected_message_state().filter(|message| {
            message.channel_id == picker.channel_id && message.id == picker.message_id
        });
        if let Some(message) = selected_message
            && !self.can_add_reaction_to_message(message, &reaction.emoji)
        {
            self.close_emoji_reaction_picker();
            return None;
        }
        let already_reacted = selected_message.is_some_and(|message| {
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
        let picker = self.emoji_reaction_picker.as_ref()?;
        let index = picker
            .filtered_items
            .iter()
            .enumerate()
            .position(|(index, _)| {
                emoji_reaction_shortcut(&picker.filtered_items, &picker.existing_reactions, index)
                    == Some(shortcut)
            })?;
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

    pub fn open_emoji_reaction_picker(&mut self) {
        if let Some(message) = self.selected_message_state() {
            if !self.can_open_reaction_picker(message) {
                return;
            }
            let guild_id = message
                .guild_id
                .or_else(|| self.selected_channel_guild_id());
            let existing_reactions = message
                .reactions
                .iter()
                .map(|reaction| reaction.emoji.clone())
                .collect::<Vec<_>>();
            let items = if self.can_add_new_reaction_for_message(message) {
                prioritize_existing_reactions(
                    self.emoji_reaction_items_for_guild(guild_id),
                    &existing_reactions,
                )
            } else {
                message
                    .reactions
                    .iter()
                    .map(|reaction| EmojiReactionItem {
                        emoji: reaction.emoji.clone(),
                        label: reaction.emoji.status_label(),
                    })
                    .collect()
            };
            self.emoji_reaction_picker = Some(EmojiReactionPickerState {
                selected: 0,
                filter: None,
                filtered_items: items.clone(),
                items,
                existing_reactions,
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

fn filter_emoji_reaction_items(
    items: Vec<EmojiReactionItem>,
    filter: &str,
) -> Vec<EmojiReactionItem> {
    filter_emoji_reaction_items_from_slice(&items, filter)
}

fn prioritize_existing_reactions(
    items: Vec<EmojiReactionItem>,
    existing_reactions: &[crate::discord::ReactionEmoji],
) -> Vec<EmojiReactionItem> {
    if existing_reactions.is_empty() {
        return items;
    }

    let mut prioritized = Vec::with_capacity(items.len());
    for existing in existing_reactions {
        if let Some(item) = items.iter().find(|item| &item.emoji == existing) {
            prioritized.push(item.clone());
        } else {
            prioritized.push(EmojiReactionItem {
                emoji: existing.clone(),
                label: existing.status_label(),
            });
        }
    }
    prioritized.extend(
        items
            .into_iter()
            .filter(|item| !existing_reactions.contains(&item.emoji)),
    );
    prioritized
}

fn filter_emoji_reaction_items_from_slice(
    items: &[EmojiReactionItem],
    filter: &str,
) -> Vec<EmojiReactionItem> {
    let filter = filter.trim();
    if filter.is_empty() {
        return items.to_vec();
    }

    let mut scored: Vec<(usize, FuzzyScore, usize, EmojiReactionItem)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            emoji_reaction_filter_score(item, filter).map(|score| {
                (
                    usize::from(emoji_reaction_is_remaining_unicode(item)),
                    score,
                    index,
                    item.clone(),
                )
            })
        })
        .collect();

    scored.sort_by_key(|(is_remaining_unicode, score, index, _)| {
        (*is_remaining_unicode, *score, *index)
    });
    scored.into_iter().map(|(_, _, _, item)| item).collect()
}

fn emoji_reaction_is_remaining_unicode(item: &EmojiReactionItem) -> bool {
    matches!(&item.emoji, crate::discord::ReactionEmoji::Unicode(emoji) if !is_quick_unicode_emoji(emoji))
}

fn emoji_reaction_filter_score(item: &EmojiReactionItem, filter: &str) -> Option<FuzzyScore> {
    let label_score = fuzzy_text_score(&item.label, filter);
    let status_score = fuzzy_text_score(&item.emoji.status_label(), filter);
    match (label_score, status_score) {
        (Some(label), Some(status)) => Some(label.min(status)),
        (Some(label), None) => Some(label),
        (None, Some(status)) => Some(status),
        (None, None) => None,
    }
}
