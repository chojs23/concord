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
    let scroll = state
        .popup_list_scroll(SelectablePopupTarget::Options)
        .expect("options have selection state");
    let visible_items = options_visible_item_count(&items, scroll, usize::from(inner.height));
    let content = Rect {
        width: inner.width.saturating_sub(1).max(1),
        ..inner
    };
    let inner_width = usize::from(content.width).max(1);
    frame.render_widget(
        Paragraph::new(options_popup_lines(
            &items,
            selected,
            visible_items,
            scroll,
            inner_width,
        )),
        content,
    );
    render_option_gauges(frame, content, &items, visible_items, scroll);
    render_vertical_scrollbar(frame, inner, scroll, visible_items, items.len());
}

pub(in crate::tui::ui) fn options_popup_list_layout(
    area: Rect,
    state: &DashboardState,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    let popup = options_popup_area(area, state);
    let inner = panel_block(state.options_popup_title(), true).inner(popup);
    let list = Rect {
        width: inner.width.saturating_sub(1).max(1),
        ..inner
    };
    let items = state.display_option_items();
    SelectablePopupLayout::new(snapshot.target, popup, list, snapshot, |start, max_rows| {
        options_row_items(&items, start, max_rows)
    })
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
            let mut spans = vec![
                selectable_popup_marker(selected),
                Span::styled(format!("{control} "), style),
                Span::styled(item.label, style),
            ];
            if !item.description.is_empty() {
                let description_style = theme::current().style(theme::HighlightGroup::Description);
                spans.push(Span::styled(" - ", description_style));
                spans.push(Span::styled(item.description, description_style));
            }
            let row = selected_row_line(Line::from(spans), selected);
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

fn options_visible_item_count(
    items: &[DisplayOptionItem],
    scroll: usize,
    available_rows: usize,
) -> usize {
    if items.is_empty() {
        return 0;
    }
    let start = scroll.min(items.len() - 1);
    let available_rows = available_rows.max(1);
    let mut used_rows = 0usize;
    let mut visible = 0usize;
    for item in &items[start..] {
        let item_height = 1 + usize::from(item.gauge.is_some());
        if visible > 0 && used_rows.saturating_add(item_height) > available_rows {
            break;
        }
        used_rows = used_rows.saturating_add(item_height);
        visible += 1;
        if used_rows >= available_rows {
            break;
        }
    }
    visible.max(1)
}

fn options_row_items(
    items: &[DisplayOptionItem],
    start: usize,
    available_rows: usize,
) -> Vec<Option<usize>> {
    let mut rows = Vec::new();
    for (index, item) in items.iter().enumerate().skip(start) {
        let item_height = 1 + usize::from(item.gauge.is_some());
        if !rows.is_empty() && rows.len().saturating_add(item_height) > available_rows {
            break;
        }
        rows.extend(std::iter::repeat_n(Some(index), item_height));
        if rows.len() >= available_rows {
            break;
        }
    }
    rows.truncate(available_rows.max(1));
    rows
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
