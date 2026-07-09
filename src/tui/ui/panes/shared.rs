use super::*;

pub(super) fn notification_count_badge(unread: ChannelUnreadState) -> Span<'static> {
    let (badge, _) = channel_unread_decoration(unread, Style::default(), false);
    badge.expect("numeric unread state always renders a badge")
}

pub(super) fn split_pane_filter_area(area: Rect, active: bool) -> (Rect, Option<Rect>) {
    if active && area.height >= 2 {
        let list_h = area.height.saturating_sub(1);
        let list_rect = Rect {
            height: list_h,
            ..area
        };
        let filter_rect = Rect {
            y: area.y + list_h,
            height: 1,
            ..area
        };
        (list_rect, Some(filter_rect))
    } else {
        (area, None)
    }
}

pub(super) fn render_pane_filter_bar_with_cursor(
    frame: &mut Frame,
    area: Option<Rect>,
    query: Option<&str>,
    cursor: Option<usize>,
    focused: bool,
) {
    let Some(area) = area else {
        return;
    };
    let cursor_x = render_pane_filter_bar(frame, area, query.unwrap_or_default(), cursor, focused);
    if focused && cursor.is_some() {
        frame.set_cursor_position(Position {
            x: area.x.saturating_add(cursor_x as u16),
            y: area.y,
        });
    }
}

/// Renders a one-line search bar at `area` and returns the visual column offset
/// of the cursor within that area (column 0 = leftmost cell of `area`).
fn render_pane_filter_bar(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    cursor_byte: Option<usize>,
    focused: bool,
) -> usize {
    let prompt = "/ ";
    let prompt_width = prompt.width();
    let available = (area.width as usize).saturating_sub(prompt_width).max(1);

    // Scroll the visible window so the cursor is always in view.
    let cursor_byte = cursor_byte.unwrap_or(query.len()).min(query.len());
    let mut start = 0usize;
    while query[start..cursor_byte].width() > available {
        // Advance start by one char boundary
        start = query[start..]
            .char_indices()
            .nth(1)
            .map(|(off, _)| start + off)
            .unwrap_or(query.len());
    }
    let mut end = cursor_byte;
    while end < query.len() {
        let next = query[end..]
            .char_indices()
            .nth(1)
            .map(|(off, _)| end + off)
            .unwrap_or(query.len());
        if query[start..next].width() > available {
            break;
        }
        end = next;
    }
    let visible = &query[start..end];
    let cursor_col = prompt_width + query[start..cursor_byte].width();

    let theme = theme::current();
    let accent = if focused { theme.accent } else { theme.border };
    let shown_query = if query.is_empty() {
        Span::styled("search...", Style::default().fg(theme.dim))
    } else {
        Span::raw(visible.to_owned())
    };
    let line = Line::from(vec![
        Span::styled(prompt, Style::default().fg(accent)),
        shown_query,
    ]);
    frame.render_widget(Paragraph::new(line), area);
    cursor_col
}
