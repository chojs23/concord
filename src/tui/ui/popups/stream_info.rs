use super::*;

const MAX_STREAM_INFO_WIDTH: u16 = 36;

pub(in crate::tui::ui) fn render_stream_info(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let lines = stream_info_lines_for_area(state, area);
    let popup = stream_info_area(area, &lines);
    if popup.is_empty() {
        return;
    }

    clear_area(frame, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::current().style(theme::HighlightGroup::Normal))
            .block(panel_block(" Streams ", false)),
        popup,
    );
}

pub(in crate::tui::ui) fn stream_info_area(area: Rect, lines: &[Line<'_>]) -> Rect {
    if lines.is_empty() || area.width < 3 || area.height < 3 {
        return Rect::default();
    }

    let content_width = lines.iter().map(Line::width).max().unwrap_or(1) as u16;
    let width = content_width
        .saturating_add(2)
        .min(MAX_STREAM_INFO_WIDTH)
        .min(area.width);
    let height = (lines.len() as u16).saturating_add(2).min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width),
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    }
}

#[cfg(test)]
pub(in crate::tui::ui) fn stream_info_lines(state: &DashboardState) -> Vec<Line<'static>> {
    stream_info_lines_for_width(state, usize::from(MAX_STREAM_INFO_WIDTH.saturating_sub(2)))
}

pub(in crate::tui::ui) fn stream_info_lines_for_area(
    state: &DashboardState,
    area: Rect,
) -> Vec<Line<'static>> {
    let content_width = area.width.min(MAX_STREAM_INFO_WIDTH).saturating_sub(2);
    stream_info_lines_for_width(state, usize::from(content_width))
}

pub(in crate::tui::ui) fn stream_info_lines_for_width(
    state: &DashboardState,
    content_width: usize,
) -> Vec<Line<'static>> {
    let sections = state.stream_info_sections();
    let mut lines = Vec::new();
    let content_width = content_width.max(1);

    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            lines.push(Line::styled(
                "=".repeat(content_width),
                theme::current().style(theme::HighlightGroup::Muted),
            ));
        }

        let status = if section.paused { "⏸" } else { "🔴" };
        lines.push(Line::from(vec![
            Span::raw(section.broadcaster.clone()),
            Span::styled(
                format!(" {status}"),
                theme::current().style(theme::HighlightGroup::Success),
            ),
        ]));
        lines.push(Line::styled(
            "-".repeat(content_width),
            theme::current().style(theme::HighlightGroup::Muted),
        ));
        for viewer in &section.viewers {
            lines.extend(
                wrap_stream_info_text(viewer, content_width)
                    .into_iter()
                    .map(Line::from),
            );
        }
    }

    lines
}

fn wrap_stream_info_text(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in value.split_whitespace() {
        if word.width() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.extend(wrap_text_lines(word, width));
            continue;
        }

        let next_width = if current.is_empty() {
            word.width()
        } else {
            current
                .width()
                .saturating_add(1)
                .saturating_add(word.width())
        };
        if next_width > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
