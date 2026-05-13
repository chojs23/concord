use crate::discord::MessageState;
use crate::discord::ids::{Id, marker::MessageMarker};

use super::scroll::{
    SCROLL_OFF, clamp_list_scroll, move_index_down, move_index_up, normalize_message_line_scroll,
    pane_content_height, scroll_message_row_down, scroll_message_row_up,
};
use super::*;

impl DashboardState {
    pub fn selected_message(&self) -> usize {
        clamp_selected_index(self.selected_message, self.message_pane_item_count())
    }

    pub(crate) fn message_scroll(&self) -> usize {
        self.message_scroll
    }

    pub(crate) fn new_messages_count(&self) -> usize {
        let Some(marker_id) = self.new_messages_marker_message_id else {
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
        if self.is_pinned_message_view_active() {
            return None;
        }
        let last_acked = self.unread_divider_last_acked_id?;
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
        if self.is_pinned_message_view_active() {
            return None;
        }
        let last_acked = self.unread_divider_last_acked_id?;
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
        self.unread_divider_last_acked_id
    }

    #[cfg(test)]
    pub fn new_messages_marker_message_id(&self) -> Option<Id<MessageMarker>> {
        self.new_messages_marker_message_id
    }

    #[cfg(test)]
    pub fn message_auto_follow(&self) -> bool {
        self.message_auto_follow
    }

    #[cfg(test)]
    pub fn message_view_height(&self) -> usize {
        self.message_view_height
    }

    pub fn visible_messages(&self) -> Vec<&MessageState> {
        self.messages()
            .into_iter()
            .skip(self.message_scroll)
            .take(self.message_content_height())
            .collect()
    }

    pub fn message_line_scroll(&self) -> usize {
        self.message_line_scroll
    }

    pub fn set_message_view_height(&mut self, height: usize) {
        self.message_view_height = height;
        self.clamp_message_viewport();
    }

    pub fn clamp_message_viewport_for_image_previews(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        self.message_content_width = content_width;
        self.message_preview_width = preview_width;
        self.message_max_preview_height = max_preview_height;
        // Retry the unread-anchor snap until the originally-acked message
        // is loaded. After it fires once, the pending flag clears and this
        // is a cheap no-op.
        self.try_apply_unread_anchor_scroll();
        self.clamp_message_viewport();
        if self.message_auto_follow {
            if self.message_view_height <= 1 {
                self.message_scroll = self.selected_message();
                self.message_line_scroll = 0;
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
        if self.messages().is_empty() || !self.message_keep_selection_visible {
            return;
        }
        if self.selected_message() == 0 {
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        let height = self.message_content_height();
        if self.selected_message() == 1 && self.message_scroll == 0 && self.message_line_scroll == 0
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
            if selected_bottom > lower_bound && self.message_scroll < self.selected_message {
                self.scroll_message_viewport_down_one_row(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                continue;
            }

            if selected_row < upper_scrolloff && self.message_scroll > 0 {
                let previous_height = self.message_rendered_height_at(
                    self.message_scroll.saturating_sub(1),
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
        if self.selected_channel_is_forum() {
            return self.focused_forum_post_selection();
        }
        if self.focus == FocusPane::Messages && !self.messages().is_empty() {
            let selected = self.selected_message();
            let visible_count = self.visible_messages().len();
            if selected >= self.message_scroll && selected < self.message_scroll + visible_count {
                Some(selected - self.message_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn scroll_message_viewport_down(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }

        if self.selected_channel_is_forum() {
            let len = self.selected_forum_post_items().len();
            move_index_down(&mut self.message_scroll, len);
            self.message_auto_follow = false;
            self.message_keep_selection_visible = false;
            return;
        }

        let viewport_height = self.message_content_height();
        let current_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                self.message_content_width,
                self.message_preview_width,
                self.message_max_preview_height,
            )
            .max(1)
        });
        let (new_top, new_offset) = match current_height {
            None => return,
            Some(h) if self.message_line_scroll + 1 < h => {
                (self.message_scroll, self.message_line_scroll + 1)
            }
            _ => (self.message_scroll.saturating_add(1), 0),
        };
        if !self.message_viewport_has_rows_below(new_top, new_offset, viewport_height) {
            return;
        }
        // Viewport scrolling intentionally drops auto-follow so that the
        // user can over-scroll without the next render re-aligning to the
        // natural bottom. The event handler still re-engages follow when a
        // new message arrives and the viewport actually shows the latest,
        // via `is_viewport_at_latest_message()`.
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_down_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        if self.is_viewport_at_latest_message() {
            self.clear_new_messages_marker();
            self.normalize_message_line_scroll(
                self.message_content_width,
                self.message_preview_width,
                self.message_max_preview_height,
            );
        }
    }

    pub fn scroll_message_viewport_up(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        if self.selected_channel_is_forum() {
            move_index_up(&mut self.message_scroll);
            self.message_auto_follow = false;
            self.message_keep_selection_visible = false;
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_up_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn scroll_message_viewport_top(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    pub fn scroll_message_viewport_bottom(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.clear_new_messages_marker();
        self.align_message_viewport_to_bottom(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        self.refresh_message_auto_follow();
    }

    pub(super) fn select_visible_message_row(&mut self, row: usize) -> bool {
        if self.selected_channel_is_forum() {
            return self.select_visible_forum_post_row(row);
        }
        if self.message_content_width == usize::MAX {
            return false;
        }

        let mut rendered_row = 0usize;
        for local_index in 0..self.visible_messages().len() {
            let index = self.message_scroll.saturating_add(local_index);
            let rendered_height = self
                .message_rendered_height_at(
                    index,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
                )
                .max(1);
            let visible_height = if local_index == 0 {
                rendered_height.saturating_sub(self.message_line_scroll)
            } else {
                rendered_height
            };
            if row < rendered_row.saturating_add(visible_height) {
                self.selected_message = index;
                self.message_auto_follow = false;
                self.message_keep_selection_visible = false;
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
        if self.selected_channel_is_forum() || self.is_pinned_message_view_active() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        self.selected_message >= messages.len().saturating_sub(1)
    }

    /// Returns true when the rendered viewport shows the bottom of the latest
    /// message, regardless of where the cursor is parked. This is the
    /// auto-scroll trigger condition. With no rendered width yet in unit tests,
    /// falls back to an item-count check against the configured view height.
    pub(super) fn is_viewport_at_latest_message(&self) -> bool {
        if self.selected_channel_is_forum() || self.is_pinned_message_view_active() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        let viewport = self.message_content_height();
        if self.message_content_width == usize::MAX {
            return self.message_scroll.saturating_add(viewport) >= messages.len();
        }
        let total = self.message_total_rendered_rows(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        let pos = self.message_scroll_row_position(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
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
        self.message_auto_follow =
            self.cursor_on_last_message() && self.is_viewport_at_latest_message();
        if self.message_auto_follow {
            self.clear_new_messages_marker();
            // Once the user has caught up (cursor + viewport on the
            // latest), retire the unread divider/banner so the indicator
            // doesn't linger after every unread message has been read.
            self.unread_divider_last_acked_id = None;
            self.pending_unread_anchor_scroll = false;
        }
    }

    pub(super) fn clear_new_messages_marker(&mut self) {
        self.new_messages_marker_message_id = None;
    }

    pub(super) fn clear_missing_new_messages_marker(&mut self) {
        if let Some(marker_id) = self.new_messages_marker_message_id
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
        self.selected_message = self.message_pane_item_count().saturating_sub(1);
        self.message_keep_selection_visible = true;
    }

    /// Snap the viewport so the user's last-read message sits at the top of
    /// the message pane and the unread divider is visible just below it.
    /// No-op until the captured `last_acked` snowflake is resolvable from
    /// the loaded slice. The call is retried each frame so the snap fires
    /// once history streams in. Once applied, the pending flag clears so
    /// subsequent navigation is not pinned to the anchor.
    pub(crate) fn try_apply_unread_anchor_scroll(&mut self) {
        if !self.pending_unread_anchor_scroll {
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
        self.message_scroll = divider_index.saturating_sub(1);
        self.message_line_scroll = 0;
        self.selected_message = divider_index.min(item_count.saturating_sub(1));
        self.message_keep_selection_visible = false;
        self.message_auto_follow = false;
        self.pending_unread_anchor_scroll = false;
    }

    fn align_message_viewport_to_bottom(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.selected_channel_is_forum() {
            self.clamp_forum_post_viewport();
            self.message_line_scroll = 0;
            return;
        }
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .message_rendered_height_at(index, content_width, preview_width, max_preview_height)
                .max(1);
            if message_height >= remaining {
                self.message_scroll = index;
                self.message_line_scroll = message_height.saturating_sub(remaining);
                return;
            }
            remaining = remaining.saturating_sub(message_height);
        }
        self.message_scroll = 0;
        self.message_line_scroll = 0;
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
            self.selected_message = index;
        }
        if let Some(message_id) = scroll_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.message_scroll = index;
        }
    }

    pub(super) fn clamp_message_viewport(&mut self) {
        let item_count = self.message_pane_item_count();
        if item_count == 0 {
            self.selected_message = 0;
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        self.selected_message = self.selected_message.min(item_count - 1);
        self.message_scroll = self.message_scroll.min(item_count - 1);
        if self.selected_channel_is_forum() {
            self.clamp_forum_post_viewport();
            self.message_line_scroll = 0;
            return;
        }
        if self.message_content_width == usize::MAX {
            self.message_scroll = clamp_list_scroll(
                self.selected_message,
                self.message_scroll,
                self.message_content_height(),
                item_count,
            );
            if self.message_scroll != self.selected_message {
                self.message_line_scroll = 0;
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

        self.message_scroll = top;
        self.message_line_scroll = offset;
        true
    }

    fn message_viewport_has_rows_below(&self, top: usize, offset: usize, height: usize) -> bool {
        let mut visible_rows = 0usize;
        for offset_from_top in 0..self.messages().len().saturating_sub(top) {
            let global_index = top + offset_from_top;
            let message_height = self
                .message_rendered_height_at(
                    global_index,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
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
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        scroll_message_row_down(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            messages_len,
            current_message_height,
        );
    }

    fn scroll_message_viewport_up_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.message_line_scroll > 0 {
            scroll_message_row_up(
                &mut self.message_scroll,
                &mut self.message_line_scroll,
                None,
            );
            return;
        }
        let previous_message_index = self.message_scroll.checked_sub(1);
        let previous_message_height = previous_message_index.map(|index| {
            self.message_rendered_height_at(index, content_width, preview_width, max_preview_height)
        });
        scroll_message_row_up(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            previous_message_height,
        );
    }

    fn normalize_message_line_scroll(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        normalize_message_line_scroll(&mut self.message_line_scroll, current_message_height);
    }

    pub(super) fn message_content_height(&self) -> usize {
        pane_content_height(self.message_view_height)
    }

    pub(super) fn message_pane_item_count(&self) -> usize {
        if self.selected_channel_is_forum() {
            self.selected_forum_post_items().len()
        } else {
            self.messages().len()
        }
    }
}
