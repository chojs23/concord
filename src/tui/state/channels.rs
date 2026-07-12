use std::{collections::BTreeMap, time::Instant};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{ChannelState, ChannelUnreadState, TypingUserState, VoiceParticipantState};

use super::{ActiveGuildScope, DashboardState, MessagePaneSource, ThreadReturnTarget};
use super::{
    channel_tree,
    model::{AppliedForumTag, ChannelBranch, ChannelPaneEntry, ChannelThreadItem, FocusPane},
    presentation::{is_direct_message_channel, sort_direct_message_channels},
    scroll::{clamp_selected_index, toggle_collapsed_key},
};
use crate::discord::AppCommand;
use crate::tui::fuzzy::{FuzzyMatchQuality, FuzzyScore, fuzzy_name_match_score};

const RECENT_CHANNEL_LIMIT: usize = 10;

impl DashboardState {
    pub fn selected_forum_post_items(&self) -> Vec<ChannelThreadItem> {
        let Some(MessagePaneSource::ForumPosts { channel_id }) = self.message_pane_source() else {
            return Vec::new();
        };
        let Some(channel) = self
            .discord
            .cache
            .channel(channel_id)
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
        // Both the forum post list and a channel's thread list fetch through the
        // same `/threads/search` request, so the "loading" placeholder applies
        // to either until the first page lands.
        let channel_id = match self.message_pane_source() {
            Some(
                MessagePaneSource::ForumPosts { channel_id }
                | MessagePaneSource::ChannelThreads { channel_id },
            ) => channel_id,
            _ => return false,
        };
        !self.requests.forum_post_lists.contains_key(&channel_id)
    }

    /// Card items for the message pane (forum posts or a channel's thread list);
    /// empty for any other source.
    pub fn selected_thread_card_items(&self) -> Vec<ChannelThreadItem> {
        match self.message_pane_source() {
            Some(MessagePaneSource::ForumPosts { .. }) => self.selected_forum_post_items(),
            Some(MessagePaneSource::ChannelThreads { channel_id }) => {
                self.channel_thread_card_items(channel_id)
            }
            _ => Vec::new(),
        }
    }

    pub fn visible_thread_card_items(&self) -> Vec<ChannelThreadItem> {
        self.visible_thread_card_items_from(&self.selected_thread_card_items())
    }

