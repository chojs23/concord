use super::*;
use ratatui_image::{Image, protocol::Protocol};

pub(in crate::tui) fn gif_picker_popup_area(area: Rect) -> Rect {
    centered_rect(area, 84, 18)
}

fn gif_picker_body(area: Rect) -> Rect {
    let inner = panel_block("", false).inner(gif_picker_popup_area(area));
    Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(4),
        ..inner
    }
}

pub(in crate::tui) fn gif_picker_preview_area(area: Rect) -> Rect {
    let body = gif_picker_body(area);
    if body.width < 40 || body.height < 3 {
        return Rect::default();
    }
    Rect {
        x: body.x + body.width / 2 + 1,
        width: body.width - body.width / 2 - 1,
        ..body
    }
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
    let query_area = Rect { height: 1, ..inner };
    let (query, cursor) = visible_query(
        picker.query.value(),
        picker.query.cursor_byte_index(),
        usize::from(inner.width),
    );
    let placeholder = picker.query.value().is_empty();
    frame.render_widget(
        Paragraph::new(if placeholder {
            "Search KLIPY".to_owned()
        } else {
            query
        })
        .style(if placeholder {
            theme::current().style(theme::HighlightGroup::Hint)
        } else {
            Style::default()
        }),
        query_area,
    );
    frame.set_cursor_position((query_area.x + cursor as u16, query_area.y));
    let preview_area = gif_picker_preview_area(area);
    let body = gif_picker_body(area);
    let list = Rect {
        width: if preview_area.is_empty() {
            body.width
        } else {
            body.width / 2
        },
        ..body
    };
    let lines = if picker.loading {
        vec![Line::from("Searching KLIPY…")]
    } else if let Some(error) = &picker.error {
        vec![Line::from(error.clone()), Line::from("Enter to retry")]
    } else if picker.results.is_empty() {
        vec![Line::from("No GIFs found")]
    } else {
        let rows = usize::from(list.height).max(1);
        let start = picker.selected.saturating_sub(rows - 1);
        picker
            .results
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .map(|(i, gif)| {
                let title = gif
                    .title
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>();
                Line::from(Span::styled(
                    truncate_display_width(
                        &format!("{} {}", if i == picker.selected { "›" } else { " " }, title),
                        usize::from(list.width),
                    ),
                    selectable_popup_label_style(
                        i == picker.selected,
                        gif.media_url(false).is_some(),
                    ),
                ))
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), list);
    if !preview_area.is_empty() && !picker.results.is_empty() {
        match preview {
            Some(Ok(protocol)) => frame.render_widget(Image::new(protocol), preview_area),
            Some(Err(error)) => frame.render_widget(
                Paragraph::new(error).wrap(Wrap { trim: false }),
                preview_area,
            ),
            None => frame.render_widget(
                Paragraph::new(
                    if picker
                        .results
                        .get(picker.selected)
                        .and_then(|gif| gif.media_url(true))
                        .is_none()
                    {
                        "Preview unavailable"
                    } else if state.show_images() {
                        "Loading preview…"
                    } else {
                        "Image previews disabled"
                    },
                ),
                preview_area,
            ),
        }
    }
    if inner.height >= 3 {
        frame.render_widget(
            Paragraph::new(format!(
                "Page {} · ↑/↓ select · PgUp/PgDn pages",
                picker.page
            )),
            Rect {
                y: inner.y + inner.height - 2,
                height: 1,
                ..inner
            },
        );
        frame.render_widget(
            Paragraph::new("Enter: add to draft · Esc: cancel")
                .style(theme::current().style(theme::HighlightGroup::Hint)),
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
