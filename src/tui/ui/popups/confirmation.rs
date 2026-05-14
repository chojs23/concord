use super::*;

pub(in crate::tui::ui) fn render_message_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some((author, content)) = state.message_delete_confirmation_lines() else {
        return;
    };

    let lines = message_delete_confirmation_lines(&author, content.as_deref(), 56);
    let popup = centered_rect(area, 60, (lines.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Delete message?", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn render_message_pin_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some((pinned, author, content)) = state.message_pin_confirmation_lines() else {
        return;
    };

    let lines = message_pin_confirmation_lines(pinned, &author, content.as_deref(), 56);
    let title = if pinned {
        "Pin message?"
    } else {
        "Unpin message?"
    };
    let popup = centered_rect(area, 60, (lines.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn message_delete_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let excerpt = content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(|content| content.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "[no text content]".to_owned());
    let excerpt = truncate_display_width(&excerpt, width.saturating_sub(2));
    vec![
        Line::from(Span::raw("Delete this message?")),
        Line::from(Span::styled(
            format!("From: {author}"),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            format!("\"{excerpt}\""),
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::raw(String::new())),
        Line::from(vec![
            Span::styled("Enter/y", Style::default().fg(ACCENT).bold()),
            Span::raw(" delete · "),
            Span::styled("Esc/n", Style::default().fg(ACCENT).bold()),
            Span::raw(" cancel"),
        ]),
    ]
}

pub(in crate::tui::ui) fn message_pin_confirmation_lines(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    let action = if pinned { "Pin" } else { "Unpin" };
    confirmation_lines(
        format!("{action} this message?"),
        author,
        content,
        width,
        format!("{action} message"),
    )
}

fn confirmation_lines(
    prompt: String,
    author: &str,
    content: Option<&str>,
    width: usize,
    action_label: String,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let excerpt = content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(|content| content.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "[no text content]".to_owned());
    let excerpt = truncate_display_width(&excerpt, width.saturating_sub(2));
    vec![
        Line::from(Span::raw(prompt)),
        Line::from(Span::styled(
            format!("From: {author}"),
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            format!("\"{excerpt}\""),
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::raw(String::new())),
        Line::from(vec![
            Span::styled("Enter/y", Style::default().fg(ACCENT).bold()),
            Span::raw(format!(" {action_label} · ")),
            Span::styled("Esc/n", Style::default().fg(ACCENT).bold()),
            Span::raw(" cancel"),
        ]),
    ]
}
