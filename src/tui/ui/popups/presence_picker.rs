use super::*;
use crate::tui::state::presence_picker::PRESENCE_PICKER_ITEMS;

pub(in crate::tui::ui) fn render_presence_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_presence_picker_open() {
        return;
    }
    let selected = state.presence_picker_selected();
    let popup = centered_rect(
        area,
        32,
        (PRESENCE_PICKER_ITEMS.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(presence_picker_lines(PRESENCE_PICKER_ITEMS, selected))
            .block(panel_block_owned("Set Presence".to_owned(), true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn presence_picker_lines(
    items: &[(crate::discord::PresenceStatus, &str, char)],
    selected: usize,
) -> Vec<Line<'static>> {
    items
        .iter()
        .enumerate()
        .map(|(index, (status, label, key))| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = format!("[{key}] ");
            let mut style = Style::default().fg(presence_color(*status));
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(label.to_string(), style),
            ])
        })
        .collect()
}
