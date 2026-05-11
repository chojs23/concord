use std::collections::HashSet;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{AppCommand, AppEvent, ChannelState};

use super::{
    ActiveGuildScope, DashboardState, PendingReadAck, READ_ACK_DEBOUNCE, ThreadReturnTarget,
};
use super::{
    model::{
        ChannelActionItem, ChannelActionKind, ChannelBranch, ChannelPaneEntry, ChannelThreadItem,
        FocusPane, MUTE_ACTION_DURATIONS, channel_action_shortcut, indexed_shortcut,
    },
    popups::ChannelActionMenuState,
    presentation::{is_direct_message_channel, sort_channels, sort_direct_message_channels},
    scroll::{
        clamp_list_viewport, clamp_selected_index, close_collapsed_key, move_index_down,
        move_index_up, open_collapsed_key, pane_content_height, toggle_collapsed_key,
    },
};

impl DashboardState {
    pub fn channel_action_menu_title(&self) -> Option<String> {
        let channel_id = match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { channel_id, .. }
            | ChannelActionMenuState::MuteDuration { channel_id, .. }
            | ChannelActionMenuState::Threads { channel_id, .. } => *channel_id,
        };
        Some(self.channel_label(channel_id))
    }

    pub fn open_selected_channel_actions(&mut self) {
        if self.focus != FocusPane::Channels {
            return;
        }
        let Some(channel_id) = self.selected_channel_action_target_id() else {
            return;
        };
        self.open_channel_actions(channel_id);
    }

    fn open_channel_actions(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.channel(channel_id) else {
            return;
        };
        if channel.is_thread() {
            return;
        }
        self.channel_action_menu = Some(ChannelActionMenuState::Actions {
            channel_id,
            selected: 0,
        });
    }

    pub fn close_channel_action_menu(&mut self) {
        self.channel_action_menu = None;
    }

    pub fn back_channel_action_menu(&mut self) {
        match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Threads { channel_id, .. })
            | Some(ChannelActionMenuState::MuteDuration { channel_id, .. }) => {
                let channel_id = *channel_id;
                self.channel_action_menu = Some(ChannelActionMenuState::Actions {
                    channel_id,
                    selected: 0,
                });
            }
            _ => self.channel_action_menu = None,
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let Some(channel) = self.discord.channel(channel_id) else {
            return Vec::new();
        };
        if channel.is_category() {
            return vec![ChannelActionItem {
                kind: ChannelActionKind::ToggleMute,
                label: if self.discord.channel_notification_muted(channel_id) {
                    "Unmute category".to_owned()
                } else {
                    "Mute category".to_owned()
                },
                enabled: true,
            }];
        }
        let thread_count = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .count();
        let thread_label = if thread_count == 0 {
            "Show threads (none)".to_owned()
        } else {
            format!("Show threads ({thread_count})")
        };
        // The Mark-as-read entry stays enabled only when the channel still
        // has either an active unread divider/banner or pending unread
        // bookkeeping, so it doesn't show as a no-op on already-read
        // channels.
        let active_channel_has_unread_snapshot = self.active_channel_id == Some(channel_id)
            && (self.unread_divider_last_acked_id.is_some() || self.pending_unread_anchor_scroll);
        let mark_as_read_enabled = active_channel_has_unread_snapshot
            || self.discord.channel_ack_target(channel_id).is_some();
        vec![
            ChannelActionItem {
                kind: ChannelActionKind::LoadPinnedMessages,
                label: "Show pinned messages".to_owned(),
                enabled: true,
            },
            ChannelActionItem {
                kind: ChannelActionKind::ShowThreads,
                label: thread_label,
                enabled: thread_count > 0,
            },
            ChannelActionItem {
                kind: ChannelActionKind::MarkAsRead,
                label: "Mark as read".to_owned(),
                enabled: mark_as_read_enabled,
            },
            ChannelActionItem {
                kind: ChannelActionKind::ToggleMute,
                label: if self.discord.channel_notification_muted(channel_id) {
                    "Unmute channel".to_owned()
                } else {
                    "Mute channel".to_owned()
                },
                enabled: true,
            },
        ]
    }

    pub fn selected_channel_mute_duration_items(&self) -> &'static [super::MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn channel_action_thread_items(&self) -> Vec<ChannelThreadItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Threads { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        self.child_thread_items(channel_id)
    }

    pub fn selected_forum_post_items(&self) -> Vec<ChannelThreadItem> {
        let Some(channel) = self
            .selected_channel_state()
            .filter(|channel| channel.is_forum())
        else {
            return Vec::new();
        };
        let Some(list) = self.forum_post_lists.get(&channel.id) else {
            return Vec::new();
        };
        let mut items =
            self.forum_post_section_items(&list.active_post_ids, channel.id, "Active posts", false);
        items.extend(self.forum_post_section_items(
            &list.archived_post_ids,
            channel.id,
            "Archived posts",
            true,
        ));
        items
    }

    pub fn selected_forum_posts_loading(&self) -> bool {
        let Some(channel) = self
            .selected_channel_state()
            .filter(|channel| channel.is_forum())
        else {
            return false;
        };
        !self.forum_post_lists.contains_key(&channel.id)
    }

    pub fn visible_forum_post_items(&self) -> Vec<ChannelThreadItem> {
        let height = self.message_content_height();
        let mut rows = 0usize;
        let mut visible = Vec::new();
        for post in self
            .selected_forum_post_items()
            .into_iter()
            .skip(self.message_scroll)
        {
            let rendered_height = post.rendered_height();
            if !visible.is_empty() && rows.saturating_add(rendered_height) > height {
                break;
            }
            rows = rows.saturating_add(rendered_height);
            visible.push(post);
            if rows >= height {
                break;
            }
        }
        visible
    }

    pub fn selected_forum_post(&self) -> usize {
        clamp_selected_index(
            self.selected_message,
            self.selected_forum_post_items().len(),
        )
    }

    pub fn focused_forum_post_selection(&self) -> Option<usize> {
        if self.focus != FocusPane::Messages || !self.selected_channel_is_forum() {
            return None;
        }
        let selected = self.selected_forum_post();
        let visible_count = self.visible_forum_post_items().len();
        if visible_count > 0
            && selected >= self.message_scroll
            && selected < self.message_scroll + visible_count
        {
            Some(selected - self.message_scroll)
        } else {
            None
        }
    }

    pub fn selected_channel_is_forum(&self) -> bool {
        self.selected_channel_state()
            .is_some_and(|channel| channel.is_forum())
    }

    pub fn selected_message_history_channel_id(&self) -> Option<Id<ChannelMarker>> {
        (!self.selected_channel_is_forum()).then_some(self.selected_channel_id()?)
    }

    pub fn selected_forum_channel(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        let channel = self
            .selected_channel_state()
            .filter(|channel| channel.is_forum())?;
        Some((channel.guild_id?, channel.id))
    }

    pub fn selected_forum_channel_with_load_more(
        &self,
    ) -> Option<(Id<GuildMarker>, Id<ChannelMarker>, bool)> {
        let (guild_id, channel_id) = self.selected_forum_channel()?;
        Some((
            guild_id,
            channel_id,
            self.should_load_more_forum_posts(channel_id),
        ))
    }

    pub fn activate_selected_forum_post(&mut self) -> Option<AppCommand> {
        let item = self
            .selected_forum_post_items()
            .get(self.selected_forum_post())?
            .clone();
        let guild_id = self
            .discord
            .channel(item.channel_id)
            .and_then(|channel| channel.guild_id)?;
        self.record_thread_return_target(item.channel_id);
        self.activate_channel(item.channel_id);
        Some(AppCommand::SubscribeGuildChannel {
            guild_id,
            channel_id: item.channel_id,
        })
    }

    fn child_thread_items(&self, channel_id: Id<ChannelMarker>) -> Vec<ChannelThreadItem> {
        let mut threads: Vec<&ChannelState> = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .collect();
        sort_thread_channels(&mut threads);
        threads
            .into_iter()
            .map(|thread| {
                self.forum_thread_item(thread, None, thread.thread_archived.unwrap_or(false))
            })
            .collect()
    }

    fn forum_post_section_items(
        &self,
        post_ids: &[Id<ChannelMarker>],
        forum_channel_id: Id<ChannelMarker>,
        section_label: &str,
        archived: bool,
    ) -> Vec<ChannelThreadItem> {
        // Two corrections versus the order Discord's `/threads/search` returns:
        //
        //  1. Pinned posts come back interleaved with everything else by
        //     activity time, but the official client lifts them to the top.
        //  2. The server-side `sort_by=last_message_time` index can be stale —
        //     posts with newer messages sometimes sit below older ones. The
        //     `last_message_id` snowflake encodes the actual message
        //     timestamp, and we keep it fresh via gateway updates, so a local
        //     resort by that field tracks Discord's UI more closely.
        let (mut pinned, mut rest): (Vec<_>, Vec<_>) = post_ids
            .iter()
            .filter_map(|post_id| self.discord.channel(*post_id))
            .filter(|post| post.is_thread() && post.parent_id == Some(forum_channel_id))
            .partition(|post| post.thread_pinned.unwrap_or(false));
        let by_last_message = |post: &&ChannelState| {
            std::cmp::Reverse(post.last_message_id.map(|id| id.get()).unwrap_or(0))
        };
        pinned.sort_by_key(by_last_message);
        rest.sort_by_key(by_last_message);

        pinned
            .into_iter()
            .chain(rest)
            .enumerate()
            .map(|(index, post)| {
                self.forum_thread_item(
                    post,
                    (index == 0).then(|| section_label.to_owned()),
                    archived,
                )
            })
            .collect()
    }

    fn forum_thread_item(
        &self,
        channel: &ChannelState,
        section_label: Option<String>,
        archived: bool,
    ) -> ChannelThreadItem {
        let preview = self
            .discord
            .messages_for_channel(channel.id)
            .into_iter()
            .next();
        ChannelThreadItem {
            channel_id: channel.id,
            section_label,
            label: channel.name.clone(),
            archived,
            locked: channel.thread_locked.unwrap_or(false),
            pinned: channel.thread_pinned.unwrap_or(false),
            preview_author_id: preview.map(|message| message.author_id),
            preview_author: preview.map(|message| message.author.clone()),
            preview_author_color: preview
                .and_then(|message| self.message_author_role_color(message)),
            preview_content: preview.map(|message| self.thread_message_preview_text(message)),
            preview_reactions: preview
                .map(|message| message.reactions.clone())
                .unwrap_or_default(),
            comment_count: channel.message_count.or(channel.total_message_sent),
            last_activity_message_id: channel
                .last_message_id
                .or_else(|| preview.map(|message| message.id)),
        }
    }

    fn should_load_more_forum_posts(&self, channel_id: Id<ChannelMarker>) -> bool {
        let Some(list) = self.forum_post_lists.get(&channel_id) else {
            return false;
        };
        if !list.has_more {
            return false;
        }
        let visible_bottom = self
            .message_scroll
            .saturating_add(self.visible_forum_post_items().len().max(1))
            .saturating_add(5);
        let selected_bottom = self.selected_forum_post().saturating_add(5);
        let len = list
            .active_post_ids
            .len()
            .saturating_add(list.archived_post_ids.len());
        visible_bottom >= len || selected_bottom >= len
    }

    pub fn selected_channel_action_index(&self) -> Option<usize> {
        match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_action_items().len(),
            )),
            ChannelActionMenuState::MuteDuration { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_mute_duration_items().len(),
            )),
            ChannelActionMenuState::Threads { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.channel_action_thread_items().len(),
            )),
        }
    }

    pub fn move_channel_action_down(&mut self) {
        let len = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelActionMenuState::MuteDuration { .. }) => {
                self.selected_channel_mute_duration_items().len()
            }
            Some(ChannelActionMenuState::Threads { .. }) => {
                self.channel_action_thread_items().len()
            }
            None => return,
        };
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::MuteDuration { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_down(selected, len);
        }
    }

    pub fn move_channel_action_up(&mut self) {
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::MuteDuration { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_up(selected);
        }
    }

    pub fn select_channel_action_row(&mut self, row: usize) -> bool {
        let len = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelActionMenuState::MuteDuration { .. }) => {
                self.selected_channel_mute_duration_items().len()
            }
            Some(ChannelActionMenuState::Threads { .. }) => {
                self.channel_action_thread_items().len()
            }
            None => return false,
        };
        if row >= len {
            return false;
        }
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::MuteDuration { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            *selected = row;
            return true;
        }
        false
    }

    pub fn activate_selected_channel_action(&mut self) -> Option<AppCommand> {
        let menu = self.channel_action_menu.clone()?;
        match menu {
            ChannelActionMenuState::Actions {
                channel_id,
                selected,
            } => {
                let items = self.selected_channel_action_items();
                let item = items
                    .get(clamp_selected_index(selected, items.len()))?
                    .clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ChannelActionKind::LoadPinnedMessages => {
                        self.enter_pinned_message_view(channel_id);
                        self.close_channel_action_menu();
                        Some(AppCommand::LoadPinnedMessages { channel_id })
                    }
                    ChannelActionKind::ShowThreads => {
                        self.channel_action_menu = Some(ChannelActionMenuState::Threads {
                            channel_id,
                            selected: 0,
                        });
                        None
                    }
                    ChannelActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_channel_action_menu();
                        // `mark_channel_as_read` already queued the
                        // `AckChannel` command via `queue_channel_ack`, so
                        // there's nothing extra for the dispatch loop here.
                        None
                    }
                    ChannelActionKind::ToggleMute => {
                        if self.discord.channel_notification_muted(channel_id) {
                            self.close_channel_action_menu();
                            self.toggle_channel_mute(channel_id, None)
                        } else {
                            self.channel_action_menu = Some(ChannelActionMenuState::MuteDuration {
                                channel_id,
                                selected: 0,
                            });
                            None
                        }
                    }
                }
            }
            ChannelActionMenuState::MuteDuration {
                channel_id,
                selected,
            } => {
                let item =
                    self.selected_channel_mute_duration_items()
                        .get(clamp_selected_index(
                            selected,
                            self.selected_channel_mute_duration_items().len(),
                        ))?;
                self.close_channel_action_menu();
                self.toggle_channel_mute(channel_id, Some(item.duration))
            }
            ChannelActionMenuState::Threads { .. } => {
                let items = self.channel_action_thread_items();
                let index = self.selected_channel_action_index()?;
                let item = items.get(index)?.clone();
                let guild_id = self
                    .discord
                    .channel(item.channel_id)
                    .and_then(|c| c.guild_id);
                self.activate_channel(item.channel_id);
                self.close_channel_action_menu();
                guild_id.map(|guild_id| AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id: item.channel_id,
                })
            }
        }
    }

    pub fn activate_channel_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { .. } => {
                let actions = self.selected_channel_action_items();
                let index = actions.iter().enumerate().position(|(index, action)| {
                    action.enabled
                        && channel_action_shortcut(&actions, index)
                            .is_some_and(|candidate| candidate == shortcut)
                })?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelActionMenuState::MuteDuration { .. } => {
                let index = self
                    .selected_channel_mute_duration_items()
                    .iter()
                    .enumerate()
                    .position(|(index, _)| indexed_shortcut(index) == Some(shortcut))?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelActionMenuState::Threads { .. } => {
                let threads = self.channel_action_thread_items();
                let index = threads
                    .iter()
                    .enumerate()
                    .position(|(index, _)| indexed_shortcut(index) == Some(shortcut))?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
        }
    }

    pub(super) fn selected_channel_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            // DMs do not carry guild-style permissions; show every channel.
            ActiveGuildScope::DirectMessages => self.discord.channels_for_guild(None),
            // Filter to channels we have VIEW_CHANNEL on, otherwise the
            // sidebar surfaces channels that REST refuses with 403.
            ActiveGuildScope::Guild(guild_id) => {
                self.discord.viewable_channels_for_guild(Some(guild_id))
            }
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        // Threads are reached through the channel action menu instead of
        // appearing as top-level entries; without this filter their parent
        // channel would not be in `category_ids`, so the roots filter below
        // would let them through and render them under the channel list.
        channels.retain(|channel| !channel.is_thread());
        if self.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels
                .into_iter()
                .map(|state| ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                })
                .collect();
        }

        let category_ids: HashSet<Id<ChannelMarker>> = channels
            .iter()
            .filter(|channel| channel.is_category())
            .map(|channel| channel.id)
            .collect();

        let mut roots: Vec<&ChannelState> = channels
            .iter()
            .copied()
            .filter(|channel| {
                channel.is_category()
                    || channel
                        .parent_id
                        .is_none_or(|parent_id| !category_ids.contains(&parent_id))
            })
            .collect();
        sort_channels(&mut roots);

        let mut entries = Vec::new();
        for root in roots {
            if !root.is_category() {
                entries.push(ChannelPaneEntry::Channel {
                    state: root,
                    branch: ChannelBranch::None,
                });
                continue;
            }

            let collapsed = self.collapsed_channel_categories.contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });
            if collapsed {
                continue;
            }

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            let last_child_index = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    ChannelBranch::Last
                } else {
                    ChannelBranch::Middle
                };
                entries.push(ChannelPaneEntry::Channel {
                    state: child,
                    branch,
                });
            }
        }

        entries
    }

    pub fn selected_channel(&self) -> usize {
        clamp_selected_index(self.selected_channel, self.channel_pane_entries().len())
    }

    pub(super) fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::CategoryHeader { .. }) | None => None,
        }
    }

    pub fn channel_scroll(&self) -> usize {
        self.channel_scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_entries()
            .into_iter()
            .skip(self.channel_scroll)
            .take(pane_content_height(self.channel_view_height))
            .collect()
    }

    pub fn focused_channel_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Channels && !self.channel_pane_entries().is_empty() {
            let selected = self.selected_channel();
            let visible_len = self.visible_channel_pane_entries().len();
            if selected >= self.channel_scroll && selected < self.channel_scroll + visible_len {
                Some(selected - self.channel_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        self.channel_view_height = height;
        let height = pane_content_height(self.channel_view_height);
        let len = self.channel_pane_entries().len();
        clamp_list_viewport(
            self.selected_channel,
            &mut self.channel_scroll,
            height,
            len,
            self.channel_keep_selection_visible,
        );
    }

    pub(super) fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self.channel_pane_entries().iter().position(|entry| {
            matches!(entry, ChannelPaneEntry::Channel { state, .. } if state.id == channel_id)
        }) {
            self.selected_channel = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
    }

    /// Builds the "X is typing…" line for the currently selected channel, or
    /// `None` when nobody is typing (or the only typer is us). Resolution
    /// order for each user: cached guild member alias → DM recipient
    /// display name → `user-{id}` fallback. Caps at three names and
    /// collapses to "Several people are typing…" beyond that.
    pub fn typing_footer_for_selected_channel(&self) -> Option<String> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.discord.channel(channel_id)?;
        let guild_id = channel.guild_id;
        let typers: Vec<Id<UserMarker>> = self
            .discord
            .typing_users(channel_id)
            .into_iter()
            .filter(|user_id| Some(*user_id) != self.current_user_id)
            .collect();
        if typers.is_empty() {
            return None;
        }

        let resolve_name = |user_id: Id<UserMarker>| -> String {
            if let Some(name) =
                guild_id.and_then(|guild_id| self.discord.member_display_name(guild_id, user_id))
            {
                return name.to_owned();
            }
            if let Some(recipient) = channel
                .recipients
                .iter()
                .find(|recipient| recipient.user_id == user_id)
            {
                return recipient.display_name.clone();
            }
            format!("user-{}", user_id.get())
        };

        let total = typers.len();
        let names: Vec<String> = typers.iter().take(3).copied().map(resolve_name).collect();
        let footer = match total {
            1 => format!("{} is typing…", names[0]),
            2 => format!("{} and {} are typing…", names[0], names[1]),
            3 => format!("{}, {}, and {} are typing…", names[0], names[1], names[2]),
            _ => "Several people are typing…".to_owned(),
        };
        Some(footer)
    }

    pub fn channel_label(&self, channel_id: Id<ChannelMarker>) -> String {
        self.discord
            .channel(channel_id)
            .map(|channel| match channel.kind.as_str() {
                "dm" | "Private" => format!("@{}", channel.name),
                "group-dm" | "Group" => channel.name.clone(),
                "category" | "GuildCategory" => channel.name.clone(),
                _ => format!("#{}", channel.name),
            })
            .unwrap_or_else(|| format!("#channel-{}", channel_id.get()))
    }

    fn toggle_channel_mute(
        &mut self,
        channel_id: Id<ChannelMarker>,
        duration: Option<crate::discord::MuteDuration>,
    ) -> Option<AppCommand> {
        let channel = self.discord.channel(channel_id)?;
        let muted = !self.discord.channel_notification_muted(channel_id);
        Some(AppCommand::SetChannelMuted {
            guild_id: channel.guild_id,
            channel_id,
            muted,
            duration,
            label: self.channel_label(channel_id),
        })
    }

    pub fn message_pane_title(&self) -> String {
        let Some(channel_id) = self.selected_channel_id() else {
            return "no channel".to_owned();
        };
        let label = self.channel_label(channel_id);
        if self.pinned_message_view_channel_id == Some(channel_id) {
            format!("{label} pinned messages")
        } else {
            label
        }
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } if Some(state.id) == self.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        toggle_collapsed_key(&mut self.collapsed_channel_categories, category_id);
    }

    pub fn open_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            open_collapsed_key(&mut self.collapsed_channel_categories, &category_id);
        }
    }

    pub fn close_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            close_collapsed_key(&mut self.collapsed_channel_categories, category_id);
        }
    }

    #[cfg(test)]
    pub fn confirm_selected_channel(&mut self) {
        let _ = self.confirm_selected_channel_command();
    }

    pub fn confirm_selected_channel_command(&mut self) -> Option<AppCommand> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { .. }) => {
                self.toggle_selected_channel_category();
                None
            }
            Some(ChannelPaneEntry::Channel { state, .. }) => {
                let channel_id = state.id;
                let command = if is_direct_message_channel(state) {
                    Some(AppCommand::SubscribeDirectMessage { channel_id })
                } else {
                    state
                        .guild_id
                        .map(|guild_id| AppCommand::SubscribeGuildChannel {
                            guild_id,
                            channel_id,
                        })
                };
                self.activate_channel(channel_id);
                command
            }
            None => None,
        }
    }

    pub(super) fn record_thread_return_target(&mut self, thread_channel_id: Id<ChannelMarker>) {
        let Some(channel_id) = self.active_channel_id else {
            return;
        };
        if channel_id == thread_channel_id {
            return;
        }
        self.thread_return_target = Some(ThreadReturnTarget {
            thread_channel_id,
            channel_id,
            selected_message: self.selected_message,
            message_scroll: self.message_scroll,
            message_line_scroll: self.message_line_scroll,
            message_keep_selection_visible: self.message_keep_selection_visible,
            message_auto_follow: self.message_auto_follow,
            new_messages_marker_message_id: self.new_messages_marker_message_id,
            unread_divider_last_acked_id: self.unread_divider_last_acked_id,
            pending_unread_anchor_scroll: self.pending_unread_anchor_scroll,
        });
    }

    pub fn return_from_opened_thread(&mut self) -> bool {
        let Some(target) = self.thread_return_target else {
            return false;
        };
        if self.active_channel_id != Some(target.thread_channel_id) {
            return false;
        }
        if !self
            .selected_channel_state()
            .is_some_and(|channel| channel.is_thread())
        {
            self.thread_return_target = None;
            return false;
        }
        if self.discord.channel(target.channel_id).is_none() {
            self.thread_return_target = None;
            return false;
        }

        self.activate_channel(target.channel_id);
        self.selected_message = target.selected_message;
        self.message_scroll = target.message_scroll;
        self.message_line_scroll = target.message_line_scroll;
        self.message_keep_selection_visible = target.message_keep_selection_visible;
        self.message_auto_follow = target.message_auto_follow;
        self.new_messages_marker_message_id = target.new_messages_marker_message_id;
        self.unread_divider_last_acked_id = target.unread_divider_last_acked_id;
        self.pending_unread_anchor_scroll = target.pending_unread_anchor_scroll;
        self.thread_return_target = None;
        self.clamp_message_viewport();
        true
    }

    pub(super) fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        let is_forum = self
            .discord
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum());
        let preserves_thread_return = self.thread_return_target.is_some_and(|target| {
            self.active_channel_id == Some(target.channel_id)
                && channel_id == target.thread_channel_id
        });
        if !preserves_thread_return {
            self.thread_return_target = None;
        }
        self.active_channel_id = Some(channel_id);
        self.pinned_message_view_channel_id = None;
        self.pinned_message_view_return_target = None;

        // Capture the unread anchor BEFORE acking. The Discord-style red
        // divider sits just above the first message newer than this
        // snapshot, and the viewport tries to open at the user's last-read
        // position. Capturing the snapshot rather than a resolved index
        // means the divider still appears once history arrives later.
        let last_acked_snapshot = if is_forum {
            None
        } else {
            self.discord.channel_last_acked_message_id(channel_id)
        };
        let has_unread = last_acked_snapshot.is_some_and(|acked| {
            self.discord
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id)
                .is_some_and(|latest| latest > acked)
        });

        self.clear_new_messages_marker();
        self.message_line_scroll = 0;

        if has_unread {
            self.unread_divider_last_acked_id = last_acked_snapshot;
            self.pending_unread_anchor_scroll = true;
            self.message_auto_follow = false;
            // Disable selection-keep until the snap lands; otherwise the
            // centering pass in `clamp_message_viewport_for_image_previews`
            // would pull the viewport to the latest message before the
            // snap can pin it to the last-read anchor.
            self.message_keep_selection_visible = false;
        } else {
            self.unread_divider_last_acked_id = None;
            self.pending_unread_anchor_scroll = false;
            self.message_auto_follow = !is_forum;
            self.message_keep_selection_visible = true;
        }

        self.selected_message = if is_forum {
            0
        } else {
            self.messages().len().saturating_sub(1)
        };
        self.message_scroll = 0;

        // If the unread anchor's last-read message is already loaded, snap
        // the viewport to it now so the first frame opens at the right
        // spot. Otherwise the snap will be retried each frame inside
        // `clamp_message_viewport_for_image_previews` until history
        // arrives.
        self.try_apply_unread_anchor_scroll();

        self.clamp_message_viewport();
        self.queue_channel_ack(channel_id);
    }

    /// Ack the channel up to its latest message and retire the unread
    /// divider/banner immediately so the visible cue matches the new
    /// fully-read state. Use this for explicit user actions like
    /// "Mark as read"; activation already runs `queue_channel_ack` on its
    /// own.
    pub fn mark_channel_as_read(&mut self, channel_id: Id<ChannelMarker>) {
        self.queue_channel_ack(channel_id);
        if self.active_channel_id == Some(channel_id) {
            self.unread_divider_last_acked_id = None;
            self.pending_unread_anchor_scroll = false;
            self.clear_new_messages_marker();
        }
    }

    /// Optimistic local ack + queued REST POST so the unread badge clears
    /// immediately on activation.
    pub(super) fn queue_channel_ack(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(message_id) = self.discord.channel_ack_target(channel_id) else {
            return;
        };
        self.pending_read_acks.remove(&channel_id);
        self.discord.apply_event(&AppEvent::MessageAck {
            channel_id,
            message_id,
            mention_count: 0,
        });
        self.pending_commands.push_back(AppCommand::AckChannel {
            channel_id,
            message_id,
        });
    }

    pub(super) fn schedule_channel_ack(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(message_id) = self.discord.channel_ack_target(channel_id) else {
            return;
        };
        self.discord.apply_event(&AppEvent::MessageAck {
            channel_id,
            message_id,
            mention_count: 0,
        });
        let deadline = std::time::Instant::now() + READ_ACK_DEBOUNCE;
        self.pending_read_acks
            .entry(channel_id)
            .and_modify(|pending| {
                pending.message_id = pending.message_id.max(message_id);
            })
            .or_insert(PendingReadAck {
                message_id,
                deadline,
            });
    }

    fn selected_channel_category_id(&self) -> Option<Id<ChannelMarker>> {
        let entries = self.channel_pane_entries();
        let selected = self.selected_channel();
        match entries.get(selected) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { branch, .. }) if branch.is_category_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn selected_channel_action_target_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            None => None,
        }
    }
}

fn sort_thread_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| std::cmp::Reverse(channel.id));
}
