use crate::discord::AppCommand;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};
use crate::tui::keybindings::KeyChord;

use super::super::model::{
    FocusPane, GuildActionItem, GuildActionKind, GuildPaneEntry, MUTE_ACTION_DURATIONS,
};
use super::super::{DashboardState, MuteActionDurationItem};
use super::{
    ActiveModalPopupKind, GuildLeaderActionState, GuildLeaveConfirmationState, ModalPopup,
};
#[cfg(test)]
use super::{LeaderActionState, LeaderMode, LeaderPopupState};

impl DashboardState {
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn open_selected_guild_actions(&mut self) {
        if let Some(action) = self.selected_guild_action_context() {
            self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
                mode: LeaderMode::Actions,
                keymap_prefix: Vec::new(),
                action: Some(LeaderActionState::Guild(action)),
            }));
        }
    }

    pub(super) fn selected_guild_action_context(&self) -> Option<GuildLeaderActionState> {
        if self.navigation.focus != FocusPane::Guilds {
            return None;
        }
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(
                GuildPaneEntry::DirectMessages
                | GuildPaneEntry::Guild { .. }
                | GuildPaneEntry::FolderHeader { .. },
            ) => Some(GuildLeaderActionState::Actions {
                selection: Default::default(),
            }),
            None => None,
        }
    }

    pub fn close_guild_leader_action(&mut self) {
        if self.is_guild_leader_action_active() {
            self.popups.clear_modal();
        }
    }

    pub fn back_guild_leader_action(&mut self) -> bool {
        if matches!(
            self.popups.guild_leader_action(),
            Some(GuildLeaderActionState::MuteDuration { .. })
        ) {
            if let Some(action) = self.popups.guild_leader_action_mut() {
                *action = GuildLeaderActionState::Actions {
                    selection: Default::default(),
                };
            }
            true
        } else {
            false
        }
    }

    pub fn selected_guild_action_items(&self) -> Vec<GuildActionItem> {
        if self.popups.guild_leader_action().is_none() {
            return Vec::new();
        }
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::Guild { state, .. }) => vec![
                GuildActionItem::new(
                    GuildActionKind::MarkAsRead,
                    "Mark server as read",
                    self.guild_ack_targets(state.id).next().is_some(),
                ),
                GuildActionItem::new(
                    GuildActionKind::ToggleMute,
                    if self.discord.cache.guild_notification_muted(state.id) {
                        "Unmute server"
                    } else {
                        "Mute server"
                    },
                    true,
                ),
                GuildActionItem::new(GuildActionKind::LeaveServer, "Leave server", true),
            ],
            Some(GuildPaneEntry::DirectMessages) => vec![GuildActionItem::new(
                GuildActionKind::NoActionsYet,
                "No server actions yet",
                false,
            )],
            Some(GuildPaneEntry::FolderHeader { folder, .. }) => vec![GuildActionItem::new(
                GuildActionKind::FolderSettings,
                "Folder settings",
                folder.id.is_some(),
            )],
            None => Vec::new(),
        }
    }

    pub fn selected_guild_mute_duration_items(&self) -> &'static [MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn select_guild_action_row(&mut self, row: usize) -> bool {
        let len = match self.popups.guild_leader_action() {
            Some(GuildLeaderActionState::Actions { .. }) => {
                self.selected_guild_action_items().len()
            }
            Some(GuildLeaderActionState::MuteDuration { .. }) => {
                self.selected_guild_mute_duration_items().len()
            }
            None => return false,
        };
        if row >= len {
            return false;
        }
        if let Some(action) = self.popups.guild_leader_action_mut() {
            match action {
                GuildLeaderActionState::Actions { selection }
                | GuildLeaderActionState::MuteDuration { selection } => selection.select(row),
            }
            return true;
        }
        false
    }

    pub fn activate_selected_guild_action(&mut self) -> Option<AppCommand> {
        let action = self.popups.guild_leader_action().cloned()?;
        match action {
            GuildLeaderActionState::Actions { selection } => {
                let items = self.selected_guild_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?;
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    GuildActionKind::MarkAsRead => self.mark_selected_guild_as_read(),
                    GuildActionKind::ToggleMute => {
                        let guild_id = self.selected_guild_cursor_id()?;
                        if self.discord.cache.guild_notification_muted(guild_id) {
                            self.close_guild_leader_action();
                            self.toggle_selected_guild_mute(None)
                        } else {
                            if let Some(action) = self.popups.guild_leader_action_mut() {
                                *action = GuildLeaderActionState::MuteDuration {
                                    selection: Default::default(),
                                };
                            }
                            None
                        }
                    }
                    GuildActionKind::LeaveServer => {
                        self.close_guild_leader_action();
                        self.open_current_guild_leave_confirmation();
                        None
                    }
                    GuildActionKind::FolderSettings => {
                        self.close_guild_leader_action();
                        self.open_selected_folder_settings();
                        None
                    }
                    GuildActionKind::NoActionsYet => None,
                }
            }
            GuildLeaderActionState::MuteDuration { selection } => {
                let item = self.selected_guild_mute_duration_items().get(
                    selection.selected_for_len(self.selected_guild_mute_duration_items().len()),
                )?;
                self.close_guild_leader_action();
                self.toggle_selected_guild_mute(Some(item.duration))
            }
        }
    }

    pub fn activate_guild_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        match self.popups.guild_leader_action()? {
            GuildLeaderActionState::Actions { .. } => {
                let actions = self.selected_guild_action_items();
                let index = self.options.key_bindings().matching_action_shortcut_index(
                    &actions,
                    shortcut,
                    |key_bindings, actions, index| {
                        key_bindings.guild_action_shortcuts(actions, index)
                    },
                    |action| action.enabled,
                )?;
                self.select_guild_action_row(index);
                self.activate_selected_guild_action()
            }
            GuildLeaderActionState::MuteDuration { .. } => {
                let index = self
                    .options
                    .key_bindings()
                    .matching_indexed_shortcut_index(
                        shortcut,
                        self.selected_guild_mute_duration_items().len(),
                    )?;
                self.select_guild_action_row(index);
                self.activate_selected_guild_action()
            }
        }
    }

    pub fn open_current_guild_leave_confirmation(&mut self) {
        let guild_id = if self.navigation.focus == FocusPane::Guilds {
            self.selected_guild_cursor_id()
        } else {
            self.selected_guild_id()
        };
        let Some(guild_id) = guild_id else {
            return;
        };
        let name = self
            .discord
            .guild(guild_id)
            .map(|guild| guild.name.clone())
            .unwrap_or_else(|| format!("server-{}", guild_id.get()));
        self.popups.modal = Some(ModalPopup::GuildLeaveConfirmation(
            GuildLeaveConfirmationState { guild_id, name },
        ));
    }

    pub fn close_guild_leave_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::GuildLeaveConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_guild_leave(&mut self) -> Option<AppCommand> {
        let confirmation = self.popups.take_guild_leave_confirmation()?;
        Some(AppCommand::LeaveGuild {
            guild_id: confirmation.guild_id,
            label: confirmation.name,
        })
    }

    pub fn guild_leave_confirmation_name(&self) -> Option<String> {
        self.popups
            .guild_leave_confirmation()
            .map(|confirmation| confirmation.name.clone())
    }

    fn mark_selected_guild_as_read(&mut self) -> Option<AppCommand> {
        let guild_id = match self.guild_pane_entries().get(self.selected_guild())? {
            GuildPaneEntry::Guild { state, .. } => state.id,
            GuildPaneEntry::DirectMessages | GuildPaneEntry::FolderHeader { .. } => return None,
        };
        let targets: Vec<_> = self.guild_ack_targets(guild_id).collect();
        if targets.is_empty() {
            return None;
        }

        for (channel_id, _) in targets.iter().copied() {
            if self.navigation.channels.active_channel_id == Some(channel_id) {
                self.messages.unread_divider_last_acked_id = None;
                self.messages.pending_unread_anchor_scroll = false;
                self.clear_new_messages_marker();
            }
        }
        self.close_guild_leader_action();
        Some(AppCommand::AckChannels { targets })
    }

    fn guild_ack_targets(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> impl Iterator<Item = (Id<ChannelMarker>, Id<MessageMarker>)> + '_ {
        self.discord
            .cache
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .filter_map(|channel| {
                self.discord
                    .cache
                    .channel_ack_target(channel.id)
                    .map(|message_id| (channel.id, message_id))
            })
    }
}
