use super::*;

pub(in crate::tui::ui) fn render_options_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::Options) {
        return;
    }

    let items = state.display_option_items();
    let selected = state.selected_option_index().unwrap_or(0);
    let popup = options_popup_area(area, state);
    let block = panel_block(state.options_popup_title(), true);
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
    render_option_gauges(frame, inner, &items, selected, visible_items);
}

pub(in crate::tui::ui) fn options_popup_area(area: Rect, state: &DashboardState) -> Rect {
    let items = state.display_option_items();
    let detail_lines = items
        .iter()
        .filter(|item| item.gauge_percent.is_some())
        .count() as u16;
    centered_rect(
        area,
        66,
        (items.len() as u16)
            .saturating_add(detail_lines)
            .saturating_add(2),
    )
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
        .flat_map(|(index, item)| {
            let selected = index == selected;
            let control = item.value.as_ref().map_or_else(
                || {
                    if item.enabled {
                        "[x]".to_owned()
                    } else {
                        "[ ]".to_owned()
                    }
                },
                |value| format!("[{value}]"),
            );
            let style = selectable_popup_label_style(selected, item.effective || index == 0);
            let row = Line::from(vec![
                selectable_popup_marker(selected),
                Span::styled(format!("{control} "), style),
                Span::styled(item.label, style),
                Span::styled(" - ", Style::default().fg(DIM)),
                Span::styled(item.description, Style::default().fg(DIM)),
            ]);
            let gauge_line = item.gauge_percent.map(|_| {
                let (min_label, max_label) = if item
                    .value
                    .as_deref()
                    .is_some_and(|value| value.ends_with('%'))
                {
                    ("0%", "100%")
                } else {
                    ("-100 dB", "0 dB")
                };
                Line::from(vec![
                    Span::styled("  ", Style::default().fg(ACCENT)),
                    Span::styled(format!("  {min_label} "), Style::default().fg(DIM)),
                    Span::styled(" ".repeat(28), Style::default()),
                    Span::styled(format!(" {max_label}"), Style::default().fg(DIM)),
                ])
            });
            std::iter::once(row).chain(gauge_line)
        })
        .map(|line| truncate_line_to_display_width(line, width))
        .collect();
    lines
}

fn render_option_gauges(
    frame: &mut Frame,
    inner: Rect,
    items: &[DisplayOptionItem],
    selected: usize,
    visible_items: usize,
) {
    let visible_items = visible_items.max(1);
    let selected = selected.min(items.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_items);
    let mut y = inner.y;
    for item in items.iter().skip(start).take(visible_items) {
        y = y.saturating_add(1);
        let Some(percent) = item.gauge_percent else {
            continue;
        };
        if y >= inner.y.saturating_add(inner.height) {
            break;
        }
        let gauge_width = inner.width.saturating_sub(19).min(28);
        if gauge_width == 0 {
            y = y.saturating_add(1);
            continue;
        }
        let gauge_area = Rect::new(inner.x.saturating_add(12), y, gauge_width, 1);
        frame.render_widget(
            Gauge::default()
                .ratio((f64::from(percent) / 100.0).clamp(0.0, 1.0))
                .label("")
                .gauge_style(Style::default().fg(ACCENT)),
            gauge_area,
        );
        y = y.saturating_add(1);
    }
}
