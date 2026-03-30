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
