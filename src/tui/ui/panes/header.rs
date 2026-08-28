use ratatui::{
    Frame,
    layout::{Alignment, Rect},
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
        theme.style(theme::HighlightGroup::HeaderTitle),
    )];
    if let Some(user) = state.current_user() {
        spans.push(Span::styled(
            " Connected as ",
            theme.style(theme::HighlightGroup::HeaderLabel),
        ));
        spans.push(Span::styled(
            format!("{user} "),
            theme.style(theme::HighlightGroup::Strong),
        ));
        let (self_mute, self_deaf) = state.current_voice_self_status();
        if self_mute {
            spans.push(Span::styled(
                "🔇 ",
                theme.style(theme::HighlightGroup::VoiceDisabled),
            ));
        }
        if self_deaf {
            spans.push(Span::styled(
                "🎧 ",
                theme.style(theme::HighlightGroup::VoiceDisabled),
            ));
        }
    } else if let Some(error) = state.gateway_error() {
        spans.push(Span::styled(
            format!(" Connection issue: {} ", truncate_header_error(error)),
            theme.style(theme::HighlightGroup::HeaderError),
        ));
    } else {
        spans.push(Span::styled(
            " Loading... ",
            theme.style(theme::HighlightGroup::HeaderWarning),
        ));
    }
    if let Some(version) = state.update_available_version() {
        spans.push(Span::styled(
            format!(" New version available: v{version} "),
            theme.style(theme::HighlightGroup::HeaderWarning),
        ));
    }
    if let Some(label) = state.active_voice_connection_label() {
        spans.push(Span::styled(
            " Voice ",
            theme.style(theme::HighlightGroup::HeaderLabel),
        ));
        spans.push(Span::styled(
            format!("{label} "),
            theme.style(theme::HighlightGroup::VoiceConnection),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .style(theme.style(theme::HighlightGroup::Normal))
            .alignment(Alignment::Left),
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
