use crate::discord::{AppCommand, MessageState, ReactionEmoji};

use super::scroll::{clamp_selected_index, move_index_down, move_index_up};
use super::{
    DashboardState, FocusPane, MessageActionItem, MessageActionKind, MessageActionMenuState,
    message_action_shortcut, popups,
};

impl DashboardState {
    pub fn activate_selected_message_pane_item(&mut self) -> Option<AppCommand> {
        if self.selected_channel_is_forum() {
            return self.activate_selected_forum_post();
        }
        self.open_selected_message_actions();
        None
    }

    pub fn is_message_action_menu_open(&self) -> bool {
        self.message_action_menu.is_some()
    }

    pub fn open_selected_message_actions(&mut self) {
        if self.focus == FocusPane::Messages && self.selected_message_state().is_some() {
            self.message_action_menu = Some(MessageActionMenuState { selected: 0 });
        }
    }

    pub fn close_message_action_menu(&mut self) {
        self.message_action_menu = None;
    }

    pub fn move_message_action_down(&mut self) {
        let actions_len = self.selected_message_action_items().len();
        if let Some(menu) = &mut self.message_action_menu {
            move_index_down(&mut menu.selected, actions_len);
        }
    }

    pub fn move_message_action_up(&mut self) {
        if let Some(menu) = &mut self.message_action_menu {
            move_index_up(&mut menu.selected);
        }
    }

