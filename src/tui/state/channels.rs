use std::collections::{BTreeMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{ChannelState, ChannelUnreadState, TypingUserState, VoiceParticipantState};

use super::{ActiveGuildScope, DashboardState, ThreadReturnTarget};
use super::{
    model::{
        ChannelActionItem, ChannelActionKind, ChannelBranch, ChannelPaneEntry, ChannelThreadItem,
        FORUM_POST_CARD_HEIGHT, FocusPane, MUTE_ACTION_DURATIONS,
    },
    popups::ChannelLeaderActionState,
    presentation::{is_direct_message_channel, sort_channels, sort_direct_message_channels},
    scroll::{
        clamp_list_viewport, clamp_selected_index, pane_content_height, toggle_collapsed_key,
    },
};
use crate::discord::AppCommand;
use crate::tui::fuzzy::{FuzzyMatchQuality, FuzzyScore, fuzzy_name_match_score};

const RECENT_CHANNEL_LIMIT: usize = 10;

impl DashboardState {
    pub fn open_selected_channel_actions(&mut self) {
        if self.navigation.focus != FocusPane::Channels {
            return;
        }
        let Some(channel_id) = self.selected_channel_action_target_id() else {
            return;
        };
        self.open_channel_actions(channel_id);
    }

    fn open_channel_actions(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        if channel.is_thread() {
            return;
        }
        self.popups.channel_leader_action = Some(ChannelLeaderActionState::Actions {
            channel_id,
            selected: 0,
        });
    }

    pub fn close_channel_leader_action(&mut self) {
        self.popups.channel_leader_action = None;
    }

    pub fn back_channel_leader_action(&mut self) -> bool {
        match self.popups.channel_leader_action.as_ref() {
            Some(
                ChannelLeaderActionState::Threads { channel_id, .. }
                | ChannelLeaderActionState::MuteDuration { channel_id, .. },
            ) => {
                let channel_id = *channel_id;
                self.popups.channel_leader_action = Some(ChannelLeaderActionState::Actions {
                    channel_id,
                    selected: 0,
                });
                true
            }
            _ => false,
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.popups.channel_leader_action.as_ref() {
            Some(ChannelLeaderActionState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return Vec::new();
        };
        if channel.is_category() {
            return vec![ChannelActionItem {
                kind: ChannelActionKind::ToggleMute,
                label: if self.discord.cache.channel_notification_muted(channel_id) {
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
        let active_channel_has_unread_snapshot = self.navigation.active_channel_id
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
        let mut actions = Vec::new();
        if channel.is_voice()
            && let Some(guild_id) = channel.guild_id
        {
            let joined_here = self.runtime.voice_connection.is_some_and(|voice| {
                voice.guild_id == guild_id && voice.channel_id == Some(channel_id)
            });
            actions.push(ChannelActionItem {
                kind: if joined_here {
                    ChannelActionKind::LeaveVoice
                } else {
                    ChannelActionKind::JoinVoice
                },
                label: if joined_here {
                    "Leave voice".to_owned()
                } else {
                    "Join voice".to_owned()
                },
                enabled: joined_here || self.discord.cache.can_connect_voice_channel(channel),
            });
        }
        actions.extend([
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
                label: if self.discord.cache.channel_notification_muted(channel_id) {
                    "Unmute channel".to_owned()
                } else {
                    "Mute channel".to_owned()
                },
                enabled: true,
            },
        ]);
        actions
    }

    pub fn selected_channel_mute_duration_items(&self) -> &'static [super::MuteActionDurationItem] {
        &MUTE_ACTION_DURATIONS
    }

    pub fn channel_action_thread_items(&self) -> Vec<ChannelThreadItem> {
        let channel_id = match self.popups.channel_leader_action.as_ref() {
            Some(ChannelLeaderActionState::Threads { channel_id, .. }) => *channel_id,
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
        let Some(list) = self.requests.forum_post_lists.get(&channel.id) else {
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
        !self.requests.forum_post_lists.contains_key(&channel.id)
    }

    pub fn visible_forum_post_items(&self) -> Vec<ChannelThreadItem> {
        let height = self.message_content_height();
        let mut rows = 0usize;
        let mut visible = Vec::new();
        for post in self
            .selected_forum_post_items()
            .into_iter()
            .skip(self.messages.message_scroll)
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
            self.messages.selected_message,
            self.selected_forum_post_items().len(),
        )
    }

    pub fn focused_forum_post_selection(&self) -> Option<usize> {
        if self.navigation.focus != FocusPane::Messages || !self.selected_channel_is_forum() {
            return None;
        }
        let selected = self.selected_forum_post();
        let visible_count = self.visible_forum_post_items().len();
        if visible_count > 0
            && selected >= self.messages.message_scroll
            && selected < self.messages.message_scroll + visible_count
        {
            Some(selected - self.messages.message_scroll)
        } else {
            None
        }
    }

    pub(super) fn select_visible_forum_post_row(&mut self, row: usize) -> bool {
        let mut rendered_row = 0usize;
        for (visible_index, post) in self.visible_forum_post_items().into_iter().enumerate() {
            if post.section_label.is_some() {
                if row == rendered_row {
                    return false;
                }
                rendered_row = rendered_row.saturating_add(1);
            }
            if row < rendered_row.saturating_add(FORUM_POST_CARD_HEIGHT) {
                let index = self.messages.message_scroll.saturating_add(visible_index);
                if index >= self.selected_forum_post_items().len() {
                    return false;
                }
                self.messages.selected_message = index;
                self.messages.message_auto_follow = false;
                self.messages.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(FORUM_POST_CARD_HEIGHT);
        }
        false
    }

    pub(super) fn clamp_forum_post_viewport(&mut self) {
        let posts = self.selected_forum_post_items();
        if posts.is_empty() {
            self.messages.message_scroll = 0;
            return;
        }

        let selected = self.messages.selected_message.min(posts.len() - 1);
        self.messages.message_scroll = self.messages.message_scroll.min(selected);
        let height = self.message_content_height().max(1);
        while self.messages.message_scroll < selected {
            let rendered_rows: usize = posts[self.messages.message_scroll..=selected]
                .iter()
                .map(|post| post.rendered_height())
                .sum();
            if rendered_rows <= height {
                break;
            }
            self.messages.message_scroll = self.messages.message_scroll.saturating_add(1);
        }
    }

    pub fn selected_channel_is_forum(&self) -> bool {
        self.selected_channel_state()
            .is_some_and(|channel| channel.is_forum())
    }

    pub fn selected_message_history_channel_id(&self) -> Option<Id<ChannelMarker>> {
        (!self.selected_channel_is_forum()).then_some(self.selected_channel_id()?)
    }

    pub fn selected_message_history_needs_reload(&self) -> bool {
        self.selected_message_history_channel_id()
            .is_some_and(|channel_id| {
                self.discord
                    .cache
                    .channel_message_bodies_are_cold(channel_id)
            })
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
                self.forum_thread_item(thread, None, thread.thread_archived().unwrap_or(false))
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
        //  2. The server-side `sort_by=last_message_time` index can be stale.
        //     Posts with newer messages sometimes sit below older ones. The
        //     `last_message_id` snowflake encodes the actual message
        //     timestamp, and we keep it fresh via gateway updates, so a local
        //     resort by that field tracks Discord's UI more closely.
        let (mut pinned, mut rest): (Vec<_>, Vec<_>) = post_ids
            .iter()
            .filter_map(|post_id| self.discord.cache.channel(*post_id))
            .filter(|post| {
                post.is_thread()
                    && post.parent_id == Some(forum_channel_id)
                    && self.discord.cache.can_view_channel(post)
            })
            .partition(|post| post.thread_pinned().unwrap_or(false));
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
        let messages = self.discord.messages_for_channel(channel.id);
        let is_forum_post = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        let preview = if is_forum_post {
            messages
                .into_iter()
                .find(|message| message.id.get() == channel.id.get())
        } else {
            messages.into_iter().next()
        };
        let deleted_starter_creator = (is_forum_post && preview.is_none())
            .then(|| self.discord.cache.thread_creator(channel.id))
            .flatten();
        let deleted_starter_author_id = deleted_starter_creator.map(|creator| creator.user_id);
        let deleted_starter_author = deleted_starter_creator.map(|creator| {
            creator
                .guild_id
                .or(channel.guild_id)
                .and_then(|guild_id| {
                    self.discord
                        .cache
                        .member_display_name(guild_id, creator.user_id)
                })
                .map(str::to_owned)
                .unwrap_or_else(|| format!("user-{}", creator.user_id.get()))
        });
        let deleted_starter_author_color = deleted_starter_creator.and_then(|creator| {
            creator.guild_id.or(channel.guild_id).and_then(|guild_id| {
                self.discord
                    .cache
                    .user_role_color(guild_id, creator.user_id)
            })
        });
        ChannelThreadItem {
            channel_id: channel.id,
            section_label,
            label: channel.name.clone(),
            archived,
            locked: channel.thread_locked().unwrap_or(false),
            pinned: channel.thread_pinned().unwrap_or(false),
            preview_author_id: preview
                .map(|message| message.author_id)
                .or(deleted_starter_author_id),
            preview_author: preview
                .map(|message| message.author.clone())
                .or(deleted_starter_author),
            preview_author_color: preview
                .and_then(|message| self.message_author_role_color(message))
                .or(deleted_starter_author_color),
            preview_content: preview
                .map(|message| {
                    if is_forum_post && message.content.is_none() && message.attachments.is_empty()
                    {
                        "original message deleted".to_owned()
                    } else {
                        self.thread_message_preview_text(message)
                    }
                })
                .or_else(|| {
                    deleted_starter_author_id.map(|_| "original message deleted".to_owned())
                }),
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
        let Some(list) = self.requests.forum_post_lists.get(&channel_id) else {
            return false;
        };
        if !list.has_more {
            return false;
        }
        let visible_bottom = self
            .messages
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
        match self.popups.channel_leader_action.as_ref()? {
            ChannelLeaderActionState::Actions { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_action_items().len(),
            )),
            ChannelLeaderActionState::MuteDuration { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_mute_duration_items().len(),
            )),
            ChannelLeaderActionState::Threads { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.channel_action_thread_items().len(),
            )),
        }
    }

    pub fn select_channel_action_row(&mut self, row: usize) -> bool {
        let len = match self.popups.channel_leader_action.as_ref() {
            Some(ChannelLeaderActionState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelLeaderActionState::MuteDuration { .. }) => {
                self.selected_channel_mute_duration_items().len()
            }
            Some(ChannelLeaderActionState::Threads { .. }) => {
                self.channel_action_thread_items().len()
            }
            None => return false,
        };
        if row >= len {
            return false;
        }
        if let Some(action) = self.popups.channel_leader_action.as_mut() {
            let selected = match action {
                ChannelLeaderActionState::Actions { selected, .. }
                | ChannelLeaderActionState::MuteDuration { selected, .. }
                | ChannelLeaderActionState::Threads { selected, .. } => selected,
            };
            *selected = row;
            return true;
        }
        false
    }

    pub fn activate_selected_channel_action(&mut self) -> Option<AppCommand> {
        let action = self.popups.channel_leader_action.clone()?;
        match action {
            ChannelLeaderActionState::Actions {
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
                        self.enter_pinned_message_view(channel_id);
                        self.close_channel_leader_action();
                        None
                    }
                    ChannelActionKind::ShowThreads => {
                        self.popups.channel_leader_action =
                            Some(ChannelLeaderActionState::Threads {
                                channel_id,
                                selected: 0,
                            });
                        None
                    }
                    ChannelActionKind::MarkAsRead => {
                        self.mark_channel_as_read(channel_id);
                        self.close_channel_leader_action();
                        // `mark_channel_as_read` already queued the
                        // `AckChannel` command via `queue_channel_ack`, so
                        // there's nothing extra for the dispatch loop here.
                        None
                    }
                    ChannelActionKind::ToggleMute => {
                        if self.discord.cache.channel_notification_muted(channel_id) {
                            self.close_channel_leader_action();
                            self.toggle_channel_mute(channel_id, None)
                        } else {
                            self.popups.channel_leader_action =
                                Some(ChannelLeaderActionState::MuteDuration {
                                    channel_id,
                                    selected: 0,
                                });
                            None
                        }
                    }
                }
            }
            ChannelLeaderActionState::MuteDuration {
                channel_id,
                selected,
            } => {
                let item =
                    self.selected_channel_mute_duration_items()
                        .get(clamp_selected_index(
                            selected,
                            self.selected_channel_mute_duration_items().len(),
                        ))?;
                self.close_channel_leader_action();
                self.toggle_channel_mute(channel_id, Some(item.duration))
            }
            ChannelLeaderActionState::Threads { .. } => {
                let items = self.channel_action_thread_items();
                let index = self.selected_channel_action_index()?;
                let item = items.get(index)?.clone();
                let guild_id = self
                    .discord
                    .channel(item.channel_id)
                    .and_then(|c| c.guild_id);
                self.activate_channel(item.channel_id);
                self.close_channel_leader_action();
                guild_id.map(|guild_id| AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id: item.channel_id,
                })
            }
        }
    }

    pub fn activate_channel_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let shortcut = shortcut.to_ascii_lowercase();
        match self.popups.channel_leader_action.as_ref()? {
            ChannelLeaderActionState::Actions { .. } => {
                let actions = self.selected_channel_action_items();
                let index = actions.iter().enumerate().position(|(index, action)| {
                    action.enabled
                        && self
                            .options
                            .key_bindings()
                            .channel_action_shortcuts(&actions, index)
                            .contains(&shortcut)
                })?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelLeaderActionState::MuteDuration { .. } => {
                let index = self
                    .selected_channel_mute_duration_items()
                    .iter()
                    .enumerate()
                    .position(|(index, _)| {
                        self.options.key_bindings().indexed_shortcut(index) == Some(shortcut)
                    })?;
                self.select_channel_action_row(index);
                self.activate_selected_channel_action()
            }
            ChannelLeaderActionState::Threads { .. } => {
                let threads = self.channel_action_thread_items();
                let index = threads.iter().enumerate().position(|(index, _)| {
                    self.options.key_bindings().indexed_shortcut(index) == Some(shortcut)
                })?;
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
        match self.navigation.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            // DMs do not carry guild-style permissions, so show every channel.
            ActiveGuildScope::DirectMessages => self.discord.cache.channels_for_guild(None),
            // Filter to channels we have VIEW_CHANNEL on, otherwise the
            // sidebar surfaces channels that REST refuses with 403.
            ActiveGuildScope::Guild(guild_id) => self
                .discord
                .cache
                .viewable_channels_for_guild(Some(guild_id)),
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        // Threads are reached through channel Leader Actions instead of
        // appearing as top-level entries. Without this filter their parent
        // channel would not be in `category_ids`, so the roots filter below
        // would let them through and render them under the channel list.
        channels.retain(|channel| !channel.is_thread());
        if self.navigation.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels
                .into_iter()
                .map(|state| ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                })
                .collect();
        }

        let voice_participants_by_channel = match self.navigation.active_guild {
            ActiveGuildScope::Guild(guild_id) => self
                .discord
                .voice_participants_by_channel_for_guild(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => BTreeMap::new(),
        };

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
                self.push_channel_pane_channel_entry(
                    &mut entries,
                    root,
                    ChannelBranch::None,
                    &voice_participants_by_channel,
                );
                continue;
            }

            let collapsed = self
                .navigation
                .collapsed_channel_categories
                .contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            if collapsed {
                children.retain(|child| self.collapsed_category_child_visible(child));
            }
            let last_child_index = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    ChannelBranch::Last
                } else {
                    ChannelBranch::Middle
                };
                self.push_channel_pane_channel_entry(
                    &mut entries,
                    child,
                    branch,
                    &voice_participants_by_channel,
                );
            }
        }

        entries
    }

    fn collapsed_category_child_visible(&self, channel: &ChannelState) -> bool {
        self.navigation.active_channel_id == Some(channel.id)
            || self.sidebar_channel_unread(channel.id) != ChannelUnreadState::Seen
    }

    fn push_channel_pane_channel_entry<'a>(
        &'a self,
        entries: &mut Vec<ChannelPaneEntry<'a>>,
        state: &'a ChannelState,
        branch: ChannelBranch,
        voice_participants_by_channel: &BTreeMap<Id<ChannelMarker>, Vec<VoiceParticipantState>>,
    ) {
        entries.push(ChannelPaneEntry::Channel { state, branch });
        if !state.is_voice() {
            return;
        }
        let Some(participants) = voice_participants_by_channel.get(&state.id) else {
            return;
        };
        entries.extend(participants.iter().cloned().map(|participant| {
            ChannelPaneEntry::VoiceParticipant {
                participant,
                parent_branch: branch,
            }
        }));
    }

    /// Returns channel pane entries filtered by the active pane filter query,
    /// or all entries if no filter is active. Category headers are omitted when
    /// a query is present so results appear as a flat list of matching channels.
    pub fn channel_pane_filtered_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let query = self
            .navigation
            .channel_pane_filter
            .as_ref()
            .map(|f| f.query.trim().to_owned())
            .filter(|q| !q.is_empty());
        let Some(query) = query else {
            return self.channel_pane_entries();
        };
        // Search directly over channels() so children inside collapsed
        // categories are included in results even when not normally visible.
        let mut scored: Vec<(FuzzyMatchQuality, FuzzyScore, usize, &ChannelState)> = self
            .channel_pane_search_channels()
            .into_iter()
            .enumerate()
            .filter_map(|(index, channel)| {
                if channel.is_thread() || channel.is_category() {
                    return None;
                }
                fuzzy_name_match_score(&channel.name, &query)
                    .map(|(quality, score)| (quality, score, index, channel))
            })
            .collect();
        scored
            .sort_by_key(|(quality, score, original_index, _)| (*quality, *score, *original_index));
        scored
            .into_iter()
            .map(|(_, _, _, state)| ChannelPaneEntry::Channel {
                state,
                branch: ChannelBranch::None,
            })
            .collect()
    }

    fn channel_pane_search_channels(&self) -> Vec<&ChannelState> {
        let mut channels = self.channels();
        channels.retain(|channel| !channel.is_thread());
        if self.navigation.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels;
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

        let mut search_channels = Vec::new();
        for root in roots {
            if !root.is_category() {
                search_channels.push(root);
                continue;
            }

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            search_channels.extend(children);
        }
        search_channels
    }

    pub fn confirm_channel_pane_filter(&mut self) -> Option<AppCommand> {
        let selected = self.selected_channel();
        let channel_id = {
            let entries = self.channel_pane_filtered_entries();
            match entries.get(selected) {
                Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
                _ => None,
            }
        };
        if let Some(channel_id) = channel_id {
            let command = self.activate_channel_command(channel_id);
            self.navigation.channel_keep_selection_visible = true;
            return command;
        }
        None
    }

    pub fn selected_channel(&self) -> usize {
        let entries = self.channel_pane_filtered_entries();
        self.selected_channel_from_entries(&entries)
    }

    pub(in crate::tui) fn selected_channel_from_entries(
        &self,
        entries: &[ChannelPaneEntry<'_>],
    ) -> usize {
        selectable_channel_index_near(entries, self.navigation.selected_channel, false).unwrap_or(0)
    }

    pub(super) fn move_channel_selection_down(&mut self) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_add(1), true);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_up(&mut self) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_sub(1), false);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_down_by(&mut self, distance: usize) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_add(distance), true);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_up_by(&mut self, distance: usize) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_sub(distance), false);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    pub(super) fn jump_channel_selection_top(&mut self) {
        self.select_channel_entry_near(0, true);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    pub(super) fn jump_channel_selection_bottom(&mut self) {
        let entries = self.channel_pane_filtered_entries();
        self.navigation.selected_channel = entries
            .iter()
            .rposition(ChannelPaneEntry::is_selectable)
            .unwrap_or(0);
        self.navigation.channel_keep_selection_visible = true;
        self.clamp_channel_viewport();
    }

    fn select_channel_entry_near(&mut self, index: usize, prefer_forward: bool) {
        let entries = self.channel_pane_filtered_entries();
        self.navigation.selected_channel =
            selectable_channel_index_near(&entries, index, prefer_forward).unwrap_or(0);
    }

    pub(super) fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(
                ChannelPaneEntry::CategoryHeader { .. } | ChannelPaneEntry::VoiceParticipant { .. },
            )
            | None => None,
        }
    }

    pub fn channel_scroll(&self) -> usize {
        self.navigation.channel_scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_filtered_entries()
            .into_iter()
            .skip(self.navigation.channel_scroll)
            .take(pane_content_height(self.navigation.channel_view_height))
            .collect()
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        self.navigation.channel_view_height = height;
        let height = pane_content_height(self.navigation.channel_view_height);
        let len = self.channel_pane_filtered_entries().len();
        clamp_list_viewport(
            self.navigation.selected_channel,
            &mut self.navigation.channel_scroll,
            height,
            len,
            self.navigation.channel_keep_selection_visible,
        );
    }

    pub(super) fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self.channel_pane_entries().iter().position(|entry| {
            matches!(entry, ChannelPaneEntry::Channel { state, .. } if state.id == channel_id)
        }) {
            self.navigation.selected_channel = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.navigation.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.navigation
            .active_channel_id
            .and_then(|channel_id| self.discord.cache.channel(channel_id))
    }

    /// Builds the "X is typing…" line for the currently selected channel, or
    /// `None` when nobody is typing (or the only typer is us). Resolution
    /// order for each user: transient typing display name ->cached guild
    /// member alias ->DM recipient display name ->`user-{id}` fallback. Caps
    /// at three names and collapses to "Several people are typing…" beyond
    /// that.
    pub fn typing_footer_for_selected_channel(&self) -> Option<String> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.discord.cache.channel(channel_id)?;
        let guild_id = channel.guild_id;
        let typers: Vec<TypingUserState> = self
            .discord
            .typing_users(channel_id)
            .into_iter()
            .filter(|typer| Some(typer.user_id) != self.discord.current_user_id)
            .collect();
        if typers.is_empty() {
            return None;
        }

        let resolve_name = |typer: TypingUserState| -> String {
            if let Some(name) = typer.display_name {
                return name;
            }
            let user_id = typer.user_id;
            if let Some(name) = guild_id
                .and_then(|guild_id| self.discord.cache.member_display_name(guild_id, user_id))
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
        let names: Vec<String> = typers.iter().take(3).cloned().map(resolve_name).collect();
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
            .cache
            .channel(channel_id)
            .map(|channel| match channel.kind.as_str() {
                "dm" | "Private" => format!("@{}", channel.name),
                "group-dm" | "Group" => channel.name.clone(),
                "category" | "GuildCategory" => channel.name.clone(),
                _ => format!("#{}", channel.name),
            })
            .unwrap_or_else(|| format!("#channel-{}", channel_id.get()))
    }

    pub fn active_voice_connection_label(&self) -> Option<String> {
        let (guild_id, channel_id, other_client) =
            if let Some(voice) = self.runtime.voice_connection {
                (voice.guild_id, voice.channel_id?, false)
            } else {
                let voice = self.discord.current_user_voice_connection()?;
                (voice.guild_id, voice.channel_id, true)
            };
        let guild = self
            .guild_name(guild_id)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("guild-{}", guild_id.get()));
        let channel = self
            .discord
            .channel(channel_id)
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| format!("channel-{}", channel_id.get()));
        let suffix = if other_client { " (other client)" } else { "" };
        Some(format!("{guild} - {channel}{suffix}"))
    }

    pub fn current_voice_self_status(&self) -> (bool, bool) {
        let remote_status = self
            .discord
            .current_user_voice_connection()
            .map(|voice| (voice.self_mute, voice.self_deaf))
            .unwrap_or((false, false));
        (
            self.options.voice_options.self_mute || remote_status.0,
            self.options.voice_options.self_deaf || remote_status.1,
        )
    }

    pub fn is_joined_voice_channel(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.runtime
            .voice_connection
            .and_then(|voice| voice.channel_id)
            .is_some_and(|voice_channel_id| voice_channel_id == channel_id)
    }

    fn toggle_channel_mute(
        &mut self,
        channel_id: Id<ChannelMarker>,
        duration: Option<crate::discord::MuteDuration>,
    ) -> Option<AppCommand> {
        let channel = self.discord.cache.channel(channel_id)?;
        let muted = !self.discord.cache.channel_notification_muted(channel_id);
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
        if self.messages.pinned_message_view_channel_id == Some(channel_id) {
            format!("{label} pinned messages")
        } else {
            label
        }
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } if Some(state.id) == self.navigation.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        toggle_collapsed_key(
            &mut self.navigation.collapsed_channel_categories,
            category_id,
        );
        self.options.options_save_pending = true;
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
                self.activate_channel_command(state.id)
            }
            Some(ChannelPaneEntry::VoiceParticipant { .. }) => None,
            None => None,
        }
    }

    fn activate_channel_command(&mut self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let command = {
            let state = self.discord.cache.channel(channel_id)?;
            if is_direct_message_channel(state) {
                Some(AppCommand::SubscribeDirectMessage { channel_id })
            } else {
                state
                    .guild_id
                    .map(|guild_id| AppCommand::SubscribeGuildChannel {
                        guild_id,
                        channel_id,
                    })
            }
        };
        self.activate_channel(channel_id);
        command
    }

    pub(super) fn record_thread_return_target(&mut self, thread_channel_id: Id<ChannelMarker>) {
        let Some(channel_id) = self.navigation.active_channel_id else {
            return;
        };
        if channel_id == thread_channel_id {
            return;
        }
        self.messages.thread_return_target = Some(ThreadReturnTarget {
            thread_channel_id,
            channel_id,
            selected_message: self.messages.selected_message,
            message_scroll: self.messages.message_scroll,
            message_line_scroll: self.messages.message_line_scroll,
            message_keep_selection_visible: self.messages.message_keep_selection_visible,
            message_auto_follow: self.messages.message_auto_follow,
            new_messages_marker_message_id: self.messages.new_messages_marker_message_id,
            unread_divider_last_acked_id: self.messages.unread_divider_last_acked_id,
            pending_unread_anchor_scroll: self.messages.pending_unread_anchor_scroll,
        });
    }

    pub fn return_from_opened_thread(&mut self) -> bool {
        let Some(target) = self.messages.thread_return_target else {
            return false;
        };
        if self.navigation.active_channel_id != Some(target.thread_channel_id) {
            return false;
        }
        if !self
            .selected_channel_state()
            .is_some_and(|channel| channel.is_thread())
        {
            self.messages.thread_return_target = None;
            return false;
        }
        if self.discord.cache.channel(target.channel_id).is_none() {
            self.messages.thread_return_target = None;
            return false;
        }

        self.activate_channel(target.channel_id);
        self.messages.selected_message = target.selected_message;
        self.messages.message_scroll = target.message_scroll;
        self.messages.message_line_scroll = target.message_line_scroll;
        self.messages.message_keep_selection_visible = target.message_keep_selection_visible;
        self.messages.message_auto_follow = target.message_auto_follow;
        self.messages.new_messages_marker_message_id = target.new_messages_marker_message_id;
        self.messages.unread_divider_last_acked_id = target.unread_divider_last_acked_id;
        self.messages.pending_unread_anchor_scroll = target.pending_unread_anchor_scroll;
        self.messages.thread_return_target = None;
        self.clamp_message_viewport();
        true
    }

    pub(super) fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.record_recent_channel(channel_id);
        let is_forum = self
            .discord
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum());
        let preserves_thread_return = self.messages.thread_return_target.is_some_and(|target| {
            self.navigation.active_channel_id == Some(target.channel_id)
                && channel_id == target.thread_channel_id
        });
        if !preserves_thread_return {
            self.messages.thread_return_target = None;
        }
        self.navigation.active_channel_id = Some(channel_id);
        self.messages.pinned_message_view_channel_id = None;
        self.messages.pinned_message_view_return_target = None;

        // Capture the unread anchor BEFORE acking. The Discord-style red
        // divider sits just above the first message newer than this
        // snapshot, and the viewport tries to open at the user's last-read
        // position. Capturing the snapshot rather than a resolved index
        // means the divider still appears once history arrives later.
        let last_acked_snapshot = if is_forum {
            None
        } else {
            self.discord.cache.channel_last_acked_message_id(channel_id)
        };
        let has_unread = last_acked_snapshot.is_some_and(|acked| {
            self.discord
                .cache
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id)
                .is_some_and(|latest| latest > acked)
        });

        self.clear_new_messages_marker();
        self.messages.message_line_scroll = 0;

        if has_unread {
            self.messages.unread_divider_last_acked_id = last_acked_snapshot;
            self.messages.pending_unread_anchor_scroll = true;
            self.messages.message_auto_follow = false;
            // Disable selection-keep until the snap lands. Otherwise the
            // centering pass in `clamp_message_viewport_for_image_previews`
            // would pull the viewport to the latest message before the
            // snap can pin it to the last-read anchor.
            self.messages.message_keep_selection_visible = false;
        } else {
            self.messages.unread_divider_last_acked_id = None;
            self.messages.pending_unread_anchor_scroll = false;
            self.messages.message_auto_follow = !is_forum;
            self.messages.message_keep_selection_visible = true;
        }

        self.messages.selected_message = if is_forum {
            0
        } else {
            self.messages().len().saturating_sub(1)
        };
        self.messages.message_scroll = 0;

        // If the unread anchor's last-read message is already loaded, snap
        // the viewport to it now so the first frame opens at the right
        // spot. Otherwise the snap will be retried each frame inside
        // `clamp_message_viewport_for_image_previews` until history
        // arrives.
        self.try_apply_unread_anchor_scroll();

        self.clamp_message_viewport();
        if is_forum {
            self.queue_forum_acks(channel_id);
        } else {
            self.queue_channel_ack(channel_id);
        }

        self.refresh_composer_emoji_candidates_for_current_query();
    }

    fn record_recent_channel(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        if channel.is_category() || channel.is_thread() {
            return;
        }

        self.navigation
            .recent_channel_ids
            .retain(|id| *id != channel_id);
        self.navigation.recent_channel_ids.push_front(channel_id);
        self.navigation
            .recent_channel_ids
            .truncate(RECENT_CHANNEL_LIMIT);
    }

    /// Ack the channel up to its latest message and retire the unread
    /// divider/banner immediately so the visible cue matches the new
    /// fully-read state. Use this for explicit user actions like
    /// "Mark as read" because activation already runs `queue_channel_ack` on its
    /// own.
    pub fn mark_channel_as_read(&mut self, channel_id: Id<ChannelMarker>) {
        if self
            .discord
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum())
        {
            self.queue_forum_acks(channel_id);
        } else {
            self.queue_channel_ack(channel_id);
        }
        if self.navigation.active_channel_id == Some(channel_id) {
            self.messages.unread_divider_last_acked_id = None;
            self.messages.pending_unread_anchor_scroll = false;
            self.clear_new_messages_marker();
        }
    }

    fn queue_forum_acks(&mut self, forum_id: Id<ChannelMarker>) {
        let mut targets = Vec::new();
        if let Some(message_id) = self.discord.cache.channel_ack_target(forum_id) {
            targets.push((forum_id, message_id));
        }
        targets.extend(self.discord.cache.forum_child_ack_targets(forum_id));
        if targets.is_empty() {
            return;
        }

        self.queue_ack_channels_command(targets);
    }

    /// Optimistic local ack + queued REST POST so the unread badge clears
    /// immediately on activation.
    pub(super) fn queue_channel_ack(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(message_id) = self.discord.cache.channel_ack_target(channel_id) else {
            return;
        };
        self.queue_ack_channel_command(channel_id, message_id);
    }

    pub(super) fn schedule_channel_ack(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(message_id) = self.discord.cache.channel_ack_target(channel_id) else {
            return;
        };
        self.queue_scheduled_ack_channel_command(channel_id, message_id);
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
            Some(ChannelPaneEntry::VoiceParticipant { parent_branch, .. })
                if parent_branch.is_category_child() =>
            {
                entries
                    .get(..selected)?
                    .iter()
                    .rev()
                    .find_map(|entry| match entry {
                        ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
                        _ => None,
                    })
            }
            _ => None,
        }
    }

    fn selected_channel_action_target_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::VoiceParticipant { .. }) => None,
            None => None,
        }
    }
}

fn selectable_channel_index_near(
    entries: &[ChannelPaneEntry<'_>],
    index: usize,
    prefer_forward: bool,
) -> Option<usize> {
    if entries.is_empty() {
        return None;
    }
    let index = index.min(entries.len() - 1);
    if entries[index].is_selectable() {
        return Some(index);
    }
    if prefer_forward {
        entries
            .iter()
            .enumerate()
            .skip(index.saturating_add(1))
            .find_map(|(index, entry)| entry.is_selectable().then_some(index))
            .or_else(|| {
                entries
                    .iter()
                    .enumerate()
                    .take(index)
                    .rev()
                    .find_map(|(index, entry)| entry.is_selectable().then_some(index))
            })
    } else {
        entries
            .iter()
            .enumerate()
            .take(index)
            .rev()
            .find_map(|(index, entry)| entry.is_selectable().then_some(index))
            .or_else(|| {
                entries
                    .iter()
                    .enumerate()
                    .skip(index.saturating_add(1))
                    .find_map(|(index, entry)| entry.is_selectable().then_some(index))
            })
    }
}

fn sort_thread_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| std::cmp::Reverse(channel.id));
}
