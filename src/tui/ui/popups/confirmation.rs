use super::*;

pub(in crate::tui::ui) fn render_message_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some((author, content)) = state.message_delete_confirmation_lines() else {
        return;
    };

    let lines = message_delete_confirmation_lines_with_key_bindings(
        &author,
        content.as_deref(),
        56,
        state.key_bindings(),
    );
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

    let lines = message_pin_confirmation_lines_with_key_bindings(
        pinned,
        &author,
        content.as_deref(),
        56,
        state.key_bindings(),
    );
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

pub(in crate::tui::ui) fn render_quit_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_quit_confirmation_open() {
        return;
    }

    let lines = quit_confirmation_lines_with_key_bindings(state.key_bindings());
    let popup = centered_rect(area, 44, (lines.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Quit", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn render_guild_leave_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(name) = state.guild_leave_confirmation_name() else {
        return;
    };

    let lines = guild_leave_confirmation_lines_with_key_bindings(&name, 56, state.key_bindings());
    let popup = centered_rect(area, 60, (lines.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Remove server?", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_delete_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_delete_confirmation_lines_with_key_bindings(
        author,
        content,
        width,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

fn message_delete_confirmation_lines_with_key_bindings(
    author: &str,
    content: Option<&str>,
    width: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
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
            Span::styled(
                key_bindings.message_confirmation_confirm_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" delete · "),
            Span::styled(
                key_bindings.message_confirmation_cancel_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" cancel"),
        ]),
    ]
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_pin_confirmation_lines(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_pin_confirmation_lines_with_key_bindings(
        pinned,
        author,
        content,
        width,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn quit_confirmation_lines() -> Vec<Line<'static>> {
    quit_confirmation_lines_with_key_bindings(&crate::tui::keybindings::KeyBindings::default())
}

fn quit_confirmation_lines_with_key_bindings(
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::raw("Quit Concord?")),
        Line::from(Span::raw(String::new())),
        Line::from(vec![
            Span::styled(
                key_bindings.message_confirmation_confirm_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" quit · "),
            Span::styled(
                key_bindings.message_confirmation_cancel_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" cancel"),
        ]),
    ]
}

fn guild_leave_confirmation_lines_with_key_bindings(
    name: &str,
    width: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    let name = truncate_display_width(name, width.max(1).saturating_sub(2));
    vec![
        Line::from(Span::raw("Remove the current server?")),
        Line::from(Span::styled(
            format!("Server: {name}"),
            Style::default().fg(Color::Red),
        )),
        Line::from(Span::raw(String::new())),
        Line::from(vec![
            Span::styled(
                key_bindings.message_confirmation_confirm_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" remove server · "),
            Span::styled(
                key_bindings.message_confirmation_cancel_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" cancel"),
        ]),
    ]
}

fn message_pin_confirmation_lines_with_key_bindings(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    let action = if pinned { "Pin" } else { "Unpin" };
    confirmation_lines(
        format!("{action} this message?"),
        author,
        content,
        width,
        format!("{action} message"),
        key_bindings,
    )
}

fn confirmation_lines(
    prompt: String,
    author: &str,
    content: Option<&str>,
    width: usize,
    action_label: String,
    key_bindings: &crate::tui::keybindings::KeyBindings,
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
            Span::styled(
                key_bindings.message_confirmation_confirm_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(format!(" {action_label} · ")),
            Span::styled(
                key_bindings.message_confirmation_cancel_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" cancel"),
        ]),
    ]
}
