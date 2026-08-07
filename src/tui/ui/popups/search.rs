use super::super::message::list::format_message_sent_time;
use super::*;
use crate::tui::state::{MemberSearchResultItem, SearchSuggestionItem};
use crate::tui::ui::loading_indicator::AsciiLoadingIndicator;

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
    let layout = search_popup_render_layout(area, &view, state.animation_frame());
    let inner = render_modal_frame(frame, layout.popup, view.mode.title());
    frame.render_widget(Paragraph::new(layout.header_lines), layout.header);
    frame.render_widget(
        Paragraph::new(search_popup_result_lines(
            &view,
            usize::from(layout.results.height),
            usize::from(layout.results.width),
        )),
        layout.results,
    );
    frame.render_widget(Paragraph::new(layout.footer_lines), layout.footer);
    let (scroll, content_len) = if view.suggestions.is_empty() {
        (view.scroll, view.results.len())
    } else {
        (view.suggestion_scroll, view.suggestions.len())
    };
    render_vertical_scrollbar(
        frame,
        Rect {
            y: layout.results.y,
            height: layout.results.height,
            ..inner
        },
        scroll,
        usize::from(layout.results.height),
        content_len,
    );
    if let Some(position) = search_popup_cursor_position(layout.popup, &view) {
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

struct SearchPopupRenderLayout {
    popup: Rect,
    header: Rect,
    results: Rect,
    footer: Rect,
    header_lines: Vec<Line<'static>>,
    footer_lines: Vec<Line<'static>>,
}

fn search_popup_render_layout(
    area: Rect,
    view: &SearchPopupView,
    animation_frame: usize,
) -> SearchPopupRenderLayout {
    let popup = search_popup_area(area, view);
    let inner = panel_block(view.mode.title(), true).inner(popup);
    let content = Rect {
        width: inner.width.saturating_sub(1).max(1),
        ..inner
    };
    let width = usize::from(content.width).max(1);
    let header_lines = search_popup_header_lines(view, width, animation_frame);
    let footer_lines = search_popup_footer_lines(view, width);
    let header_height = u16::try_from(header_lines.len())
        .unwrap_or(u16::MAX)
        .min(content.height);
    let footer_height = u16::try_from(footer_lines.len())
        .unwrap_or(u16::MAX)
        .min(content.height.saturating_sub(header_height));
    let results_height = content
        .height
        .saturating_sub(header_height)
        .saturating_sub(footer_height);
    let header = Rect {
        height: header_height,
        ..content
    };
    let results = Rect {
        y: content.y.saturating_add(header_height),
        height: results_height,
        ..content
    };
    let footer = Rect {
        y: results.y.saturating_add(results.height),
        height: footer_height,
        ..content
    };
    SearchPopupRenderLayout {
        popup,
        header,
        results,
        footer,
        header_lines,
        footer_lines,
    }
}

pub(in crate::tui::ui) fn search_popup_list_layout(
    area: Rect,
    state: &DashboardState,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    let view = state
        .search_popup_view()
        .expect("selectable search popup has a view");
    let layout = search_popup_render_layout(area, &view, state.animation_frame());
    SelectablePopupLayout::new(
        snapshot.target,
        layout.popup,
        layout.results,
        snapshot,
        |start, max_rows| {
            (start..snapshot.item_count.min(start.saturating_add(max_rows)))
                .map(Some)
                .collect()
        },
    )
}

fn search_popup_header_lines(
    view: &SearchPopupView,
    width: usize,
    animation_frame: usize,
) -> Vec<Line<'static>> {
    let mut lines = view
        .fields
        .iter()
        .map(|field| search_field_line(field, width))
        .collect::<Vec<_>>();
    lines.push(Line::from(Span::styled(
        "─".repeat(width.max(1)),
        theme::current().style(theme::HighlightGroup::Decoration),
    )));
    if view.loading {
        lines.extend(
            AsciiLoadingIndicator::new(
                "Searching...",
                theme::current().style(theme::HighlightGroup::Hint),
            )
            .lines(animation_frame),
        );
    } else {
        let status = if let Some(error) = &view.error {
            error.clone()
        } else if let Some(total) = view.total_results {
            format!("{} result(s)", total)
        } else {
            match view.mode {
                SearchPopupMode::Message => "Enter filters, then [Enter] search".to_owned(),
                SearchPopupMode::Member => "Type to filter members".to_owned(),
            }
        };
        push_wrapped_styled_popup_text(
            &mut lines,
            &status,
            width,
            theme::current().style(theme::HighlightGroup::Hint),
        );
    }

    lines
}

