use super::*;

const CHANNEL_SWITCHER_POPUP_WIDTH: u16 = 74;

pub(in crate::tui::ui) fn render_channel_switcher_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_channel_switcher_open() {
        return;
    }

    let query = state.channel_switcher_query().unwrap_or_default();
    let query_cursor = state
        .channel_switcher_query_cursor_byte_index()
        .unwrap_or(query.len());
    let items = state.channel_switcher_items();
    let selected = state.selected_channel_switcher_index().unwrap_or(0);
    let popup = channel_switcher_popup_area(area);
    let max_result_lines = usize::from(popup.height.saturating_sub(4)).max(1);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(channel_switcher_lines(
            &items,
            selected,
            query,
            query_cursor,
            max_result_lines,
            popup.width.saturating_sub(2) as usize,
        ))
        .block(panel_block("Channel Switcher", true))
        .wrap(Wrap { trim: false }),
        popup,
    );
    if let Some(position) = channel_switcher_cursor_position(area, state) {
        frame.set_cursor_position(position);
    }
}

pub(in crate::tui::ui) fn channel_switcher_popup_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(2).clamp(8, 22);
    centered_rect(area, CHANNEL_SWITCHER_POPUP_WIDTH, height)
}

pub(in crate::tui::ui) fn channel_switcher_item_index_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<usize> {
    if !state.is_channel_switcher_open() {
        return None;
    }
    let popup = channel_switcher_popup_area(area);
    let inner = panel_block("", false).inner(popup);
    if column < inner.x
        || column >= inner.x.saturating_add(inner.width)
        || row < inner.y
        || row >= inner.y.saturating_add(inner.height)
    {
        return None;
    }
    let line = row.saturating_sub(inner.y) as usize;
    let result_line = line.checked_sub(2)?;
    let items = state.channel_switcher_items();
    let selected = state.selected_channel_switcher_index().unwrap_or(0);
    let max_result_lines = usize::from(popup.height.saturating_sub(4)).max(1);
    channel_switcher_visible_result_rows(&items, selected, max_result_lines)
        .get(result_line)
        .and_then(|row| match row {
            ChannelSwitcherResultRow::Item(index) => Some(*index),
            ChannelSwitcherResultRow::Group(_) => None,
        })
}

pub(in crate::tui::ui) fn channel_switcher_cursor_position(
    area: Rect,
    state: &DashboardState,
) -> Option<Position> {
    if !state.is_channel_switcher_open() {
        return None;
    }
    let query = state.channel_switcher_query().unwrap_or_default();
    let cursor = state
        .channel_switcher_query_cursor_byte_index()?
        .min(query.len());
    let popup = channel_switcher_popup_area(area);
    let inner_width = usize::from(popup.width.saturating_sub(2)).max(1);
    let (_, cursor_offset) = visible_channel_switcher_query(query, cursor, inner_width);
    Some(Position::new(
        popup
            .x
            .saturating_add(1)
            .saturating_add(cursor_offset as u16),
        popup.y.saturating_add(1),
    ))
}

pub(in crate::tui::ui) fn channel_switcher_lines(
    items: &[ChannelSwitcherItem],
    selected: usize,
    query: &str,
    query_cursor: usize,
    max_result_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        channel_switcher_search_line(query, query_cursor, width),
        Line::from(Span::styled(
            "─".repeat(width.max(1)),
            Style::default().fg(DIM),
        )),
    ];

    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "No channels found",
            Style::default().fg(DIM),
        )));
    } else {
        lines.extend(channel_switcher_result_lines(
            items,
            selected,
            max_result_lines,
        ));
    }

    lines
}

fn channel_switcher_search_line(query: &str, query_cursor: usize, width: usize) -> Line<'static> {
    let shown_query = if query.is_empty() {
        Span::styled("search channels", Style::default().fg(DIM))
    } else {
        Span::raw(visible_channel_switcher_query(query, query_cursor, width).0)
    };
    Line::from(vec![
        Span::styled("🔎 ", Style::default().fg(ACCENT)),
        shown_query,
    ])
}

