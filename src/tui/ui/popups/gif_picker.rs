use super::*;
use crate::tui::text::truncate_display_width_from;
use ratatui_image::{Image, protocol::Protocol};

pub(in crate::tui) fn gif_picker_popup_area(area: Rect) -> Rect {
    centered_rect(area, 84, 18)
}

pub(in crate::tui) fn gif_picker_search_area(area: Rect) -> Rect {
    let inner = panel_block("", false).inner(gif_picker_popup_area(area));
    Rect {
        x: inner.x.saturating_add(1),
        width: inner.width.saturating_sub(2),
        height: inner.height.min(1),
        ..inner
    }
}

fn gif_picker_body(area: Rect) -> Rect {
    let inner = panel_block("", false).inner(gif_picker_popup_area(area));
    Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(3),
        ..inner
    }
}

pub(in crate::tui) fn gif_picker_preview_area(area: Rect) -> Rect {
    let body = gif_picker_body(area);
    if body.width < 40 || body.height < 4 {
        return Rect::default();
    }
    Rect {
        x: body.x + body.width / 2 + 1,
        width: body.width - body.width / 2 - 1,
        ..body
    }
}

fn gif_picker_list_area(area: Rect) -> Rect {
    let body = gif_picker_body(area);
    let width = if gif_picker_preview_area(area).is_empty() {
        body.width
    } else {
        body.width / 2
    };
    Rect {
        x: body.x.saturating_add(1),
        width: width.saturating_sub(2),
        ..body
    }
}

pub(in crate::tui::ui) fn gif_picker_list_layout(
    area: Rect,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    SelectablePopupLayout::new(
        snapshot.target,
        gif_picker_popup_area(area),
        gif_picker_list_area(area),
        snapshot,
        |start, max_rows| {
            (start..snapshot.item_count.min(start.saturating_add(max_rows)))
                .map(Some)
                .collect()
        },
    )
}

pub(in crate::tui::ui) fn render_gif_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    preview: Option<Result<&Protocol, &str>>,
) {
    let Some(picker) = state.gif_picker() else {
        return;
    };
    let inner = render_modal_frame(
        frame,
        gif_picker_popup_area(area),
        "GIFs · Powered by KLIPY",
    );
    if inner.is_empty() {
        return;
    }
    let query_area = gif_picker_search_area(area);
    let query_marker = editable_field_marker(picker.query_selected);
    let query_prefix_width = query_marker.width();
    let query_width = usize::from(query_area.width).saturating_sub(query_prefix_width);
    let (query, cursor) = visible_query(
        picker.query.value(),
        picker.query.cursor_byte_index(),
        query_width,
    );
    let placeholder = picker.query.value().is_empty();
    let search_style = if picker.query_selected {
        theme::current().style(theme::HighlightGroup::ActiveField)
    } else {
        theme::current().style(theme::HighlightGroup::Hint)
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{}{}",
            query_marker,
            if placeholder { "Search KLIPY" } else { &query }
        ))
        .style(search_style),
        query_area,
    );
    if picker.query_editing {
        frame.set_cursor_position((
            query_area
                .x
                .saturating_add(query_prefix_width as u16 + cursor as u16),
            query_area.y,
        ));
    }
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(inner.width)))
            .style(theme::current().style(theme::HighlightGroup::Decoration)),
        Rect {
            y: inner.y.saturating_add(1),
            height: 1,
            ..inner
        },
    );
    let preview_area = gif_picker_preview_area(area);
    let list = gif_picker_list_area(area);
    let selected = picker.selected_result_index();
    let scroll = state
        .active_selectable_popup_snapshot()
        .filter(|snapshot| snapshot.target == SelectablePopupTarget::GifResults)
        .map(|snapshot| gif_picker_list_layout(area, snapshot).scroll)
        .unwrap_or(0);
    let lines = if picker.results.is_empty() && picker.loading {
        vec![Line::from("Searching KLIPY…")]
    } else if picker.results.is_empty()
        && let Some(error) = &picker.error
    {
        vec![Line::from(error.clone()), Line::from("r: retry search")]
    } else if picker.results.is_empty() {
        vec![Line::from("No GIFs found")]
    } else {
        picker
            .results
            .iter()
            .enumerate()
            .skip(scroll)
            .take(usize::from(list.height))
            .map(|(i, gif)| {
                let title = gif
                    .title
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>();
                let marker = editable_field_marker(Some(i) == selected);
                let title_width = usize::from(list.width).saturating_sub(marker.width());
                let title =
                    truncate_display_width_from(&title, picker.horizontal_scroll, title_width);
                Line::from(Span::styled(
                    truncate_display_width(&format!("{marker}{title}"), usize::from(list.width)),
                    selectable_popup_label_style(
                        Some(i) == selected,
                        gif.media_url(false).is_some(),
                    ),
                ))
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), list);
    if !preview_area.is_empty() {
        match preview {
            Some(Ok(protocol)) => frame.render_widget(Image::new(protocol), preview_area),
            Some(Err(error)) => frame.render_widget(
                Paragraph::new(error).wrap(Wrap { trim: false }),
                preview_area,
            ),
            None => frame.render_widget(
                Paragraph::new(if picker.selected_gif().is_none() {
                    "Select a GIF to preview"
                } else if picker
                    .selected_gif()
                    .and_then(|gif| gif.media_url(true))
                    .is_none()
                {
                    "Preview unavailable"
                } else if state.show_images() {
                    "Loading preview…"
                } else {
                    "Image previews disabled"
                }),
                preview_area,
            ),
        }
    }
    if picker.results.len() > usize::from(list.height) {
        render_vertical_scrollbar(
            frame,
            list,
            scroll,
            usize::from(list.height),
            picker.results.len(),
        );
    }
    let footer = picker
        .error
        .as_deref()
        .or_else(|| (picker.loading && !picker.results.is_empty()).then_some("Loading more…"));
    if inner.height >= 3
        && let Some(footer) = footer
    {
        frame.render_widget(
            Paragraph::new(footer),
            Rect {
                y: inner.y + inner.height - 1,
                height: 1,
                ..inner
            },
        );
    }
}

fn visible_query(query: &str, cursor: usize, width: usize) -> (String, usize) {
    let available = width.saturating_sub(1);
    let mut start = 0;
    while query[start..cursor].width() > available {
        start += query[start..]
            .chars()
            .next()
            .expect("nonempty prefix")
            .len_utf8();
    }
    (
        truncate_display_width(&query[start..], width),
        query[start..cursor].width().min(available),
    )
}
