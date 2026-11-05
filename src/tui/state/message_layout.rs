use std::time::Duration;

use crate::discord::{Id, MessageState, ReactionEmoji, marker::MessageMarker};

use super::{MessageRowContentMetrics, MessageRowContentMetricsCacheKey, *};
use crate::tui::{
    media,
    message::{
        format as message_format,
        layout::{MessageViewportPlan, standalone_message_rendered_height},
        rows::MessageRowMetrics,
        time as message_time,
    },
};

const AUTHOR_GROUP_MAX_GAP: Duration = Duration::from_secs(7 * 60);

impl DashboardState {
    pub(crate) fn message_scroll_row_position(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.message_pane_uses_thread_cards() {
            return self.thread_card_rendered_rows_before(self.messages.message_scroll);
        }
        (0..self.messages.message_scroll)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .saturating_add(self.messages.message_line_scroll)
    }

    pub(crate) fn message_total_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.message_pane_uses_thread_cards() {
            return self.thread_card_total_rendered_rows();
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
            // Discord webhooks can override their username per message.
            // Discord starts a new group when that username changes, but
            // keeps avatar-only changes in the existing group.
            let webhook_username_changed =
                current.webhook_id.is_some() && previous.author != current.author;
            previous.author_id != current.author_id
                || webhook_username_changed
                || messages_exceed_author_group_gap(previous, current)
        })
    }

    pub(crate) fn message_header_line_count_at(&self, index: usize) -> usize {
        usize::from(self.message_starts_author_group_at(index))
    }

    pub(crate) fn message_bottom_gap_after(&self, index: usize) -> usize {
        usize::from(self.message_has_bottom_gap_after(index))
            * crate::tui::message::rows::MESSAGE_ROW_GAP
    }

    fn selected_message_extra_top_line_at(&self, index: usize) -> usize {
        usize::from(
            self.messages().get(index).is_some()
                && index == self.messages.selected_message
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
        let content_metrics = self.message_row_content_metrics(
            index,
            message,
            content_width,
            preview_width,
            max_preview_height,
        );
        MessageRowMetrics {
            top_rows: self.message_extra_top_lines(index),
            header_rows: self.message_header_line_count_at(index),
            content_rows: content_metrics.content_rows,
            reaction_rows: content_metrics.reaction_rows,
            preview_rows: content_metrics.preview_rows,
            selected_extra_top_rows: self.selected_message_extra_top_line_at(index),
            selected_extra_bottom_rows: usize::from(
                selected_for_bottom && !self.message_has_bottom_gap_after(index),
            ),
            bottom_gap_rows: self.message_bottom_gap_after(index),
        }
    }

    fn message_row_content_metrics(
        &self,
        index: usize,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> MessageRowContentMetrics {
        let messages = self.messages();
        let Some(state_message) = messages.get(index) else {
            return self.compute_message_row_content_metrics(
                message,
                content_width,
                preview_width,
                max_preview_height,
            );
        };
        if state_message.id != message.id {
            return self.compute_message_row_content_metrics(
                message,
                content_width,
                preview_width,
                max_preview_height,
            );
        }

        let key = MessageRowContentMetricsCacheKey {
            message_id: message.id.get(),
            content_width,
            preview_width,
            max_preview_height,
            show_custom_emoji: self.show_custom_emoji(),
        };
        if let Some(metrics) = self
            .layout_cache
            .message_row_content_metrics_cache
            .borrow()
            .get(&key)
        {
            return *metrics;
        }

        let metrics = self.compute_message_row_content_metrics(
            message,
            content_width,
            preview_width,
            max_preview_height,
        );
        self.layout_cache
            .message_row_content_metrics_cache
            .borrow_mut()
            .insert(key, metrics);
        metrics
    }

    fn compute_message_row_content_metrics(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> MessageRowContentMetrics {
        let (mut body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        body_lines.extend(message_format::format_message_translation_lines(
            message,
            self,
            content_width,
        ));
        let previews = message.flow_inline_previews();
        let album = media::image_preview_album_layout(&previews, preview_width, max_preview_height);
        MessageRowContentMetrics {
            content_rows: body_lines.len(),
            reaction_rows: reaction_lines.len(),
            preview_rows: album
                .height
                .saturating_add(usize::from(album.overflow_count > 0)),
        }
    }

    pub(super) fn selected_message_rendered_row(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let span = self
            .messages
            .selected_message
            .saturating_sub(self.messages.message_scroll);
        let row: usize = (0..span)
            .map(|offset| {
                self.message_rendered_height_at(
                    self.messages.message_scroll + offset,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum();
        row.saturating_sub(self.messages.message_line_scroll)
    }

    pub(super) fn selected_message_rendered_height(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self
            .messages()
            .get(self.messages.selected_message)
            .is_none()
        {
            return 1;
        }
        self.message_rendered_height_at(
            self.messages.selected_message,
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
        let start = self.messages.selected_message.saturating_add(1);
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
        let (mut body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        body_lines.extend(message_format::format_message_translation_lines(
            message,
            self,
            content_width,
        ));
        1 + body_lines.len() + reaction_lines.len()
    }

    pub(super) fn message_rendered_height(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let (mut body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        body_lines.extend(message_format::format_message_translation_lines(
            message,
            self,
            content_width,
        ));
        let previews = message.inline_previews();
        let album = media::image_preview_album_layout(&previews, preview_width, max_preview_height);
        standalone_message_rendered_height(
            body_lines.len(),
            reaction_lines.len(),
            album
                .height
                .saturating_add(usize::from(album.overflow_count > 0)),
        )
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
            index == self.messages.selected_message,
        )
        .total_rows()
    }

    /// Resolves a click at the given viewport row/column to the reaction
    /// chip it landed on, if any. `row`/`col` are relative to the top-left of
    /// the message list area; `avatar_offset` is the left gutter width the
    /// renderer reserves before message content (see `avatar_gutter_width`),
    /// which the caller must supply since it is a rendering concern, not
    /// cached state. Walks visible messages the same way
    /// `select_visible_message_row` does, then recomputes that message's
    /// reaction layout to pinpoint the chip, mirroring how `InteractionMap`
    /// derives every other hit region on demand rather than caching one.
    pub(in crate::tui) fn reaction_chip_at_viewport_position(
        &self,
        row: usize,
        col: usize,
        avatar_offset: u16,
    ) -> Option<(Id<MessageMarker>, ReactionEmoji)> {
        if self.message_pane_uses_thread_cards() {
            return None;
        }
        if self.messages.message_content_width == usize::MAX {
            return None;
        }
        let col = col.checked_sub(avatar_offset as usize)?;
        let content_width = self.messages.message_content_width;
        let preview_width = self.messages.message_preview_width;
        let max_preview_height = self.messages.message_max_preview_height;
        let show_custom_emoji = self.show_custom_emoji();
        let selected = self.focused_message_selection();

        let mut rendered_row = 0usize;
        for local_index in 0..self.visible_messages().len() {
            let index = self.messages.message_scroll.saturating_add(local_index);
            let message = self.messages().get(index).copied()?;
            let mut metrics = self.message_row_metrics_at_with_selected_bottom(
                index,
                message,
                content_width,
                preview_width,
                max_preview_height,
                selected == Some(local_index),
            );
            // An unfocused grouped message does not render its selection's top border.
            if selected != Some(local_index) {
                metrics.selected_extra_top_rows = 0;
            }
            let rendered_height = metrics.total_rows().max(1);
            let line_offset = usize::from(local_index == 0) * self.messages.message_line_scroll;
            let visible_height = rendered_height.saturating_sub(line_offset);
            if row < rendered_row.saturating_add(visible_height) {
                let local_row = row.saturating_sub(rendered_row).saturating_add(line_offset);
                let reaction_top = metrics.reaction_top_offset();
                if local_row < reaction_top
                    || local_row >= reaction_top.saturating_add(metrics.reaction_rows)
                {
                    return None;
                }
                let reaction_line = u16::try_from(local_row - reaction_top).unwrap_or(u16::MAX);
                let col = u16::try_from(col).unwrap_or(u16::MAX);
                let emoji = message_format::reaction_emoji_at(
                    &message.reactions,
                    content_width.max(8),
                    show_custom_emoji,
                    reaction_line,
                    col,
                )?;
                return Some((message.id, emoji));
            }
            rendered_row = rendered_row.saturating_add(visible_height);
        }
        None
    }

    /// Resolves a click at the given viewport row/column to the inline image
    /// preview thumbnail it landed on, if any, identified by message id and
    /// attachment filename. `row`/`col` are relative to the top-left of the
    /// message list area, as in `reaction_chip_at_viewport_position`. Walks
    /// the same per-message layout the renderer uses to place inline preview
    /// albums (`media::image_preview_album_layout`, consumed by
    /// `inline_preview_spacer_for_message`) and recomputes album cell
    /// geometry on demand, mirroring how `InteractionMap` derives every other
    /// hit region rather than caching one. Does not cover thread-card or
    /// Components-V2 section thumbnails.
    pub(in crate::tui) fn image_preview_at_viewport_position(
        &self,
        row: usize,
        col: usize,
        avatar_offset: u16,
    ) -> Option<(Id<MessageMarker>, String)> {
        if self.message_pane_uses_thread_cards() {
            return None;
        }
        if self.messages.message_content_width == usize::MAX {
            return None;
        }
        let col = col.checked_sub(avatar_offset as usize)?;
        let content_width = self.messages.message_content_width;
        let preview_width = self.messages.message_preview_width;
        let max_preview_height = self.messages.message_max_preview_height;
        let messages = self.visible_messages();
        let selected = self.focused_message_selection();
        let plan = MessageViewportPlan::new(
            &messages,
            selected,
            self,
            content_width,
            preview_width,
            max_preview_height,
        );
        let row = row as isize;
        let col = u16::try_from(col).unwrap_or(u16::MAX);

        for viewport_row in plan.rows() {
            let previews = viewport_row.message.flow_inline_previews();
            if previews.is_empty() {
                continue;
            }
            let album =
                media::image_preview_album_layout(&previews, preview_width, max_preview_height);
            let preview_top = viewport_row.body_top + viewport_row.metrics.body_rows() as isize;
            for cell in &album.cells {
                let cell_top = preview_top.saturating_add(cell.y_offset_rows as isize);
                if row < cell_top || row >= cell_top.saturating_add(cell.height as isize) {
                    continue;
                }
                if col < cell.x_offset_columns
                    || col >= cell.x_offset_columns.saturating_add(cell.width)
                {
                    continue;
                }
                let filename = previews[cell.preview_index].filename.to_owned();
                return Some((viewport_row.message.id, filename));
            }
        }
        None
    }

    /// Builds the add/remove-reaction command that toggles the current
    /// user's own reaction with `emoji` on `message`, honoring add-reaction
    /// permission the same way the emoji-reaction picker does (see
    /// `activate_selected_emoji_reaction`). Returns `None` when the user has
    /// not already reacted and lacks permission to add a new reaction.
    pub(in crate::tui) fn toggle_own_reaction_command(
        &self,
        message: &MessageState,
        emoji: ReactionEmoji,
    ) -> Option<AppCommand> {
        let already_reacted = message
            .reactions
            .iter()
            .any(|existing| existing.me && existing.emoji == emoji);
        if !already_reacted && !self.can_add_reaction_to_message(message, &emoji) {
            return None;
        }
        Some(if already_reacted {
            AppCommand::RemoveReaction {
                channel_id: message.channel_id,
                message_id: message.id,
                emoji,
            }
        } else {
            AppCommand::AddReaction {
                channel_id: message.channel_id,
                message_id: message.id,
                emoji,
            }
        })
    }

    /// Builds the toggle command and selects the clicked message by identity,
    /// since changing focus or selection can move grouped-message borders.
    pub(in crate::tui) fn toggle_reaction_chip_at(
        &mut self,
        row: usize,
        col: usize,
        avatar_offset: u16,
    ) -> Option<AppCommand> {
        let (message_id, emoji) =
            self.reaction_chip_at_viewport_position(row, col, avatar_offset)?;
        let (index, message) = self
            .messages()
            .into_iter()
            .enumerate()
            .find(|(_, message)| message.id == message_id)?;
        let command = self.toggle_own_reaction_command(message, emoji);
        self.focus_pane(FocusPane::Messages);
        self.messages.selected_message = index;
        self.messages.message_auto_follow = false;
        self.messages.message_keep_selection_visible = false;
        command
    }
}

fn messages_exceed_author_group_gap(previous: &MessageState, current: &MessageState) -> bool {
    let previous_created = Duration::from_millis(message_time::message_unix_millis(previous.id));
    let current_created = Duration::from_millis(message_time::message_unix_millis(current.id));
    current_created.saturating_sub(previous_created) >= AUTHOR_GROUP_MAX_GAP
}
