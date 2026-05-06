use crate::discord::AppCommand;

use super::scroll::{clamp_selected_index, move_index_down, move_index_up};
use super::{
    DashboardState, FocusPane, MessageActionItem, MessageActionKind, MessageActionMenuState,
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
        let is_own_regular_message =
            Some(message.author_id) == self.current_user_id && message.message_kind.is_regular();
        if is_own_regular_message {
            if message.content.is_some() {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::Edit,
                    label: "Edit message".to_owned(),
                    enabled: true,
                });
            }
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
        if capabilities.has_image {
            actions.push(MessageActionItem {
                kind: MessageActionKind::ViewImage,
                label: "View image".to_owned(),
                enabled: true,
            });
        }
        actions.push(MessageActionItem {
            kind: MessageActionKind::AddReaction,
            label: "Add reaction".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::SetPinned(!message.pinned),
            label: if message.pinned {
                "Unpin message".to_owned()
            } else {
                "Pin message".to_owned()
            },
            enabled: true,
        });
        if !message.reactions.is_empty() {
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
                    label: format!("Remove {} reaction", reaction.emoji.status_label()),
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
                let message = self.selected_message_state()?;
                if Some(message.author_id) != self.current_user_id {
                    self.close_message_action_menu();
                    return None;
                }
                let channel_id = message.channel_id;
                let message_id = message.id;
                self.close_message_action_menu();
                Some(AppCommand::DeleteMessage {
                    channel_id,
                    message_id,
                })
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
            MessageActionKind::DownloadImage => {
                self.close_message_action_menu();
                None
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
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                self.close_message_action_menu();
                Some(AppCommand::SetMessagePinned {
                    channel_id,
                    message_id,
                    pinned,
                })
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
}
