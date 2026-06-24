use crate::discord::AppCommand;
use crate::discord::ids::{Id, marker::ChannelMarker};
use crate::tui::keybindings::KeyChord;

use super::super::model::{
    ChannelActionItem, ChannelActionKind, ChannelPaneEntry, FocusPane, MUTE_ACTION_DURATIONS,
};
use super::super::{DashboardState, MuteActionDurationItem};
use super::ChannelLeaderActionState;
#[cfg(test)]
use super::{LeaderActionState, LeaderMode, LeaderPopupState, ModalPopup};

impl DashboardState {
    #[cfg(test)]
    pub fn open_selected_channel_actions(&mut self) {
        if self.navigation.focus != FocusPane::Channels {
            return;
        }
        let Some(channel_id) = self.selected_channel_action_target_id() else {
            return;
        };
        self.open_channel_actions(channel_id);
    }

    #[cfg(test)]
    fn open_channel_actions(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        if channel.is_thread() {
            return;
        }
        self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
            mode: LeaderMode::Actions,
            keymap_prefix: Vec::new(),
            action: Some(LeaderActionState::Channel(
                ChannelLeaderActionState::Actions {
                    channel_id,
                    selection: Default::default(),
                },
            )),
        }));
    }

    pub(super) fn selected_channel_action_context(&self) -> Option<ChannelLeaderActionState> {
        if self.navigation.focus != FocusPane::Channels {
            return None;
        }
        let channel_id = self.selected_channel_action_target_id()?;
        let channel = self.discord.cache.channel(channel_id)?;
        (!channel.is_thread()).then_some(ChannelLeaderActionState::Actions {
            channel_id,
            selection: Default::default(),
        })
    }

    pub fn close_channel_leader_action(&mut self) {
        if self.is_channel_leader_action_active() {
            self.popups.clear_modal();
        }
    }

    pub fn back_channel_leader_action(&mut self) -> bool {
        match self.popups.channel_leader_action() {
            Some(ChannelLeaderActionState::MuteDuration { channel_id, .. }) => {
                let channel_id = *channel_id;
                if let Some(action) = self.popups.channel_leader_action_mut() {
                    *action = ChannelLeaderActionState::Actions {
                        channel_id,
                        selection: Default::default(),
                    };
                }
                true
            }
            _ => false,
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.popups.channel_leader_action() {
            Some(ChannelLeaderActionState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return Vec::new();
        };
        // Threads live under text-like channels. Forums already show their posts
        // as the channel view, and categories and voice channels cannot host
        // threads, so the action is offered everywhere else. The list itself is
        // filled by the `/threads/search` fetch once the view opens, so this no
        // longer depends on threads already sitting in the gateway cache.
        let can_show_threads = !channel.is_category() && !channel.is_forum() && !channel.is_voice();
        let active_channel_has_unread_snapshot = self.navigation.channels.active_channel_id
            == Some(channel_id)
            && (self.messages.unread_divider_last_acked_id.is_some()
                || self.messages.pending_unread_anchor_scroll);
        let mark_as_read_enabled = active_channel_has_unread_snapshot
            || self.discord.cache.channel_ack_target(channel_id).is_some()
            || (channel.is_forum()
                && !self
                    .discord
                    .cache
                    .forum_child_ack_targets(channel_id)
                    .is_empty());
        let joined_here = channel.is_voice()
            && channel.guild_id.is_some_and(|guild_id| {
                self.runtime.voice_connection.is_some_and(|voice| {
                    voice.guild_id == guild_id && voice.channel_id == Some(channel_id)
                })
            });
        let can_join_voice = channel.is_voice()
            && !joined_here
            && self.discord.cache.can_connect_voice_channel(channel);
        let mute_label = match (
            self.discord.cache.channel_notification_muted(channel_id),
            channel.is_category(),
        ) {
            (true, true) => "Unmute category",
            (true, false) => "Unmute channel",
            (false, true) => "Mute category",
            (false, false) => "Mute channel",
        };

        vec![
            ChannelActionItem::new(ChannelActionKind::JoinVoice, "Join voice", can_join_voice),
            ChannelActionItem::new(ChannelActionKind::LeaveVoice, "Leave voice", joined_here),
            ChannelActionItem::new(
                ChannelActionKind::LoadPinnedMessages,
                "Show pinned messages",
                !channel.is_category() && !channel.is_forum(),
            ),
            ChannelActionItem::new(
                ChannelActionKind::ShowThreads,
                "Show threads",
                can_show_threads,
            ),
            ChannelActionItem::new(
                ChannelActionKind::MarkAsRead,
                "Mark as read",
                mark_as_read_enabled,
            ),
            ChannelActionItem::new(ChannelActionKind::ToggleMute, mute_label, true),
        ]
    }

    pub fn selected_channel_mute_duration_items(&self) -> &'static [MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn select_channel_action_row(&mut self, row: usize) -> bool {
        let len = match self.popups.channel_leader_action() {
            Some(ChannelLeaderActionState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelLeaderActionState::MuteDuration { .. }) => {
                self.selected_channel_mute_duration_items().len()
            }
            None => return false,
        };
        if row >= len {
            return false;
        }
        if let Some(action) = self.popups.channel_leader_action_mut() {
            let selection = match action {
                ChannelLeaderActionState::Actions { selection, .. }
                | ChannelLeaderActionState::MuteDuration { selection, .. } => selection,
            };
            selection.select(row);
            return true;
        }
        false
    }

    /// Make `channel_id` the active channel so the message pane, which always
    /// follows the active channel, can render a pinned-message or thread-list
    /// view for it. Without this a never-opened channel keeps showing the
    /// previously active channel, so the view silently fails to switch.
    /// Skipped when the channel is already open so its viewport is not reset.
    fn open_channel_for_pane_view(&mut self, channel_id: Id<ChannelMarker>) {
        if self.selected_channel_id() != Some(channel_id) {
            self.activate_channel(channel_id);
        }
    }

    pub fn activate_selected_channel_action(&mut self) -> Option<AppCommand> {
        let action = self.popups.channel_leader_action().cloned()?;
        match action {
            ChannelLeaderActionState::Actions {
                channel_id,
                selection,
            } => {
                let items = self.selected_channel_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ChannelActionKind::JoinVoice => {
                        self.close_channel_leader_action();
                        self.discord
                            .cache
                            .channel(channel_id)
                            .and_then(|channel| channel.guild_id)
                            .map(|guild_id| AppCommand::JoinVoiceChannel {
                                guild_id,
                                channel_id,
                                self_mute: self.options.voice_options.self_mute,
                                self_deaf: self.options.voice_options.self_deaf,
                                allow_microphone_transmit: self
                                    .options
                                    .voice_options
                                    .allow_microphone_transmit,
                                microphone_sensitivity: self
                                    .options
                                    .voice_options
                                    .microphone_sensitivity,
                                microphone_volume: self.options.voice_options.microphone_volume,
                                voice_output_volume: self.options.voice_options.voice_output_volume,
                            })
                    }
                    ChannelActionKind::LeaveVoice => {
                        self.close_channel_leader_action();
                        self.discord
                            .cache
                            .channel(channel_id)
                            .and_then(|channel| channel.guild_id)
                            .map(|guild_id| AppCommand::LeaveVoiceChannel {
                                guild_id,
                                self_mute: self.options.voice_options.self_mute,
                                self_deaf: self.options.voice_options.self_deaf,
                            })
                    }
                    ChannelActionKind::LoadPinnedMessages => {
                        self.close_channel_leader_action();
                        self.open_channel_for_pane_view(channel_id);
                        self.enter_pinned_message_view(channel_id);
                        // Move focus into the message pane like the thread list
                        // does, so the pinned list is immediately navigable.
                        self.focus_pane(FocusPane::Messages);
                        None
                    }
                    ChannelActionKind::ShowThreads => {
                        // Open the channel's threads as a card list in the message pane.
                        self.close_channel_leader_action();
                        self.open_channel_for_pane_view(channel_id);
                        self.enter_channel_thread_list_view(channel_id);
                        self.focus_pane(FocusPane::Messages);
                        None
                    }
                    ChannelActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_channel_leader_action();
                        None
                    }
                    ChannelActionKind::ToggleMute => {
                        if self.discord.cache.channel_notification_muted(channel_id) {
                            self.close_channel_leader_action();
                            self.toggle_channel_mute(channel_id, None)
                        } else {
                            if let Some(action) = self.popups.channel_leader_action_mut() {
                                *action = ChannelLeaderActionState::MuteDuration {
                                    channel_id,
                                    selection: Default::default(),
                                };
                            }
                            None
                        }
                    }
                }
            }
            ChannelLeaderActionState::MuteDuration {
                channel_id,
                selection,
            } => {
                let item = self.selected_channel_mute_duration_items().get(
                    selection.selected_for_len(self.selected_channel_mute_duration_items().len()),
                )?;
                self.close_channel_leader_action();
                self.toggle_channel_mute(channel_id, Some(item.duration))
            }
        }
    }

    pub fn activate_channel_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        match self.popups.channel_leader_action()? {
            ChannelLeaderActionState::Actions { .. } => {
                let actions = self.selected_channel_action_items();
                let index = self.options.key_bindings().matching_action_shortcut_index(
                    &actions,
                    shortcut,
                    |key_bindings, actions, index| {
                        key_bindings.channel_action_shortcuts(actions, index)
                    },
                    |action| action.enabled,
                )?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelLeaderActionState::MuteDuration { .. } => {
                let index = self
                    .options
                    .key_bindings()
                    .matching_indexed_shortcut_index(
                        shortcut,
                        self.selected_channel_mute_duration_items().len(),
                    )?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
        }
    }

    fn selected_channel_action_target_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(
                ChannelPaneEntry::Channel { state, .. } | ChannelPaneEntry::Thread { state, .. },
            ) => Some(state.id),
            Some(ChannelPaneEntry::VoiceParticipant { .. }) => None,
            None => None,
        }
    }
}
