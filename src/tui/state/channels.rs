use std::{
    cell::{Ref, RefCell},
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{
    ArchivedThreadRequestTarget, ChannelState, ChannelUnreadState, ForumPostDataRequestTarget,
    TypingUserState, VoiceParticipantState, custom_emoji_image_url,
};

use super::{
    ActiveGuildScope, DashboardState, MessagePaneSource, ThreadCardImagePreview, ThreadReturnTarget,
};
use super::{
    channel_tree,
    model::{
        AppliedForumTag, ChannelBranch, ChannelPaneCursor, ChannelPaneEntry, ChannelThreadItem,
        FocusPane,
    },
    presentation::{is_direct_message_channel, sort_direct_message_channels},
    scroll::{clamp_selected_index, toggle_collapsed_key},
};
use crate::discord::AppCommand;
use crate::tui::{
    fuzzy::{FuzzyMatchQuality, FuzzyScore, fuzzy_name_match_score},
    ui::thread_card,
};

const RECENT_CHANNEL_LIMIT: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThreadCardListEntry {
    channel_id: Id<ChannelMarker>,
    section_label: Option<&'static str>,
    archived: bool,
    has_tags: bool,
    has_preview_image: bool,
    rendered_height: usize,
    rendered_row_start: usize,
}

impl ThreadCardListEntry {
    fn new(
        channel: &ChannelState,
        section_label: Option<&'static str>,
        archived: bool,
        has_tags: bool,
        has_preview_image: bool,
        card_width: usize,
        show_images: bool,
    ) -> Self {
        Self {
            channel_id: channel.id,
            section_label,
            archived,
            has_tags,
            has_preview_image,
            rendered_height: thread_card::thread_card_height_for(
                thread_card::ThreadCardHeightInput {
                    label: &channel.name,
                    pinned: channel.thread_pinned().unwrap_or(false),
                    archived,
                    locked: channel.thread_locked().unwrap_or(false),
                    has_tags,
                    has_preview_image,
                },
                card_width,
                show_images,
            ) + usize::from(section_label.is_some()),
            rendered_row_start: 0,
        }
    }

    fn card_height(self) -> usize {
        self.rendered_height
            .saturating_sub(usize::from(self.section_label.is_some()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThreadCardListLayout {
    list_width: usize,
    list_height: usize,
    show_images: bool,
}

#[derive(Debug)]
struct CachedThreadCardList {
    source: Option<MessagePaneSource>,
    layout: ThreadCardListLayout,
    entries: Vec<ThreadCardListEntry>,
}

#[derive(Debug, Default)]
pub(super) struct ThreadCardListCacheState {
    // Keep only ordering and row geometry here. Full cards contain cloned
    // message, reaction, tag, and attachment data, so materializing the whole
    // forum list would make every draw scale with off-screen post count.
    cached: RefCell<Option<CachedThreadCardList>>,
}

fn set_thread_card_entry_row_starts(entries: &mut [ThreadCardListEntry]) -> usize {
    let mut rendered_row_start = 0usize;
    for entry in entries {
        entry.rendered_row_start = rendered_row_start;
        rendered_row_start = rendered_row_start.saturating_add(entry.rendered_height);
    }
    rendered_row_start
}

impl DashboardState {
    pub fn selected_forum_post_items(&self) -> Vec<ChannelThreadItem> {
        if !matches!(
            self.message_pane_source(),
            Some(MessagePaneSource::ForumPosts { .. })
        ) {
            return Vec::new();
        }
        self.selected_thread_card_entries()
            .iter()
            .filter_map(|entry| self.materialize_thread_card(*entry))
            .collect()
    }

    pub fn selected_forum_posts_loading(&self) -> bool {
        self.selected_forum_channel()
            .is_some_and(|(_, channel_id)| {
                !self
                    .discord
                    .cache
                    .archived_threads_have_response(channel_id)
            })
    }

    pub fn visible_thread_card_items(&self) -> Vec<ChannelThreadItem> {
        self.visible_thread_card_entries()
            .into_iter()
            .filter_map(|entry| self.materialize_thread_card(entry))
            .collect()
    }

    fn visible_thread_card_entries(&self) -> Vec<ThreadCardListEntry> {
        let entries = self.selected_thread_card_entries();
        let height = self.message_content_height();
        let mut rows = 0usize;
        let mut visible = Vec::new();
        for entry in entries.iter().skip(self.messages.message_scroll) {
            if !visible.is_empty() && rows.saturating_add(entry.rendered_height) > height {
                break;
            }
            rows = rows.saturating_add(entry.rendered_height);
            visible.push(*entry);
            if rows >= height {
                break;
            }
        }
        visible
    }

    pub fn selected_forum_post(&self) -> usize {
        clamp_selected_index(
            self.messages.selected_message,
            self.selected_thread_card_count(),
        )
    }

    pub fn selected_thread_card(&self) -> usize {
        clamp_selected_index(
            self.messages.selected_message,
            self.selected_thread_card_count(),
        )
    }

    pub fn focused_thread_card_selection(&self) -> Option<usize> {
        if self.navigation.focus != FocusPane::Messages || !self.message_pane_uses_thread_cards() {
            return None;
        }
        let selected = self.selected_thread_card();
        let visible_count = self.visible_thread_card_entries().len();
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
        let mut rendered_row = 0usize;
        let visible_entries = self.visible_thread_card_entries();
        for (visible_index, entry) in visible_entries.iter().enumerate() {
            if entry.section_label.is_some() {
                if row == rendered_row {
                    return false;
                }
                rendered_row = rendered_row.saturating_add(1);
            }
            let card_height = entry.card_height();
            if row < rendered_row.saturating_add(card_height) {
                let index = self.messages.message_scroll.saturating_add(visible_index);
                if index >= self.selected_thread_card_count() {
                    return false;
                }
                self.messages.selected_message = index;
                self.messages.message_auto_follow = false;
                self.messages.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(card_height);
        }
        false
    }

    pub(super) fn clamp_thread_card_viewport(&mut self) {
        let viewport = {
            let entries = self.selected_thread_card_entries();
            if entries.is_empty() {
                None
            } else {
                let selected = self.messages.selected_message.min(entries.len() - 1);
                let height = self.message_content_height().max(1);
                let mut earliest_visible = selected;
                let mut rendered_rows = entries[selected].rendered_height;
                while earliest_visible > 0 {
                    let candidate = earliest_visible - 1;
                    let candidate_rows =
                        rendered_rows.saturating_add(entries[candidate].rendered_height);
                    if candidate_rows > height {
                        break;
                    }
                    earliest_visible = candidate;
                    rendered_rows = candidate_rows;
                }
                Some((selected, earliest_visible))
            }
        };
        let Some((selected, earliest_visible)) = viewport else {
            self.messages.message_scroll = 0;
            return;
        };
        self.messages.message_scroll = self
            .messages
            .message_scroll
            .min(selected)
            .max(earliest_visible);
    }

    pub fn selected_message_history_channel_id(&self) -> Option<Id<ChannelMarker>> {
        match self.message_pane_source()? {
            MessagePaneSource::ChannelMessages { channel_id } => Some(channel_id),
            MessagePaneSource::PinnedMessages { .. }
            | MessagePaneSource::ForumPosts { .. }
            | MessagePaneSource::ChannelThreads { .. } => None,
        }
    }

    /// Switch to the normal message history for `channel_id` only when it is
    /// not already visible. Re-activating the active history resets its cursor
    /// and viewport to the latest message while an around-message request is
    /// still loading.
    pub(super) fn activate_message_history_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
        scope: Option<ActiveGuildScope>,
    ) {
        if self.selected_message_history_channel_id() == Some(channel_id) {
            return;
        }
        if let Some(scope) = scope {
            self.activate_guild(scope);
        }
        self.restore_channel_cursor(Some(channel_id));
        self.activate_channel(channel_id);
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

    pub(crate) fn selected_forum_post_data_target(&self) -> Option<ForumPostDataRequestTarget> {
        let (guild_id, channel_id) = self.selected_forum_channel()?;
        let missing_thread_ids = self
            .visible_thread_card_entries()
            .into_iter()
            .map(|entry| entry.channel_id)
            .filter(|thread_id| !self.discord.cache.thread_post_data_loaded(*thread_id))
            .collect();
        Some(ForumPostDataRequestTarget {
            guild_id,
            channel_id,
            thread_ids: missing_thread_ids,
        })
    }

    pub(crate) fn selected_archived_thread_request_target(
        &self,
    ) -> Option<ArchivedThreadRequestTarget> {
        let (guild_id, channel_id) = self.selected_forum_channel()?;
        let should_load_more = self.selected_forum_is_near_loaded_end();
        let cursor = self
            .discord
            .cache
            .next_archived_thread_page_cursor(channel_id, should_load_more)?;
        Some(ArchivedThreadRequestTarget {
            guild_id,
            channel_id,
            cursor,
        })
    }

    fn selected_forum_is_near_loaded_end(&self) -> bool {
        let item_count = self.selected_thread_card_count();
        if item_count == 0 {
            return true;
        }
        const PREFETCH_CARDS: usize = 3;
        let visible_end = self
            .messages
            .message_scroll
            .saturating_add(self.visible_thread_card_entries().len());
        let selected_end = self.selected_forum_post().saturating_add(1);
        visible_end.max(selected_end).saturating_add(PREFETCH_CARDS) >= item_count
    }

    /// Open the selected card (a thread or forum post) as the active channel.
    pub fn activate_selected_thread_card(&mut self) -> Option<AppCommand> {
        let channel_id = self
            .selected_thread_card_entries()
            .get(self.selected_thread_card())?
            .channel_id;
        self.activate_thread_card_item(channel_id)
    }

    fn activate_thread_card_item(&mut self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let guild_id = self
            .discord
            .channel(channel_id)
            .and_then(|channel| channel.guild_id)?;
        self.record_thread_return_target(channel_id);
        self.activate_channel(channel_id);
        Some(AppCommand::SubscribeGuildChannel {
            guild_id,
            channel_id,
        })
    }

    pub(super) fn clear_thread_card_list_cache(&mut self) {
        self.thread_cards.cached.get_mut().take();
    }

    fn selected_thread_card_entries(&self) -> Ref<'_, [ThreadCardListEntry]> {
        let source = self.message_pane_source();
        let layout = ThreadCardListLayout {
            list_width: self.messages.message_view_width,
            list_height: self.message_content_height(),
            show_images: self.show_images(),
        };
        let needs_rebuild = self
            .thread_cards
            .cached
            .borrow()
            .as_ref()
            .is_none_or(|cached| cached.source != source || cached.layout != layout);
        if needs_rebuild {
            let entries = self.build_thread_card_entries(source, layout);
            self.thread_cards.cached.replace(Some(CachedThreadCardList {
                source,
                layout,
                entries,
            }));
        }
        Ref::map(self.thread_cards.cached.borrow(), |cached| {
            cached
                .as_ref()
                .expect("thread card cache is initialized")
                .entries
                .as_slice()
        })
    }

    fn build_thread_card_entries(
        &self,
        source: Option<MessagePaneSource>,
        layout: ThreadCardListLayout,
    ) -> Vec<ThreadCardListEntry> {
        let card_width = layout.list_width.max(4);
        let mut entries = match source {
            Some(MessagePaneSource::ForumPosts { channel_id }) => {
                self.forum_thread_card_entries(channel_id, card_width, layout.show_images)
            }
            Some(MessagePaneSource::ChannelThreads { channel_id }) => {
                let active_ids = self.discord.cache.active_thread_ids_for_parent(channel_id);
                self.active_thread_card_entries(
                    &active_ids,
                    channel_id,
                    "Active threads",
                    card_width,
                    layout.show_images,
                )
            }
            Some(
                MessagePaneSource::ChannelMessages { .. }
                | MessagePaneSource::PinnedMessages { .. },
            )
            | None => Vec::new(),
        };
        let total_rows = set_thread_card_entry_row_starts(&mut entries);
        if layout.list_height > 0 && total_rows > layout.list_height {
            self.update_thread_card_entry_heights(
                &mut entries,
                card_width.saturating_sub(1).max(4),
                layout.show_images,
            );
            set_thread_card_entry_row_starts(&mut entries);
        }
        entries
    }

    fn forum_thread_card_entries(
        &self,
        channel_id: Id<ChannelMarker>,
        card_width: usize,
        show_images: bool,
    ) -> Vec<ThreadCardListEntry> {
        let Some(channel) = self
            .discord
            .cache
            .channel(channel_id)
            .filter(|channel| channel.is_forum())
        else {
            return Vec::new();
        };
        let active_post_ids = self.discord.cache.active_thread_ids_for_parent(channel.id);
        let active_post_ids_set = active_post_ids.iter().copied().collect::<BTreeSet<_>>();
        let archived_post_ids = self
            .discord
            .cache
            .archived_thread_ids_for_parent(channel.id)
            .into_iter()
            .filter(|post_id| !active_post_ids_set.contains(post_id))
            .collect::<Vec<_>>();
        let mut entries = self.active_thread_card_entries(
            &active_post_ids,
            channel.id,
            "Active posts",
            card_width,
            show_images,
        );
        entries.extend(self.archived_forum_thread_card_entries(
            &archived_post_ids,
            channel.id,
            card_width,
            show_images,
        ));
        entries
    }

    fn active_thread_card_entries(
        &self,
        thread_ids: &[Id<ChannelMarker>],
        parent_channel_id: Id<ChannelMarker>,
        section_label: &'static str,
        card_width: usize,
        show_images: bool,
    ) -> Vec<ThreadCardListEntry> {
        // Discord displays pinned posts first, then orders the remaining active
        // posts by recent activity. The last message snowflake is updated by
        // Gateway events and gives this view a stable local ordering.
        let (mut pinned, mut rest): (Vec<_>, Vec<_>) = thread_ids
            .iter()
            .filter_map(|thread_id| self.discord.cache.channel(*thread_id))
            .filter(|thread| {
                thread.is_thread()
                    && thread.parent_id == Some(parent_channel_id)
                    && self.discord.cache.can_view_channel(thread)
            })
            .partition(|thread| thread.thread_pinned().unwrap_or(false));
        let by_last_message = |thread: &&ChannelState| {
            std::cmp::Reverse(thread.last_message_id.map(|id| id.get()).unwrap_or(0))
        };
        pinned.sort_by_key(by_last_message);
        rest.sort_by_key(by_last_message);

        pinned
            .into_iter()
            .chain(rest)
            .enumerate()
            .map(|(index, thread)| {
                ThreadCardListEntry::new(
                    thread,
                    (index == 0).then_some(section_label),
                    false,
                    self.thread_card_has_visible_tags(thread),
                    self.thread_card_has_preview_image(thread),
                    card_width,
                    show_images,
                )
            })
            .collect()
    }

    fn archived_forum_thread_card_entries(
        &self,
        post_ids: &[Id<ChannelMarker>],
        forum_channel_id: Id<ChannelMarker>,
        card_width: usize,
        show_images: bool,
    ) -> Vec<ThreadCardListEntry> {
        // The archived endpoint already orders rows by archive timestamp,
        // newest first. Preserve that order instead of re-sorting by message
        // snowflake, which can differ from the time a post was archived.
        post_ids
            .iter()
            .filter_map(|post_id| self.discord.cache.channel(*post_id))
            .filter(|post| {
                post.is_thread()
                    && post.parent_id == Some(forum_channel_id)
                    && post.thread_archived() == Some(true)
                    && self.discord.cache.can_view_channel(post)
            })
            .enumerate()
            .map(|(index, post)| {
                ThreadCardListEntry::new(
                    post,
                    (index == 0).then_some("Archived posts"),
                    true,
                    self.thread_card_has_visible_tags(post),
                    self.thread_card_has_preview_image(post),
                    card_width,
                    show_images,
                )
            })
            .collect()
    }

    fn thread_card_has_visible_tags(&self, channel: &ChannelState) -> bool {
        let Some(parent) = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
        else {
            return false;
        };
        channel
            .applied_tags
            .iter()
            .any(|tag_id| parent.available_tags.iter().any(|tag| tag.id == *tag_id))
    }

    fn thread_card_has_preview_image(&self, channel: &ChannelState) -> bool {
        let is_forum_post = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        self.thread_card_preview_message(channel, is_forum_post)
            .is_some_and(|message| {
                message
                    .attachments_in_display_order()
                    .any(|attachment| attachment.inline_preview_url().is_some())
            })
    }

    fn update_thread_card_entry_heights(
        &self,
        entries: &mut [ThreadCardListEntry],
        card_width: usize,
        show_images: bool,
    ) {
        for entry in entries {
            let Some(channel) = self.discord.cache.channel(entry.channel_id) else {
                continue;
            };
            entry.rendered_height = thread_card::thread_card_height_for(
                thread_card::ThreadCardHeightInput {
                    label: &channel.name,
                    pinned: channel.thread_pinned().unwrap_or(false),
                    archived: entry.archived,
                    locked: channel.thread_locked().unwrap_or(false),
                    has_tags: entry.has_tags,
                    has_preview_image: entry.has_preview_image,
                },
                card_width,
                show_images,
            ) + usize::from(entry.section_label.is_some());
        }
    }

    fn materialize_thread_card(&self, entry: ThreadCardListEntry) -> Option<ChannelThreadItem> {
        let channel = self.discord.cache.channel(entry.channel_id)?;
        Some(self.thread_card_item(
            channel,
            entry.section_label.map(str::to_owned),
            entry.archived,
        ))
    }

    pub(super) fn selected_thread_card_count(&self) -> usize {
        self.selected_thread_card_entries().len()
    }

    pub(crate) fn thread_card_rendered_rows_before(&self, index: usize) -> usize {
        let entries = self.selected_thread_card_entries();
        entries.get(index).map_or_else(
            || {
                entries
                    .last()
                    .map(|entry| {
                        entry
                            .rendered_row_start
                            .saturating_add(entry.rendered_height)
                    })
                    .unwrap_or(0)
            },
            |entry| entry.rendered_row_start,
        )
    }

    pub(crate) fn thread_card_total_rendered_rows(&self) -> usize {
        self.selected_thread_card_entries()
            .last()
            .map(|entry| {
                entry
                    .rendered_row_start
                    .saturating_add(entry.rendered_height)
            })
            .unwrap_or(0)
    }

    pub(super) fn thread_card_item(
        &self,
        channel: &ChannelState,
        section_label: Option<String>,
        archived: bool,
    ) -> ChannelThreadItem {
        let is_forum_post = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        let applied_tags = self.forum_thread_applied_tags(channel);
        let preview = self.thread_card_preview_message(channel, is_forum_post);
        // Thread metadata, including its owner, arrives before `/post-data`.
        // Do not treat that early owner record as proof that the starter was
        // deleted. Only the completed post-data response can establish that.
        let preview_loading = is_forum_post
            && preview.is_none()
            && !self.discord.cache.thread_post_data_loaded(channel.id);
        let starter_creator = (is_forum_post && preview.is_none())
            .then(|| self.discord.cache.thread_creator(channel.id))
            .flatten();
        let starter_author_id = starter_creator.map(|creator| creator.user_id);
        let starter_author = starter_creator.map(|creator| {
            self.discord
                .cache
                .user_display_name_for_channel(channel.id, creator.user_id)
                .unwrap_or_else(|| format!("user-{}", creator.user_id.get()))
        });
        let starter_author_color = starter_creator.and_then(|creator| {
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
                .or(starter_author_id),
            preview_author: preview
                .map(|message| message.author.clone())
                .or(starter_author),
            preview_author_color: preview
                .and_then(|message| self.message_author_role_color(message))
                .or(starter_author_color),
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
                    if preview_loading {
                        None
                    } else {
                        starter_author_id.map(|_| "original message deleted".to_owned())
                    }
                }),
            preview_loading,
            preview_image: preview.and_then(|message| {
                message
                    .attachments_in_display_order()
                    .find(|attachment| attachment.inline_preview_url().is_some())
                    .cloned()
                    .map(|attachment| ThreadCardImagePreview {
                        message_id: message.id,
                        attachment,
                    })
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

    fn thread_card_preview_message(
        &self,
        channel: &ChannelState,
        is_forum_post: bool,
    ) -> Option<&crate::discord::MessageState> {
        let messages = self.discord.messages_for_channel(channel.id);
        if is_forum_post {
            messages
                .into_iter()
                .find(|message| message.id.get() == channel.id.get())
        } else {
            messages.into_iter().next()
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
                    let animated = parent
                        .guild_id
                        .and_then(|guild_id| {
                            self.discord
                                .cache
                                .custom_emojis_for_guild(guild_id)
                                .iter()
                                .find(|emoji| emoji.id == emoji_id)
                                .map(|emoji| emoji.animated)
                        })
                        .unwrap_or(false);
                    custom_emoji_image_url(emoji_id.get(), animated)
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
        if !self.discord.cache.thread_is_joined(channel_id) {
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
                .sidebar_channels_for_guild(Some(guild_id)),
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
                        channel_id: state.id,
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
            // Only threads that are both active and joined are projected into
            // the channel tree. Active discovery and current-user membership
            // remain independent in the Discord state cache.
            if self.discord.cache.thread_is_sidebar_active(channel.id)
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
                channel_id: state.id,
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
                    || (channel.is_thread()
                        && !self.discord.cache.thread_is_sidebar_active(channel.id))
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

    pub(super) fn selected_channel_cursor(&self) -> Option<ChannelPaneCursor> {
        self.channel_pane_entries()
            .get(self.selected_channel())
            .map(ChannelPaneEntry::cursor)
    }

    #[cfg(test)]
    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut result = Vec::new();
        let mut previous_entry_index = None;
        for row in self.visible_channel_pane_rows() {
            let entry_index = row.entry_index();
            if previous_entry_index != Some(entry_index) {
                result.push(row.entry().clone());
                previous_entry_index = Some(entry_index);
            }
        }
        result
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        let selected_line = self.selected_channel_line_from_entries();
        let len = self.count_channel_lines();
        self.navigation
            .channels
            .list
            .set_view_height_and_clamp(height, selected_line, len);
    }

    pub(super) fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        self.restore_channel_pane_cursor(channel_id.map(ChannelPaneCursor::Channel));
    }

    pub(super) fn restore_channel_pane_cursor(&mut self, cursor: Option<ChannelPaneCursor>) {
        let Some(cursor) = cursor else {
            return;
        };
        if let Some(index) = self
            .channel_pane_entries()
            .iter()
            .position(|entry| entry.cursor() == cursor)
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
    /// Names use the same channel-scoped identity resolver as voice. Caps at
    /// three names and collapses to "Several people are typing…" beyond that.
    pub fn typing_footer_for_selected_channel(&self) -> Option<String> {
        let channel_id = self.selected_channel_id()?;
        self.discord.cache.channel(channel_id)?;
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
            let user_id = typer.user_id;
            self.discord
                .cache
                .user_display_name_for_channel(channel_id, user_id)
                .unwrap_or_else(|| format!("user-{}", user_id.get()))
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
        let broadcasting = self
            .runtime
            .active_stream_broadcast
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id));
        let suffix = match (other_client, broadcasting) {
            (true, true) => " (other client) 🔴",
            (true, false) => " (other client)",
            (false, true) => " 🔴",
            (false, false) => "",
        };
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
        enum SelectedChannelPaneEntry {
            Category,
            Channel(Id<ChannelMarker>),
            VoiceParticipant,
        }

        let selected = self
            .channel_pane_entries()
            .get(self.selected_channel())
            .map(|entry| match entry {
                ChannelPaneEntry::CategoryHeader { .. } => SelectedChannelPaneEntry::Category,
                ChannelPaneEntry::Channel { state, .. }
                | ChannelPaneEntry::Thread { state, .. } => {
                    SelectedChannelPaneEntry::Channel(state.id)
                }
                ChannelPaneEntry::VoiceParticipant { .. } => {
                    SelectedChannelPaneEntry::VoiceParticipant
                }
            });

        match selected {
            Some(SelectedChannelPaneEntry::Category) => {
                self.toggle_selected_channel_category();
                None
            }
            Some(SelectedChannelPaneEntry::Channel(channel_id)) => {
                self.activate_channel_command(channel_id)
            }
            Some(SelectedChannelPaneEntry::VoiceParticipant) => {
                self.open_selected_channel_actions();
                None
            }
            None => None,
        }
    }

    fn activate_channel_command(&mut self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let command = self.channel_subscription_command(channel_id);
        self.activate_channel(channel_id);
        command
    }

    pub(in crate::tui) fn selected_channel_subscription_command(&self) -> Option<AppCommand> {
        self.navigation
            .channels
            .active_channel_id
            .and_then(|channel_id| self.channel_subscription_command(channel_id))
    }

    fn channel_subscription_command(&self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
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
        if self
            .discord
            .cache
            .channel_message_bodies_are_cold(channel_id)
            || self.message_history_refresh.is_stale(channel_id)
        {
            self.record_latest_message_history_loading(channel_id);
        }
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