fn search_popup_result_lines(
    view: &SearchPopupView,
    max_result_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    if max_result_lines == 0 {
        return Vec::new();
    }
    if !view.suggestions.is_empty() {
        let start = view
            .suggestion_scroll
            .min(view.suggestions.len().saturating_sub(max_result_lines));
        return view
            .suggestions
            .iter()
            .enumerate()
            .skip(start)
            .take(max_result_lines)
            .map(|(index, suggestion)| {
                search_suggestion_line(suggestion, index == view.selected_suggestion, width)
            })
            .collect();
    }

    if view.results.is_empty() && !view.loading {
        return vec![Line::from(Span::styled(
            "No results",
            theme::current().style(theme::HighlightGroup::Placeholder),
        ))];
    }

    let start = view
        .scroll
        .min(view.results.len().saturating_sub(max_result_lines));
    view.results
        .iter()
        .enumerate()
        .skip(start)
        .take(max_result_lines)
        .map(|(index, result)| search_result_line(result, index == view.selected, width))
        .collect()
}

fn search_popup_footer_lines(view: &SearchPopupView, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if view.suggestions.is_empty() && view.has_more {
        push_wrapped_styled_popup_text(
            &mut lines,
            "More results: [Down/PageDown] load more at the end",
            width,
            theme::current().style(theme::HighlightGroup::Hint),
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
            theme::current().style(theme::HighlightGroup::Placeholder),
        )
    } else if field.active {
        Span::styled(
            truncate_display_width(&field.value, available),
            editable_field_value_style(true, false),
        )
    } else {
        Span::styled(
            truncate_display_width(&field.value, available),
            editable_field_value_style(false, false),
        )
    };
    let label_style = editable_field_label_style(field.active, false);
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
                Style::default(),
            ));
            spans.push(Span::styled(
                format!("{} ", item.author),
                theme::current().style(theme::HighlightGroup::MessageAuthor),
            ));
            spans.push(Span::styled(
                format!("{}: ", format_message_sent_time(item.message_id)),
                theme::current().style(theme::HighlightGroup::MessageTimestamp),
            ));
            spans.push(Span::raw(item.content.clone()));
        }
        SearchResultItem::Member(item) => {
            push_member_search_spans(&mut spans, item, selected, true);
        }
    }
    let mut line = truncate_line_to_display_width(Line::from(spans), width);
    line.style = style;
    selected_row_line(line, selected)
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
            push_member_search_spans(&mut spans, item, selected, false);
        }
        SearchSuggestionItem::Channel(item) => {
            spans.push(Span::styled(
                format!("#{}", item.channel_label),
                theme::current().style(theme::HighlightGroup::Heading),
            ));
            if let Some(guild_label) = &item.guild_label {
                spans.push(Span::styled(
                    format!(" in {guild_label}"),
                    theme::current().style(theme::HighlightGroup::SearchContext),
                ));
            }
        }
    }
    let mut line = truncate_line_to_display_width(Line::from(spans), width);
    line.style = style;
    selected_row_line(line, selected)
}

fn push_member_search_spans(
    spans: &mut Vec<Span<'static>>,
    item: &MemberSearchResultItem,
    selected: bool,
    show_bot_marker: bool,
) {
    spans.push(Span::styled(
        format!("{} ", presence_marker(item.status)),
        selected_presence_style(selected, item.status),
    ));
    spans.push(Span::styled(
        item.display_name.clone(),
        theme::current().style(theme::HighlightGroup::Strong),
    ));
    if let Some(username) = &item.username {
        spans.push(Span::styled(
            format!(" @{username}"),
            theme::current().style(theme::HighlightGroup::SearchContext),
        ));
    }
    if show_bot_marker && item.is_bot {
        spans.push(Span::raw(" [bot]"));
    }
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
        .min(popup.x.saturating_add(popup.width.saturating_sub(3)));
    Some(Position::new(x, popup.y.saturating_add(1 + row as u16)))
}
