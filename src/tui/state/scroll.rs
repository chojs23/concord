use std::{collections::HashSet, hash::Hash};

use crate::tui::keybindings::SelectionAction;

pub(in crate::tui) const SCROLL_OFF: usize = 3;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct VerticalScrollState {
    scroll: usize,
    view_height: usize,
    total_lines: usize,
}

impl VerticalScrollState {
    pub(super) fn scroll(&self) -> usize {
        self.scroll
    }

    pub(super) fn set_view_height(&mut self, height: usize) {
        self.view_height = height;
        self.clamp_scroll();
    }

    pub(super) fn set_total_lines(&mut self, total_lines: usize) {
        self.total_lines = total_lines;
        self.clamp_scroll();
    }

    pub(super) fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
        self.clamp_scroll();
    }

    pub(super) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub(super) fn page_down(&mut self) {
        self.scroll = self.scroll.saturating_add((self.view_height / 2).max(1));
        self.clamp_scroll();
    }

    pub(super) fn page_up(&mut self) {
        self.scroll = self.scroll.saturating_sub((self.view_height / 2).max(1));
    }

    pub(super) fn page(&mut self, action: SelectionAction) {
        match action {
            SelectionAction::Next => self.page_down(),
            SelectionAction::Previous => self.page_up(),
        }
    }

    /// Adjusts the offset only when `[start, end)` leaves the viewport. Keeping
    /// an already visible range still is what makes cursor movement feel local.
    pub(super) fn reveal(&mut self, start: usize, end: usize) {
        if start < self.scroll {
            self.scroll = start;
        } else if self.view_height > 0 && end > self.scroll + self.view_height {
            self.scroll = end.saturating_sub(self.view_height);
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let visible = self.view_height.min(self.total_lines);
        self.scroll = self.scroll.min(self.total_lines.saturating_sub(visible));
    }

    pub(super) fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub(super) fn is_near_bottom(&self, threshold: usize) -> bool {
        self.scroll
            .saturating_add(self.view_height)
            .saturating_add(threshold)
            >= self.total_lines
    }
}

pub(super) fn pane_content_height(height: usize) -> usize {
    height.max(1)
}

pub(super) fn clamp_selected_index(selected: usize, len: usize) -> usize {
    selected.min(len.saturating_sub(1))
}

pub(super) fn move_index_down(selected: &mut usize, len: usize) {
    move_index_down_by(selected, len, 1);
}

pub(super) fn move_index_down_by(selected: &mut usize, len: usize, distance: usize) {
    if len == 0 {
        return;
    }
    *selected = selected.saturating_add(distance).min(len - 1);
}

pub(super) fn move_index_up(selected: &mut usize) {
    move_index_up_by(selected, 1);
}

pub(super) fn move_index_up_by(selected: &mut usize, distance: usize) {
    *selected = selected.saturating_sub(distance);
}

pub(super) fn scroll_list_down(scroll: &mut usize, height: usize, len: usize) {
    let max_scroll = len.saturating_sub(height);
    *scroll = scroll.saturating_add(1).min(max_scroll);
}

pub(super) fn scroll_list_up(scroll: &mut usize) {
    *scroll = scroll.saturating_sub(1);
}

pub(super) fn clamp_list_scroll_to_bounds(scroll: &mut usize, height: usize, len: usize) {
    *scroll = (*scroll).min(len.saturating_sub(height));
}

pub(super) fn clamp_list_viewport(
    selected: usize,
    scroll: &mut usize,
    height: usize,
    len: usize,
    keep_selection_visible: bool,
) {
    if keep_selection_visible {
        *scroll = clamp_list_scroll(selected, *scroll, height, len);
    } else {
        clamp_list_scroll_to_bounds(scroll, height, len);
    }
}

pub(super) fn last_index(len: usize) -> usize {
    len.saturating_sub(1)
}

pub(super) fn toggle_collapsed_key<T>(set: &mut HashSet<T>, key: T)
where
    T: Eq + Hash,
{
    if set.contains(&key) {
        set.remove(&key);
    } else {
        set.insert(key);
    }
}

pub(super) fn clamp_list_scroll(
    cursor: usize,
    mut scroll: usize,
    height: usize,
    len: usize,
) -> usize {
    if len == 0 {
        return 0;
    }

    let max_scroll = len.saturating_sub(height);
    scroll = scroll.min(max_scroll);
    let scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);

    let lower_bound = scroll
        .saturating_add(height)
        .saturating_sub(1)
        .saturating_sub(scrolloff);
    if cursor > lower_bound {
        scroll = cursor
            .saturating_add(1)
            .saturating_add(scrolloff)
            .saturating_sub(height);
    }

    let upper_bound = scroll.saturating_add(scrolloff);
    if cursor < upper_bound {
        scroll = cursor.saturating_sub(scrolloff);
    }

    scroll.min(max_scroll)
}

pub(super) fn scroll_message_row_down(
    message_scroll: &mut usize,
    message_line_scroll: &mut usize,
    messages_len: usize,
    current_message_height: Option<usize>,
) {
    let Some(height) = current_message_height.map(|height| height.max(1)) else {
        *message_line_scroll = 0;
        return;
    };

    if (*message_line_scroll).saturating_add(1) < height {
        *message_line_scroll = (*message_line_scroll).saturating_add(1);
    } else if *message_scroll < messages_len.saturating_sub(1) {
        *message_scroll = (*message_scroll).saturating_add(1);
        *message_line_scroll = 0;
    }
}

pub(super) fn scroll_message_row_up(
    message_scroll: &mut usize,
    message_line_scroll: &mut usize,
    previous_message_height: Option<usize>,
) {
    if *message_line_scroll > 0 {
        *message_line_scroll = (*message_line_scroll).saturating_sub(1);
        return;
    }
    if *message_scroll == 0 {
        return;
    }

    *message_scroll = (*message_scroll).saturating_sub(1);
    *message_line_scroll = previous_message_height
        .map(|height| height.max(1))
        .unwrap_or(1)
        .saturating_sub(1);
}

pub(super) fn normalize_message_line_scroll(
    message_line_scroll: &mut usize,
    current_message_height: Option<usize>,
) {
    let Some(height) = current_message_height.map(|height| height.max(1)) else {
        *message_line_scroll = 0;
        return;
    };

    *message_line_scroll = (*message_line_scroll).min(height.saturating_sub(1));
}
