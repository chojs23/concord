use super::*;
use crate::tui::state::{ConfirmationButton, MessageConfirmationKind, NotificationInboxTab};

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

    let lines = message_confirmation_lines(
        kind,
        &author,
        content.as_deref(),
        56,
        state.active_confirmation_button(),
    );
    let popup = message_confirmation_popup_area(area, lines.len());
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

    let lines = quit_confirmation_popup_lines(state.active_confirmation_button());
    let popup = quit_confirmation_popup_area(area);
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

    let lines = guild_leave_confirmation_lines(&name, 56, state.active_confirmation_button());
    let popup = guild_leave_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Leave server?", lines);
}

pub(in crate::tui::ui) fn render_thread_delete_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ThreadDeleteConfirmation) {
        return;
    }

    let Some((name, noun)) = state.thread_delete_confirmation_target() else {
        return;
    };

    let lines =
        thread_delete_confirmation_lines(&name, noun, 56, state.active_confirmation_button());
    let popup = thread_delete_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, format!("Delete {noun}?"), lines);
}

pub(in crate::tui::ui) fn render_notification_inbox_mark_all_confirmation(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.notification_inbox_is_confirming_mark_all() {
        return;
    }

    let Some(tab) = state.notification_inbox_tab() else {
        return;
    };

    let lines =
        notification_inbox_mark_all_confirmation_lines(tab, state.active_confirmation_button());
    let popup = message_confirmation_popup_area(area, lines.len());
    render_modal_paragraph(frame, popup, "Mark read?", lines);
}

pub(in crate::tui::ui) fn message_confirmation_popup_area(area: Rect, line_count: usize) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn message_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let (kind, author, content) = state.message_confirmation_lines()?;
    let lines = message_confirmation_lines(
        kind,
        &author,
        content.as_deref(),
        56,
        state.active_confirmation_button(),
    );
    Some(message_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn quit_confirmation_popup_area(area: Rect) -> Rect {
    centered_rect(
        area,
        44,
        (quit_confirmation_popup_lines(ConfirmationButton::default()).len() as u16)
            .saturating_add(2),
    )
}

pub(in crate::tui::ui) fn guild_leave_confirmation_popup_area(
    area: Rect,
    line_count: usize,
) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn guild_leave_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let name = state.guild_leave_confirmation_name()?;
    let lines = guild_leave_confirmation_lines(&name, 56, state.active_confirmation_button());
    Some(guild_leave_confirmation_popup_area(area, lines.len()))
}

pub(in crate::tui::ui) fn thread_delete_confirmation_popup_area(
    area: Rect,
    line_count: usize,
) -> Rect {
    centered_rect(area, 60, (line_count as u16).saturating_add(2))
}

pub(in crate::tui::ui) fn thread_delete_confirmation_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let (name, noun) = state.thread_delete_confirmation_target()?;
    let lines =
        thread_delete_confirmation_lines(&name, noun, 56, state.active_confirmation_button());
    Some(thread_delete_confirmation_popup_area(area, lines.len()))
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_delete_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::Delete,
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_pin_confirmation_lines(
    pinned: bool,
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::Pin { pinned },
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn quit_confirmation_lines() -> Vec<Line<'static>> {
    quit_confirmation_popup_lines(ConfirmationButton::default())
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_remove_embeds_confirmation_lines(
    author: &str,
    content: Option<&str>,
    width: usize,
) -> Vec<Line<'static>> {
    message_confirmation_lines(
        MessageConfirmationKind::RemoveEmbeds,
        author,
        content,
        width,
        ConfirmationButton::default(),
    )
}

fn quit_confirmation_popup_lines(active: ConfirmationButton) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::raw("Quit Concord?")),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

fn guild_leave_confirmation_lines(
    name: &str,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let name = truncate_display_width(name, width.max(1).saturating_sub(2));
    let mut lines = vec![
        Line::from(Span::raw("Leave the current server?")),
        Line::from(Span::styled(
            format!("Server: {name}"),
            Style::default().fg(theme::current().error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

fn thread_delete_confirmation_lines(
    name: &str,
    noun: &str,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let name = truncate_display_width(name, width.max(1).saturating_sub(2));
    let label = capitalize_first(noun);
    let mut lines = vec![
        Line::from(Span::raw(format!("Permanently delete this {noun}?"))),
        Line::from(Span::styled(
            format!("{label}: {name}"),
            Style::default().fg(theme::current().error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

fn confirmation_button_lines(active: ConfirmationButton) -> Vec<Line<'static>> {
    vec![
        popup_button_line("y", "confirm", active == ConfirmationButton::Confirm),
        popup_button_line("n", "cancel", active == ConfirmationButton::Cancel),
    ]
}

fn notification_inbox_mark_all_confirmation_lines(
    tab: NotificationInboxTab,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let target = match tab {
        NotificationInboxTab::Unreads => "all unread channels",
        NotificationInboxTab::Mentions => "all mentions",
    };
    let mut lines = vec![
        Line::from(Span::raw(format!("Mark {target} as read?"))),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}

/// Uppercase the first ASCII letter so a noun like "post" renders as "Post:" in
/// the confirmation body. The nouns are known ASCII words, so this is sufficient.
fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn message_confirmation_lines(
    kind: MessageConfirmationKind,
    author: &str,
    content: Option<&str>,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    confirmation_lines(kind.prompt(), author, content, width, active)
}

fn confirmation_lines(
    prompt: String,
    author: &str,
    content: Option<&str>,
    width: usize,
    active: ConfirmationButton,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let excerpt = content
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(|content| content.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "[no text content]".to_owned());
    let excerpt = truncate_display_width(&excerpt, width.saturating_sub(2));
    let mut lines = vec![
        Line::from(Span::raw(prompt)),
        Line::from(Span::styled(
            format!("From: {author}"),
            Style::default().fg(theme::current().dim),
        )),
        Line::from(Span::styled(
            format!("\"{excerpt}\""),
            Style::default().fg(theme::current().error),
        )),
        Line::from(Span::raw(String::new())),
    ];
    lines.extend(confirmation_button_lines(active));
    lines
}
