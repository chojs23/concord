use std::time::Duration;

use crate::discord::MessageState;

use super::*;
use crate::tui::{media, message_format, message_rows::MessageRowMetrics, message_time};

const AUTHOR_GROUP_MAX_GAP: Duration = Duration::from_secs(5 * 60);

impl DashboardState {
    pub(crate) fn message_scroll_row_position(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.selected_channel_is_forum() {
            return self
                .selected_forum_post_items()
                .into_iter()
                .take(self.message_scroll)
                .map(|post| post.rendered_height())
                .sum();
        }
        (0..self.message_scroll)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .saturating_add(self.message_line_scroll)
    }

    pub(crate) fn message_total_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.selected_channel_is_forum() {
            return self
                .selected_forum_post_items()
                .into_iter()
                .map(|post| post.rendered_height())
                .sum();
        }
        (0..self.messages().len())
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    /// Returns true when the message at `index` (within `self.messages()`)
    /// should be preceded by a date separator because its local date differs
    /// from the previous message's, or because it is the first loaded message
    /// and needs day context at the top of the pane.
    pub(crate) fn message_starts_new_day_at(&self, index: usize) -> bool {
        let messages = self.messages();
        let Some(current) = messages.get(index) else {
            return true;
        };
        let previous_id = index
            .checked_sub(1)
            .and_then(|prev_index| messages.get(prev_index).map(|message| message.id));
        message_time::message_starts_new_day(current.id, previous_id)
    }

    /// Number of extra rows that the message at `index` reserves above its
    /// avatar/header line. These rows are painted by `message_viewport_lines`
    /// before the message body, so scroll and media-target math must use the
    /// same count as the renderer.
    pub(crate) fn message_extra_top_lines(&self, index: usize) -> usize {
        let mut extra = usize::from(self.message_starts_new_day_at(index));
        if self.should_draw_unread_divider_at(index) {
            extra += 1;
        }
        extra
    }

    pub(crate) fn message_starts_author_group_at(&self, index: usize) -> bool {
        let messages = self.messages();
        let Some(current) = messages.get(index) else {
            return false;
        };
        if index == 0 || self.message_extra_top_lines(index) > 0 {
            return true;
        }
        messages.get(index - 1).is_none_or(|previous| {
            previous.author_id != current.author_id
                || messages_exceed_author_group_gap(previous, current)
        })
    }

    pub(crate) fn message_header_line_count_at(&self, index: usize) -> usize {
        usize::from(self.message_starts_author_group_at(index))
    }

    pub(crate) fn message_bottom_gap_after(&self, index: usize) -> usize {
        usize::from(self.message_has_bottom_gap_after(index))
            * crate::tui::message_rows::MESSAGE_ROW_GAP
    }

    fn selected_message_extra_top_line_at(&self, index: usize) -> usize {
        usize::from(
            self.messages().get(index).is_some()
                && index == self.selected_message
                && !self.message_starts_author_group_at(index),
        )
    }

    fn message_has_bottom_gap_after(&self, index: usize) -> bool {
        let Some(next) = index.checked_add(1) else {
            return true;
        };
        next >= self.messages().len() || self.message_starts_author_group_at(next)
    }

    pub(in crate::tui) fn message_row_metrics_at_with_selected_bottom(
        &self,
        index: usize,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
        selected_for_bottom: bool,
    ) -> MessageRowMetrics {
        let (body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        let previews = message.inline_previews();
        let album = media::image_preview_album_layout(&previews, preview_width, max_preview_height);
        MessageRowMetrics {
            top_rows: self.message_extra_top_lines(index),
            header_rows: self.message_header_line_count_at(index),
            content_rows: body_lines.len(),
            reaction_rows: reaction_lines.len(),
            preview_rows: album
                .height
                .saturating_add(usize::from(album.overflow_count > 0)),
            selected_extra_top_rows: self.selected_message_extra_top_line_at(index),
            selected_extra_bottom_rows: usize::from(
                selected_for_bottom && !self.message_has_bottom_gap_after(index),
            ),
            bottom_gap_rows: self.message_bottom_gap_after(index),
        }
    }

    pub(super) fn selected_message_rendered_row(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let span = self.selected_message.saturating_sub(self.message_scroll);
        let row: usize = (0..span)
            .map(|offset| {
                self.message_rendered_height_at(
                    self.message_scroll + offset,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum();
        row.saturating_sub(self.message_line_scroll)
    }

    pub(super) fn selected_message_rendered_height(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.messages().get(self.selected_message).is_none() {
            return 1;
        }
        self.message_rendered_height_at(
            self.selected_message,
            content_width,
            preview_width,
            max_preview_height,
        )
    }

    pub(super) fn following_message_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
        count: usize,
    ) -> usize {
        let messages_len = self.messages().len();
        let start = self.selected_message.saturating_add(1);
        (0..count)
            .map(|offset| start + offset)
            .take_while(|&index| index < messages_len)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn message_base_line_count_for_width(
        &self,
        message: &MessageState,
        content_width: usize,
    ) -> usize {
        let (body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        1 + body_lines.len() + reaction_lines.len()
    }

    pub(super) fn message_rendered_height(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let (body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        let previews = message.inline_previews();
        let album = media::image_preview_album_layout(&previews, preview_width, max_preview_height);
        1 + body_lines.len()
            + reaction_lines.len()
            + album
                .height
                .saturating_add(usize::from(album.overflow_count > 0))
            + crate::tui::message_rows::MESSAGE_ROW_GAP
    }

    /// Same as `message_rendered_height` but also accounts for an optional
    /// date-separator line above the message body. Use this everywhere the
    /// caller knows the message's index inside `self.messages()` so scroll
    /// math stays consistent with what the renderer actually paints.
    pub(super) fn message_rendered_height_at(
        &self,
        index: usize,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let messages = self.messages();
        let Some(message) = messages.get(index).copied() else {
            return 0;
        };
        self.message_row_metrics_at_with_selected_bottom(
            index,
            message,
            content_width,
            preview_width,
            max_preview_height,
            index == self.selected_message,
        )
        .total_rows()
    }
}

fn messages_exceed_author_group_gap(previous: &MessageState, current: &MessageState) -> bool {
    let previous_created = Duration::from_millis(message_time::message_unix_millis(previous.id));
    let current_created = Duration::from_millis(message_time::message_unix_millis(current.id));
    current_created.saturating_sub(previous_created) >= AUTHOR_GROUP_MAX_GAP
}
