mod events;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker, RoleMarker},
};
use crate::discord::{ChannelState, MessageHistoryAfterMode, MessageState, is_thread_kind};
use crate::tui::format;
use crate::tui::format::{
    MentionTarget, RenderedText, TextHighlightKind, render_user_mentions,
    render_user_mentions_with_highlights, replace_custom_emoji_markup,
};

use super::scroll::{
    SCROLL_OFF, clamp_list_scroll, move_index_down, move_index_down_by, move_index_up,
    normalize_message_line_scroll, pane_content_height, scroll_message_row_down,
    scroll_message_row_up,
};
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnreadBanner {
    pub since_message_id: Id<MessageMarker>,
    pub unread_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessagePaneSource {
    ChannelMessages { channel_id: Id<ChannelMarker> },
    PinnedMessages { channel_id: Id<ChannelMarker> },
    ForumPosts { channel_id: Id<ChannelMarker> },
    ChannelThreads { channel_id: Id<ChannelMarker> },
}

impl MessagePaneSource {
    pub fn channel_id(self) -> Id<ChannelMarker> {
        match self {
            Self::ChannelMessages { channel_id }
            | Self::PinnedMessages { channel_id }
            | Self::ForumPosts { channel_id }
            | Self::ChannelThreads { channel_id } => channel_id,
        }
    }

    fn uses_message_items(self) -> bool {
        matches!(
            self,
            Self::ChannelMessages { .. } | Self::PinnedMessages { .. }
        )
    }

    fn uses_forum_posts(self) -> bool {
        matches!(self, Self::ForumPosts { .. })
    }

    /// Sources that render `ChannelThreadItem` cards. Forum-only bits (loading,
    /// sections, "load more") gate on the narrower `uses_forum_posts` instead.
    fn uses_thread_cards(self) -> bool {
        matches!(self, Self::ForumPosts { .. } | Self::ChannelThreads { .. })
    }

    fn can_load_message_history(self) -> bool {
        matches!(self, Self::ChannelMessages { .. })
    }

    fn shows_unread_markers(self) -> bool {
        matches!(self, Self::ChannelMessages { .. })
    }

    fn supports_auto_follow(self) -> bool {
        matches!(self, Self::ChannelMessages { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ThreadReturnTarget {
    pub(super) thread_channel_id: Id<ChannelMarker>,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) selected_message: usize,
    pub(super) message_scroll: usize,
    pub(super) message_line_scroll: usize,
    pub(super) message_keep_selection_visible: bool,
    pub(super) message_auto_follow: bool,
    pub(super) new_messages_marker_message_id: Option<Id<MessageMarker>>,
    pub(super) unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    pub(super) pending_unread_anchor_scroll: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PinnedMessageViewReturnTarget {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) selected_message: usize,
    pub(super) message_scroll: usize,
    pub(super) message_line_scroll: usize,
    pub(super) message_keep_selection_visible: bool,
    pub(super) message_auto_follow: bool,
    pub(super) new_messages_marker_message_id: Option<Id<MessageMarker>>,
    pub(super) unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    pub(super) pending_unread_anchor_scroll: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ThreadListViewReturnTarget {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) selected_message: usize,
    pub(super) message_scroll: usize,
    pub(super) message_line_scroll: usize,
    pub(super) message_keep_selection_visible: bool,
    pub(super) message_auto_follow: bool,
    pub(super) new_messages_marker_message_id: Option<Id<MessageMarker>>,
    pub(super) unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    pub(super) pending_unread_anchor_scroll: bool,
}

#[derive(Debug)]
pub(super) struct MessageViewportState {
    pub(super) selected_message: usize,
    pub(super) message_scroll: usize,
    pub(super) message_line_scroll: usize,
    pub(super) message_keep_selection_visible: bool,
    pub(super) message_auto_follow: bool,
    pub(super) new_messages_marker_message_id: Option<Id<MessageMarker>>,
    /// Snowflake of the last message the user had acked at the moment the
    /// active channel was opened. Captured *before* the activation-time
    /// ack so it survives the immediate ack flush, lets the renderer place
    /// a Discord-style red divider just above the first unread message,
    /// and lets the scroll math anchor the viewport to the user's
    /// last-read position once history arrives. `None` when the channel
    /// had no unread state at activation.
    pub(super) unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    /// Set on activation when an unread anchor needs to be applied to the
    /// viewport once history is available. Cleared the first frame the
    /// anchor is found among the loaded messages, so subsequent navigation
    /// is not pinned to the original anchor position.
    pub(super) pending_unread_anchor_scroll: bool,
    pub(super) message_view_height: usize,
    pub(super) message_content_width: usize,
    pub(super) message_preview_width: u16,
    pub(super) message_max_preview_height: u16,
    pub(super) pinned_message_view_channel_id: Option<Id<ChannelMarker>>,
    pub(super) pinned_message_view_return_target: Option<PinnedMessageViewReturnTarget>,
    pub(super) thread_list_view_channel_id: Option<Id<ChannelMarker>>,
    pub(super) thread_list_view_return_target: Option<ThreadListViewReturnTarget>,
    pub(super) thread_return_target: Option<ThreadReturnTarget>,
}

impl Default for MessageViewportState {
    fn default() -> Self {
        Self {
            selected_message: 0,
            message_scroll: 0,
            message_line_scroll: 0,
            message_keep_selection_visible: true,
            message_auto_follow: true,
            new_messages_marker_message_id: None,
            unread_divider_last_acked_id: None,
            pending_unread_anchor_scroll: false,
            message_view_height: 1,
            message_content_width: usize::MAX,
            message_preview_width: 0,
            message_max_preview_height: 0,
            pinned_message_view_channel_id: None,
            pinned_message_view_return_target: None,
            thread_list_view_channel_id: None,
            thread_list_view_return_target: None,
            thread_return_target: None,
        }
    }
}

impl DashboardState {
    pub fn message_pane_source(&self) -> Option<MessagePaneSource> {
        let channel = self.selected_channel_state()?;
        if channel.is_forum() {
            return Some(MessagePaneSource::ForumPosts {
                channel_id: channel.id,
            });
        }
        if self.messages.thread_list_view_channel_id == Some(channel.id) {
            return Some(MessagePaneSource::ChannelThreads {
                channel_id: channel.id,
            });
        }
        if self.messages.pinned_message_view_channel_id == Some(channel.id) {
            Some(MessagePaneSource::PinnedMessages {
                channel_id: channel.id,
            })
        } else {
            Some(MessagePaneSource::ChannelMessages {
                channel_id: channel.id,
            })
        }
    }

    pub(crate) fn message_pane_uses_message_items(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::uses_message_items)
    }

    pub(crate) fn message_pane_uses_forum_posts(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::uses_forum_posts)
    }

    pub(crate) fn message_pane_uses_thread_cards(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::uses_thread_cards)
    }

    pub(crate) fn message_pane_can_load_message_history(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::can_load_message_history)
    }

    pub(crate) fn message_pane_shows_unread_markers(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::shows_unread_markers)
    }

    pub(crate) fn message_pane_supports_auto_follow(&self) -> bool {
        self.message_pane_source()
            .is_some_and(MessagePaneSource::supports_auto_follow)
    }

    pub fn selected_message(&self) -> usize {
        clamp_selected_index(
            self.messages.selected_message,
            self.message_pane_item_count(),
        )
    }

    pub(crate) fn message_scroll(&self) -> usize {
        self.messages.message_scroll
    }

    pub(crate) fn new_messages_count(&self) -> usize {
        let Some(marker_id) = self.messages.new_messages_marker_message_id else {
            return 0;
        };
        let messages = self.messages();
        messages
            .iter()
            .position(|message| message.id == marker_id)
            .map(|index| messages.len().saturating_sub(index))
            .unwrap_or(0)
    }

    /// Index of the first loaded message whose snowflake is newer than the
    /// captured `unread_divider_last_acked_id`. Snowflake IDs encode message
    /// ordering, so the comparison resolves the divider position even when
    /// the originally-acked message is no longer in the loaded slice (e.g.
    /// because history was trimmed). Returns `None` when no anchor is
    /// captured or every loaded message is at-or-before the anchor.
    pub(crate) fn unread_divider_message_index(&self) -> Option<usize> {
        if !self.message_pane_shows_unread_markers() {
            return None;
        }
        let last_acked = self.messages.unread_divider_last_acked_id?;
        let messages = self.messages();
        messages.iter().position(|message| message.id > last_acked)
    }

    pub(crate) fn should_draw_unread_divider_at(&self, index: usize) -> bool {
        self.unread_divider_message_index() == Some(index)
    }

    /// Returns the captured snapshot together with the number of currently
    /// loaded messages newer than it. The renderer uses this to draw the
    /// Discord-style "since {time} you have {count} unread messages"
    /// banner above the message pane. `None` when no anchor is captured
    /// or no loaded message is newer than the snapshot.
    pub(crate) fn unread_banner(&self) -> Option<UnreadBanner> {
        if !self.message_pane_shows_unread_markers() {
            return None;
        }
        let last_acked = self.messages.unread_divider_last_acked_id?;
        let messages = self.messages();
        let unread_count = messages.iter().filter(|m| m.id > last_acked).count();
        if unread_count == 0 {
            return None;
        }
        Some(UnreadBanner {
            since_message_id: last_acked,
            unread_count,
        })
    }

    #[cfg(test)]
    pub fn unread_divider_last_acked_id(&self) -> Option<Id<MessageMarker>> {
        self.messages.unread_divider_last_acked_id
    }

    #[cfg(test)]
    pub fn new_messages_marker_message_id(&self) -> Option<Id<MessageMarker>> {
        self.messages.new_messages_marker_message_id
    }

    #[cfg(test)]
    pub fn message_auto_follow(&self) -> bool {
        self.messages.message_auto_follow
    }

    #[cfg(test)]
    pub fn message_view_height(&self) -> usize {
        self.messages.message_view_height
    }

    pub fn visible_messages(&self) -> Vec<&MessageState> {
        self.messages()
            .into_iter()
            .skip(self.messages.message_scroll)
            .take(self.message_content_height())
            .collect()
    }

    pub fn message_line_scroll(&self) -> usize {
        self.messages.message_line_scroll
    }

    pub fn set_message_view_height(&mut self, height: usize) {
        self.messages.message_view_height = height;
        self.clamp_message_viewport();
    }

    pub fn clamp_message_viewport_for_image_previews(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        self.messages.message_content_width = content_width;
        self.messages.message_preview_width = preview_width;
        self.messages.message_max_preview_height = max_preview_height;
        // Retry the unread-anchor snap until the originally-acked message
        // is loaded. After it fires once, the pending flag clears and this
        // is a cheap no-op.
        self.try_apply_unread_anchor_scroll();
        self.clamp_message_viewport();
        if self.messages.message_auto_follow {
            if self.messages.message_view_height <= 1 {
                self.messages.message_scroll = self.selected_message();
                self.messages.message_line_scroll = 0;
            } else {
                self.align_message_viewport_to_bottom(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
            }
            return;
        }
        self.normalize_message_line_scroll(content_width, preview_width, max_preview_height);
        if self.messages().is_empty() || !self.messages.message_keep_selection_visible {
            return;
        }
        if self.selected_message() == 0 {
            self.messages.message_scroll = 0;
            self.messages.message_line_scroll = 0;
            return;
        }

        let height = self.message_content_height();
        if self.selected_message() == 1
            && self.messages.message_scroll == 0
            && self.messages.message_line_scroll == 0
        {
            let selected_row = self.selected_message_rendered_row(
                content_width,
                preview_width,
                max_preview_height,
            );
            let selected_bottom = selected_row.saturating_add(
                self.selected_message_rendered_height(
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .saturating_sub(1),
            );
            if selected_bottom < height {
                return;
            }
        }

        if self.center_selected_message(content_width, preview_width, max_preview_height) {
            return;
        }

        let upper_scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);
        let max_iterations = self
            .messages()
            .into_iter()
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .max(1);

        for _ in 0..max_iterations {
            let lower_scrolloff = self
                .following_message_rendered_rows(
                    content_width,
                    preview_width,
                    max_preview_height,
                    SCROLL_OFF,
                )
                .min(height.saturating_sub(1));
            let lower_bound = height.saturating_sub(1).saturating_sub(lower_scrolloff);
            let selected_row = self.selected_message_rendered_row(
                content_width,
                preview_width,
                max_preview_height,
            );
            let selected_bottom = selected_row.saturating_add(
                self.selected_message_rendered_height(
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .saturating_sub(1),
            );
            if selected_bottom > lower_bound
                && self.messages.message_scroll < self.messages.selected_message
            {
                self.scroll_message_viewport_down_one_row(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                continue;
            }

            if selected_row < upper_scrolloff && self.messages.message_scroll > 0 {
                let previous_height = self.message_rendered_height_at(
                    self.messages.message_scroll.saturating_sub(1),
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                let candidate_bottom = selected_bottom.saturating_add(previous_height);
                if candidate_bottom < height {
                    self.scroll_message_viewport_up_one_row(
                        content_width,
                        preview_width,
                        max_preview_height,
                    );
                    continue;
                }
            }

            break;
        }
    }

    pub fn focused_message_selection(&self) -> Option<usize> {
        if self.message_pane_uses_thread_cards() {
            return self.focused_thread_card_selection();
        }
        if self.navigation.focus == FocusPane::Messages
            && self.message_pane_uses_message_items()
            && !self.messages().is_empty()
        {
            let selected = self.selected_message();
            let visible_count = self.visible_messages().len();
            if selected >= self.messages.message_scroll
                && selected < self.messages.message_scroll + visible_count
            {
                Some(selected - self.messages.message_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn scroll_message_viewport_down(&mut self) {
        if self.navigation.focus != FocusPane::Messages
            || self.messages.message_content_width == usize::MAX
        {
            return;
        }

        if self.message_pane_uses_thread_cards() {
            let len = self.selected_thread_card_items().len();
            move_index_down(&mut self.messages.message_scroll, len);
            self.messages.message_auto_follow = false;
            self.messages.message_keep_selection_visible = false;
            return;
        }

        let viewport_height = self.message_content_height();
        let current_height = self.messages().get(self.messages.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.messages.message_scroll,
                self.messages.message_content_width,
                self.messages.message_preview_width,
                self.messages.message_max_preview_height,
            )
            .max(1)
        });
        let (new_top, new_offset) = match current_height {
            None => return,
            Some(h) if self.messages.message_line_scroll + 1 < h => (
                self.messages.message_scroll,
                self.messages.message_line_scroll + 1,
            ),
            _ => (self.messages.message_scroll.saturating_add(1), 0),
        };
        if !self.message_viewport_has_rows_below(new_top, new_offset, viewport_height) {
            return;
        }
        // Viewport scrolling intentionally drops auto-follow so that the
        // user can over-scroll without the next render re-aligning to the
        // natural bottom. The event handler still re-engages follow when a
        // new message arrives and the viewport actually shows the latest,
        // via `is_viewport_at_latest_message()`.
        self.messages.message_auto_follow = false;
        self.messages.message_keep_selection_visible = false;
        self.scroll_message_viewport_down_one_row(
            self.messages.message_content_width,
            self.messages.message_preview_width,
            self.messages.message_max_preview_height,
        );
        if self.is_viewport_at_latest_message() {
            self.ack_new_messages_marker_if_present();
            self.clear_new_messages_marker();
            self.normalize_message_line_scroll(
                self.messages.message_content_width,
                self.messages.message_preview_width,
                self.messages.message_max_preview_height,
            );
        }
    }

    pub(super) fn half_page_message_down(&mut self, distance: usize) {
        let distance = distance.max(1);
        if self.message_pane_uses_thread_cards()
            || self.messages.message_content_width == usize::MAX
        {
            let len = self.message_pane_item_count();
            move_index_down_by(&mut self.messages.selected_message, len, distance);
            self.messages.message_keep_selection_visible = true;
            self.clamp_message_viewport();
            self.refresh_message_auto_follow();
            return;
        }

        self.scroll_message_viewport_down_by_rows(distance);
        if let Some(selected) = self.last_visible_message_index() {
            self.messages.selected_message = selected;
        }
        self.messages.message_keep_selection_visible = false;
        self.refresh_message_auto_follow();
    }

    pub fn next_newer_history_command_for_down_by(&self, distance: usize) -> Option<AppCommand> {
        if !self.message_pane_can_load_message_history()
            || self.navigation.focus != FocusPane::Messages
        {
            return None;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return None;
        }
        let start = self.selected_message();
        let end = start
            .saturating_add(distance.max(1))
            .min(messages.len() - 1);
        let scan_end = if end > start { end - 1 } else { start };
        self.newer_history_command_in_message_range(start, scan_end)
    }

    pub(in crate::tui) fn selected_message_history_catch_up_command(&self) -> Option<AppCommand> {
        let channel_id = self.selected_message_history_channel_id()?;
        let after = self.messages().iter().map(|message| message.id).max()?;
        Some(AppCommand::LoadMessageHistoryAfter {
            channel_id,
            after,
            mode: MessageHistoryAfterMode::CatchUp,
        })
    }

    pub fn next_newer_history_command_for_half_page_down(&self) -> Option<AppCommand> {
        let distance = (self.message_content_height() / 2).max(1);
        if self.message_pane_uses_thread_cards()
            || self.messages.message_content_width == usize::MAX
        {
            return self.next_newer_history_command_for_down_by(distance);
        }

        self.newer_history_command_in_viewport_rows(
            self.message_content_height().saturating_add(distance),
        )
    }

    fn newer_history_command_in_message_range(
        &self,
        start: usize,
        end: usize,
    ) -> Option<AppCommand> {
        let channel_id = self.selected_message_history_channel_id()?;
        let messages = self.messages();
        for index in start..=end {
            let lower_id = messages.get(index)?.id;
            let Some(upper_id) = self
                .discord
                .cache
                .message_history_gap_after(channel_id, lower_id)
            else {
                continue;
            };
            let next_cached_id = messages
                .get(index.saturating_add(1))
                .map(|message| message.id);
            if next_cached_id == Some(upper_id) {
                return Some(AppCommand::LoadMessageHistoryAfter {
                    channel_id,
                    after: lower_id,
                    mode: MessageHistoryAfterMode::GapFill,
                });
            }
        }
        None
    }

    fn newer_history_command_in_viewport_rows(&self, rows: usize) -> Option<AppCommand> {
        if !self.message_pane_can_load_message_history()
            || self.navigation.focus != FocusPane::Messages
        {
            return None;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return None;
        }

        let mut top = self.messages.message_scroll;
        let mut offset = self.messages.message_line_scroll;
        for _ in 0..rows.max(1) {
            let _ = messages.get(top)?;
            let height = self
                .message_rendered_height_at(
                    top,
                    self.messages.message_content_width,
                    self.messages.message_preview_width,
                    self.messages.message_max_preview_height,
                )
                .max(1);
            if offset + 1 < height {
                offset += 1;
                continue;
            }
            if let Some(command) = self.newer_history_command_in_message_range(top, top) {
                return Some(command);
            }
            if top + 1 >= messages.len() {
                return None;
            }
            top += 1;
            offset = 0;
        }
        None
    }

    pub fn scroll_message_viewport_up(&mut self) {
        if self.navigation.focus != FocusPane::Messages
            || self.messages.message_content_width == usize::MAX
        {
            return;
        }
        if self.message_pane_uses_thread_cards() {
            move_index_up(&mut self.messages.message_scroll);
            self.messages.message_auto_follow = false;
            self.messages.message_keep_selection_visible = false;
            return;
        }
        self.messages.message_auto_follow = false;
        self.messages.message_keep_selection_visible = false;
        self.scroll_message_viewport_up_one_row(
            self.messages.message_content_width,
            self.messages.message_preview_width,
            self.messages.message_max_preview_height,
        );
    }

    pub(super) fn half_page_message_up(&mut self, distance: usize) {
        let distance = distance.max(1);
        if self.message_pane_uses_thread_cards()
            || self.messages.message_content_width == usize::MAX
        {
            self.messages.selected_message =
                self.messages.selected_message.saturating_sub(distance);
            self.messages.message_keep_selection_visible = true;
            self.clamp_message_viewport();
            self.refresh_message_auto_follow();
            return;
        }

        self.scroll_message_viewport_up_by_rows(distance);
        self.messages.selected_message = self.messages.message_scroll;
        self.messages.message_keep_selection_visible = false;
        self.refresh_message_auto_follow();
    }

    #[cfg(test)]
    pub fn scroll_message_viewport_top(&mut self) {
        if self.navigation.focus != FocusPane::Messages {
            return;
        }
        self.messages.message_auto_follow = false;
        self.messages.message_keep_selection_visible = false;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
    }

    #[cfg(test)]
    pub fn scroll_message_viewport_bottom(&mut self) {
        if self.navigation.focus != FocusPane::Messages
            || self.messages.message_content_width == usize::MAX
        {
            return;
        }
        self.messages.message_auto_follow = false;
        self.messages.message_keep_selection_visible = false;
        self.ack_new_messages_marker_if_present();
        self.clear_new_messages_marker();
        self.align_message_viewport_to_bottom(
            self.messages.message_content_width,
            self.messages.message_preview_width,
            self.messages.message_max_preview_height,
        );
        self.refresh_message_auto_follow();
    }

    pub(super) fn select_visible_message_row(&mut self, row: usize) -> bool {
        if self.message_pane_uses_thread_cards() {
            return self.select_visible_thread_card_row(row);
        }
        if self.messages.message_content_width == usize::MAX {
            return false;
        }

        let mut rendered_row = 0usize;
        for local_index in 0..self.visible_messages().len() {
            let index = self.messages.message_scroll.saturating_add(local_index);
            let rendered_height = self
                .message_rendered_height_at(
                    index,
                    self.messages.message_content_width,
                    self.messages.message_preview_width,
                    self.messages.message_max_preview_height,
                )
                .max(1);
            let visible_height = if local_index == 0 {
                rendered_height.saturating_sub(self.messages.message_line_scroll)
            } else {
                rendered_height
            };
            if row < rendered_row.saturating_add(visible_height) {
                self.messages.selected_message = index;
                self.messages.message_auto_follow = false;
                self.messages.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(visible_height);
        }
        false
    }

    /// Returns true when the cursor sits on the last message in the active
    /// channel. This is the auto-follow trigger condition: when an event
    /// arrives, follow (cursor jump + scroll) only fires if the cursor was
    /// already on the latest message and the viewport was at the latest.
    pub(super) fn cursor_on_last_message(&self) -> bool {
        if !self.message_pane_supports_auto_follow() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        self.messages.selected_message >= messages.len().saturating_sub(1)
    }

    /// Returns true when the rendered viewport shows the bottom of the latest
    /// message, regardless of where the cursor is parked. This is the
    /// auto-scroll trigger condition. With no rendered width yet in unit tests,
    /// falls back to an item-count check against the configured view height.
    pub(super) fn is_viewport_at_latest_message(&self) -> bool {
        if !self.message_pane_supports_auto_follow() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        let viewport = self.message_content_height();
        if self.messages.message_content_width == usize::MAX {
            return self.messages.message_scroll.saturating_add(viewport) >= messages.len();
        }
        let total = self.message_total_rendered_rows(
            self.messages.message_content_width,
            self.messages.message_preview_width,
            self.messages.message_max_preview_height,
        );
        let pos = self.message_scroll_row_position(
            self.messages.message_content_width,
            self.messages.message_preview_width,
            self.messages.message_max_preview_height,
        );
        total.saturating_sub(pos) <= viewport
    }

    /// Re-engages auto-follow only when the cursor is on the last message and
    /// the viewport is showing it. Either condition alone is not enough. If the
    /// user has scrolled the viewport off the bottom while the cursor remains
    /// on the last message, the next render must not snap the viewport back.
    /// Moving the cursor away from the last message also disengages, so the
    /// bottom-snap inside `clamp_message_viewport_for_image_previews` won't
    /// fight cursor-visibility centering.
    pub(super) fn refresh_message_auto_follow(&mut self) {
        self.messages.message_auto_follow =
            self.cursor_on_last_message() && self.is_viewport_at_latest_message();
        if self.messages.message_auto_follow {
            self.ack_new_messages_marker_if_present();
            self.clear_new_messages_marker();
            // Once the user has caught up (cursor + viewport on the
            // latest), retire the unread divider/banner so the indicator
            // doesn't linger after every unread message has been read.
            self.messages.unread_divider_last_acked_id = None;
            self.messages.pending_unread_anchor_scroll = false;
        }
    }

    pub(super) fn clear_new_messages_marker(&mut self) {
        self.messages.new_messages_marker_message_id = None;
    }

    fn ack_new_messages_marker_if_present(&mut self) {
        if self.messages.new_messages_marker_message_id.is_some()
            && let Some(channel_id) = self.selected_message_history_channel_id()
        {
            self.schedule_channel_ack(channel_id);
        }
    }

    pub(super) fn clear_missing_new_messages_marker(&mut self) {
        if let Some(marker_id) = self.messages.new_messages_marker_message_id
            && !self
                .messages()
                .iter()
                .any(|message| message.id == marker_id)
        {
            self.clear_new_messages_marker();
        }
    }

    pub(super) fn follow_latest_message(&mut self) {
        // Only updates the selection. Scroll position is left for
        // `align_message_viewport_to_bottom` to recompute on the next render.
        // Touching scroll/line_scroll here would briefly collapse the viewport
        // to a single-message state, and a key press (e.g. `k`) landing in
        // that window flips auto_follow off before alignment runs again,
        // stranding the viewport with empty space below the last message.
        self.messages.selected_message = self.message_pane_item_count().saturating_sub(1);
        self.messages.message_keep_selection_visible = true;
    }

    /// Snap the viewport so the user's last-read message sits at the top of
    /// the message pane and the unread divider is visible just below it.
    /// No-op until the captured `last_acked` snowflake is resolvable from
    /// the loaded slice. The call is retried each frame so the snap fires
    /// once history streams in. Once applied, the pending flag clears so
    /// subsequent navigation is not pinned to the anchor.
    pub(crate) fn try_apply_unread_anchor_scroll(&mut self) {
        if !self.messages.pending_unread_anchor_scroll {
            return;
        }
        let Some(divider_index) = self.unread_divider_message_index() else {
            return;
        };
        let item_count = self.message_pane_item_count();
        if item_count == 0 {
            return;
        }
        // Anchor: place the last-read message (one row above the divider)
        // at the top of the viewport. Park the cursor on the first unread
        // so j/k navigation begins where the user left off, and disable
        // selection-keep so the next frame's centering pass does not pull
        // the viewport away from the anchor.
        self.messages.message_scroll = divider_index.saturating_sub(1);
        self.messages.message_line_scroll = 0;
        self.messages.selected_message = divider_index.min(item_count.saturating_sub(1));
        self.messages.message_keep_selection_visible = false;
        self.messages.message_auto_follow = false;
        self.messages.pending_unread_anchor_scroll = false;
    }

    fn align_message_viewport_to_bottom(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.message_pane_uses_thread_cards() {
            self.clamp_thread_card_viewport();
            self.messages.message_line_scroll = 0;
            return;
        }
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .message_rendered_height_at(index, content_width, preview_width, max_preview_height)
                .max(1);
            if message_height >= remaining {
                self.messages.message_scroll = index;
                self.messages.message_line_scroll = message_height.saturating_sub(remaining);
                return;
            }
            remaining = remaining.saturating_sub(message_height);
        }
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
    }

    pub(super) fn restore_message_position(
        &mut self,
        selected_message_id: Option<Id<MessageMarker>>,
        scroll_message_id: Option<Id<MessageMarker>>,
    ) {
        let message_ids: Vec<_> = self
            .messages()
            .into_iter()
            .map(|message| message.id)
            .collect();
        if let Some(message_id) = selected_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.messages.selected_message = index;
        }
        if let Some(message_id) = scroll_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.messages.message_scroll = index;
        }
    }

    pub(super) fn clamp_message_viewport(&mut self) {
        let item_count = self.message_pane_item_count();
        if item_count == 0 {
            self.messages.selected_message = 0;
            self.messages.message_scroll = 0;
            self.messages.message_line_scroll = 0;
            return;
        }

        self.messages.selected_message = self.messages.selected_message.min(item_count - 1);
        self.messages.message_scroll = self.messages.message_scroll.min(item_count - 1);
        if self.message_pane_uses_thread_cards() {
            self.clamp_thread_card_viewport();
            self.messages.message_line_scroll = 0;
            return;
        }
        if self.messages.message_content_width == usize::MAX {
            self.messages.message_scroll = clamp_list_scroll(
                self.messages.selected_message,
                self.messages.message_scroll,
                self.message_content_height(),
                item_count,
            );
            if self.messages.message_scroll != self.messages.selected_message {
                self.messages.message_line_scroll = 0;
            }
        }
    }

    fn center_selected_message(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> bool {
        let selected = self.selected_message();
        let height = self.message_content_height();
        if self.messages().get(selected).is_none() {
            return false;
        }
        let selected_height = self
            .message_rendered_height_at(selected, content_width, preview_width, max_preview_height)
            .max(1);
        let mut top = selected;
        let mut offset = 0usize;
        let mut remaining = (height / 2).saturating_sub(selected_height / 2);

        while remaining > 0 && top > 0 {
            let previous_index = top.saturating_sub(1);
            if self.messages().get(previous_index).is_none() {
                break;
            }
            let previous_height = self
                .message_rendered_height_at(
                    previous_index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .max(1);
            if remaining >= previous_height {
                remaining = remaining.saturating_sub(previous_height);
                top = previous_index;
                offset = 0;
            } else {
                top = previous_index;
                offset = previous_height.saturating_sub(remaining);
                remaining = 0;
            }
        }

        if remaining > 0 || !self.message_viewport_has_rows_below(top, offset, height) {
            return false;
        }

        self.messages.message_scroll = top;
        self.messages.message_line_scroll = offset;
        true
    }

    fn message_viewport_has_rows_below(&self, top: usize, offset: usize, height: usize) -> bool {
        let mut visible_rows = 0usize;
        for offset_from_top in 0..self.messages().len().saturating_sub(top) {
            let global_index = top + offset_from_top;
            let message_height = self
                .message_rendered_height_at(
                    global_index,
                    self.messages.message_content_width,
                    self.messages.message_preview_width,
                    self.messages.message_max_preview_height,
                )
                .max(1);
            let visible_height = if offset_from_top == 0 {
                message_height.saturating_sub(offset)
            } else {
                message_height
            };
            visible_rows = visible_rows.saturating_add(visible_height);
            if visible_rows >= height {
                return true;
            }
        }
        false
    }

    fn scroll_message_viewport_down_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let messages_len = self.messages().len();
        let current_message_height = self.messages().get(self.messages.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.messages.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        scroll_message_row_down(
            &mut self.messages.message_scroll,
            &mut self.messages.message_line_scroll,
            messages_len,
            current_message_height,
        );
    }

    fn scroll_message_viewport_down_by_rows(&mut self, rows: usize) {
        for _ in 0..rows {
            let before = (
                self.messages.message_scroll,
                self.messages.message_line_scroll,
            );
            self.scroll_message_viewport_down();
            let after = (
                self.messages.message_scroll,
                self.messages.message_line_scroll,
            );
            if before == after {
                break;
            }
        }
    }

    fn scroll_message_viewport_up_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.messages.message_line_scroll > 0 {
            scroll_message_row_up(
                &mut self.messages.message_scroll,
                &mut self.messages.message_line_scroll,
                None,
            );
            return;
        }
        let previous_message_index = self.messages.message_scroll.checked_sub(1);
        let previous_message_height = previous_message_index.map(|index| {
            self.message_rendered_height_at(index, content_width, preview_width, max_preview_height)
        });
        scroll_message_row_up(
            &mut self.messages.message_scroll,
            &mut self.messages.message_line_scroll,
            previous_message_height,
        );
    }

    fn scroll_message_viewport_up_by_rows(&mut self, rows: usize) {
        for _ in 0..rows {
            let before = (
                self.messages.message_scroll,
                self.messages.message_line_scroll,
            );
            self.scroll_message_viewport_up();
            let after = (
                self.messages.message_scroll,
                self.messages.message_line_scroll,
            );
            if before == after {
                break;
            }
        }
    }

    fn last_visible_message_index(&self) -> Option<usize> {
        let messages = self.messages();
        let mut index = self.messages.message_scroll;
        let mut remaining = self.message_content_height();
        let mut last = messages.get(index).map(|_| index)?;

        while messages.get(index).is_some() {
            let height = self
                .message_rendered_height_at(
                    index,
                    self.messages.message_content_width,
                    self.messages.message_preview_width,
                    self.messages.message_max_preview_height,
                )
                .max(1);
            let visible_rows = if index == self.messages.message_scroll {
                height.saturating_sub(self.messages.message_line_scroll)
            } else {
                height
            }
            .max(1);

            last = index;
            if visible_rows >= remaining {
                break;
            }
            remaining = remaining.saturating_sub(visible_rows);
            index = index.saturating_add(1);
        }

        Some(last)
    }

    fn normalize_message_line_scroll(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let current_message_height = self.messages().get(self.messages.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.messages.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        normalize_message_line_scroll(
            &mut self.messages.message_line_scroll,
            current_message_height,
        );
    }

    pub(super) fn message_content_height(&self) -> usize {
        pane_content_height(self.messages.message_view_height)
    }

    pub(super) fn message_pane_item_count(&self) -> usize {
        match self.message_pane_source() {
            Some(
                MessagePaneSource::ForumPosts { .. } | MessagePaneSource::ChannelThreads { .. },
            ) => self.selected_thread_card_items().len(),
            Some(
                MessagePaneSource::ChannelMessages { .. }
                | MessagePaneSource::PinnedMessages { .. },
            ) => self.messages().len(),
            _ => 0,
        }
    }
}

impl DashboardState {
    /// Locate the thread a THREAD_CREATED (kind 18) message started. Prefers the
    /// reference channel id, falling back to a child thread matched by name
    /// (older system messages carried no reference).
    fn started_thread_channel(&self, message: &MessageState) -> Option<&ChannelState> {
        if message.message_kind.code() != 18 {
            return None;
        }
        let referenced_thread = message
            .reference
            .as_ref()
            .and_then(|reference| reference.channel_id)
            .and_then(|channel_id| self.discord.cache.channel(channel_id))
            .filter(|channel| channel.is_thread() && self.discord.cache.can_view_channel(channel));
        referenced_thread.or_else(|| {
            let thread_name = message.content.as_deref()?.trim();
            if thread_name.is_empty() {
                return None;
            }
            self.discord
                .cache
                .viewable_channels_for_guild(message.guild_id)
                .into_iter()
                .find(|channel| {
                    channel.is_thread()
                        && channel.parent_id == Some(message.channel_id)
                        && channel.name == thread_name
                })
        })
    }

    /// Build the card item for a THREAD_CREATED message so the "started a thread"
    /// box reuses the forum-post card UI. Synthesizes a placeholder item from the
    /// [`ThreadSummary`] (then the message) when the thread channel is uncached.
    pub(crate) fn thread_card_item_for_message(
        &self,
        message: &MessageState,
    ) -> Option<ChannelThreadItem> {
        if message.message_kind.code() != 18 {
            return None;
        }
        if let Some(channel) = self.started_thread_channel(message) {
            let archived = channel.thread_archived().unwrap_or(false);
            return Some(self.forum_thread_item(channel, None, archived));
        }

        let summary = self.thread_summary_for_message(message);
        let preview = summary
            .as_ref()
            .and_then(|summary| summary.latest_message_preview.as_ref());
        let label = summary
            .as_ref()
            .map(|summary| summary.name.clone())
            .or_else(|| {
                message
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "thread".to_owned());
        Some(ChannelThreadItem {
            channel_id: summary
                .as_ref()
                .map_or(message.channel_id, |summary| summary.channel_id),
            section_label: None,
            label,
            archived: summary
                .as_ref()
                .and_then(|summary| summary.archived)
                .unwrap_or(false),
            locked: summary
                .as_ref()
                .and_then(|summary| summary.locked)
                .unwrap_or(false),
            pinned: false,
            preview_author_id: None,
            preview_author: preview.map(|preview| preview.author.clone()),
            preview_author_color: None,
            preview_content: preview.map(|preview| preview.content.clone()),
            applied_tags: Vec::new(),
            preview_reactions: Vec::new(),
            comment_count: summary
                .as_ref()
                .and_then(|summary| summary.message_count.or(summary.total_message_sent)),
            new_message_count: 0,
            last_activity_message_id: summary
                .as_ref()
                .and_then(|summary| summary.latest_message_id),
        })
    }

    pub(crate) fn thread_summary_for_message(
        &self,
        message: &MessageState,
    ) -> Option<ThreadSummary> {
        let thread = self.started_thread_channel(message);
        thread.map(|channel| {
            let latest_cached_message = self
                .discord
                .messages_for_channel(channel.id)
                .into_iter()
                .max_by_key(|message| message.id);
            let latest_message_id = channel
                .last_message_id
                .or_else(|| latest_cached_message.map(|message| message.id));
            let latest_message_preview = latest_cached_message
                .filter(|message| Some(message.id) == latest_message_id)
                .map(|message| ThreadMessagePreview {
                    author: message.author.clone(),
                    content: self.thread_message_preview_text(message),
                });
            ThreadSummary {
                channel_id: channel.id,
                name: channel.name.clone(),
                message_count: channel.message_count,
                total_message_sent: channel.total_message_sent,
                archived: channel.thread_archived(),
                locked: channel.thread_locked(),
                latest_message_id,
                latest_message_preview,
            }
        })
    }

    pub(super) fn thread_message_preview_text(&self, message: &MessageState) -> String {
        if let Some(content) =
            message_preview_text(message.content.as_deref(), &message.sticker_names)
        {
            return self
                .render_user_mentions(message.guild_id, &message.mentions, &content)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }

        if !message.attachments.is_empty() {
            return "[attachment]".to_owned();
        }

        if message.content.is_some() {
            "<empty message>".to_owned()
        } else {
            "<message content unavailable>".to_owned()
        }
    }

    pub(crate) fn render_user_mentions(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> String {
        let value = if self.show_custom_emoji() {
            replace_custom_emoji_markup(value)
        } else {
            format::replace_custom_emoji_markup_with_ids(value)
        };
        render_user_mentions(
            &value,
            |user_id| self.resolve_mention_display_name(guild_id, mentions, user_id),
            |role_id| self.resolve_role_mention_name(guild_id, role_id),
            |channel_id| self.resolve_channel_mention_name(channel_id),
        )
    }

    pub(crate) fn render_user_mentions_with_highlights(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        mention_everyone: bool,
        mention_roles: &[Id<RoleMarker>],
        value: &str,
    ) -> RenderedText {
        let current_user_id = self.discord.current_user_id.map(|id| id.get());
        let current_user_role_ids = guild_id
            .and_then(|guild_id| self.discord.cache.current_user_role_ids_for_guild(guild_id));
        let mut rendered = render_user_mentions_with_highlights(
            value,
            |user_id| self.resolve_mention_display_name(guild_id, mentions, user_id),
            |role_id| self.resolve_role_mention_name(guild_id, role_id),
            |channel_id| self.resolve_channel_mention_name(channel_id),
            |target| match target {
                MentionTarget::User(user_id) => {
                    if current_user_id == Some(user_id) {
                        Some(TextHighlightKind::SelfMention)
                    } else {
                        Some(TextHighlightKind::OtherMention)
                    }
                }
                MentionTarget::Role(role_id) => {
                    let role_id = Id::new(role_id);
                    if mention_roles.contains(&role_id)
                        && current_user_role_ids.is_some_and(|role_ids| role_ids.contains(&role_id))
                    {
                        Some(TextHighlightKind::SelfMention)
                    } else {
                        Some(TextHighlightKind::OtherMention)
                    }
                }
                // Channel mentions never notify, but we still highlight them
                // like role mentions so `#channel-name` stays distinct.
                MentionTarget::Channel(_) => Some(TextHighlightKind::OtherMention),
            },
        );
        if current_user_id.is_some() {
            let everyone_kind = if mention_everyone {
                TextHighlightKind::SelfMention
            } else {
                TextHighlightKind::OtherMention
            };
            add_literal_mention_highlights(&mut rendered, "@everyone", everyone_kind);
            add_literal_mention_highlights(&mut rendered, "@here", everyone_kind);
        }
        normalize_text_highlights(&mut rendered.highlights);
        format::replace_custom_emoji_markup_in_rendered_with_images(
            rendered,
            self.show_custom_emoji(),
        )
    }

    fn resolve_role_mention_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        role_id: u64,
    ) -> Option<String> {
        let guild_id = guild_id?;
        self.discord
            .cache
            .roles_for_guild(guild_id)
            .into_iter()
            .find(|role| role.id.get() == role_id)
            .map(|role| role.name.clone())
    }

    fn resolve_channel_mention_name(&self, channel_id: u64) -> Option<String> {
        // `parse_mention` already rejects zero ids, so the `Id::new` call
        // never sees the forbidden value.
        let id = Id::<ChannelMarker>::new(channel_id);
        self.discord
            .cache
            .channel(id)
            .map(|channel| channel.name.clone())
    }

    fn resolve_mention_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        user_id: u64,
    ) -> Option<String> {
        let mention = mentions
            .iter()
            .find(|mention| mention.user_id.get() == user_id);
        if let Some(guild_nick) = mention.and_then(|mention| mention.guild_nick.as_deref()) {
            return Some(guild_nick.to_owned());
        }
        if let Some(display_name) = guild_id.and_then(|guild_id| {
            let user_id = Id::<UserMarker>::new(user_id);
            self.discord.cache.member_display_name(guild_id, user_id)
        }) {
            return Some(display_name.to_owned());
        }
        mention.map(|mention| mention.display_name.clone())
    }

    pub(crate) fn forwarded_snapshot_mention_guild_id(
        &self,
        snapshot: &MessageSnapshotInfo,
    ) -> Option<Id<GuildMarker>> {
        snapshot
            .source_channel_id
            .and_then(|channel_id| self.discord.cache.channel(channel_id))
            .and_then(|channel| channel.guild_id)
    }

    pub(super) fn record_thread_channel_upserted(&mut self, channel: &crate::discord::ChannelInfo) {
        if !is_thread_kind(&channel.kind) {
            return;
        }
        let Some(parent_id) = channel.parent_id else {
            return;
        };
        let Some(list) = self.requests.forum_post_lists.get_mut(&parent_id) else {
            return;
        };
        let id = channel.channel_id;
        // Re-section the post when its archive state changes (our Archive action
        // or someone else's THREAD_UPDATE), so an existing post moves between the
        // active and archived sections instead of staying put. A payload without
        // `thread_metadata` carries no archive info, so we only insert-if-new.
        match channel.thread_archived() {
            Some(true) => {
                if list.archived_post_ids.contains(&id) {
                    return;
                }
                list.active_post_ids.retain(|existing| *existing != id);
                list.archived_post_ids.insert(0, id);
            }
            Some(false) => {
                if list.active_post_ids.contains(&id) {
                    return;
                }
                list.archived_post_ids.retain(|existing| *existing != id);
                list.active_post_ids.insert(0, id);
            }
            None => {
                if !list.active_post_ids.contains(&id) && !list.archived_post_ids.contains(&id) {
                    list.active_post_ids.insert(0, id);
                }
            }
        }
    }

    pub(super) fn record_forum_posts_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        threads: &[crate::discord::ChannelInfo],
        has_more: bool,
    ) {
        let list = self
            .requests
            .forum_post_lists
            .entry(channel_id)
            .or_default();
        if archive_state == ForumPostArchiveState::Active && offset == 0 {
            list.active_post_ids.clear();
            if self.navigation.channels.active_channel_id == Some(channel_id) {
                self.messages.selected_message = 0;
                self.messages.message_scroll = 0;
                self.messages.message_line_scroll = 0;
                self.messages.message_auto_follow = false;
            }
        } else if archive_state == ForumPostArchiveState::Archived && offset == 0 {
            list.archived_post_ids.clear();
        }
        for thread in threads {
            let thread_id = thread.channel_id;
            match archive_state {
                ForumPostArchiveState::Active => {
                    list.archived_post_ids.retain(|id| *id != thread_id);
                    if !list.active_post_ids.contains(&thread_id) {
                        list.active_post_ids.push(thread_id);
                    }
                }
                ForumPostArchiveState::Archived => {
                    if !list.active_post_ids.contains(&thread_id)
                        && !list.archived_post_ids.contains(&thread_id)
                    {
                        list.archived_post_ids.push(thread_id);
                    }
                }
            }
        }
        list.has_more = match archive_state {
            // Once active search is exhausted, the archived search stream may
            // still have old forum posts. Keep the UI asking for more until an
            // archived page says it is exhausted.
            ForumPostArchiveState::Active => true,
            ForumPostArchiveState::Archived => has_more,
        };
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        match self.message_pane_source() {
            Some(MessagePaneSource::ChannelMessages { channel_id }) => {
                self.discord.cache.messages_for_channel(channel_id)
            }
            Some(MessagePaneSource::PinnedMessages { channel_id }) => {
                self.discord.cache.pinned_messages_for_channel(channel_id)
            }
            Some(
                MessagePaneSource::ForumPosts { .. } | MessagePaneSource::ChannelThreads { .. },
            )
            | None => Vec::new(),
        }
    }

    pub fn enter_pinned_message_view(&mut self, channel_id: Id<ChannelMarker>) {
        if self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum())
        {
            return;
        }
        if !self.is_pinned_message_view_active() {
            self.record_pinned_message_view_return_target(channel_id);
        }
        self.messages.pinned_message_view_channel_id = Some(channel_id);
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_auto_follow = false;
        self.clear_new_messages_marker();
        self.messages.message_keep_selection_visible = true;
        self.clamp_message_viewport();
    }

    fn record_pinned_message_view_return_target(&mut self, channel_id: Id<ChannelMarker>) {
        if self.selected_channel_id() != Some(channel_id) {
            return;
        }
        self.messages.pinned_message_view_return_target = Some(PinnedMessageViewReturnTarget {
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

    pub fn return_from_pinned_message_view(&mut self) -> bool {
        if !self.is_pinned_message_view_active() {
            return false;
        }
        let Some(target) = self.messages.pinned_message_view_return_target else {
            return false;
        };
        if self.selected_channel_id() != Some(target.channel_id) {
            self.messages.pinned_message_view_return_target = None;
            return false;
        }

        self.messages.pinned_message_view_channel_id = None;
        self.messages.pinned_message_view_return_target = None;
        self.messages.selected_message = target.selected_message;
        self.messages.message_scroll = target.message_scroll;
        self.messages.message_line_scroll = target.message_line_scroll;
        self.messages.message_keep_selection_visible = target.message_keep_selection_visible;
        self.messages.message_auto_follow = target.message_auto_follow;
        self.messages.new_messages_marker_message_id = target.new_messages_marker_message_id;
        self.messages.unread_divider_last_acked_id = target.unread_divider_last_acked_id;
        self.messages.pending_unread_anchor_scroll = target.pending_unread_anchor_scroll;
        self.clamp_message_viewport();
        true
    }

    pub(super) fn is_pinned_message_view_active(&self) -> bool {
        matches!(
            self.message_pane_source(),
            Some(MessagePaneSource::PinnedMessages { .. })
        )
    }

    pub fn pinned_message_view_channel_id(&self) -> Option<Id<ChannelMarker>> {
        match self.message_pane_source()? {
            MessagePaneSource::PinnedMessages { channel_id } => Some(channel_id),
            MessagePaneSource::ChannelMessages { .. }
            | MessagePaneSource::ForumPosts { .. }
            | MessagePaneSource::ChannelThreads { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn is_pinned_message_view(&self) -> bool {
        self.is_pinned_message_view_active()
    }

    /// Open a non-forum channel's threads as cards in the message pane (forum
    /// channels already show their own post list). Captures a return target so
    /// Esc restores the prior channel view.
    pub fn enter_channel_thread_list_view(&mut self, channel_id: Id<ChannelMarker>) {
        if self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| channel.is_forum())
        {
            return;
        }
        if !self.is_channel_thread_list_view_active() {
            self.record_channel_thread_list_view_return_target(channel_id);
        }
        self.messages.thread_list_view_channel_id = Some(channel_id);
        self.messages.selected_message = 0;
        self.messages.message_scroll = 0;
        self.messages.message_line_scroll = 0;
        self.messages.message_auto_follow = false;
        self.clear_new_messages_marker();
        self.messages.message_keep_selection_visible = true;
        self.clamp_message_viewport();
    }

    fn record_channel_thread_list_view_return_target(&mut self, channel_id: Id<ChannelMarker>) {
        if self.selected_channel_id() != Some(channel_id) {
            return;
        }
        self.messages.thread_list_view_return_target = Some(ThreadListViewReturnTarget {
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

    pub fn return_from_channel_thread_list_view(&mut self) -> bool {
        if !self.is_channel_thread_list_view_active() {
            return false;
        }
        let Some(target) = self.messages.thread_list_view_return_target else {
            return false;
        };
        if self.selected_channel_id() != Some(target.channel_id) {
            self.messages.thread_list_view_return_target = None;
            return false;
        }

        self.messages.thread_list_view_channel_id = None;
        self.messages.thread_list_view_return_target = None;
        self.messages.selected_message = target.selected_message;
        self.messages.message_scroll = target.message_scroll;
        self.messages.message_line_scroll = target.message_line_scroll;
        self.messages.message_keep_selection_visible = target.message_keep_selection_visible;
        self.messages.message_auto_follow = target.message_auto_follow;
        self.messages.new_messages_marker_message_id = target.new_messages_marker_message_id;
        self.messages.unread_divider_last_acked_id = target.unread_divider_last_acked_id;
        self.messages.pending_unread_anchor_scroll = target.pending_unread_anchor_scroll;
        self.clamp_message_viewport();
        true
    }

    pub(super) fn is_channel_thread_list_view_active(&self) -> bool {
        matches!(
            self.message_pane_source(),
            Some(MessagePaneSource::ChannelThreads { .. })
        )
    }

    #[cfg(test)]
    pub fn is_channel_thread_list_view(&self) -> bool {
        self.is_channel_thread_list_view_active()
    }

    pub fn selected_message_state(&self) -> Option<&MessageState> {
        if !self.message_pane_uses_message_items() {
            return None;
        }
        self.messages().get(self.selected_message()).copied()
    }

    pub(crate) fn reply_target_message_state(&self) -> Option<&MessageState> {
        let message_id = self.composer.reply_target_message_id?;
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
    }

    pub fn next_older_history_command(&mut self) -> Option<AppCommand> {
        if !self.message_pane_can_load_message_history() {
            return None;
        }
        let channel_id = self.selected_message_history_channel_id()?;
        let before = self.older_history_cursor()?;
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(before),
        })
    }

    pub fn next_older_history_command_for_half_page_up(&mut self) -> Option<AppCommand> {
        if !self.message_pane_can_load_message_history() {
            return None;
        }
        let channel_id = self.selected_message_history_channel_id()?;
        let before = self.older_history_viewport_cursor()?;
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(before),
        })
    }

    pub(super) fn select_loaded_referenced_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if self.selected_channel_id() != Some(channel_id) {
            return;
        }
        self.select_message_by_id(message_id);
    }

    fn select_message_by_id(&mut self, message_id: Id<MessageMarker>) -> bool {
        let Some(index) = self
            .messages()
            .iter()
            .position(|message| message.id == message_id)
        else {
            return false;
        };
        self.messages.selected_message = index;
        self.messages.message_keep_selection_visible = true;
        self.messages.message_auto_follow = false;
        self.clamp_message_viewport();
        true
    }

    fn older_history_cursor(&self) -> Option<Id<MessageMarker>> {
        if !self.message_pane_can_load_message_history()
            || self.navigation.focus != FocusPane::Messages
            || self.messages().is_empty()
            || self.selected_message() != 0
        {
            return None;
        }

        self.messages().first().map(|message| message.id)
    }

    fn older_history_viewport_cursor(&self) -> Option<Id<MessageMarker>> {
        if !self.message_pane_can_load_message_history()
            || self.navigation.focus != FocusPane::Messages
            || self.messages().is_empty()
            || self.messages.message_scroll != 0
            || self.messages.message_line_scroll != 0
        {
            return None;
        }

        self.messages().first().map(|message| message.id)
    }

    pub fn missing_thread_preview_load_requests(
        &self,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        let mut seen = HashSet::new();
        self.visible_messages()
            .into_iter()
            .filter_map(|message| {
                let summary = self.thread_summary_for_message(message)?;
                let latest_message_id = summary.latest_message_id?;
                summary
                    .latest_message_preview
                    .is_none()
                    .then_some((summary.channel_id, latest_message_id))
            })
            .filter(|key| seen.insert(*key))
            .collect()
    }
}

fn message_preview_text(content: Option<&str>, sticker_names: &[String]) -> Option<String> {
    content
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            sticker_names
                .first()
                .map(|name| format!("[Sticker: {name}]"))
        })
}
