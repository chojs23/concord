use super::super::message::list::format_message_sent_time;
use super::*;
use crate::tui::state::{MemberSearchResultItem, SearchSuggestionItem};

const SEARCH_POPUP_WIDTH: u16 = 86;

pub(in crate::tui::ui) fn render_search_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::Search) {
        return;
    }
    let Some(view) = state.search_popup_view() else {
        return;
    };
    let popup = search_popup_area(area, &view);
    let max_result_lines = search_popup_result_capacity(popup, &view);
    frame.render_widget(Clear, popup);
    render_modal_paragraph(
        frame,
        popup,
        view.mode.title(),
        search_popup_lines(&view, max_result_lines, popup.width as usize - 2),
    );
    if let Some(position) = search_popup_cursor_position(popup, &view) {
        frame.set_cursor_position(position);
    }
}

pub(in crate::tui::ui) fn search_popup_area(area: Rect, view: &SearchPopupView) -> Rect {
    let field_rows = view.fields.len() as u16;
    let height = area
        .height
        .saturating_sub(2)
        .clamp(field_rows.saturating_add(8), 24);
    centered_rect(area, SEARCH_POPUP_WIDTH, height)
}

pub(in crate::tui::ui) fn search_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let view = state.search_popup_view()?;
    Some(search_popup_area(area, &view))
}

fn search_popup_result_capacity(popup: Rect, view: &SearchPopupView) -> usize {
    usize::from(
        popup
            .height
            .saturating_sub(view.fields.len() as u16)
            .saturating_sub(4),
    )
    .max(1)
}

pub(in crate::tui::ui) fn search_popup_lines(
    view: &SearchPopupView,
    max_result_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = view
        .fields
        .iter()
        .map(|field| search_field_line(field, width))
        .collect::<Vec<_>>();
    let status = if view.loading {
        "Searching...".to_owned()
    } else if let Some(error) = &view.error {
        error.clone()
    } else if let Some(total) = view.total_results {
        format!("{} result(s)", total)
    } else {
        match view.mode {
            SearchPopupMode::Message => "Enter filters, then [Enter] search".to_owned(),
            SearchPopupMode::Member => "Type to filter members".to_owned(),
        }
    };
    lines.push(Line::from(Span::styled(
        "─".repeat(width.max(1)),
        Style::default().fg(DIM),
    )));
    push_wrapped_styled_popup_text(&mut lines, &status, width, Style::default().fg(DIM));

    if !view.suggestions.is_empty() {
        let start = search_popup_result_start(
            view.suggestions.len(),
            view.selected_suggestion,
            max_result_lines,
        );
        for (index, suggestion) in view
            .suggestions
            .iter()
            .enumerate()
            .skip(start)
            .take(max_result_lines)
        {
            lines.push(search_suggestion_line(
                suggestion,
                index == view.selected_suggestion,
                width,
            ));
        }
        return lines;
    }

    if view.results.is_empty() && !view.loading {
        lines.push(Line::from(Span::styled(
            "No results",
            Style::default().fg(DIM),
        )));
        return lines;
    }

    let start = search_popup_result_start(view.results.len(), view.selected, max_result_lines);
    for (index, result) in view
        .results
        .iter()
        .enumerate()
        .skip(start)
        .take(max_result_lines)
    {
        lines.push(search_result_line(result, index == view.selected, width));
    }
    if view.has_more {
        push_wrapped_styled_popup_text(
            &mut lines,
            "More results: [Down/PageDown] load more at the end",
            width,
            Style::default().fg(DIM),
        );
    }
    lines
}

fn search_field_line(field: &SearchFieldView, width: usize) -> Line<'static> {
    let label = format!("{:>11}: ", field.label);
    let available = width.saturating_sub(label.width()).max(1);
    let value = if field.value.is_empty() {
        Span::styled(
            truncate_display_width(&field.placeholder, available),
            Style::default().fg(DIM),
        )
    } else {
        Span::raw(truncate_display_width(&field.value, available))
    };
    let label_style = if field.active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![Span::styled(label, label_style), value])
}

fn search_result_line(result: &SearchResultItem, selected: bool, width: usize) -> Line<'static> {
    let style = if selected {
        highlight_style()
    } else {
        Style::default()
    };
    let mut spans = vec![selectable_popup_marker(selected)];
    match result {
        SearchResultItem::Message(item) => {
            spans.push(Span::styled(
                format!("#{} ", item.channel_label),
                Style::default().fg(ACCENT),
            ));
            spans.push(Span::styled(
                format!("{} ", item.author),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{}: ", format_message_sent_time(item.message_id)),
                Style::default().fg(DIM),
            ));
            spans.push(Span::raw(item.content.clone()));
        }
        SearchResultItem::Member(item) => {
            push_member_search_spans(&mut spans, item, true);
        }
    }
    let mut line = truncate_line_to_display_width(Line::from(spans), width);
    line.style = style;
    line
}

fn search_suggestion_line(
    suggestion: &SearchSuggestionItem,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let style = if selected {
        highlight_style()
    } else {
        Style::default()
    };
    let mut spans = vec![selectable_popup_marker(selected)];
    match suggestion {
        SearchSuggestionItem::Member(item) => {
            push_member_search_spans(&mut spans, item, false);
        }
        SearchSuggestionItem::Channel(item) => {
            spans.push(Span::styled(
                format!("#{}", item.channel_label),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            if let Some(guild_label) = &item.guild_label {
                spans.push(Span::styled(
                    format!(" in {guild_label}"),
                    Style::default().fg(DIM),
                ));
            }
        }
    }
    let mut line = truncate_line_to_display_width(Line::from(spans), width);
    line.style = style;
    line
}

fn push_member_search_spans(
    spans: &mut Vec<Span<'static>>,
    item: &MemberSearchResultItem,
    show_bot_marker: bool,
) {
    spans.push(Span::styled(
        format!("{} ", presence_marker(item.status)),
        Style::default().fg(presence_color(item.status)),
    ));
    spans.push(Span::styled(
        item.display_name.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    if let Some(username) = &item.username {
        spans.push(Span::styled(
            format!(" @{username}"),
            Style::default().fg(DIM),
        ));
    }
    if show_bot_marker && item.is_bot {
        spans.push(Span::styled(" [bot]", Style::default().fg(DIM)));
    }
}

fn search_popup_result_start(len: usize, selected: usize, max_result_lines: usize) -> usize {
    if len == 0 || max_result_lines == 0 {
        return 0;
    }
    selected
        .min(len.saturating_sub(1))
        .saturating_sub(max_result_lines / 2)
}

fn search_popup_cursor_position(popup: Rect, view: &SearchPopupView) -> Option<Position> {
    let row = view.fields.iter().position(|field| field.active)?;
    let field = view.fields.get(row)?;
    let label_width = format!("{:>11}: ", field.label).width();
    let value_width = field.value[..field.cursor.min(field.value.len())].width();
    let x = popup
        .x
        .saturating_add(1)
        .saturating_add((label_width + value_width) as u16)
        .min(popup.x.saturating_add(popup.width.saturating_sub(2)));
    Some(Position::new(x, popup.y.saturating_add(1 + row as u16)))
}
