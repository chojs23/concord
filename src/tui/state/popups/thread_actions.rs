use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{AppCommand, MuteDuration};
use crate::tui::keybindings::KeyChord;

use super::super::model::{FocusPane, MUTE_ACTION_DURATIONS};
use super::super::{
    DashboardState, MuteActionDurationItem, ThreadActionItem, ThreadActionKind,
    ThreadNotificationItem,
};
use super::{
    ActiveModalPopupKind, ModalPopup, ThreadActionMenuState, ThreadDeleteConfirmationState,
};

impl DashboardState {
    /// Open the action menu for the focused thread (a regular thread or a forum
    /// post). Returns `false` (so the caller can fall back to other action
    /// contexts) when a thread is not the current focus.
    pub fn open_selected_thread_actions(&mut self) -> bool {
        let Some((guild_id, channel_id)) = self.focused_thread_action_target() else {
            return false;
        };
        self.popups.modal = Some(ModalPopup::ThreadActionMenu(
            ThreadActionMenuState::Actions {
                guild_id,
                channel_id,
                selection: Default::default(),
            },
        ));
        true
    }

    /// The parent guild and thread id of the focused thread, whether it is a
    /// forum post selected in the messages pane (the forum's post list) or any
    /// thread shown in the channel pane.
    fn focused_thread_action_target(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        match self.navigation.focus {
            FocusPane::Messages if self.message_pane_uses_forum_posts() => {
                let (guild_id, _) = self.selected_forum_channel()?;
                let item = self
                    .selected_forum_post_items()
                    .get(self.selected_forum_post())?
                    .clone();
                Some((guild_id, item.channel_id))
            }
            FocusPane::Channels => {
                let entries = self.channel_pane_entries();
                let channel = entries.get(self.selected_channel())?.channel_state()?;
                let guild_id = channel.guild_id?;
                // Any focused thread opens the action menu. A forum post is just a
                // thread whose parent happens to be a forum; the forum-specific
                // rows gate on that further down.
                channel.is_thread().then_some((guild_id, channel.id))
            }
            _ => None,
        }
    }

    pub fn is_thread_action_menu_active(&self) -> bool {
        self.popups.thread_action_menu().is_some()
    }