    fn visible_thread_card_items_from(
        &self,
        items: &[ChannelThreadItem],
    ) -> Vec<ChannelThreadItem> {
        let height = self.message_content_height();
        let mut rows = 0usize;
        let mut visible = Vec::new();
        for post in items.iter().skip(self.messages.message_scroll) {
            let rendered_height = post.rendered_height();
            if !visible.is_empty() && rows.saturating_add(rendered_height) > height {
                break;
            }
            rows = rows.saturating_add(rendered_height);
            visible.push(post.clone());
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

    pub fn selected_thread_card(&self) -> usize {
        clamp_selected_index(
            self.messages.selected_message,
            self.selected_thread_card_items().len(),
        )
    }

    pub fn focused_thread_card_selection(&self) -> Option<usize> {
        if self.navigation.focus != FocusPane::Messages || !self.message_pane_uses_thread_cards() {
            return None;
        }
        let selected = self.selected_thread_card();
        let visible_count = self.visible_thread_card_items().len();
        if visible_count > 0
            && selected >= self.messages.message_scroll
            && selected < self.messages.message_scroll + visible_count
        {
            Some(selected - self.messages.message_scroll)
        } else {
            None
        }
    }

    pub(super) fn select_visible_thread_card_row(&mut self, row: usize) -> bool {
        let items = self.selected_thread_card_items();
        let mut rendered_row = 0usize;
        for (visible_index, post) in self
            .visible_thread_card_items_from(&items)
            .into_iter()
            .enumerate()
        {
            if post.section_label.is_some() {
                if row == rendered_row {
                    return false;
                }
                rendered_row = rendered_row.saturating_add(1);
            }
            if row < rendered_row.saturating_add(post.card_height()) {
                let index = self.messages.message_scroll.saturating_add(visible_index);
                if index >= items.len() {
                    return false;
                }
                self.messages.selected_message = index;
                self.messages.message_auto_follow = false;
                self.messages.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(post.card_height());
        }
        false
    }

    pub(super) fn clamp_thread_card_viewport(&mut self) {
        let posts = self.selected_thread_card_items();
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

    pub fn selected_message_history_channel_id(&self) -> Option<Id<ChannelMarker>> {
        match self.message_pane_source()? {
            MessagePaneSource::ChannelMessages { channel_id } => Some(channel_id),
            MessagePaneSource::PinnedMessages { .. }
            | MessagePaneSource::ForumPosts { .. }
            | MessagePaneSource::ChannelThreads { .. } => None,
        }
    }

    pub fn selected_message_history_needs_reload(&self) -> bool {
        self.selected_message_history_channel_id()
            .is_some_and(|channel_id| {
                self.discord
                    .cache
                    .channel_message_bodies_are_cold(channel_id)
                    || self.selected_message_history_is_stale()
            })
    }

    pub fn selected_message_history_is_stale(&self) -> bool {
        self.selected_message_history_channel_id()
            .is_some_and(|channel_id| self.message_history_refresh.is_stale(channel_id))
    }

    pub fn selected_forum_channel(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        let MessagePaneSource::ForumPosts { channel_id } = self.message_pane_source()? else {
            return None;
        };
        let channel = self.discord.cache.channel(channel_id)?;
        Some((channel.guild_id?, channel_id))
    }

    /// The `(guild, channel)` whose threads should be fetched via
    /// `/threads/search`: a forum showing its posts, or any non-forum channel
    /// whose thread-list view is open. `None` for every other pane source.
    fn thread_card_fetch_channel(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        let channel_id = match self.message_pane_source()? {
            MessagePaneSource::ForumPosts { channel_id }
            | MessagePaneSource::ChannelThreads { channel_id } => channel_id,
            _ => return None,
        };
        let channel = self.discord.cache.channel(channel_id)?;
        Some((channel.guild_id?, channel_id))
    }

    pub fn selected_forum_channel_with_load_more(
        &self,
    ) -> Option<(Id<GuildMarker>, Id<ChannelMarker>, bool)> {
        let (guild_id, channel_id) = self.thread_card_fetch_channel()?;
        Some((
            guild_id,
            channel_id,
            self.should_load_more_forum_posts(channel_id),
        ))
    }

    /// Open the selected card (a thread or forum post) as the active channel.
    pub fn activate_selected_thread_card(&mut self) -> Option<AppCommand> {
        let item = self
            .selected_thread_card_items()
            .get(self.selected_thread_card())?
            .clone();
        self.activate_thread_card_item(item)
    }

    fn activate_thread_card_item(&mut self, item: ChannelThreadItem) -> Option<AppCommand> {
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

    /// Cards for a non-forum channel's thread list. Mirrors the forum post list:
    /// the active and archived sections come from the `/threads/search` fetch
    /// (`forum_post_lists`). Gateway-cached child threads the search has not
    /// returned yet are merged in so joined or freshly created threads show
    /// immediately, even before the fetch lands or if the search endpoint is
    /// unavailable for this channel.
    pub(super) fn channel_thread_card_items(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<ChannelThreadItem> {
        let (mut active_ids, mut archived_ids) = self
            .requests
            .forum_post_lists
            .get(&channel_id)
            .map(|list| (list.active_post_ids.clone(), list.archived_post_ids.clone()))
            .unwrap_or_default();
        for thread in channel_tree::sorted_child_threads(self.channels(), channel_id) {
            if active_ids.contains(&thread.id) || archived_ids.contains(&thread.id) {
                continue;
            }
            if thread.thread_archived().unwrap_or(false) {
                archived_ids.push(thread.id);
            } else {
                active_ids.push(thread.id);
            }
        }
        let mut items =
            self.forum_post_section_items(&active_ids, channel_id, "Active threads", false);
        items.extend(self.forum_post_section_items(
            &archived_ids,
            channel_id,
            "Archived threads",
            true,
        ));
        items
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

    pub(super) fn forum_thread_item(
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
        let applied_tags = self.forum_thread_applied_tags(channel);
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
            applied_tags,
            preview_reactions: preview
                .map(|message| message.reactions.clone())
                .unwrap_or_default(),
            comment_count: channel.message_count.or(channel.total_message_sent),
            new_message_count: self.forum_thread_new_message_count(channel.id),
            last_activity_message_id: channel
                .last_message_id
                .or_else(|| preview.map(|message| message.id)),
        }
    }

    fn forum_thread_applied_tags(&self, channel: &ChannelState) -> Vec<AppliedForumTag> {
        let Some(parent) = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
        else {
            return Vec::new();
        };
        channel
            .applied_tags
            .iter()
            .filter_map(|tag_id| {
                let tag = parent.available_tags.iter().find(|tag| tag.id == *tag_id)?;
                // Discord sends exactly one of the two: a custom tag carries an
                // `emoji_id` (its `emoji_name` is null) and a unicode tag carries
                // the character in `emoji_name` (its `emoji_id` is null).
                let custom_emoji_url = tag.emoji_id.map(|emoji_id| {
                    format!("https://cdn.discordapp.com/emojis/{}.png", emoji_id.get())
                });
                let unicode_emoji = if custom_emoji_url.is_some() {
                    None
                } else {
                    tag.emoji_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_owned)
                };
                Some(AppliedForumTag {
                    name: tag.name.clone(),
                    unicode_emoji,
                    custom_emoji_url,
                })
            })
            .collect()
    }

    fn forum_thread_new_message_count(&self, channel_id: Id<ChannelMarker>) -> usize {
        if self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| channel.is_thread() && !channel.current_user_joined_thread)
        {
            return 0;
        }
        let last_acked = self.discord.cache.channel_last_acked_message_id(channel_id);
        let loaded_count = self
            .discord
            .messages_for_channel(channel_id)
            .into_iter()
            .filter(|message| last_acked.is_none_or(|acked| message.id > acked))
            .count();
        if loaded_count > 0 {
            return loaded_count;
        }

        match self.discord.cache.channel_unread(channel_id) {
            ChannelUnreadState::Mentioned(count) | ChannelUnreadState::Notified(count) => {
                usize::try_from(count).unwrap_or(usize::MAX)
            }
            ChannelUnreadState::Unread => 1,
            ChannelUnreadState::Seen => 0,
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
            .saturating_add(self.visible_thread_card_items().len().max(1))
            .saturating_add(5);
        let selected_bottom = self.selected_forum_post().saturating_add(5);
        let len = list
            .active_post_ids
            .len()
            .saturating_add(list.archived_post_ids.len());
        visible_bottom >= len || selected_bottom >= len
    }

    pub(super) fn selected_channel_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.navigation.guilds.active {
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
        if self.navigation.guilds.active == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            // DMs and group DMs render as a tree: each conversation is a root,
            // with the people currently in its call nested beneath it, mirroring
            // how guild voice channels list their participants.
            let mut entries = Vec::new();
            for state in channels.into_iter().filter(|state| !state.is_thread()) {
                entries.push(ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                });
                let participants = self
                    .discord
                    .voice_participants_for_private_channel(state.id);
                entries.extend(participants.into_iter().map(|participant| {
                    ChannelPaneEntry::VoiceParticipant {
                        participant,
                        parent_branch: ChannelBranch::None,
                    }
                }));
            }
            return entries;
        }

        let voice_participants_by_channel = match self.navigation.guilds.active {
            ActiveGuildScope::Guild(guild_id) => self
                .discord
                .voice_participants_by_channel_for_guild(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => BTreeMap::new(),
        };

        // Group joined threads by parent channel once. Looking them up per entry
        // avoids rescanning every channel for each row, which made sidebar
        // building O(N^2) and stuttered navigation on large guilds.
        let mut joined_threads_by_parent: BTreeMap<Id<ChannelMarker>, Vec<&ChannelState>> =
            BTreeMap::new();
        for channel in &channels {
            // Archived threads stay out of the sidebar even when joined; they
            // live in the thread-list view's "Archived" section instead. The
            // `/threads/search` fetch pulls archived joined threads into the
            // cache, so without this filter they would leak into the pane.
            if channel.is_thread()
                && channel.current_user_joined_thread
                && !channel.thread_archived().unwrap_or(false)
                && let Some(parent_id) = channel.parent_id
            {
                joined_threads_by_parent
                    .entry(parent_id)
                    .or_default()
                    .push(*channel);
            }
        }
        for threads in joined_threads_by_parent.values_mut() {
            channel_tree::sort_thread_channels(threads);
        }

        let mut entries = Vec::new();
        for root in channel_tree::sorted_channel_tree_roots(&channels) {
            if !root.is_category() {
                self.push_channel_pane_channel_entry(
                    &mut entries,
                    root,
                    ChannelBranch::None,
                    &voice_participants_by_channel,
                    &joined_threads_by_parent,
                );
                continue;
            }

            let mut children = channel_tree::sorted_category_children(&channels, root.id);
            if children.is_empty()
                && !self
                    .discord
                    .cache
                    .can_manage_channel_structure_in_channel(root)
            {
                continue;
            }

            let collapsed = self
                .navigation
                .channels
                .collapsed_channel_categories
                .contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });

            if collapsed {
                children.retain(|child| self.collapsed_category_child_visible(child));
            }
            let child_count = children.len();
            for (index, child) in children.into_iter().enumerate() {
                let branch = channel_tree::child_branch(index, child_count);
                self.push_channel_pane_channel_entry(
                    &mut entries,
                    child,
                    branch,
                    &voice_participants_by_channel,
                    &joined_threads_by_parent,
                );
            }
        }

        entries
    }

    fn collapsed_category_child_visible(&self, channel: &ChannelState) -> bool {
        self.navigation.channels.active_channel_id == Some(channel.id)
            || self.sidebar_channel_unread(channel.id) != ChannelUnreadState::Seen
    }

    fn push_channel_pane_channel_entry<'a>(
        &'a self,
        entries: &mut Vec<ChannelPaneEntry<'a>>,
        state: &'a ChannelState,
        branch: ChannelBranch,
        voice_participants_by_channel: &BTreeMap<Id<ChannelMarker>, Vec<VoiceParticipantState>>,
        joined_threads_by_parent: &BTreeMap<Id<ChannelMarker>, Vec<&'a ChannelState>>,
    ) {
        entries.push(ChannelPaneEntry::Channel { state, branch });
        if let Some(threads) = joined_threads_by_parent.get(&state.id) {
            Self::push_joined_thread_entries(entries, threads, branch);
        }
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

    fn push_joined_thread_entries<'a>(
        entries: &mut Vec<ChannelPaneEntry<'a>>,
        threads: &[&'a ChannelState],
        parent_branch: ChannelBranch,
    ) {
        entries.extend(threads.iter().enumerate().map(|(index, &state)| {
            let branch = channel_tree::child_branch(index, threads.len());
            ChannelPaneEntry::Thread {
                state,
                parent_branch,
                branch,
            }
        }));
    }

    /// Returns channel pane entries filtered by the active pane filter query,
    /// or all entries if no filter is active. Category headers are omitted when
    /// a query is present so results appear as a flat list of matching channels.
    pub fn channel_pane_filtered_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let query = self
            .navigation
            .channels
            .filter
            .as_ref()
            .map(|f| f.query().trim().to_owned())
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
                if channel.is_category()
                    || (channel.is_thread() && !channel.current_user_joined_thread)
                {
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
        if self.navigation.guilds.active == ActiveGuildScope::DirectMessages {
            channels.retain(|channel| !channel.is_thread());
            sort_direct_message_channels(&mut channels);
            return channels;
        }

        let mut search_channels = Vec::new();
        for root in channel_tree::sorted_channel_tree_roots(&channels) {
            if !root.is_category() {
                search_channels.push(root);
                continue;
            }

            let children = channel_tree::sorted_category_children(&channels, root.id);
            search_channels.extend(children);
        }
        search_channels
    }

    pub fn confirm_channel_pane_filter(&mut self) -> Option<AppCommand> {
        let selected = self.selected_channel();
        let channel_id = {
            let entries = self.channel_pane_filtered_entries();
            entries.get(selected).and_then(ChannelPaneEntry::channel_id)
        };
        if let Some(channel_id) = channel_id {
            let command = self.activate_channel_command(channel_id);
            self.navigation.channels.list.keep_selection_visible();
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
        selectable_channel_index_near(entries, self.navigation.channels.list.selected, false)
            .unwrap_or(0)
    }

    pub(super) fn move_channel_selection_down(&mut self) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_add(1), true);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_up(&mut self) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_sub(1), false);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_down_by(&mut self, distance: usize) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_add(distance), true);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    pub(super) fn move_channel_selection_up_by(&mut self, distance: usize) {
        let selected = self.selected_channel();
        self.select_channel_entry_near(selected.saturating_sub(distance), false);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    pub(super) fn jump_channel_selection_top(&mut self) {
        self.select_channel_entry_near(0, true);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    pub(super) fn jump_channel_selection_bottom(&mut self) {
        let entries = self.channel_pane_filtered_entries();
        self.navigation.channels.list.selected = entries
            .iter()
            .rposition(ChannelPaneEntry::is_selectable)
            .unwrap_or(0);
        self.navigation.channels.list.keep_selection_visible();
        self.clamp_channel_viewport();
    }

    fn select_channel_entry_near(&mut self, index: usize, prefer_forward: bool) {
        let entries = self.channel_pane_filtered_entries();
        self.navigation.channels.list.selected =
            selectable_channel_index_near(&entries, index, prefer_forward).unwrap_or(0);
    }

    pub(super) fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        self.channel_pane_entries()
            .get(self.selected_channel())
            .and_then(ChannelPaneEntry::channel_id)
    }

    pub fn channel_scroll(&self) -> usize {
        self.navigation.channels.list.scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_filtered_entries()
            .into_iter()
            .skip(self.navigation.channels.list.scroll)
            .take(self.navigation.channels.list.content_height())
            .collect()
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        let len = self.channel_pane_filtered_entries().len();
        let selected = self.navigation.channels.list.selected;
        self.navigation
            .channels
            .list
            .set_view_height_and_clamp(height, selected, len);
    }

    pub(super) fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self
            .channel_pane_entries()
            .iter()
            .position(|entry| entry.channel_id() == Some(channel_id))
        {
            self.navigation.channels.list.selected = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.navigation.channels.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.navigation
            .channels
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
        let (scope, channel_id, other_client) = if let Some(voice) = self.runtime.voice_connection {
            (voice.scope, voice.channel_id?, false)
        } else {
            let voice = self.discord.current_user_voice_connection()?;
            (voice.scope, voice.channel_id, true)
        };
        let channel = self
            .discord
            .channel(channel_id)
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| format!("channel-{}", channel_id.get()));
        let suffix = if other_client { " (other client)" } else { "" };
        // Guild voice shows "guild - channel"; a DM/group-DM call has no guild,
        // so it is labeled under "Direct Messages" instead.
        let prefix = match scope.guild_id() {
            Some(guild_id) => self
                .guild_name(guild_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("guild-{}", guild_id.get())),
            None => "Direct Messages".to_owned(),
        };
        Some(format!("{prefix} - {channel}{suffix}"))
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

    pub(super) fn toggle_channel_mute(
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
        match self.message_pane_source() {
            Some(MessagePaneSource::PinnedMessages { channel_id }) => {
                format!("{} pinned messages", self.channel_label(channel_id))
            }
            Some(MessagePaneSource::ChannelThreads { channel_id }) => {
                format!("Threads · {}", self.channel_label(channel_id))
            }
            Some(source) => self.channel_label(source.channel_id()),
            None => "no channel".to_owned(),
        }
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } | ChannelPaneEntry::Thread { state, .. }
                if Some(state.id) == self.navigation.channels.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        toggle_collapsed_key(
            &mut self.navigation.channels.collapsed_channel_categories,
            category_id,
        );
        self.options.ui_state_save_pending = true;
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
            Some(
                ChannelPaneEntry::Channel { state, .. } | ChannelPaneEntry::Thread { state, .. },
            ) => self.activate_channel_command(state.id),
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
        let Some(channel_id) = self.navigation.channels.active_channel_id else {
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
        if self.navigation.channels.active_channel_id != Some(target.thread_channel_id) {
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
        self.activate_channel_at(channel_id, Instant::now());
    }

    pub(super) fn activate_channel_at(&mut self, channel_id: Id<ChannelMarker>, now: Instant) {
        self.record_message_channel_view_transition(channel_id, now);
        self.record_recent_channel(channel_id);
        let is_forum = self
            .discord
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum());
        let preserves_thread_return = self.messages.thread_return_target.is_some_and(|target| {
            self.navigation.channels.active_channel_id == Some(target.channel_id)
                && channel_id == target.thread_channel_id
        });
        if !preserves_thread_return {
            self.messages.thread_return_target = None;
        }
        self.navigation.channels.active_channel_id = Some(channel_id);
        self.messages.pinned_message_view_channel_id = None;
        self.messages.pinned_message_view_return_target = None;
        self.messages.thread_list_view_channel_id = None;
        self.messages.thread_list_view_return_target = None;

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
        if !is_forum {
            self.queue_channel_ack(channel_id);
        }

        self.refresh_composer_emoji_candidates_for_current_query();
    }

    fn record_message_channel_view_transition(
        &mut self,
        channel_id: Id<ChannelMarker>,
        now: Instant,
    ) {
        if let Some(previous_channel_id) = self.selected_message_history_channel_id()
            && previous_channel_id != channel_id
        {
            self.message_history_refresh
                .record_channel_left(previous_channel_id, now);
        }
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        if channel.is_forum() || channel.is_category() || channel.is_thread() {
            return;
        }
        self.message_history_refresh
            .mark_stale_if_elapsed(channel_id, now);
    }

    pub(super) fn record_message_history_refreshed(&mut self, channel_id: Id<ChannelMarker>) {
        self.message_history_refresh.record_refreshed(channel_id);
    }

    fn record_recent_channel(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        if channel.is_category() || channel.is_thread() {
            return;
        }

        self.navigation
            .channels
            .recent_channel_ids
            .retain(|id| *id != channel_id);
        self.navigation
            .channels
            .recent_channel_ids
            .push_front(channel_id);
        self.navigation
            .channels
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
        if self.navigation.channels.active_channel_id == Some(channel_id) {
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
            Some(ChannelPaneEntry::Channel { branch, .. }) if branch.is_category_child() => {
                channel_tree::preceding_category_id(&entries, selected)
            }
            Some(ChannelPaneEntry::Thread { parent_branch, .. })
                if parent_branch.is_category_child() =>
            {
                channel_tree::preceding_category_id(&entries, selected)
            }
            Some(ChannelPaneEntry::VoiceParticipant { parent_branch, .. })
                if parent_branch.is_category_child() =>
            {
                channel_tree::preceding_category_id(&entries, selected)
            }
            _ => None,
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
