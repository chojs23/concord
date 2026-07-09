use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::state::DashboardState;

use super::super::theme;

pub(in crate::tui::ui) fn render_header(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let theme = theme::current();
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    let mut spans = vec![Span::styled(
        title,
        Style::default().fg(theme.accent).bold(),
    )];
    if let Some(user) = state.current_user() {
        spans.push(Span::styled(
            " Connected as ",
            Style::default().fg(theme.dim),
        ));
        spans.push(Span::styled(
            format!("{user} "),
            Style::default().fg(theme.text).bold(),
        ));
        let (self_mute, self_deaf) = state.current_voice_self_status();
        if self_mute {
            spans.push(Span::styled("🔇 ", Style::default().fg(theme.warning)));
        }
        if self_deaf {
            spans.push(Span::styled("🎧 ", Style::default().fg(theme.warning)));
        }
    } else if let Some(error) = state.gateway_error() {
        spans.push(Span::styled(
            format!(" Connection issue: {} ", truncate_header_error(error)),
            Style::default().fg(theme.error).bold(),
        ));
    } else {
        spans.push(Span::styled(
            " Loading... ",
            Style::default().fg(theme.warning).bold(),
        ));
    }
    if let Some(version) = state.update_available_version() {
        spans.push(Span::styled(
            format!(" New version available: v{version} "),
            Style::default().fg(theme.warning).bold(),
        ));
    }
    if let Some(label) = state.active_voice_connection_label() {
        spans.push(Span::styled(" Voice ", Style::default().fg(theme.dim)));
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(theme.warning).bold(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
}

fn truncate_header_error(error: &str) -> String {
    const MAX_CHARS: usize = 96;
    let mut chars = error.chars();
    let truncated: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
