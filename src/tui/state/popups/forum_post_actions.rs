use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{AppCommand, MuteDuration};

use super::super::model::{FocusPane, MUTE_ACTION_DURATIONS};
use super::super::{
    DashboardState, ForumPostActionItem, ForumPostActionKind, ForumPostNotificationItem,
    MuteActionDurationItem,
};
use super::{
    ActiveModalPopupKind, ForumPostActionMenuState, ForumPostDeleteConfirmationState, ModalPopup,
};

impl DashboardState {
    /// Open the action menu for the focused forum post. Returns `false` (so the
    /// caller can fall back to other action contexts) when a forum post is not
    /// the current focus.
    pub fn open_selected_forum_post_actions(&mut self) -> bool {
        let Some((guild_id, channel_id)) = self.focused_forum_post_action_target() else {
            return false;
        };
        self.popups.modal = Some(ModalPopup::ForumPostActionMenu(
            ForumPostActionMenuState::Actions {
                guild_id,
                channel_id,
                selection: Default::default(),
            },
        ));
        true
    }

    /// The parent guild and thread id of the forum post under the cursor, when
    /// the messages pane is showing forum posts and one is selected.
    fn focused_forum_post_action_target(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        if self.navigation.focus != FocusPane::Messages || !self.message_pane_uses_forum_posts() {
            return None;
        }
        let (guild_id, _) = self.selected_forum_channel()?;
        let item = self
            .selected_forum_post_items()
            .get(self.selected_forum_post())?
            .clone();
        Some((guild_id, item.channel_id))
    }

    pub fn is_forum_post_action_menu_active(&self) -> bool {
        self.popups.forum_post_action_menu().is_some()
    }

