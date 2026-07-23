use crate::discord::ids::{Id, marker::ChannelMarker};
use crate::discord::{AppCommand, DiscordAction};
use crate::tui::keybindings::KeyChord;

use super::super::model::{
    ActionAvailability, ChannelActionItem, ChannelActionKind, ChannelPaneEntry, FocusPane,
    MUTE_ACTION_DURATIONS,
};
use super::super::{DashboardState, MuteActionDurationItem};
use super::ModalPopup;
use super::{ChannelActionMenuState, SelectablePopupState};

impl DashboardState {
    pub(in crate::tui) fn open_selected_channel_actions(&mut self) {
        if let Some(menu) = self.selected_channel_action_context() {
            self.popups.modal = Some(ModalPopup::ChannelActionMenu(menu));
        }
    }

    pub(super) fn selected_channel_action_context(&self) -> Option<ChannelActionMenuState> {
        if self.navigation.focus != FocusPane::Channels {
            return None;
        }
        match self.channel_pane_entries().get(self.selected_channel())? {
            ChannelPaneEntry::VoiceParticipant {
                channel_id,
                participant,
                ..
            } => Some(ChannelActionMenuState::ParticipantActions {
                channel_id: *channel_id,
                user_id: participant.user_id,
                display_name: participant.display_name.clone(),
                selection: Default::default(),
            }),
            ChannelPaneEntry::CategoryHeader { state, .. }
            | ChannelPaneEntry::Channel { state, .. } => Some(ChannelActionMenuState::Actions {
                channel_id: state.id,
                selection: Default::default(),
            }),
            ChannelPaneEntry::Thread { .. } => None,
        }
    }

    pub fn close_channel_action_menu(&mut self) {
        if self.is_channel_action_menu_active() {
            self.popups.clear_modal();
        }
    }

    pub fn back_channel_action_menu(&mut self) -> bool {
        match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::MuteDuration { channel_id, .. }) => {
                let channel_id = *channel_id;
                if let Some(action) = self.popups.channel_action_menu_mut() {
                    *action = ChannelActionMenuState::Actions {
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
        let channel_id = match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::Actions { channel_id, .. }) => *channel_id,
            Some(ChannelActionMenuState::ParticipantActions { .. }) => {
                return vec![ChannelActionItem::new(
                    ChannelActionKind::ParticipantAudioSettings,
                    "Audio settings",
                    ActionAvailability::Enabled,
                )];
            }
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
        let can_read_history =
            self.discord_action_allowed_in_channel(channel_id, DiscordAction::ReadMessageHistory);
        let can_show_threads = !channel.is_category()
            && !channel.is_forum()
            && !channel.is_voice()
            && can_read_history;
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
        let joined_here = channel.supports_voice_call()
            && self.runtime.voice_connection.is_some_and(|voice| {
                voice.scope == channel.voice_scope() && voice.channel_id == Some(channel_id)
            });
        // Guild voice needs the connect permission. DM and group-DM calls have
        // no guild permission model, so they bypass this policy.
        let join_voice_disabled_reason = if !channel.supports_voice_call() {
            Some("not a voice channel".to_owned())
        } else if joined_here {
            Some("already joined".to_owned())
        } else if channel.guild_id.is_none() {
            None
        } else {
            self.discord_action_block_reason_in_channel(channel_id, DiscordAction::JoinVoiceChannel)
        };
        let can_transmit_microphone = channel.guild_id.is_none()
            || self
                .discord
                .cache
                .can_transmit_microphone_in_voice_channel(channel);
        let join_voice_label = if join_voice_disabled_reason.is_none() && !can_transmit_microphone {
            "Join voice (listen only)"
        } else {
            "Join voice"
        };
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
            ChannelActionItem::new(
                ChannelActionKind::JoinVoice,
                join_voice_label,
                join_voice_disabled_reason,
            ),
            ChannelActionItem::new(
                ChannelActionKind::LeaveVoice,
                "Leave voice",
                (!joined_here).then(|| "not connected here".to_owned()),
            ),
            ChannelActionItem::new(
                ChannelActionKind::ShowPinnedMessages,
                "Show pinned messages",
                if channel.is_category() {
                    Some("not supported for categories".to_owned())
                } else if channel.is_forum() {
                    Some("not supported for forums".to_owned())
                } else if !can_read_history {
                    Some("Read Message History required".to_owned())
                } else {
                    None
                },
            ),
            ChannelActionItem::new(
                ChannelActionKind::ShowThreads,
                "Show threads",
                if can_show_threads {
                    None
                } else if !can_read_history {
                    Some("Read Message History required".to_owned())
                } else {
                    Some("threads not supported here".to_owned())
                },
            ),
            ChannelActionItem::new(
                ChannelActionKind::MarkAsRead,
                "Mark as read",
                (!mark_as_read_enabled).then(|| "no unread messages".to_owned()),
            ),
            ChannelActionItem::new(
                ChannelActionKind::ToggleMute,
                mute_label,
                ActionAvailability::Enabled,
            ),
        ]
    }

