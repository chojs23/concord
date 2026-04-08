pub(super) const MAX_EMOJI_REACTION_VISIBLE_ITEMS: usize = 10;

pub(super) fn visible_item_range(
    items_len: usize,
    selected: usize,
    visible_items: usize,
) -> std::ops::Range<usize> {
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible_items)
        .min(items_len.saturating_sub(visible_items));
    let end = (start + visible_items).min(items_len);
    start..end
}
