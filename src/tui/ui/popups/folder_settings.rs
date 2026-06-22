use super::*;
use ratatui::layout::Position;

pub(in crate::tui::ui) fn render_folder_settings_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_folder_settings_open() {
        return;
    }

    let name = state.folder_settings_name_value().unwrap_or_default();
    let color = state.folder_settings_color_value().unwrap_or_default();
    let name_active = state.folder_settings_name_active();
    let color_active = state.folder_settings_color_active();
    let editing = state.is_folder_settings_editing();
    let color_error = state.folder_settings_color_error();
    let popup = folder_settings_popup_area(area);
    frame.render_widget(Clear, popup);
    let inner_width = popup.width.saturating_sub(2) as usize;
    let lines = truncate_popup_lines(
        vec![
            folder_settings_input_line("Name", name, name_active),
            Line::default(),
            folder_settings_input_line("Color code", color, color_active),
            Line::from(Span::styled(
                color_error
                    .unwrap_or("Use #RRGGBB or leave blank")
                    .to_owned(),
                Style::default().fg(if color_error.is_some() {
                    MENTION_ORANGE
                } else {
                    DIM
                }),
            )),
            Line::default(),
            Line::from(Span::styled(
                popup_shortcut_help_text(&[
                    ("Enter", "select"),
                    ("s", "save"),
                    ("Esc", "close/cancel"),
                ]),
                Style::default().fg(DIM),
            )),
        ],
        inner_width,
    );
    render_modal_paragraph(frame, popup, "Folder Settings", lines);

    if !editing {
        return;
    }

    let active_row = if name_active { 1 } else { 3 };
    let active_value = if name_active { name } else { color };
    let cursor = state
        .folder_settings_cursor_byte_index()
        .unwrap_or(active_value.len())
        .min(active_value.len());
    let value_before_cursor = &active_value[..cursor];
    let cursor_x = popup
        .x
        .saturating_add(1)
        .saturating_add(folder_settings_input_prefix_width(name_active) as u16)
        .saturating_add(value_before_cursor.width() as u16)
        .min(popup.x.saturating_add(popup.width.saturating_sub(1)));
    frame.set_cursor_position(Position {
        x: cursor_x,
        y: popup.y.saturating_add(active_row),
    });
}

pub(in crate::tui::ui) fn folder_settings_popup_area(area: Rect) -> Rect {
    centered_rect(area, 52, 9)
}

fn folder_settings_input_line(label: &'static str, value: &str, active: bool) -> Line<'static> {
    let marker = if active { "› " } else { "  " };
    let style = if active {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(ACCENT)),
        Span::styled(format!("{label}: "), style),
        Span::raw(value.to_owned()),
    ])
}

fn folder_settings_input_prefix_width(name_active: bool) -> usize {
    if name_active {
        "› Name: ".width()
    } else {
        "› Color code: ".width()
    }
}