    pub fn select_message_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_message_action_items().len() {
            return false;
        }
        if let Some(menu) = &mut self.message_action_menu {
            menu.selected = row;
            return true;
        }
        false
    }

    pub fn selected_message_action_items(&self) -> Vec<MessageActionItem> {
        let Some(message) = self.selected_message_state() else {
            return Vec::new();
        };
        let mut actions = vec![MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "Reply".to_owned(),
            enabled: true,
        }];

        let capabilities = message.capabilities();
        let is_own_chat_message = Some(message.author_id) == self.current_user_id
            && message.message_kind.is_regular_or_reply();
        if is_own_chat_message && message.content.is_some() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::Edit,
                label: "Edit message".to_owned(),
                enabled: true,
            });
        }
        if self.can_delete_message(message) {
            actions.push(MessageActionItem {
                kind: MessageActionKind::Delete,
                label: "Delete message".to_owned(),
                enabled: true,
            });
        }
        if self.thread_summary_for_message(message).is_some() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::OpenThread,
                label: "Open thread".to_owned(),
                enabled: true,
            });
        }
        if capabilities.has_image && self.show_images() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::ViewImage,
                label: "View image".to_owned(),
                enabled: true,
            });
        }
        if message.message_kind.is_regular_or_reply() {
            // Image attachments already have a direct `d` download path in the
            // image viewer, so the message-level menu only surfaces downloads
            // for the file/video kinds that have no other entry point.
            for (index, attachment) in message.attachments_in_display_order().enumerate() {
                if attachment.is_image() && attachment.preferred_url().is_some() {
                    continue;
                }
                if attachment.preferred_url().is_none() {
                    continue;
                }
                actions.push(MessageActionItem {
                    kind: MessageActionKind::DownloadAttachment(index),
                    label: format!("Download {}", attachment.filename),
                    enabled: true,
                });
            }
        }
        if self.can_open_reaction_picker(message) {
            actions.push(MessageActionItem {
                kind: MessageActionKind::AddReaction,
                label: "Add reaction".to_owned(),
                enabled: true,
            });
        }
        actions.push(MessageActionItem {
            kind: MessageActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        });
        if self.can_pin_messages_for_message(message) {
            actions.push(MessageActionItem {
                kind: MessageActionKind::SetPinned(!message.pinned),
                label: if message.pinned {
                    "Unpin message".to_owned()
                } else {
                    "Pin message".to_owned()
                },
                enabled: true,
            });
        }
        if !message.reactions.is_empty() && self.can_show_reaction_users_for_message(message) {
            actions.push(MessageActionItem {
                kind: MessageActionKind::ShowReactionUsers,
                label: "Show reacted users".to_owned(),
                enabled: true,
            });
        }
        for (index, reaction) in message.reactions.iter().enumerate() {
            if reaction.me {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::RemoveReaction(index),
                    label: format!(
                        "Remove {} reaction",
                        action_reaction_label(&reaction.emoji, self.show_custom_emoji())
                    ),
                    enabled: true,
                });
            }
        }
        if let Some(poll) = &message.poll
            && !poll.results_finalized.unwrap_or(false)
        {
            if poll.allow_multiselect {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::OpenPollVotePicker,
                    label: "Choose poll votes".to_owned(),
                    enabled: true,
                });
            } else {
                for answer in &poll.answers {
                    actions.push(MessageActionItem {
                        kind: MessageActionKind::VotePollAnswer(answer.answer_id),
                        label: if answer.me_voted {
                            format!("Remove poll vote: {}", answer.text)
                        } else {
                            format!("Vote poll: {}", answer.text)
                        },
                        enabled: true,
                    });
                }
            }
        }
        actions
    }

    pub fn selected_message_action_index(&self) -> Option<usize> {
        self.message_action_menu.as_ref().map(|menu| {
            clamp_selected_index(menu.selected, self.selected_message_action_items().len())
        })
    }

    pub fn selected_message_action(&self) -> Option<MessageActionItem> {
        let index = self.selected_message_action_index()?;
        self.selected_message_action_items().get(index).cloned()
    }

    pub fn activate_selected_message_action(&mut self) -> Option<AppCommand> {
        let action = self.selected_message_action()?;
        if !action.enabled {
            return None;
        }

        match action.kind {
            MessageActionKind::Reply => {
                self.start_reply_composer();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::Edit => {
                self.start_edit_composer();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::Delete => {
                self.open_selected_message_delete_confirmation();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::OpenThread => {
                let channel_id = self
                    .selected_message_state()
                    .and_then(|message| self.thread_summary_for_message(message))?
                    .channel_id;
                self.record_thread_return_target(channel_id);
                self.activate_channel(channel_id);
                self.close_message_action_menu();
                None
            }
            MessageActionKind::ViewImage => {
                self.close_message_action_menu();
                self.open_image_viewer_for_selected_message();
                None
            }
            MessageActionKind::DownloadAttachment(index) => {
                let message = self.selected_message_state()?;
                let attachment = message.attachments_in_display_order().nth(index)?;
                let url = attachment.preferred_url()?.to_owned();
                let filename = attachment.filename.clone();
                self.close_message_action_menu();
                Some(AppCommand::DownloadAttachment {
                    url,
                    filename,
                    source: crate::discord::DownloadAttachmentSource::MessageAction,
                })
            }
            MessageActionKind::AddReaction => {
                self.open_emoji_reaction_picker();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::RemoveReaction(index) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reaction = message.reactions.get(index)?.clone();
                self.close_message_action_menu();
                Some(AppCommand::RemoveReaction {
                    channel_id,
                    message_id,
                    emoji: reaction.emoji,
                })
            }
            MessageActionKind::ShowProfile => {
                let message = self.selected_message_state()?;
                let user_id = message.author_id;
                let guild_id = message.guild_id;
                self.close_message_action_menu();
                self.open_user_profile_popup(user_id, guild_id)
            }
            MessageActionKind::ShowReactionUsers => {
                let message = self.selected_message_state()?;
                if !self.can_show_reaction_users_for_message(message) {
                    self.close_message_action_menu();
                    return None;
                }
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reactions = message
                    .reactions
                    .iter()
                    .map(|reaction| reaction.emoji.clone())
                    .collect::<Vec<_>>();
                if reactions.is_empty() {
                    self.close_message_action_menu();
                    return None;
                }
                self.close_message_action_menu();
                Some(AppCommand::LoadReactionUsers {
                    channel_id,
                    message_id,
                    reactions,
                })
            }
            MessageActionKind::SetPinned(pinned) => {
                self.open_selected_message_pin_confirmation(pinned);
                self.close_message_action_menu();
                None
            }
            MessageActionKind::OpenPollVotePicker => {
                self.open_poll_vote_picker();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::VotePollAnswer(answer_id) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let poll = message.poll.as_ref()?;
                let mut answer_ids = if poll.allow_multiselect {
                    poll.answers
                        .iter()
                        .filter(|answer| answer.me_voted && answer.answer_id != answer_id)
                        .map(|answer| answer.answer_id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if !poll
                    .answers
                    .iter()
                    .any(|answer| answer.answer_id == answer_id && answer.me_voted)
                {
                    answer_ids.push(answer_id);
                }
                self.close_message_action_menu();
                Some(AppCommand::VotePoll {
                    channel_id,
                    message_id,
                    answer_ids,
                })
            }
        }
    }

    pub(super) fn can_add_reaction_to_message(
        &self,
        message: &MessageState,
        emoji: &ReactionEmoji,
    ) -> bool {
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        if !self.discord.can_read_message_history_in_channel(channel) {
            return false;
        }
        message
            .reactions
            .iter()
            .any(|reaction| &reaction.emoji == emoji)
            || self.discord.can_add_reactions_in_channel(channel)
    }

    pub(super) fn can_open_reaction_picker(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        self.discord.can_read_message_history_in_channel(channel)
            && (self.discord.can_add_reactions_in_channel(channel) || !message.reactions.is_empty())
    }

    pub(super) fn can_add_new_reaction_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        self.discord.can_add_reactions_in_channel(channel)
    }

    fn can_show_reaction_users_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        self.discord.can_read_message_history_in_channel(channel)
    }

    fn can_delete_message(&self, message: &MessageState) -> bool {
        if Some(message.author_id) == self.current_user_id {
            return true;
        }
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        self.discord.can_manage_messages_in_channel(channel)
    }

    fn can_pin_messages_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.channel(message.channel_id) else {
            return true;
        };
        self.discord.can_pin_messages_in_channel(channel)
    }

    pub fn activate_message_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let actions = self.selected_message_action_items();
        let index = actions.iter().enumerate().position(|(index, action)| {
            action.enabled
                && message_action_shortcut(&actions, index)
                    .is_some_and(|candidate| candidate == shortcut)
        })?;
        self.select_message_action_row(index);
        self.activate_selected_message_action()
    }

    pub fn direct_copy_selected_message_content(&mut self) {
        let Some(content) = self
            .selected_message_state()
            .and_then(|message| message.content.as_ref())
        else {
            return;
        };
        self.copy_message_content_requested = Some(content.clone());
    }

    pub(in crate::tui) fn take_copy_message_content_request(&mut self) -> Option<String> {
        self.copy_message_content_requested.take()
    }

    pub fn direct_open_selected_message_reaction_picker(&mut self) {
        self.open_emoji_reaction_picker();
    }

    pub fn direct_reply_to_selected_message(&mut self) {
        self.start_reply_composer();
    }

    pub fn direct_edit_selected_message(&mut self) {
        self.start_edit_composer();
    }

    pub fn direct_open_selected_message_image_viewer(&mut self) {
        self.open_image_viewer_for_selected_message();
    }

    pub fn direct_show_selected_message_profile(&mut self) -> Option<AppCommand> {
        let message = self.selected_message_state()?;
        self.open_user_profile_popup(message.author_id, message.guild_id)
    }

    pub fn direct_open_selected_message_pin_confirmation(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        self.open_selected_message_pin_confirmation(!message.pinned);
    }

    pub fn open_selected_message_delete_confirmation(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if !self.can_delete_message(message) {
            return;
        }
        self.message_delete_confirmation = Some(popups::MessageDeleteConfirmationState {
            channel_id: message.channel_id,
            message_id: message.id,
            author: message.author.clone(),
            content: message.content.clone(),
        });
    }

    pub fn is_message_delete_confirmation_open(&self) -> bool {
        self.message_delete_confirmation.is_some()
    }

    pub fn close_message_delete_confirmation(&mut self) {
        self.message_delete_confirmation = None;
    }

    pub fn confirm_message_delete(&mut self) -> Option<AppCommand> {
        let confirmation = self.message_delete_confirmation.take()?;
        Some(AppCommand::DeleteMessage {
            channel_id: confirmation.channel_id,
            message_id: confirmation.message_id,
        })
    }

    pub fn message_delete_confirmation_lines(&self) -> Option<(String, Option<String>)> {
        let confirmation = self.message_delete_confirmation.as_ref()?;
        Some((confirmation.author.clone(), confirmation.content.clone()))
    }

    pub fn open_selected_message_pin_confirmation(&mut self, pinned: bool) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if !self.can_pin_messages_for_message(message) {
            return;
        }
        self.message_pin_confirmation = Some(popups::MessagePinConfirmationState {
            channel_id: message.channel_id,
            message_id: message.id,
            pinned,
            author: message.author.clone(),
            content: message.content.clone(),
        });
    }

    pub fn is_message_pin_confirmation_open(&self) -> bool {
        self.message_pin_confirmation.is_some()
    }

    pub fn close_message_pin_confirmation(&mut self) {
        self.message_pin_confirmation = None;
    }

    pub fn confirm_message_pin(&mut self) -> Option<AppCommand> {
        let confirmation = self.message_pin_confirmation.take()?;
        Some(AppCommand::SetMessagePinned {
            channel_id: confirmation.channel_id,
            message_id: confirmation.message_id,
            pinned: confirmation.pinned,
        })
    }

    pub fn message_pin_confirmation_lines(&self) -> Option<(bool, String, Option<String>)> {
        let confirmation = self.message_pin_confirmation.as_ref()?;
        Some((
            confirmation.pinned,
            confirmation.author.clone(),
            confirmation.content.clone(),
        ))
    }
}

fn action_reaction_label(emoji: &ReactionEmoji, show_custom_emoji: bool) -> String {
    match emoji {
        ReactionEmoji::Custom { id, .. } if !show_custom_emoji => id.get().to_string(),
        _ => emoji.status_label(),
    }
}
