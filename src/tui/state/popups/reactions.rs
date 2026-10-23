use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};
use crate::discord::{AppCommand, MessageState, ReactionEmoji};
use crate::tui::fuzzy::{FuzzyScore, fuzzy_text_score};
use crate::tui::keybindings::SelectionAction;

use super::super::emoji::{
    custom_emoji_can_be_used_directly, custom_emoji_identity, custom_emoji_reaction_item,
    is_quick_unicode_emoji, quick_emoji_reaction_items, remaining_unicode_emoji_reaction_items,
};
use super::super::{
    DashboardState, EmojiReactionItem, EmojiReactionPickerState, ReactionUsersPopupState,
};
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup, SelectablePopupTarget};

impl DashboardState {
    pub fn reaction_users_popup(&self) -> Option<&ReactionUsersPopupState> {
        self.popups.reaction_users_popup()
    }

    pub fn emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        if let Some(picker) = self.popups.emoji_reaction_picker() {
            return picker.items.clone();
        }

        self.emoji_reaction_items_for_guild(self.picker_guild_id())
    }

    fn emoji_reaction_items_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Vec<EmojiReactionItem> {
        let pins = self.pinned_emojis();
        let mut items = quick_emoji_reaction_items(pins);
        let is_pinned = |emoji: &ReactionEmoji| pins.iter().any(|pin| pin.pin_identity_eq(emoji));

        if let Some(guild_id) = guild_id {
            items.extend(
                self.discord
                    .cache
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .filter(|emoji| {
                        emoji.available
                            && custom_emoji_can_be_used_directly(
                                emoji,
                                false,
                                self.current_user_has_nitro(),
                            )
                            && !is_pinned(&custom_emoji_identity(emoji))
                    })
                    .map(custom_emoji_reaction_item),
            );
        }

        let can_use_external_emojis = self
            .selected_message_state()
            .and_then(|message| self.discord.cache.channel(message.channel_id))
            .or_else(|| self.selected_channel_state())
            .is_none_or(|channel| {
                self.discord
                    .cache
                    .can_use_external_emojis_in_channel(channel)
            });
        if self.current_user_has_nitro() && can_use_external_emojis {
            items.extend(
                self.discord
                    .cache
                    .all_custom_emojis()
                    .filter(|(emoji_guild_id, _)| {
                        guild_id.is_none_or(|guild_id| guild_id != **emoji_guild_id)
                    })
                    .flat_map(|(_, emojis)| emojis)
                    .filter(|emoji| {
                        emoji.available
                            && custom_emoji_can_be_used_directly(
                                emoji,
                                true,
                                self.current_user_has_nitro(),
                            )
                            && !is_pinned(&custom_emoji_identity(emoji))
                    })
                    .map(custom_emoji_reaction_item),
            );
        }

        items.extend(remaining_unicode_emoji_reaction_items(pins));

        items
    }

    pub fn filtered_emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        if let Some(picker) = self.popups.emoji_reaction_picker() {
            return picker.filtered_items.clone();
        }

        let items = self.emoji_reaction_items();
        let Some(filter) = self.emoji_reaction_filter() else {
            return items;
        };

        filter_emoji_reaction_items(items, filter, self.pinned_emojis())
    }

    pub fn filtered_emoji_reaction_items_slice(&self) -> Option<&[EmojiReactionItem]> {
        self.popups
            .emoji_reaction_picker()
            .map(|picker| picker.filtered_items.as_slice())
    }

    pub fn emoji_reaction_filter(&self) -> Option<&str> {
        self.popups
            .emoji_reaction_picker()
            .and_then(|picker| picker.filter.as_deref())
    }

    pub fn existing_emoji_reactions(&self) -> &[crate::discord::ReactionEmoji] {
        self.popups
            .emoji_reaction_picker()
            .map(|picker| picker.existing_reactions.as_slice())
            .unwrap_or(&[])
    }

    pub fn own_emoji_reactions(&self) -> &[crate::discord::ReactionEmoji] {
        self.popups
            .emoji_reaction_picker()
            .map(|picker| picker.own_reactions.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_filtering_emoji_reactions(&self) -> bool {
        self.emoji_reaction_filter().is_some()
    }

    pub fn is_editing_emoji_reaction_filter(&self) -> bool {
        self.popups
            .emoji_reaction_picker()
            .is_some_and(|picker| picker.filter_editing)
    }

    pub fn close_emoji_reaction_picker(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker) {
            self.popups.clear_modal();
        }
    }

    pub fn close_reaction_users_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ReactionUsers) {
            self.popups.clear_modal();
        }
    }

    pub fn open_reaction_users_popup(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<(ReactionEmoji, u64)>,
    ) {
        let popup = ReactionUsersPopupState::new(channel_id, message_id, reactions);
        self.popups.set_modal(ModalPopup::ReactionUsers(popup));
    }

    pub fn navigate_reaction_users_popup(&mut self, action: SelectionAction) -> Option<AppCommand> {
        if !self.popups.reaction_users_popup()?.is_viewing_users() {
            self.move_selectable_popup(SelectablePopupTarget::ReactionList, action);
            return None;
        }
        let popup = self.popups.reaction_users_popup_mut()?;
        match action {
            SelectionAction::Previous => {
                popup.user_scroll.scroll_up();
                None
            }
            SelectionAction::Next => {
                popup.user_scroll.scroll_down();
                let (emoji, after) = popup.take_load_more()?;
                Some(AppCommand::LoadReactionUsers {
                    channel_id: popup.channel_id,
                    message_id: popup.message_id,
                    emoji,
                    after: Some(after),
                })
            }
        }
    }

    /// Fetches the next page after a page or half-page scroll of the user list,
    /// when the reader is near the bottom and more users remain.
    pub fn reaction_users_popup_take_load_more(&mut self) -> Option<AppCommand> {
        let popup = self.popups.reaction_users_popup_mut()?;
        let (emoji, after) = popup.take_load_more()?;
        Some(AppCommand::LoadReactionUsers {
            channel_id: popup.channel_id,
            message_id: popup.message_id,
            emoji,
            after: Some(after),
        })
    }

    pub fn activate_reaction_users_popup(&mut self) -> Option<AppCommand> {
        let popup = self.popups.reaction_users_popup_mut()?;
        if popup.is_viewing_users() {
            return None;
        }
        let emoji = popup.open_selected()?;
        Some(AppCommand::LoadReactionUsers {
            channel_id: popup.channel_id,
            message_id: popup.message_id,
            emoji,
            after: None,
        })
    }

    pub fn reaction_users_popup_back(&mut self) -> bool {
        self.popups
            .reaction_users_popup_mut()
            .is_some_and(|popup| popup.back_to_list())
    }

    pub fn set_reaction_users_document_view_height(&mut self, height: usize) {
        if let Some(popup) = self.popups.reaction_users_popup_mut() {
            if !popup.is_viewing_users() {
                return;
            }
            popup.set_user_view_height(height);
        }
    }

    pub fn move_emoji_reaction_down(&mut self) {
        self.move_selectable_popup(SelectablePopupTarget::EmojiReactions, SelectionAction::Next);
    }

    pub fn move_emoji_reaction_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::EmojiReactions,
            SelectionAction::Previous,
        );
    }

    pub fn selected_emoji_reaction_index_for_len(&self, len: usize) -> Option<usize> {
        self.popups
            .emoji_reaction_picker()
            .map(|picker| picker.selection.selected_for_len(len))
    }

    pub fn selected_emoji_reaction(&self) -> Option<EmojiReactionItem> {
        let items = self.filtered_emoji_reaction_items();
        let index = self.selected_emoji_reaction_index_for_len(items.len())?;
        items.get(index).cloned()
    }

    pub fn activate_selected_emoji_reaction(&mut self) -> Option<AppCommand> {
        let picker = self.popups.emoji_reaction_picker().cloned()?;
        let reaction = self.selected_emoji_reaction()?;
        let selected_message = self.selected_message_state().filter(|message| {
            message.channel_id == picker.channel_id && message.id == picker.message_id
        });
        let already_reacted = selected_message.is_some_and(|message| {
            message.channel_id == picker.channel_id
                && message.id == picker.message_id
                && message
                    .reactions
                    .iter()
                    .any(|existing| existing.me && existing.emoji == reaction.emoji)
        });
        if !already_reacted
            && selected_message
                .is_some_and(|message| !self.can_add_reaction_to_message(message, &reaction.emoji))
        {
            self.close_emoji_reaction_picker();
            return None;
        }
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
        let picker = self.popups.emoji_reaction_picker()?;
        let index = picker
            .filtered_items
            .iter()
            .enumerate()
            .position(|(index, _)| {
                self.options
                    .key_bindings()
                    .emoji_reaction_shortcut(&picker.filtered_items, index)
                    == Some(shortcut)
            })?;
        if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
            picker.selection.select(index);
        }
        self.activate_selected_emoji_reaction()
    }

    pub fn start_emoji_reaction_filter(&mut self) {
        if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
            picker.filter = Some(String::new());
            picker.filter_editing = true;
            picker.filtered_items = picker.items.clone();
            picker.selection.select(0);
        }
    }

    pub fn commit_emoji_reaction_filter(&mut self) {
        if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
            picker.filter_editing = false;
        }
    }

    pub fn push_emoji_reaction_filter_char(&mut self, value: char) {
        let pins = &self.options.reaction_options.pinned_emojis;
        if let Some(picker) = self.popups.emoji_reaction_picker_mut()
            && let Some(filter) = &mut picker.filter
        {
            filter.push(value);
            picker.filtered_items =
                filter_emoji_reaction_items_from_slice(&picker.items, filter, pins);
            picker.selection.select(0);
        }
    }

    pub fn pop_emoji_reaction_filter_char(&mut self) {
        let pins = &self.options.reaction_options.pinned_emojis;
        if let Some(picker) = self.popups.emoji_reaction_picker_mut()
            && let Some(filter) = &mut picker.filter
        {
            filter.pop();
            picker.filtered_items =
                filter_emoji_reaction_items_from_slice(&picker.items, filter, pins);
            picker.selection.select(0);
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
            let own_reactions = message
                .reactions
                .iter()
                .filter(|reaction| reaction.me)
                .map(|reaction| reaction.emoji.clone())
                .collect::<Vec<_>>();
            let items = self.emoji_reaction_picker_items_for_message(message, guild_id);
            self.popups
                .set_modal(ModalPopup::EmojiReactionPicker(EmojiReactionPickerState {
                    selection: Default::default(),
                    filter: None,
                    filter_editing: false,
                    filtered_items: items.clone(),
                    items,
                    existing_reactions,
                    own_reactions,
                    guild_id,
                    channel_id: message.channel_id,
                    message_id: message.id,
                }));
        }
    }

    fn emoji_reaction_picker_items_for_message(
        &self,
        message: &MessageState,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Vec<EmojiReactionItem> {
        if self.can_add_new_reaction_for_message(message) {
            let existing_reactions = message
                .reactions
                .iter()
                .map(|reaction| reaction.emoji.clone())
                .collect::<Vec<_>>();
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
        }
    }

    /// Toggles the pinned state of the currently highlighted reaction and
    /// re-sorts the open picker's item list to reflect it immediately.
    pub fn toggle_selected_emoji_reaction_pin(&mut self) {
        let Some(reaction) = self.selected_emoji_reaction() else {
            return;
        };
        self.toggle_pinned_emoji(&reaction.emoji);
    }

    /// Rebuilds the open emoji reaction picker's item list (e.g. after a pin
    /// is toggled) so the pinned-first ordering shows without closing it.
    pub(in crate::tui::state) fn refresh_open_emoji_reaction_picker_items(&mut self) {
        let Some(picker) = self.popups.emoji_reaction_picker() else {
            return;
        };
        let channel_id = picker.channel_id;
        let message_id = picker.message_id;
        let guild_id = picker.guild_id;
        let filter = picker.filter.clone();
        let Some(message) = self
            .selected_message_state()
            .filter(|message| message.channel_id == channel_id && message.id == message_id)
        else {
            return;
        };
        let items = self.emoji_reaction_picker_items_for_message(message, guild_id);
        let pins = self.options.reaction_options.pinned_emojis.clone();
        let Some(picker) = self.popups.emoji_reaction_picker_mut() else {
            return;
        };
        picker.filtered_items = match &filter {
            Some(filter) => filter_emoji_reaction_items_from_slice(&items, filter, &pins),
            None => items.clone(),
        };
        picker.items = items;
        picker.selection.select(0);
    }

    fn picker_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.popups
            .emoji_reaction_picker()
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
    pins: &[ReactionEmoji],
) -> Vec<EmojiReactionItem> {
    filter_emoji_reaction_items_from_slice(&items, filter, pins)
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
    pins: &[ReactionEmoji],
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
                    usize::from(emoji_reaction_is_remaining_unicode(item, pins)),
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

fn emoji_reaction_is_remaining_unicode(item: &EmojiReactionItem, pins: &[ReactionEmoji]) -> bool {
    matches!(
        &item.emoji,
        crate::discord::ReactionEmoji::Unicode(emoji) if !is_quick_unicode_emoji(emoji, pins)
    )
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
