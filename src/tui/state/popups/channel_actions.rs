use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{
    ActionDecision, AppCommand, ChannelState, DiscordAction, MemberModeration, StreamCaptureTarget,
    VoiceScope,
};
use crate::tui::keybindings::{KeyChord, SelectionAction};

use super::super::model::{
    ActionAvailability, ChannelActionItem, ChannelActionKind, ChannelPaneEntry, FocusPane,
    MUTE_ACTION_DURATIONS,
};
use super::super::presentation::sort_channels;
use super::super::{DashboardState, MuteActionDurationItem};
use super::{ActiveModalPopupKind, ConfirmationButton, GroupLeaveConfirmationState, ModalPopup};
use super::{ChannelActionMenuState, SelectablePopupState, SelectablePopupTarget};

impl DashboardState {
    pub(in crate::tui) fn open_selected_channel_actions(&mut self) {
        if let Some(menu) = self.selected_channel_action_context() {
            self.popups.set_modal(ModalPopup::ChannelActionMenu(menu));
        }
    }

    pub(super) fn selected_channel_action_context(&self) -> Option<ChannelActionMenuState> {
        if self.navigation.focus != FocusPane::Channels {
            return None;
        }
        match self.selected_channel_pane_entry()? {
            ChannelPaneEntry::VoiceParticipant {
                channel_id,
                participant,
                ..
            } => Some(ChannelActionMenuState::ParticipantActions {
                channel_id,
                user_id: participant.user_id,
                display_name: participant.display_name,
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
            Some(
                ChannelActionMenuState::MuteDuration { channel_id, .. }
                | ChannelActionMenuState::StreamTargets { channel_id, .. },
            ) => {
                let channel_id = *channel_id;
                if let Some(action) = self.popups.channel_action_menu_mut() {
                    *action = ChannelActionMenuState::Actions {
                        channel_id,
                        selection: Default::default(),
                    };
                }
                true
            }
            Some(ChannelActionMenuState::MoveTargets {
                channel_id,
                user_id,
                display_name,
                ..
            }) => {
                let menu = ChannelActionMenuState::ParticipantActions {
                    channel_id: *channel_id,
                    user_id: *user_id,
                    display_name: display_name.clone(),
                    selection: Default::default(),
                };
                if let Some(action) = self.popups.channel_action_menu_mut() {
                    *action = menu;
                }
                true
            }
            _ => false,
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::Actions { channel_id, .. }) => *channel_id,
            Some(ChannelActionMenuState::ParticipantActions {
                channel_id,
                user_id,
                ..
            }) => {
                let watch_disabled_reason =
                    if let Some(channel) = self.discord.cache.channel(*channel_id) {
                        let scope = channel.voice_scope();
                        let joined_here = self
                            .discord
                            .cache
                            .current_user_voice_connection()
                            .is_some_and(|voice| {
                                voice.scope == scope && voice.channel_id == *channel_id
                            });
                        if !joined_here {
                            Some("join this voice channel first".to_owned())
                        } else if self.discord.cache.current_user_id() == Some(*user_id) {
                            Some("cannot watch your own stream".to_owned())
                        } else {
                            let participants = match scope {
                                VoiceScope::Guild(guild_id) => self
                                    .discord
                                    .cache
                                    .voice_participants_for_channel(guild_id, *channel_id),
                                VoiceScope::Private(_) => self
                                    .discord
                                    .cache
                                    .voice_participants_for_private_channel(*channel_id),
                            };
                            participants
                                .iter()
                                .find(|participant| participant.user_id == *user_id)
                                .map(|participant| {
                                    (!participant.self_stream)
                                        .then(|| "participant is not streaming".to_owned())
                                })
                                .unwrap_or_else(|| Some("participant left the channel".to_owned()))
                        }
                    } else {
                        Some("voice channel unavailable".to_owned())
                    };
                let mut items = vec![
                    ChannelActionItem::new(
                        ChannelActionKind::WatchStream,
                        "Watch stream",
                        watch_disabled_reason,
                    ),
                    ChannelActionItem::new(
                        ChannelActionKind::ParticipantAudioSettings,
                        "Audio settings",
                        ActionAvailability::Enabled,
                    ),
                ];
                if let Some(guild_id) = self
                    .discord
                    .cache
                    .channel(*channel_id)
                    .and_then(|channel| channel.guild_id)
                {
                    items.extend(self.participant_voice_moderation_items(guild_id, *user_id));
                }
                return items;
            }
            Some(
                ChannelActionMenuState::StreamTargets { .. }
                | ChannelActionMenuState::MoveTargets { .. },
            ) => return Vec::new(),
            _ => return Vec::new(),
        };
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return Vec::new();
        };
        // Threads live under text-like channels. Forums already show their posts
        // as the channel view, and categories and voice channels cannot host
        // threads, so the action is offered everywhere else. Gateway active
        // thread events fill the list without a REST search.
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
        let stream_disabled_reason = if !joined_here {
            Some("join this voice channel first".to_owned())
        } else if channel.guild_id.is_none() {
            None
        } else {
            self.discord_action_block_reason_in_channel(channel_id, DiscordAction::StreamVoice)
        };
        let stream_label =
            self.stream_broadcast_action_label_for(channel.voice_scope(), channel_id);
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

        let mut items = vec![
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
                ChannelActionKind::ToggleStream,
                stream_label,
                stream_disabled_reason,
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
        ];
        // Closing a group DM would leave the group, so only offer this for 1:1 DMs.
        if channel.is_dm() {
            items.push(ChannelActionItem::new(
                ChannelActionKind::CloseDm,
                "Close DM",
                ActionAvailability::Enabled,
            ));
        } else if channel.is_group_dm() {
            items.push(ChannelActionItem::new(
                ChannelActionKind::LeaveGroup,
                "Leave group",
                ActionAvailability::Enabled,
            ));
        }
        items.push(ChannelActionItem::new(
            ChannelActionKind::ToggleMute,
            mute_label,
            ActionAvailability::Enabled,
        ));
        items
    }

