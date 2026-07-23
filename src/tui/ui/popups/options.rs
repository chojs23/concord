use super::*;

const OPTIONS_GAUGE_X_OFFSET: u16 = 12;

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
    let inner = render_modal_frame(frame, popup, state.options_popup_title());
    let visible_items = usize::from(inner.height).max(1);
    let inner_width = usize::from(inner.width).max(1);
    let scroll = state.options_popup_scroll();
    frame.render_widget(
        Paragraph::new(options_popup_lines(
            &items,
            selected,
            visible_items,
            scroll,
            inner_width,
        )),
        inner,
    );
    render_option_gauges(frame, inner, &items, visible_items, scroll);
}

pub(in crate::tui::ui) fn options_popup_visible_items(area: Rect, state: &DashboardState) -> usize {
    let popup = options_popup_area(area, state);
    let inner = panel_block(state.options_popup_title(), true).inner(popup);
    usize::from(inner.height).max(1)
}

pub(in crate::tui::ui) fn options_popup_area(area: Rect, state: &DashboardState) -> Rect {
    let items = state.display_option_items();
    let detail_lines = items.iter().filter(|item| item.gauge.is_some()).count() as u16;
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
    scroll: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let visible_items = visible_items.max(1);
    let width = width.max(1);
    let selected = selected.min(items.len().saturating_sub(1));
    let start = scroll.min(items.len().saturating_sub(visible_items));
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
            let row = selected_row_line(
                Line::from(vec![
                    selectable_popup_marker(selected),
                    Span::styled(format!("{control} "), style),
                    Span::styled(item.label, style),
                    Span::styled(
                        " - ",
                        theme::current().style(theme::HighlightGroup::Description),
                    ),
                    Span::styled(
                        item.description,
                        theme::current().style(theme::HighlightGroup::Description),
                    ),
                ]),
                selected,
            );
            let gauge_line = item.gauge.map(|gauge| {
                let (min_label, max_label) = if item
                    .value
                    .as_deref()
                    .is_some_and(|value| value.ends_with('%'))
                {
                    ("0%".to_owned(), format!("{}%", gauge.maximum()))
                } else {
                    ("-100 dB".to_owned(), "0 dB".to_owned())
                };
                popup_gauge_line(
                    OPTIONS_GAUGE_X_OFFSET,
                    &min_label,
                    max_label,
                    theme::current().style(theme::HighlightGroup::Description),
                )
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
    visible_items: usize,
    scroll: usize,
) {
    let visible_items = visible_items.max(1);
    let start = scroll.min(items.len().saturating_sub(visible_items));
    let mut y = inner.y;
    for item in items.iter().skip(start).take(visible_items) {
        y = y.saturating_add(1);
        let Some(gauge) = item.gauge else {
            continue;
        };
        if y >= inner.y.saturating_add(inner.height) {
            break;
        }
        render_popup_gauge(
            frame,
            inner,
            PopupGauge {
                x_offset: OPTIONS_GAUGE_X_OFFSET,
                width_margin: 19,
                y,
                value: gauge.value(),
                maximum: gauge.maximum(),
                style: theme::current().apply(
                    theme::HighlightGroup::GaugeFill,
                    theme::current().style(theme::HighlightGroup::Normal),
                ),
            },
        );
        y = y.saturating_add(1);
    }
}
