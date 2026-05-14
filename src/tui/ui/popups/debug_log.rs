use super::*;

pub(in crate::tui::ui) fn render_debug_log_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_debug_log_popup_open() {
        return;
    }

    const POPUP_TARGET_WIDTH: u16 = 78;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let visible_log_lines = usize::from(area.height).saturating_sub(6).max(1);
    let lines = debug_log_popup_lines(
        state.debug_log_lines(),
        state.debug_channel_visibility(),
        visible_log_lines,
        usize::from(popup_width.saturating_sub(2)),
        &RenderCtx::new(state.theme()),
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(bg_clear(state.theme().background), popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Debug logs", true, state.theme().accent))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn debug_log_popup_lines(
    entries: Vec<String>,
    channel_visibility: ChannelVisibilityStats,
    visible_log_lines: usize,
    width: usize,
    ctx: &RenderCtx<'_>,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let visible_log_lines = visible_log_lines.max(1);
    let mut lines = Vec::new();

    // Header line: visible vs. permission-hidden channels for the active
    // scope. Helps the user diagnose "why is this channel missing" without
    // diving into the logs.
    let visibility_text = format!(
        "Channels: {} visible · {} hidden by permissions",
        channel_visibility.visible, channel_visibility.hidden,
    );
    lines.push(Line::from(Span::styled(
        visibility_text,
        Style::default().fg(ctx.theme.accent),
    )));
    lines.push(Line::from(Span::raw(String::new())));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "No errors recorded in this process.",
            Style::default().fg(ctx.theme.dim),
        )));
    } else {
        let wrapped = entries
            .into_iter()
            .flat_map(|entry| wrap_text_lines(&entry, width))
            .collect::<Vec<_>>();
        let start = wrapped.len().saturating_sub(visible_log_lines);
        for entry in wrapped.into_iter().skip(start) {
            lines.push(Line::from(Span::styled(
                entry,
                Style::default().fg(Color::Red),
            )));
        }
    }
    lines
}