    fn participant_voice_moderation_items(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Vec<ChannelActionItem> {
        let Some(channel_id) = self
            .discord
            .cache
            .user_voice_channel_in_guild(guild_id, user_id)
        else {
            return Vec::new();
        };
        let Some(participant) = self
            .discord
            .cache
            .voice_participants_for_channel(guild_id, channel_id)
            .into_iter()
            .find(|participant| participant.user_id == user_id)
        else {
            return Vec::new();
        };
        [
            (
                ChannelActionKind::ServerMute,
                DiscordAction::MuteMember,
                if participant.mute {
                    "Server unmute"
                } else {
                    "Server mute"
                },
            ),
            (
                ChannelActionKind::ServerDeafen,
                DiscordAction::DeafenMember,
                if participant.deaf {
                    "Server undeafen"
                } else {
                    "Server deafen"
                },
            ),
            (
                ChannelActionKind::MoveMember,
                DiscordAction::MoveMember,
                "Move to...",
            ),
            (
                ChannelActionKind::DisconnectMember,
                DiscordAction::MoveMember,
                "Disconnect",
            ),
        ]
        .into_iter()
        .filter(|(kind, _, _)| {
            *kind != ChannelActionKind::MoveMember
                || self
                    .member_move_target_channels(guild_id, channel_id)
                    .next()
                    .is_some()
        })
        .filter(|(_, action, _)| {
            self.discord
                .cache
                .member_moderation_decision(guild_id, user_id, *action)
                == ActionDecision::Allowed
        })
        .map(|(kind, _, label)| ChannelActionItem::new(kind, label, ActionAvailability::Enabled))
        .collect()
    }

    pub(in crate::tui) fn selected_member_move_targets(&self) -> &[(Id<ChannelMarker>, String)] {
        match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::MoveTargets { targets, .. }) => targets,
            _ => &[],
        }
    }

    pub(in crate::tui) fn is_channel_action_move_target_phase(&self) -> bool {
        matches!(
            self.popups.channel_action_menu(),
            Some(ChannelActionMenuState::MoveTargets { .. })
        )
    }

    fn member_move_target_channels(
        &self,
        guild_id: Id<GuildMarker>,
        current_channel_id: Id<ChannelMarker>,
    ) -> impl Iterator<Item = &ChannelState> {
        self.discord
            .cache
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(move |channel| channel.is_voice() && channel.id != current_channel_id)
            .filter(|channel| {
                self.discord.cache.member_move_destination_decision(channel)
                    == ActionDecision::Allowed
            })
    }

    fn member_move_targets(
        &self,
        guild_id: Id<GuildMarker>,
        current_channel_id: Id<ChannelMarker>,
    ) -> Vec<(Id<ChannelMarker>, String)> {
        let mut channels: Vec<_> = self
            .member_move_target_channels(guild_id, current_channel_id)
            .collect();
        sort_channels(&mut channels);
        channels
            .into_iter()
            .map(|channel| (channel.id, channel.name.clone()))
            .collect()
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
            ChannelActionMenuState::StreamTargets {
                targets, selection, ..
            } => Some(selection.selected_for_len(targets.len())),
            ChannelActionMenuState::MoveTargets {
                targets, selection, ..
            } => Some(selection.selected_for_len(targets.len())),
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
            Some(ChannelActionMenuState::StreamTargets { targets, .. }) => targets.len(),
            Some(ChannelActionMenuState::MoveTargets { targets, .. }) => targets.len(),
            None => 0,
        }
    }

    pub(super) fn channel_action_selection_mut(&mut self) -> Option<&mut SelectablePopupState> {
        match self.popups.channel_action_menu_mut()? {
            ChannelActionMenuState::Actions { selection, .. }
            | ChannelActionMenuState::ParticipantActions { selection, .. }
            | ChannelActionMenuState::MuteDuration { selection, .. }
            | ChannelActionMenuState::StreamTargets { selection, .. }
            | ChannelActionMenuState::MoveTargets { selection, .. } => Some(selection),
        }
    }

    pub(in crate::tui) fn channel_action_menu_title(&self) -> &'static str {
        match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::ParticipantActions { .. }) => "Participant actions",
            Some(ChannelActionMenuState::StreamTargets { .. }) => "Share screen",
            Some(ChannelActionMenuState::MoveTargets { .. }) => "Move to",
            Some(
                ChannelActionMenuState::Actions { .. }
                | ChannelActionMenuState::MuteDuration { .. },
            )
            | None => "Channel actions",
        }
    }

    pub fn move_channel_action_down(&mut self) {
        self.move_selectable_popup(SelectablePopupTarget::ChannelActions, SelectionAction::Next);
    }

    pub fn move_channel_action_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::ChannelActions,
            SelectionAction::Previous,
        );
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
                                input_source: self.options.voice_options.input_source.clone(),
                                output_source: self.options.voice_options.output_source.clone(),
                                allow_microphone_transmit: self
                                    .options
                                    .voice_options
                                    .allow_microphone_transmit
                                    && (channel.guild_id.is_none()
                                        || self
                                            .discord
                                            .cache
                                            .can_transmit_microphone_in_voice_channel(channel)),
                                noise_suppression: self.options.voice_options.noise_suppression,
                                microphone_buffer_ms: self
                                    .options
                                    .voice_options
                                    .microphone_buffer_ms,
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
                    ChannelActionKind::ToggleStream => {
                        self.close_channel_action_menu();
                        self.toggle_current_voice_stream_command()
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
                    ChannelActionKind::CloseDm => {
                        self.close_channel_action_menu();
                        Some(AppCommand::CloseDm { channel_id })
                    }
                    ChannelActionKind::LeaveGroup => {
                        self.open_group_leave_confirmation(channel_id);
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
                    ChannelActionKind::WatchStream
                    | ChannelActionKind::ParticipantAudioSettings
                    | ChannelActionKind::ServerMute
                    | ChannelActionKind::ServerDeafen
                    | ChannelActionKind::MoveMember
                    | ChannelActionKind::DisconnectMember => None,
                }
            }
            ChannelActionMenuState::ParticipantActions {
                channel_id,
                user_id,
                display_name,
                selection,
            } => {
                let items = self.selected_channel_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?;
                if !item.is_enabled() {
                    return None;
                }
                let guild_id = self
                    .discord
                    .cache
                    .channel(channel_id)
                    .and_then(|channel| channel.guild_id);
                let moderate = |moderation| {
                    guild_id.map(|guild_id| AppCommand::ModerateMember {
                        guild_id,
                        user_id,
                        moderation,
                    })
                };
                match item.kind {
                    ChannelActionKind::ServerMute | ChannelActionKind::ServerDeafen => {
                        let guild_id = guild_id?;
                        let voice_channel_id = self
                            .discord
                            .cache
                            .user_voice_channel_in_guild(guild_id, user_id)?;
                        let participant = self
                            .discord
                            .cache
                            .voice_participants_for_channel(guild_id, voice_channel_id)
                            .into_iter()
                            .find(|participant| participant.user_id == user_id)?;
                        self.close_channel_action_menu();
                        moderate(if item.kind == ChannelActionKind::ServerMute {
                            MemberModeration::SetMute(!participant.mute)
                        } else {
                            MemberModeration::SetDeaf(!participant.deaf)
                        })
                    }
                    ChannelActionKind::DisconnectMember => {
                        self.close_channel_action_menu();
                        moderate(MemberModeration::Disconnect)
                    }
                    ChannelActionKind::MoveMember => {
                        let guild_id = guild_id?;
                        let voice_channel_id = self
                            .discord
                            .cache
                            .user_voice_channel_in_guild(guild_id, user_id)?;
                        let targets = self.member_move_targets(guild_id, voice_channel_id);
                        if let Some(action) = self.popups.channel_action_menu_mut() {
                            *action = ChannelActionMenuState::MoveTargets {
                                channel_id,
                                user_id,
                                display_name,
                                targets,
                                selection: Default::default(),
                            };
                        }
                        None
                    }
                    ChannelActionKind::WatchStream => {
                        self.close_channel_action_menu();
                        self.discord.cache.channel(channel_id).map(|channel| {
                            AppCommand::WatchVoiceStream {
                                scope: channel.voice_scope(),
                                channel_id,
                                user_id,
                                display_name,
                            }
                        })
                    }
                    ChannelActionKind::ParticipantAudioSettings => {
                        self.open_voice_participant_audio_popup(user_id, display_name);
                        None
                    }
                    _ => None,
                }
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
            ChannelActionMenuState::StreamTargets {
                scope,
                channel_id,
                targets,
                selection,
            } => {
                let target = targets
                    .get(selection.selected_for_len(targets.len()))?
                    .clone();
                self.close_channel_action_menu();
                self.show_stream_broadcast_preparing_toast(scope, channel_id);
                Some(AppCommand::StartVoiceStream {
                    scope,
                    channel_id,
                    target,
                })
            }
            ChannelActionMenuState::MoveTargets {
                channel_id,
                user_id,
                targets,
                selection,
                ..
            } => {
                let (target_id, _) = targets.get(selection.selected_for_len(targets.len()))?;
                let guild_id = self.discord.cache.channel(channel_id)?.guild_id?;
                self.close_channel_action_menu();
                Some(AppCommand::ModerateMember {
                    guild_id,
                    user_id,
                    moderation: MemberModeration::Move(*target_id),
                })
            }
        }
    }

    fn open_group_leave_confirmation(&mut self, channel_id: Id<ChannelMarker>) {
        let name = self.channel_label(channel_id);
        // Rejoining needs another member, so Enter must not leave by default.
        self.popups.confirmation_button = ConfirmationButton::Cancel;
        self.popups.set_modal(ModalPopup::GroupLeaveConfirmation(
            GroupLeaveConfirmationState { channel_id, name },
        ));
    }

    pub fn close_group_leave_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::GroupLeaveConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_group_leave(&mut self) -> Option<AppCommand> {
        let confirmation = self.popups.take_group_leave_confirmation()?;
        Some(AppCommand::LeaveGroup {
            channel_id: confirmation.channel_id,
        })
    }

    pub fn group_leave_confirmation_name(&self) -> Option<String> {
        self.popups
            .group_leave_confirmation()
            .map(|confirmation| confirmation.name.clone())
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
            ChannelActionMenuState::StreamTargets { targets, .. } => {
                let index = self
                    .options
                    .key_bindings()
                    .matching_indexed_shortcut_index(shortcut, targets.len())?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelActionMenuState::MoveTargets { targets, .. } => {
                let index = self
                    .options
                    .key_bindings()
                    .matching_indexed_shortcut_index(shortcut, targets.len())?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
        }
    }

    pub(in crate::tui) fn selected_stream_capture_targets(&self) -> &[StreamCaptureTarget] {
        match self.popups.channel_action_menu() {
            Some(ChannelActionMenuState::StreamTargets { targets, .. }) => targets,
            _ => &[],
        }
    }

    pub(in crate::tui) fn is_channel_action_stream_target_phase(&self) -> bool {
        matches!(
            self.popups.channel_action_menu(),
            Some(ChannelActionMenuState::StreamTargets { .. })
        )
    }
}
