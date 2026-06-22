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
    let lines = truncate_message_url_picker_lines(
        message_url_picker_lines(&urls, selected),
        popup.width.saturating_sub(2) as usize,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Open URL", true))
            .wrap(Wrap { trim: false }),
        popup,
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
            let shortcut = shortcut_prefix(
                crate::tui::keybindings::KeyBindings::default().indexed_shortcut(index),
            );
            let style = selectable_popup_label_style(selected, true);
            Line::from(vec![
                selectable_popup_marker(selected),
                selectable_popup_shortcut_span(shortcut),
                Span::styled(item.label.to_owned(), style),
            ])
        })
        .collect()
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_url_picker_lines_for_width(
    urls: &[MessageUrlItem],
    selected: usize,
    width: usize,
) -> Vec<Line<'static>> {
    truncate_message_url_picker_lines(message_url_picker_lines(urls, selected), width)
}

fn truncate_message_url_picker_lines(
    lines: Vec<Line<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    truncate_popup_lines(lines, width.max(1))
}