    pub fn selected_channel_mute_duration_items(&self) -> &'static [MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn selected_channel_action_index(&self) -> Option<usize> {
        match self.popups.channel_action_menu()? {
            ChannelActionMenuState::Actions { selection, .. }
            | ChannelActionMenuState::ParticipantActions { selection, .. } => {
                Some(selection.selected_for_len(self.selected_channel_action_items().len()))
            }
            ChannelActionMenuState::MuteDuration { selection, .. } => {
                Some(selection.selected_for_len(self.selected_channel_mute_duration_items().len()))
            }
        }
    }

    pub(in crate::tui) fn channel_action_row_count(&self) -> usize {
        match self.popups.channel_action_menu() {
            Some(
                ChannelActionMenuState::Actions { .. }
                | ChannelActionMenuState::ParticipantActions { .. },
            ) => self.selected_channel_action_items().len(),
            Some(ChannelActionMenuState::MuteDuration { .. }) => {
                self.selected_channel_mute_duration_items().len()
            }
            None => 0,
        }
    }

    pub(super) fn channel_action_selection_mut(&mut self) -> Option<&mut SelectablePopupState> {
        match self.popups.channel_action_menu_mut()? {
            ChannelActionMenuState::Actions { selection, .. }
            | ChannelActionMenuState::ParticipantActions { selection, .. }
            | ChannelActionMenuState::MuteDuration { selection, .. } => Some(selection),
        }
    }

    pub(in crate::tui) fn channel_action_menu_title(&self) -> &'static str {
        match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::ParticipantActions { .. }) => "Participant actions",
            Some(
                ChannelActionMenuState::Actions { .. }
                | ChannelActionMenuState::MuteDuration { .. },
            )
            | None => "Channel actions",
        }
    }

    pub fn move_channel_action_down(&mut self) {
        let len = self.channel_action_row_count();
        if let Some(selection) = self.channel_action_selection_mut() {
            selection.move_down(len);
        }
    }

    pub fn move_channel_action_up(&mut self) {
        if let Some(selection) = self.channel_action_selection_mut() {
            selection.move_up();
        }
    }

    pub fn select_channel_action_row(&mut self, row: usize) -> bool {
        if row >= self.channel_action_row_count() {
            return false;
        }
        if let Some(selection) = self.channel_action_selection_mut() {
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
        let action = self.popups.channel_action_menu().cloned()?;
        match action {
            ChannelActionMenuState::Actions {
                channel_id,
                selection,
            } => {
                let items = self.selected_channel_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                if !item.is_enabled() {
                    return None;
                }
                match item.kind {
                    ChannelActionKind::JoinVoice => {
                        self.close_channel_action_menu();
                        self.discord.cache.channel(channel_id).map(|channel| {
                            AppCommand::JoinVoiceChannel {
                                scope: channel.voice_scope(),
                                channel_id,
                                self_mute: self.options.voice_options.self_mute,
                                self_deaf: self.options.voice_options.self_deaf,
                                allow_microphone_transmit: self
                                    .options
                                    .voice_options
                                    .allow_microphone_transmit
                                    && (channel.guild_id.is_none()
                                        || self
                                            .discord
                                            .cache
                                            .can_transmit_microphone_in_voice_channel(channel)),
                                microphone_sensitivity: self
                                    .options
                                    .voice_options
                                    .microphone_sensitivity,
                                microphone_volume: self.options.voice_options.microphone_volume,
                                voice_output_volume: self.options.voice_options.voice_output_volume,
                                participant_playback_settings: self
                                    .voice_participant_playback_settings_snapshot(),
                            }
                        })
                    }
                    ChannelActionKind::LeaveVoice => {
                        self.close_channel_action_menu();
                        self.discord.cache.channel(channel_id).map(|channel| {
                            AppCommand::LeaveVoiceChannel {
                                scope: channel.voice_scope(),
                                self_mute: self.options.voice_options.self_mute,
                                self_deaf: self.options.voice_options.self_deaf,
                            }
                        })
                    }
                    ChannelActionKind::ShowPinnedMessages => {
                        self.close_channel_action_menu();
                        self.open_channel_for_pane_view(channel_id);
                        self.enter_pinned_message_view(channel_id);
                        // Move focus into the message pane like the thread list
                        // does, so the pinned list is immediately navigable.
                        self.focus_pane(FocusPane::Messages);
                        None
                    }
                    ChannelActionKind::ShowThreads => {
                        // Open the channel's threads as a card list in the message pane.
                        self.close_channel_action_menu();
                        self.open_channel_for_pane_view(channel_id);
                        self.enter_channel_thread_list_view(channel_id);
                        self.focus_pane(FocusPane::Messages);
                        None
                    }
                    ChannelActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_channel_action_menu();
                        None
                    }
                    ChannelActionKind::ToggleMute => {
                        if self.discord.cache.channel_notification_muted(channel_id) {
                            self.close_channel_action_menu();
                            self.toggle_channel_mute(channel_id, None)
                        } else {
                            if let Some(action) = self.popups.channel_action_menu_mut() {
                                *action = ChannelActionMenuState::MuteDuration {
                                    channel_id,
                                    selection: Default::default(),
                                };
                            }
                            None
                        }
                    }
                    ChannelActionKind::ParticipantAudioSettings => None,
                }
            }
            ChannelActionMenuState::ParticipantActions {
                user_id,
                display_name,
                selection,
                ..
            } => {
                let items = self.selected_channel_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?;
                if !item.is_enabled() {
                    return None;
                }
                if item.kind == ChannelActionKind::ParticipantAudioSettings {
                    self.open_voice_participant_audio_popup(user_id, display_name);
                }
                None
            }
            ChannelActionMenuState::MuteDuration {
                channel_id,
                selection,
            } => {
                let item = self.selected_channel_mute_duration_items().get(
                    selection.selected_for_len(self.selected_channel_mute_duration_items().len()),
                )?;
                self.close_channel_action_menu();
                self.toggle_channel_mute(channel_id, Some(item.duration))
            }
        }
    }

    pub fn activate_channel_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        match self.popups.channel_action_menu()? {
            ChannelActionMenuState::Actions { .. }
            | ChannelActionMenuState::ParticipantActions { .. } => {
                let actions = self.selected_channel_action_items();
                let index = self.options.key_bindings().matching_action_shortcut_index(
                    &actions,
                    shortcut,
                    |key_bindings, actions, index| {
                        key_bindings.channel_action_shortcuts(actions, index)
                    },
                    |action| action.is_enabled(),
                )?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelActionMenuState::MuteDuration { .. } => {
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
}