    pub fn is_forum_post_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.forum_post_action_menu(),
            Some(ForumPostActionMenuState::MuteDuration { .. })
        )
    }

    pub fn is_forum_post_action_notification_phase(&self) -> bool {
        matches!(
            self.popups.forum_post_action_menu(),
            Some(ForumPostActionMenuState::NotificationSettings { .. })
        )
    }

    pub fn close_forum_post_action_menu(&mut self) {
        if self.is_forum_post_action_menu_active() {
            self.popups.clear_modal();
        }
    }

    /// Step back from a submenu to the top-level actions. Returns `false` when
    /// already at the top level so the caller can close the menu instead.
    pub fn back_forum_post_action_menu(&mut self) -> bool {
        match self.popups.forum_post_action_menu() {
            Some(
                ForumPostActionMenuState::MuteDuration {
                    guild_id,
                    channel_id,
                    ..
                }
                | ForumPostActionMenuState::NotificationSettings {
                    guild_id,
                    channel_id,
                    ..
                },
            ) => {
                let (guild_id, channel_id) = (*guild_id, *channel_id);
                if let Some(menu) = self.popups.forum_post_action_menu_mut() {
                    *menu = ForumPostActionMenuState::Actions {
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

    pub fn selected_forum_post_action_items(&self) -> Vec<ForumPostActionItem> {
        let channel_id = match self.popups.forum_post_action_menu() {
            Some(ForumPostActionMenuState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };

        // A thread is "unread" when there is an ack target sitting ahead of the
        // last read message.
        let mark_as_read_enabled = self.discord.cache.channel_ack_target(channel_id).is_some();
        // Discord only offers muting once you follow the post, so the mute row is
        // gated on membership (and unmute stays available while still followed).
        let followed = self.is_forum_post_followed(channel_id);
        let follow_label = if followed {
            "Unfollow post"
        } else {
            "Follow post"
        };
        let mute_label = if self.discord.cache.channel_notification_muted(channel_id) {
            "Unmute post"
        } else {
            "Mute post"
        };

        // Closing is allowed for the author or a moderator; lock and pin are
        // moderator-only.
        let can_moderate = self.can_moderate_forum_post(channel_id);
        let can_manage = self.can_manage_forum_post(channel_id);
        let close_label = if self.is_forum_post_archived(channel_id) {
            "Reopen post"
        } else {
            "Close post"
        };
        let lock_label = if self.is_forum_post_locked(channel_id) {
            "Unlock post"
        } else {
            "Lock post"
        };
        let pin_label = if self.is_forum_post_pinned(channel_id) {
            "Unpin post"
        } else {
            "Pin post"
        };

        vec![
            ForumPostActionItem::new(
                ForumPostActionKind::MarkAsRead,
                "Mark as read",
                mark_as_read_enabled,
            ),
            ForumPostActionItem::new(ForumPostActionKind::ToggleFollow, follow_label, true),
            ForumPostActionItem::new(ForumPostActionKind::Close, close_label, can_manage),
            ForumPostActionItem::new(ForumPostActionKind::Lock, lock_label, can_moderate),
            ForumPostActionItem::new(ForumPostActionKind::Edit, "Edit post", can_manage),
            ForumPostActionItem::new(ForumPostActionKind::CopyLink, "Copy link", true),
            ForumPostActionItem::new(ForumPostActionKind::ToggleMute, mute_label, followed),
            ForumPostActionItem::new(
                ForumPostActionKind::NotificationSettings,
                "Notification settings",
                followed,
            ),
            ForumPostActionItem::new(ForumPostActionKind::Pin, pin_label, can_moderate),
            // Deleting removes the whole thread (moderator-only); the author's
            // body-only "delete message" is a separate action we do not offer.
            ForumPostActionItem::new(ForumPostActionKind::Delete, "Delete post", can_moderate),
            ForumPostActionItem::new(ForumPostActionKind::CopyId, "Copy thread ID", true),
        ]
    }

    pub fn selected_forum_post_mute_duration_items(&self) -> &'static [MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    /// Build the three notification-level rows for the submenu, marking the
    /// current level with `[x]`. Unknown current level defaults to `4`
    /// (Only @mentions, Discord's default).
    pub fn selected_forum_post_notification_items(&self) -> Vec<ForumPostNotificationItem> {
        let channel_id = match self.popups.forum_post_action_menu() {
            Some(ForumPostActionMenuState::NotificationSettings { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let current_flags = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|ch| ch.current_user_thread_notification_flags)
            .unwrap_or(4);
        vec![
            ForumPostNotificationItem::new("All messages", 2, current_flags),
            ForumPostNotificationItem::new("Only @mentions", 4, current_flags),
            ForumPostNotificationItem::new("Nothing", 8, current_flags),
        ]
    }

    pub fn selected_forum_post_action_index(&self) -> Option<usize> {
        match self.popups.forum_post_action_menu()? {
            ForumPostActionMenuState::Actions { selection, .. } => {
                Some(selection.selected_for_len(self.selected_forum_post_action_items().len()))
            }
            ForumPostActionMenuState::MuteDuration { selection, .. } => Some(
                selection.selected_for_len(self.selected_forum_post_mute_duration_items().len()),
            ),
            ForumPostActionMenuState::NotificationSettings { selection, .. } => Some(
                selection.selected_for_len(self.selected_forum_post_notification_items().len()),
            ),
        }
    }

    fn forum_post_action_row_count(&self) -> usize {
        match self.popups.forum_post_action_menu() {
            Some(ForumPostActionMenuState::Actions { .. }) => {
                self.selected_forum_post_action_items().len()
            }
            Some(ForumPostActionMenuState::MuteDuration { .. }) => {
                self.selected_forum_post_mute_duration_items().len()
            }
            Some(ForumPostActionMenuState::NotificationSettings { .. }) => {
                self.selected_forum_post_notification_items().len()
            }
            None => 0,
        }
    }

    fn forum_post_action_selection_mut(&mut self) -> Option<&mut super::SelectablePopupState> {
        match self.popups.forum_post_action_menu_mut()? {
            ForumPostActionMenuState::Actions { selection, .. }
            | ForumPostActionMenuState::MuteDuration { selection, .. }
            | ForumPostActionMenuState::NotificationSettings { selection, .. } => Some(selection),
        }
    }

    pub fn move_forum_post_action_down(&mut self) {
        let len = self.forum_post_action_row_count();
        if let Some(selection) = self.forum_post_action_selection_mut() {
            selection.move_down(len);
        }
    }

    pub fn move_forum_post_action_up(&mut self) {
        if let Some(selection) = self.forum_post_action_selection_mut() {
            selection.move_up();
        }
    }

    pub fn activate_selected_forum_post_action(&mut self) -> Option<AppCommand> {
        let menu = self.popups.forum_post_action_menu().cloned()?;
        match menu {
            ForumPostActionMenuState::Actions {
                guild_id,
                channel_id,
                selection,
            } => {
                let items = self.selected_forum_post_action_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ForumPostActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_forum_post_action_menu();
                        None
                    }
                    ForumPostActionKind::CopyLink => {
                        let url = format!("https://discord.com/channels/{guild_id}/{channel_id}");
                        self.runtime.copy_text_requested = Some((url, "Link copied"));
                        self.close_forum_post_action_menu();
                        None
                    }
                    ForumPostActionKind::CopyId => {
                        self.runtime.copy_text_requested =
                            Some((channel_id.get().to_string(), "Thread ID copied"));
                        self.close_forum_post_action_menu();
                        None
                    }
                    ForumPostActionKind::ToggleFollow => {
                        self.close_forum_post_action_menu();
                        self.toggle_forum_post_follow(channel_id)
                    }
                    ForumPostActionKind::ToggleMute => {
                        if self.discord.cache.channel_notification_muted(channel_id) {
                            self.close_forum_post_action_menu();
                            self.toggle_forum_post_mute(channel_id, None)
                        } else {
                            if let Some(menu) = self.popups.forum_post_action_menu_mut() {
                                *menu = ForumPostActionMenuState::MuteDuration {
                                    guild_id,
                                    channel_id,
                                    selection: Default::default(),
                                };
                            }
                            None
                        }
                    }
                    ForumPostActionKind::Close => {
                        self.close_forum_post_action_menu();
                        self.toggle_forum_post_archived(channel_id)
                    }
                    ForumPostActionKind::Lock => {
                        self.close_forum_post_action_menu();
                        self.toggle_forum_post_locked(channel_id)
                    }
                    ForumPostActionKind::Pin => {
                        self.close_forum_post_action_menu();
                        self.toggle_forum_post_pinned(channel_id)
                    }
                    ForumPostActionKind::Delete => {
                        self.close_forum_post_action_menu();
                        self.open_forum_post_delete_confirmation(channel_id);
                        None
                    }
                    ForumPostActionKind::Edit => {
                        self.close_forum_post_action_menu();
                        self.open_forum_post_edit(channel_id);
                        None
                    }
                    ForumPostActionKind::NotificationSettings => {
                        if let Some(menu) = self.popups.forum_post_action_menu_mut() {
                            *menu = ForumPostActionMenuState::NotificationSettings {
                                guild_id,
                                channel_id,
                                selection: Default::default(),
                            };
                        }
                        None
                    }
                }
            }
            ForumPostActionMenuState::MuteDuration {
                channel_id,
                selection,
                ..
            } => {
                let items = self.selected_forum_post_mute_duration_items();
                let item = items.get(selection.selected_for_len(items.len()))?;
                let duration = item.duration;
                self.close_forum_post_action_menu();
                self.toggle_forum_post_mute(channel_id, Some(duration))
            }
            ForumPostActionMenuState::NotificationSettings {
                channel_id,
                selection,
                ..
            } => {
                let items = self.selected_forum_post_notification_items();
                let item = items.get(selection.selected_for_len(items.len()))?.clone();
                self.close_forum_post_action_menu();
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
    fn is_forum_post_followed(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .map(|channel| channel.current_user_joined_thread)
            .unwrap_or(false)
    }

    fn toggle_forum_post_follow(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let followed = self.is_forum_post_followed(channel_id);
        Some(AppCommand::SetThreadFollowed {
            channel_id,
            followed: !followed,
            label: self.channel_label(channel_id),
        })
    }

    /// Build the thread-member mute command for a forum post. Unlike a regular
    /// channel, this targets the thread-member settings endpoint.
    fn toggle_forum_post_mute(
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

    fn is_forum_post_archived(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_archived())
            .unwrap_or(false)
    }

    fn is_forum_post_locked(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_locked())
            .unwrap_or(false)
    }

    fn is_forum_post_pinned(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.thread_pinned())
            .unwrap_or(false)
    }

    /// Whether the user can moderate the post (lock/pin/delete). Resolves the
    /// manage permission against the thread, which inherits from its parent.
    fn can_moderate_forum_post(&self, channel_id: Id<ChannelMarker>) -> bool {
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
    fn can_manage_forum_post(&self, channel_id: Id<ChannelMarker>) -> bool {
        let is_owner = self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| {
                channel.owner_id.is_some() && channel.owner_id == self.current_user_id()
            });
        is_owner || self.can_moderate_forum_post(channel_id)
    }

    fn toggle_forum_post_archived(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        Some(AppCommand::SetForumPostArchived {
            channel_id,
            archived: !self.is_forum_post_archived(channel_id),
            label: self.channel_label(channel_id),
        })
    }

    fn toggle_forum_post_locked(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        Some(AppCommand::SetForumPostLocked {
            channel_id,
            locked: !self.is_forum_post_locked(channel_id),
            label: self.channel_label(channel_id),
        })
    }

    fn toggle_forum_post_pinned(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let current_flags = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.flags)
            .unwrap_or(0);
        Some(AppCommand::SetForumPostPinned {
            channel_id,
            pinned: !self.is_forum_post_pinned(channel_id),
            current_flags,
            label: self.channel_label(channel_id),
        })
    }

    fn open_forum_post_delete_confirmation(&mut self, channel_id: Id<ChannelMarker>) {
        let name = self.channel_label(channel_id);
        self.popups.modal = Some(ModalPopup::ForumPostDeleteConfirmation(
            ForumPostDeleteConfirmationState { channel_id, name },
        ));
    }

    pub fn close_forum_post_delete_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ForumPostDeleteConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_forum_post_delete(&mut self) -> Option<AppCommand> {
        let confirmation = self.popups.take_forum_post_delete_confirmation()?;
        Some(AppCommand::DeleteForumPost {
            channel_id: confirmation.channel_id,
            label: confirmation.name,
        })
    }

    pub fn forum_post_delete_confirmation_name(&self) -> Option<String> {
        self.popups
            .forum_post_delete_confirmation()
            .map(|confirmation| confirmation.name.clone())
    }
}
