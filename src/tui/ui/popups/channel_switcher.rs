use super::*;

const CHANNEL_SWITCHER_POPUP_WIDTH: u16 = 74;

pub(in crate::tui::ui) fn render_channel_switcher_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(view) = state.channel_switcher_view() else {
        return;
    };

    let popup = channel_switcher_popup_area(area);
    let max_result_lines = usize::from(popup.height.saturating_sub(4)).max(1);
    let inner = render_modal_frame(frame, popup, "Channel Switcher");
    let content = Rect {
        width: inner.width.saturating_sub(1).max(1),
        ..inner
    };
    frame.render_widget(
        Paragraph::new(channel_switcher_lines(
            view,
            max_result_lines,
            usize::from(content.width),
        )),
        content,
    );
    render_vertical_scrollbar(
        frame,
        Rect {
            y: inner.y.saturating_add(2),
            height: max_result_lines.min(u16::MAX as usize) as u16,
            ..inner
        },
        view.scroll,
        channel_switcher_visible_result_rows(view.items, view.scroll, max_result_lines)
            .iter()
            .filter(|row| matches!(row, ChannelSwitcherResultRow::Item(_)))
            .count()
            .max(1),
        view.items.len(),
    );
    frame.set_cursor_position(channel_switcher_cursor_position_for_view(area, view));
}

pub(in crate::tui::ui) fn channel_switcher_popup_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(2).clamp(8, 22);
    centered_rect(area, CHANNEL_SWITCHER_POPUP_WIDTH, height)
}

pub(in crate::tui::ui) fn channel_switcher_list_layout(
    area: Rect,
    state: &DashboardState,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    let popup = channel_switcher_popup_area(area);
    let inner = panel_block("", false).inner(popup);
    let list = Rect {
        y: inner.y.saturating_add(2),
        width: inner.width.saturating_sub(1).max(1),
        height: inner.height.saturating_sub(2),
        ..inner
    };
    let view = state
        .channel_switcher_view()
        .expect("channel switcher layout requires view");
    SelectablePopupLayout::new(snapshot.target, popup, list, snapshot, |start, max_rows| {
        channel_switcher_visible_result_rows(view.items, start, max_rows)
            .into_iter()
            .map(|row| match row {
                ChannelSwitcherResultRow::Item(index) => Some(index),
                ChannelSwitcherResultRow::Group(_) => None,
            })
            .collect()
    })
}

#[cfg(test)]
pub(in crate::tui::ui) fn channel_switcher_cursor_position(
    area: Rect,
    state: &DashboardState,
) -> Option<Position> {
    let view = state.channel_switcher_view()?;
    Some(channel_switcher_cursor_position_for_view(area, view))
}

fn channel_switcher_cursor_position_for_view(
    area: Rect,
    view: ChannelSwitcherView<'_>,
) -> Position {
    let cursor = view.query_cursor.min(view.query.len());
    let popup = channel_switcher_popup_area(area);
    let inner_width = usize::from(popup.width.saturating_sub(3)).max(1);
    let (_, cursor_offset) = visible_channel_switcher_query(view.query, cursor, inner_width);
    Position::new(
        popup
            .x
            .saturating_add(1)
            .saturating_add(cursor_offset as u16),
        popup.y.saturating_add(1),
    )
}

pub(in crate::tui::ui) fn channel_switcher_lines(
    view: ChannelSwitcherView<'_>,
    max_result_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        channel_switcher_search_line(view.query, view.query_cursor, width),
        Line::from(Span::styled(
            "─".repeat(width.max(1)),
            theme::current().style(theme::HighlightGroup::Decoration),
        )),
    ];

    if view.items.is_empty() {
        lines.push(Line::from(Span::styled(
            match view.mode {
                ChannelSwitcherMode::Channels => "No channels found",
                ChannelSwitcherMode::Guilds => "No servers found",
            },
            theme::current().style(theme::HighlightGroup::Placeholder),
        )));
    } else {
        lines.extend(channel_switcher_result_lines(
            view.items,
            view.selected,
            max_result_lines,
            view.scroll,
        ));
    }

    lines
}

fn channel_switcher_search_line(query: &str, query_cursor: usize, width: usize) -> Line<'static> {
    let shown_query = if query.is_empty() {
        Span::styled(
            "where would you like to go?",
            theme::current().style(theme::HighlightGroup::Placeholder),
        )
    } else {
        Span::styled(
            visible_channel_switcher_query(query, query_cursor, width).0,
            theme::current().style(theme::HighlightGroup::ActiveField),
        )
    };
    Line::from(vec![
        Span::styled(
            "🔎 ",
            theme::current().style(theme::HighlightGroup::ActiveField),
        ),
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
    scroll: usize,
) -> Vec<Line<'static>> {
    let selected = selected.min(items.len().saturating_sub(1));
    let rows = channel_switcher_visible_result_rows(items, scroll, max_result_lines);
    rows.into_iter()
        .map(|row| match row {
            ChannelSwitcherResultRow::Item(index) => {
                channel_switcher_item_line(&items[index], index == selected)
            }
            ChannelSwitcherResultRow::Group(label) => Line::from(Span::styled(
                label,
                theme::current().style(theme::HighlightGroup::Heading),
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
    scroll: usize,
    max_result_lines: usize,
) -> Vec<ChannelSwitcherResultRow> {
    // Group headers are interleaved as the window is walked and share the row
    // budget, so the trailing `truncate` keeps the popup height.
    let start = scroll.min(items.len().saturating_sub(1));
    let end = items.len().min(start.saturating_add(max_result_lines));
    let mut rows = Vec::new();
    let mut last_group: Option<&str> = None;
    for (index, item) in items.iter().enumerate().skip(start).take(end - start) {
        let display = item.display();
        if last_group != Some(display.group_label.as_str()) {
            rows.push(ChannelSwitcherResultRow::Group(display.group_label.clone()));
            last_group = Some(display.group_label.as_str());
        }
        rows.push(ChannelSwitcherResultRow::Item(index));
    }
    rows.truncate(max_result_lines.max(1));
    rows
}

fn channel_switcher_item_line(item: &ChannelSwitcherItem, selected: bool) -> Line<'static> {
    let display = item.display();
    let style = if selected {
        highlight_style()
    } else {
        Style::default()
    };
    let badge = channel_switcher_unread_badge(display.badge_state)
        .map(|badge| selected_text_span(selected, badge));
    let (_, name_style) = channel_unread_decoration(display.unread, style, false);
    let name_style = selected_text_style(selected, name_style);
    let indent = "  ".repeat(display.depth.saturating_add(1));
    let parent = display
        .parent_label
        .as_ref()
        .map(|label| format!("{label} / "))
        .unwrap_or_default();
    let mut spans = vec![
        selection_marker(selected),
        Span::raw(indent),
        Span::styled(
            parent,
            theme::current().style(theme::HighlightGroup::SearchContext),
        ),
    ];
    if let Some(badge) = badge {
        spans.push(badge);
    }
    spans.push(Span::styled(display.label.clone(), name_style));
    selected_row_line(Line::from(spans), selected)
}

fn channel_switcher_unread_badge(unread: ChannelUnreadState) -> Option<Span<'static>> {
    let (badge, _) = channel_unread_decoration(unread, Style::default(), false);
    badge
}
