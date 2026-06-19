use super::*;
use crate::tui::state::MessageConfirmationKind;

pub(in crate::tui::ui) fn render_message_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageConfirmation) {
        return;
    }

    let Some((kind, author, content)) = state.message_confirmation_lines() else {
        return;
    };

    let lines = message_confirmation_lines_with_key_bindings(
        kind,
        &author,
        content.as_deref(),
        56,
        state.key_bindings(),
    );
    let popup = clear_centered_popup_area(frame, area, 60, (lines.len() as u16).saturating_add(2));
    render_modal_paragraph(frame, popup, kind.title(), lines);
}

pub(in crate::tui::ui) fn render_quit_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::QuitConfirmation) {
        return;
    }

    let lines = quit_confirmation_lines_with_key_bindings(state.key_bindings());
    let popup = clear_centered_popup_area(frame, area, 44, (lines.len() as u16).saturating_add(2));
    render_modal_paragraph(frame, popup, "Quit", lines);
}

pub(in crate::tui::ui) fn render_guild_leave_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::GuildLeaveConfirmation) {
        return;
    }

    let Some(name) = state.guild_leave_confirmation_name() else {
        return;
    };

    let lines = guild_leave_confirmation_lines_with_key_bindings(&name, 56, state.key_bindings());
    let popup = clear_centered_popup_area(frame, area, 60, (lines.len() as u16).saturating_add(2));
    render_modal_paragraph(frame, popup, "Leave server?", lines);
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_delete_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines_with_key_bindings(
        MessageConfirmationKind::Delete,
        author,
        content,
        width,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_pin_confirmation_lines(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines_with_key_bindings(
        MessageConfirmationKind::Pin { pinned },
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

#[cfg(test)]
pub(in crate::tui::ui) fn message_remove_embeds_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines_with_key_bindings(
        MessageConfirmationKind::RemoveEmbeds,
        author,
        content,
        width,
        &crate::tui::keybindings::KeyBindings::default(),
    )
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
        Line::from(Span::raw("Leave the current server?")),
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
            Span::raw(" leave server · "),
            Span::styled(
                key_bindings.message_confirmation_cancel_label(),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::raw(" cancel"),
        ]),
    ]
}

fn message_confirmation_lines_with_key_bindings(
    kind: MessageConfirmationKind,
    author: &str,
    content: Option<&str>,
    width: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    confirmation_lines(
        kind.prompt(),
        author,
        content,
        width,
        kind.action_label(),
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
