use std::{collections::HashSet, hash::Hash};

pub(super) const SCROLL_OFF: usize = 3;

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

pub(super) fn open_collapsed_key<T>(set: &mut HashSet<T>, key: &T)
where
    T: Eq + Hash,
{
    set.remove(key);
}

pub(super) fn close_collapsed_key<T>(set: &mut HashSet<T>, key: T)
where
    T: Eq + Hash,
{
    set.insert(key);
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
