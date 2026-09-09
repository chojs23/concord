use super::*;

pub(in crate::tui::ui) fn render_message_url_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageUrlPicker) {
        return;
    }

    let urls = state.selected_message_url_items();
    if urls.is_empty() {
        return;
    }
    let selected = state.selected_message_url_index().unwrap_or(0);
    let popup = message_url_picker_popup_area(area, urls.len());
    render_selectable_popup_list(
        frame,
        popup,
        "Open URL",
        message_url_picker_lines(&urls, selected),
        state
            .popup_list_scroll(SelectablePopupTarget::MessageUrls)
            .expect("message URLs have selection state"),
    );
}

pub(in crate::tui::ui) fn message_url_picker_popup_area(area: Rect, url_count: usize) -> Rect {
    centered_rect(area, 54, (url_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn message_url_picker_lines(
    urls: &[MessageUrlItem],
    selected: usize,
) -> Vec<Line<'static>> {
    urls.iter()
        .enumerate()
        .map(|(index, item)| {
            let selected = index == selected;
            let shortcut = shortcut_prefix(crate::tui::keybindings::KeyBindings::indexed_shortcut(
                index,
            ));
            let style = selectable_popup_label_style(selected, true);
            selected_row_line(
                Line::from(vec![
                    selection_marker(selected),
                    selectable_popup_shortcut_span(shortcut),
                    Span::styled(item.label.to_owned(), style),
                ]),
                selected,
            )
        })
        .collect()
}
