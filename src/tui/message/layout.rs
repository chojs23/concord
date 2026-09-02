use crate::discord::MessageState;
use crate::tui::state::DashboardState;

use super::rows::{MESSAGE_ROW_GAP, MessageRowMetrics};

#[derive(Clone, Copy, Debug)]
pub(in crate::tui) struct MessageViewportRow<'a> {
    pub(in crate::tui) global_index: usize,
    pub(in crate::tui) message: &'a MessageState,
    pub(in crate::tui) metrics: MessageRowMetrics,
    pub(in crate::tui) message_top: isize,
    pub(in crate::tui) body_top: isize,
    pub(in crate::tui) reaction_top: isize,
    pub(in crate::tui) line_offset: usize,
    pub(in crate::tui) body_skip: usize,
    pub(in crate::tui) item_line_offset: usize,
    pub(in crate::tui) show_header: bool,
    pub(in crate::tui) bottom_gap: bool,
    pub(in crate::tui) selected: bool,
    pub(in crate::tui) starts_new_day: bool,
    pub(in crate::tui) shows_unread_divider: bool,
}

impl MessageViewportRow<'_> {
    pub(in crate::tui) fn image_preview_row(
        &self,
        body_line_index: Option<usize>,
        y_offset_rows: usize,
    ) -> isize {
        match body_line_index {
            Some(line_index) => self
                .body_top
                .saturating_add(self.metrics.header_rows as isize)
                .saturating_add(line_index as isize)
                .saturating_add(y_offset_rows as isize),
            None => self
                .body_top
                .saturating_add(self.metrics.body_rows() as isize)
                .saturating_add(y_offset_rows as isize)
                .saturating_sub(1),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::tui) struct MessageViewportPlan<'a> {
    rows: Vec<MessageViewportRow<'a>>,
}

impl<'a> MessageViewportPlan<'a> {
    pub(in crate::tui) fn new(
        messages: &[&'a MessageState],
        selected: Option<usize>,
        state: &DashboardState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> Self {
        let state_messages = state.messages();
        let mut rendered_rows: isize = 0;
        let mut rows = Vec::with_capacity(messages.len());

        for (local_index, message) in messages.iter().enumerate() {
            let global_index = state.message_scroll().saturating_add(local_index);
            let line_offset = usize::from(local_index == 0) * state.message_line_scroll();
            let has_state_message = state_messages
                .get(global_index)
                .is_some_and(|state_message| state_message.id == message.id);
            let starts_new_day = has_state_message && state.message_starts_new_day_at(global_index);
            let shows_unread_divider =
                has_state_message && state.should_draw_unread_divider_at(global_index);
            let separator_lines = usize::from(starts_new_day) + usize::from(shows_unread_divider);
            let show_header = if has_state_message {
                state.message_starts_author_group_at(global_index)
            } else {
                true
            };
            let bottom_gap = if has_state_message {
                state.message_bottom_gap_after(global_index) > 0
            } else {
                true
            };
            let selected_row = selected == Some(local_index);
            let metrics = state.message_row_metrics_at_with_selected_bottom(
                global_index,
                message,
                content_width,
                preview_width,
                max_preview_height,
                selected_row,
            );
            let message_top = rendered_rows - line_offset as isize;
            let body_top = message_top + metrics.body_top_offset() as isize;
            let reaction_top = message_top + metrics.reaction_top_offset() as isize;
            let body_skip = line_offset.saturating_sub(separator_lines);
            let selected_grouped_continuation = selected_row && !show_header;
            let item_line_offset = if selected_grouped_continuation {
                body_skip.saturating_sub(1)
            } else {
                body_skip
            };

            rows.push(MessageViewportRow {
                global_index,
                message,
                metrics,
                message_top,
                body_top,
                reaction_top,
                line_offset,
                body_skip,
                item_line_offset,
                show_header,
                bottom_gap,
                selected: selected_row,
                starts_new_day,
                shows_unread_divider,
            });

            rendered_rows = rendered_rows
                .saturating_add(metrics.visible_rows_after_scroll(line_offset) as isize);
        }

        Self { rows }
    }

    pub(in crate::tui) fn rows(&self) -> &[MessageViewportRow<'a>] {
        &self.rows
    }

    pub(in crate::tui) fn row(&self, local_index: usize) -> Option<&MessageViewportRow<'a>> {
        self.rows.get(local_index)
    }
}

pub(in crate::tui) fn standalone_message_rendered_height(
    content_rows: usize,
    reaction_rows: usize,
    preview_rows: usize,
) -> usize {
    1 + content_rows + reaction_rows + preview_rows + MESSAGE_ROW_GAP
}
