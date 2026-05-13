pub(in crate::tui) const MESSAGE_ROW_GAP: usize = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct MessageRowMetrics {
    pub(in crate::tui) top_rows: usize,
    pub(in crate::tui) header_rows: usize,
    pub(in crate::tui) content_rows: usize,
    pub(in crate::tui) reaction_rows: usize,
    pub(in crate::tui) preview_rows: usize,
    pub(in crate::tui) selected_extra_top_rows: usize,
    pub(in crate::tui) selected_extra_bottom_rows: usize,
    pub(in crate::tui) bottom_gap_rows: usize,
}

impl MessageRowMetrics {
    pub(in crate::tui) fn body_rows(self) -> usize {
        self.header_rows.saturating_add(self.content_rows)
    }

    pub(in crate::tui) fn base_rows(self) -> usize {
        self.body_rows().saturating_add(self.reaction_rows)
    }

    pub(in crate::tui) fn body_top_offset(self) -> usize {
        self.top_rows.saturating_add(self.selected_extra_top_rows)
    }

    pub(in crate::tui) fn reaction_top_offset(self) -> usize {
        self.body_top_offset()
            .saturating_add(self.body_rows())
            .saturating_add(self.preview_rows)
    }

    pub(in crate::tui) fn total_rows(self) -> usize {
        self.top_rows
            .saturating_add(self.base_rows())
            .saturating_add(self.preview_rows)
            .saturating_add(self.selected_extra_top_rows)
            .saturating_add(self.selected_extra_bottom_rows)
            .saturating_add(self.bottom_gap_rows)
    }

    pub(in crate::tui) fn visible_rows_after_scroll(self, line_offset: usize) -> usize {
        self.total_rows().saturating_sub(line_offset)
    }
}
