pub(super) const MAX_EMOJI_REACTION_VISIBLE_ITEMS: usize = 10;

pub(super) fn visible_window(
    scroll: usize,
    visible_items: usize,
    items_len: usize,
) -> std::ops::Range<usize> {
    let visible_items = visible_items.max(1).min(items_len.max(1));
    let start = scroll.min(items_len.saturating_sub(visible_items));
    start..(start + visible_items).min(items_len)
}