    pub fn is_thread_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.thread_action_menu(),
            Some(ThreadActionMenuState::MuteDuration { .. })
        )
    }

    pub fn is_thread_action_notification_phase(&self) -> bool {
        matches!(
            self.popups.thread_action_menu(),
            Some(ThreadActionMenuState::NotificationSettings { .. })
        )
    }

    /// The noun for the active action menu's target: "post" for a forum post,
    /// "thread" otherwise. Used by the renderer for the menu titles.
    pub fn thread_action_menu_noun(&self) -> &'static str {
        let channel_id = match self.popups.thread_action_menu() {
            Some(
                ThreadActionMenuState::Actions { channel_id, .. }
                | ThreadActionMenuState::MuteDuration { channel_id, .. }
                | ThreadActionMenuState::NotificationSettings { channel_id, .. },
            ) => *channel_id,
            None => return "thread",
        };
        let is_forum_post = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        if is_forum_post { "post" } else { "thread" }
    }

    pub fn close_thread_action_menu(&mut self) {
        if self.is_thread_action_menu_active() {
            self.popups.clear_modal();
        }
    }

    /// Step back from a submenu to the top-level actions. Returns `false` when
    /// already at the top level so the caller can close the menu instead.
    pub fn back_thread_action_menu(&mut self) -> bool {
        match self.popups.thread_action_menu() {
            Some(
                ThreadActionMenuState::MuteDuration {
                    guild_id,
                    channel_id,
                    ..
                }
                | ThreadActionMenuState::NotificationSettings {
                    guild_id,
                    channel_id,
                    ..
                },
            ) => {
                let (guild_id, channel_id) = (*guild_id, *channel_id);
                if let Some(menu) = self.popups.thread_action_menu_mut() {
                    *menu = ThreadActionMenuState::Actions {
                        guild_id,
                        channel_id,
                        selection: Default::default(),
                    };
                }
                true
            }
            _ => false,
        }
    }

    pub fn selected_thread_action_items(&self) -> Vec<ThreadActionItem> {
        let channel_id = match self.popups.thread_action_menu() {
            Some(ThreadActionMenuState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };

        // A forum post is a thread whose parent is a forum channel. Pin is the
        // only forum-only action, and the labels read "post" for forum posts and
        // "thread" for regular threads.
        let is_forum_post = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        let noun = if is_forum_post { "post" } else { "thread" };

        // A thread is "unread" when there is an ack target sitting ahead of the
        // last read message.
        let mark_as_read_enabled = self.discord.cache.channel_ack_target(channel_id).is_some();
        // Discord only offers muting once you follow the thread, so the mute row
        // is gated on membership (and unmute stays available while still followed).
        let followed = self.is_thread_followed(channel_id);
        let follow_label = format!("{} {noun}", if followed { "Unfollow" } else { "Follow" });
        let mute_label = format!(
            "{} {noun}",
            if self.discord.cache.channel_notification_muted(channel_id) {
                "Unmute"
            } else {
                "Mute"
            }
        );

        // Closing is allowed for the author or a moderator; lock and pin are
        // moderator-only.
        let can_moderate = self.can_moderate_thread(channel_id);
        let can_manage = self.can_manage_thread(channel_id);
        let close_label = format!(
            "{} {noun}",
            if self.is_thread_archived(channel_id) {
                "Reopen"
            } else {
                "Close"
            }
        );
        let lock_label = format!(
            "{} {noun}",
            if self.is_thread_locked(channel_id) {
                "Unlock"
            } else {
                "Lock"
            }
        );

        let mut items = vec![
            ThreadActionItem::new(
                ThreadActionKind::MarkAsRead,
                "Mark as read",
                mark_as_read_enabled,
            ),
            ThreadActionItem::new(ThreadActionKind::ToggleFollow, follow_label, true),
            ThreadActionItem::new(ThreadActionKind::Close, close_label, can_manage),
            ThreadActionItem::new(ThreadActionKind::Lock, lock_label, can_moderate),
            ThreadActionItem::new(ThreadActionKind::Edit, format!("Edit {noun}"), can_manage),
            ThreadActionItem::new(ThreadActionKind::CopyLink, "Copy link", true),
            ThreadActionItem::new(ThreadActionKind::ToggleMute, mute_label, followed),
            ThreadActionItem::new(
                ThreadActionKind::NotificationSettings,
                "Notification settings",
                followed,
            ),
        ];
        // Pinning only exists within a parent forum, so the row is forum-only.
        if is_forum_post {
            let pin_label = if self.is_thread_pinned(channel_id) {
                "Unpin post"
            } else {
                "Pin post"
            };
            items.push(ThreadActionItem::new(
                ThreadActionKind::Pin,
                pin_label,
                can_moderate,
            ));
        }
        // Deleting removes the whole thread (moderator-only); the author's
        // body-only "delete message" is a separate action we do not offer.
        items.push(ThreadActionItem::new(
            ThreadActionKind::Delete,
            format!("Delete {noun}"),
            can_moderate,
        ));
        items.push(ThreadActionItem::new(
            ThreadActionKind::CopyId,
            "Copy thread ID",
            true,
        ));
        items
    }

    /// Move the highlight to `row` within the top-level action list. Returns
    /// `false` when the menu sits in a submenu or the row is out of range.
    fn select_thread_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_thread_action_items().len() {
            return false;
        }
        if let Some(ThreadActionMenuState::Actions { selection, .. }) =
            self.popups.thread_action_menu_mut()
        {
            selection.select(row);
            return true;
        }
        false
    }

    /// Activate the action bound to `shortcut`, if an enabled row in the
    /// top-level list claims it. Mirrors the message action menu's shortcuts.
    pub fn activate_thread_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        let actions = self.selected_thread_action_items();
        let index = self.key_bindings().matching_action_shortcut_index(
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.thread_action_shortcuts(actions, index),
            |action| action.enabled,
        )?;
        self.select_thread_action_row(index);
        self.activate_selected_thread_action()
    }

    pub fn selected_thread_mute_duration_items(&self) -> &'static [MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    /// Build the three notification-level rows for the submenu, marking the
    /// current level with `[x]`. Unknown current level defaults to `4`
    /// (Only @mentions, Discord's default).
    pub fn selected_thread_notification_items(&self) -> Vec<ThreadNotificationItem> {
        let channel_id = match self.popups.thread_action_menu() {
            Some(ThreadActionMenuState::NotificationSettings { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let current_flags = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|ch| ch.current_user_thread_notification_flags)
            .unwrap_or(4);
        vec![
            ThreadNotificationItem::new("All messages", 2, current_flags),
            ThreadNotificationItem::new("Only @mentions", 4, current_flags),
            ThreadNotificationItem::new("Nothing", 8, current_flags),
        ]
    }

    pub fn selected_thread_action_index(&self) -> Option<usize> {
        match self.popups.thread_action_menu()? {
            ThreadActionMenuState::Actions { selection, .. } => {
                Some(selection.selected_for_len(self.selected_thread_action_items().len()))
            }
            ThreadActionMenuState::MuteDuration { selection, .. } => {
                Some(selection.selected_for_len(self.selected_thread_mute_duration_items().len()))
            }
            ThreadActionMenuState::NotificationSettings { selection, .. } => {
                Some(selection.selected_for_len(self.selected_thread_notification_items().len()))
            }
        }
    }

    fn thread_action_row_count(&self) -> usize {
        match self.popups.thread_action_menu() {
            Some(ThreadActionMenuState::Actions { .. }) => {
                self.selected_thread_action_items().len()
            }
            Some(ThreadActionMenuState::MuteDuration { .. }) => {
                self.selected_thread_mute_duration_items().len()
            }
            Some(ThreadActionMenuState::NotificationSettings { .. }) => {
                self.selected_thread_notification_items().len()
            }
            None => 0,
        }
    }

    fn thread_action_selection_mut(&mut self) -> Option<&mut super::SelectablePopupState> {
        match self.popups.thread_action_menu_mut()? {
            ThreadActionMenuState::Actions { selection, .. }
            | ThreadActionMenuState::MuteDuration { selection, .. }
            | ThreadActionMenuState::NotificationSettings { selection, .. } => Some(selection),
        }
    }

    pub fn move_thread_action_down(&mut self) {
        let len = self.thread_action_row_count();
        if let Some(selection) = self.thread_action_selection_mut() {
            selection.move_down(len);
        }
    }

    pub fn move_thread_action_up(&mut self) {
        if let Some(selection) = self.thread_action_selection_mut() {
            selection.move_up();
        }
    }

    pub fn activate_selected_thread_action(&mut self) -> Option<AppCommand> {
        let menu = self.popups.thread_action_menu().cloned()?;
        match menu {
            ThreadActionMenuState::Actions {
                guild_id,
                channel_id,
                selection,
            } => {
                let items = self.selected_thread_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ThreadActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_thread_action_menu();
                        None
                    }
                    ThreadActionKind::CopyLink => {
                        let url = format!("https://discord.com/channels/{guild_id}/{channel_id}");
                        self.runtime.copy_text_requested = Some((url, "Link copied"));
                        self.close_thread_action_menu();
                        None
                    }
                    ThreadActionKind::CopyId => {
                        self.runtime.copy_text_requested =
                            Some((channel_id.get().to_string(), "Thread ID copied"));
                        self.close_thread_action_menu();
                        None
                    }
                    ThreadActionKind::ToggleFollow => {
                        self.close_thread_action_menu();
                        self.toggle_thread_follow(channel_id)
                    }
                    ThreadActionKind::ToggleMute => {
                        if self.discord.cache.channel_notification_muted(channel_id) {
                            self.close_thread_action_menu();
                            self.toggle_thread_mute(channel_id, None)
                        } else {
                            if let Some(menu) = self.popups.thread_action_menu_mut() {
                                *menu = ThreadActionMenuState::MuteDuration {
                                    guild_id,
                                    channel_id,
                                    selection: Default::default(),
                                };
                            }
                            None
                        }
                    }
                    ThreadActionKind::Close => {
                        self.close_thread_action_menu();
                        self.toggle_thread_archived(channel_id)
                    }
                    ThreadActionKind::Lock => {
                        self.close_thread_action_menu();
                        self.toggle_thread_locked(channel_id)
                    }
                    ThreadActionKind::Pin => {
                        self.close_thread_action_menu();
                        self.toggle_thread_pinned(channel_id)
                    }
                    ThreadActionKind::Delete => {
                        self.close_thread_action_menu();
                        self.open_thread_delete_confirmation(channel_id);
                        None
                    }
                    ThreadActionKind::Edit => {
                        self.close_thread_action_menu();
                        self.open_thread_edit(channel_id);
                        None
                    }
                    ThreadActionKind::NotificationSettings => {
                        if let Some(menu) = self.popups.thread_action_menu_mut() {
                            *menu = ThreadActionMenuState::NotificationSettings {
                                guild_id,
                                channel_id,
                                selection: Default::default(),
                            };
                        }
                        None
                    }
                }
            }
            ThreadActionMenuState::MuteDuration {
                channel_id,
                selection,
                ..
            } => {
                let items = self.selected_thread_mute_duration_items();
                let item = items.get(selection.selected_for_len(items.len()))?;
                let duration = item.duration;
                self.close_thread_action_menu();
                self.toggle_thread_mute(channel_id, Some(duration))
            }
            ThreadActionMenuState::NotificationSettings {
                channel_id,
                selection,
                ..
            } => {
                let items = self.selected_thread_notification_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                self.close_thread_action_menu();
                Some(AppCommand::SetThreadNotificationLevel {
                    channel_id,
                    flags: item.flags,
                    label: self.channel_label(channel_id),
                })
            }
        }
    }

    /// Whether the current user is a member of the post thread (i.e. following
    /// it). Discord gates muting on this.
    fn is_thread_followed(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .map(|channel| channel.current_user_joined_thread)
            .unwrap_or(false)
    }

    fn toggle_thread_follow(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let followed = self.is_thread_followed(channel_id);
        Some(AppCommand::SetThreadFollowed {
            channel_id,
            followed: !followed,
            label: self.channel_label(channel_id),
        })
    }

    /// Build the thread-member mute command for a thread. Unlike a regular
    /// channel, this targets the thread-member settings endpoint.
    fn toggle_thread_mute(
        &self,
        channel_id: Id<ChannelMarker>,
        duration: Option<MuteDuration>,
    ) -> Option<AppCommand> {
        let channel = self.discord.cache.channel(channel_id)?;
        let muted = !self.discord.cache.channel_notification_muted(channel_id);
        Some(AppCommand::SetThreadMuted {
            guild_id: channel.guild_id,
            channel_id,
            muted,
            duration,
            label: self.channel_label(channel_id),
        })
    }

    fn is_thread_archived(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_archived())
            .unwrap_or(false)
    }

    fn is_thread_locked(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_locked())
            .unwrap_or(false)
    }

    fn is_thread_pinned(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_pinned())
            .unwrap_or(false)
    }

    /// Whether the user can moderate the post (lock/pin/delete). Resolves the
    /// manage permission against the thread, which inherits from its parent.
    fn can_moderate_thread(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| {
                self.discord
                    .cache
                    .can_manage_channel_structure_in_channel(channel)
            })
    }

    /// Whether the user can close or edit the post: the author always can,
    /// otherwise it requires the moderator permission.
    fn can_manage_thread(&self, channel_id: Id<ChannelMarker>) -> bool {
        let is_owner = self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| {
                channel.owner_id.is_some() && channel.owner_id == self.current_user_id()
            });
        is_owner || self.can_moderate_thread(channel_id)
    }

    fn toggle_thread_archived(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        Some(AppCommand::SetThreadArchived {
            channel_id,
            archived: !self.is_thread_archived(channel_id),
            label: self.channel_label(channel_id),
        })
    }

    fn toggle_thread_locked(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        Some(AppCommand::SetThreadLocked {
            channel_id,
            locked: !self.is_thread_locked(channel_id),
            label: self.channel_label(channel_id),
        })
    }

    fn toggle_thread_pinned(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let current_flags = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.flags)
            .unwrap_or(0);
        Some(AppCommand::SetThreadPinned {
            channel_id,
            pinned: !self.is_thread_pinned(channel_id),
            current_flags,
            label: self.channel_label(channel_id),
        })
    }

    fn open_thread_delete_confirmation(&mut self, channel_id: Id<ChannelMarker>) {
        let name = self.channel_label(channel_id);
        let is_forum_post = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        self.popups.modal = Some(ModalPopup::ThreadDeleteConfirmation(
            ThreadDeleteConfirmationState {
                channel_id,
                name,
                is_forum_post,
            },
        ));
    }

    pub fn close_thread_delete_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_thread_delete(&mut self) -> Option<AppCommand> {
        let confirmation = self.popups.take_thread_delete_confirmation()?;
        Some(AppCommand::DeleteThread {
            channel_id: confirmation.channel_id,
            label: confirmation.name,
        })
    }

    /// The display name and noun ("post" for a forum post, "thread" otherwise)
    /// for the open delete confirmation gate.
    pub fn thread_delete_confirmation_target(&self) -> Option<(String, &'static str)> {
        self.popups
            .thread_delete_confirmation()
            .map(|confirmation| {
                let noun = if confirmation.is_forum_post {
                    "post"
                } else {
                    "thread"
                };
                (confirmation.name.clone(), noun)
            })
    }
}
