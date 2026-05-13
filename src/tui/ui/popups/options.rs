use super::*;

pub(in crate::tui::ui) fn render_options_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_options_popup_open() {
        return;
    }

    let items = state.display_option_items();
    let selected = state.selected_option_index().unwrap_or(0);
    let popup = centered_rect(area, 66, (items.len() as u16).saturating_add(2));
    let block = panel_block("Options", true);
    let inner = block.inner(popup);
    let visible_items = usize::from(inner.height).max(1);
    let inner_width = usize::from(inner.width).max(1);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(options_popup_lines(
            &items,
            selected,
            visible_items,
            inner_width,
        ))
        .block(block),
        popup,
    );
}

pub(in crate::tui::ui) fn options_popup_lines(
    items: &[DisplayOptionItem],
    selected: usize,
    visible_items: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let visible_items = visible_items.max(1);
    let width = width.max(1);
    let selected = selected.min(items.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_items);
    let lines: Vec<Line<'static>> = items
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_items)
        .map(|(index, item)| {
            let marker = if index == selected { "› " } else { "  " };
            let control = item.value.map_or_else(
                || {
                    if item.enabled {
                        "[x]".to_owned()
                    } else {
                        "[ ]".to_owned()
                    }
                },
                |value| format!("[{value}]"),
            );
            let mut style = if item.effective || index == 0 {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{control} "), style),
                Span::styled(item.label, style),
                Span::styled(" - ", Style::default().fg(DIM)),
                Span::styled(item.description, Style::default().fg(DIM)),
            ])
        })
        .map(|line| truncate_line_to_display_width(line, width))
        .collect();
    lines
}
