use super::*;
use crate::tui::keybindings::KeymapBindingSummary;

pub(in crate::tui::ui) fn render_keymap_help_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::KeymapHelp) {
        return;
    }

    let lines = keymap_help_popup_lines(state.keymap_binding_summaries());
    render_keymap_popup(frame, area, "Keymap", lines, state);
}

fn render_keymap_popup(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
    state: &DashboardState,
) {
    let popup = keymap_popup_area(area);
    let inner = render_modal_frame(frame, popup, title);

    let total_lines = lines.len();
    let viewport = usize::from(inner.height);
    let scroll_position = state
        .keymap_popup_scroll()
        .min(total_lines.saturating_sub(viewport));
    let visible_lines = lines
        .into_iter()
        .skip(scroll_position)
        .take(viewport)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(visible_lines).wrap(Wrap { trim: false }),
        inner,
    );
    render_vertical_scrollbar(frame, inner, scroll_position, viewport, total_lines);
}

const KEYMAP_POPUP_WIDTH: u16 = 72;
const KEYMAP_POPUP_HEIGHT: u16 = 18;

pub(in crate::tui::ui) fn keymap_popup_area(area: Rect) -> Rect {
    let width = KEYMAP_POPUP_WIDTH.min(area.width.saturating_sub(2)).max(8);
    let height = KEYMAP_POPUP_HEIGHT
        .min(area.height.saturating_sub(2))
        .max(6);
    centered_rect(area, width, height)
}

pub(in crate::tui::ui) fn keymap_popup_text_area(area: Rect) -> Rect {
    panel_block("Keymap", true).inner(keymap_popup_area(area))
}

pub(in crate::tui::ui) fn keymap_popup_total_lines(state: &DashboardState) -> usize {
    if state.is_active_modal_popup(ActiveModalPopupKind::KeymapHelp) {
        keymap_help_popup_lines(state.keymap_binding_summaries()).len()
    } else {
        0
    }
}

pub(in crate::tui::ui) fn keymap_help_popup_lines(
    summaries: Vec<KeymapBindingSummary>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current_scope = "";

    for summary in summaries {
        if summary.scope != current_scope {
            if !lines.is_empty() {
                lines.push(Line::from(Span::raw(String::new())));
            }
            current_scope = summary.scope;
            lines.push(Line::from(Span::styled(
                format!("[{}]", summary.scope),
                Style::default()
                    .fg(theme::current().accent)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        lines.push(Line::from(vec![
            Span::styled(
                format!("[{}] ", summary.keys.join(" / ")),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(summary.action),
        ]));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No key mappings.",
            Style::default().fg(theme::current().dim),
        )));
    }

    lines
}
