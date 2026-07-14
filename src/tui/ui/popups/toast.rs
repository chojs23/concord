use super::*;
use crate::tui::state::ToastKind;

pub(in crate::tui::ui) fn render_toast(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(toast) = state.toast_message() else {
        return;
    };

    let popup = toast_area(area, toast.text);
    if popup.is_empty() {
        return;
    }

    let feedback_style = toast_feedback_style(toast.kind);
    clear_area(frame, popup);
    frame.render_widget(
        Paragraph::new(toast_line(
            toast.text,
            popup.width.saturating_sub(2) as usize,
        ))
        .style(feedback_style)
        .block(panel_block("", true).border_style(feedback_style)),
        popup,
    );
}

pub(in crate::tui::ui) fn toast_area(area: Rect, text: &str) -> Rect {
    if area.width < 3 || area.height < 3 {
        return Rect::default();
    }

    let content_width = text.width().max(1) as u16;
    let width = content_width.saturating_add(2).min(area.width);
    Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width,
        height: 3,
    }
}

pub(in crate::tui::ui) fn toast_line(text: &str, width: usize) -> Line<'static> {
    Line::from(Span::raw(truncate_display_width(text, width)))
}

fn toast_feedback_style(kind: ToastKind) -> Style {
    theme::current().style(match kind {
        ToastKind::Info => theme::HighlightGroup::Info,
        ToastKind::Success => theme::HighlightGroup::Success,
        ToastKind::Error => theme::HighlightGroup::Error,
    })
}