fn visible_channel_switcher_query(query: &str, cursor: usize, width: usize) -> (String, usize) {
    let prefix_width = "🔎 ".width();
    let available = width.saturating_sub(prefix_width).max(1);
    let cursor = clamp_query_cursor(query, cursor);
    let mut start = 0usize;
    while query[start..cursor].width() > available {
        start = next_query_boundary(query, start);
    }

    let mut end = cursor;
    while end < query.len() {
        let next = next_query_boundary(query, end);
        if query[start..next].width() > available {
            break;
        }
        end = next;
    }

    let cursor_offset = prefix_width
        .saturating_add(query[start..cursor].width())
        .min(width.saturating_sub(1));
    (query[start..end].to_owned(), cursor_offset)
}

fn clamp_query_cursor(query: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(query.len());
    while cursor > 0 && !query.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn next_query_boundary(query: &str, cursor: usize) -> usize {
    let cursor = clamp_query_cursor(query, cursor);
    query[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(query.len())
}

fn channel_switcher_result_lines(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> Vec<Line<'static>> {
    let selected = selected.min(items.len().saturating_sub(1));
    let rows = channel_switcher_visible_result_rows(items, selected, max_result_lines);
    rows.into_iter()
        .map(|row| match row {
            ChannelSwitcherResultRow::Item(index) => {
                channel_switcher_item_line(&items[index], index == selected)
            }
            ChannelSwitcherResultRow::Group(label) => Line::from(Span::styled(
                label,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
        })
        .collect()
}

enum ChannelSwitcherResultRow {
    Group(String),
    Item(usize),
}

fn channel_switcher_visible_result_rows(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> Vec<ChannelSwitcherResultRow> {
    let selected = selected.min(items.len().saturating_sub(1));
    let start = channel_switcher_visible_start(items, selected, max_result_lines);
    let end = items.len().min(start.saturating_add(max_result_lines));
    let mut rows = Vec::new();
    let mut last_group: Option<&str> = None;
    for (index, item) in items.iter().enumerate().skip(start).take(end - start) {
        if last_group != Some(item.group_label.as_str()) {
            rows.push(ChannelSwitcherResultRow::Group(item.group_label.clone()));
            last_group = Some(item.group_label.as_str());
        }
        rows.push(ChannelSwitcherResultRow::Item(index));
    }
    rows.truncate(max_result_lines.max(1));
    rows
}

fn channel_switcher_visible_start(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> usize {
    if items.is_empty() || max_result_lines == 0 {
        return 0;
    }
    selected.saturating_sub(max_result_lines / 2)
}

fn channel_switcher_item_line(item: &ChannelSwitcherItem, selected: bool) -> Line<'static> {
    let style = if selected {
        highlight_style()
    } else {
        Style::default()
    };
    let badge = channel_switcher_unread_badge(item);
    let (_, name_style) = channel_unread_decoration(item.unread, style, false);
    let marker = if selected { "› " } else { "  " };
    let indent = "  ".repeat(item.depth.saturating_add(1));
    let parent = item
        .parent_label
        .as_ref()
        .map(|label| format!("{label} / "))
        .unwrap_or_default();
    let mut spans = vec![
        Span::styled(marker, Style::default().fg(ACCENT)),
        Span::raw(indent),
        Span::styled(parent, Style::default().fg(DIM)),
    ];
    if let Some(badge) = badge {
        spans.push(badge);
    }
    spans.push(Span::styled(item.channel_label.clone(), name_style));
    Line::from(spans)
}

fn channel_switcher_unread_badge(item: &ChannelSwitcherItem) -> Option<Span<'static>> {
    let (badge, _) = channel_unread_decoration(item.unread, Style::default(), false);
    if item.guild_id.is_none() && item.unread != ChannelUnreadState::Seen {
        if item.unread_message_count > 0 {
            let count = u32::try_from(item.unread_message_count).unwrap_or(u32::MAX);
            return channel_unread_decoration(
                ChannelUnreadState::Notified(count),
                Style::default(),
                false,
            )
            .0;
        }
        if item.unread == ChannelUnreadState::Unread {
            return channel_unread_decoration(
                ChannelUnreadState::Notified(1),
                Style::default(),
                false,
            )
            .0;
        }
    }
    badge
}
