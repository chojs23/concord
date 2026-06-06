use super::*;

pub(in crate::tui::ui) fn render_attachment_viewer(
    frame: &mut Frame,
    messages_area: Rect,
    frame_area: Rect,
    state: &DashboardState,
    image_preview: Option<ImagePreview<'_>>,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::AttachmentViewer) {
        return;
    }

    let Some(item) = state.selected_attachment_viewer_item() else {
        return;
    };

    let zoom = state.attachment_viewer_zoom();
    let popup = attachment_viewer_popup(messages_area, frame_area, zoom);
    let title_width = usize::from(popup.width.saturating_sub(4)).max(1);
    let title = truncate_display_width(&attachment_viewer_title(&item), title_width);
    frame.render_widget(Clear, popup);
    let block = panel_block_owned(title, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let body_area = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let download_area = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: inner.height.min(1),
        ..inner
    };
    let hint_y = popup.y.saturating_add(popup.height);
    let hint_lines = wrapped_styled_popup_lines(
        state.key_bindings().attachment_viewer_download_hint(),
        usize::from(popup.width),
        Style::default().fg(DIM),
    );
    let available_hint_height = frame_area
        .y
        .saturating_add(frame_area.height)
        .saturating_sub(hint_y);
    let hint_area = (available_hint_height > 0).then_some(Rect {
        y: hint_y,
        height: available_hint_height.min(u16::try_from(hint_lines.len()).unwrap_or(u16::MAX)),
        ..popup
    });

    if item.is_image
        && state.show_images()
        && let Some(image_preview) = image_preview
    {
        let preview_area = centered_viewer_preview_area(
            body_area,
            image_preview.preview_width,
            image_preview.preview_height,
        );
        render_image_preview(frame, preview_area, image_preview.state);
    } else if item.is_image && state.show_images() {
        frame.render_widget(
            Paragraph::new(format!("loading {}...", item.filename))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            body_area,
        );
    } else {
        render_attachment_details(frame, body_area, &item);
    }

    if let Some(message) = state.attachment_viewer_download_message() {
        frame.render_widget(
            Paragraph::new(truncate_display_width(
                message,
                download_area.width.saturating_sub(1).into(),
            ))
            .style(Style::default().fg(Color::Green)),
            download_area,
        );
    }
    if let Some(hint_area) = hint_area {
        frame.render_widget(
            Paragraph::new(hint_lines).alignment(Alignment::Center),
            hint_area,
        );
    }
}

pub(in crate::tui::ui) fn centered_viewer_preview_area(
    area: Rect,
    preview_width: u16,
    preview_height: u16,
) -> Rect {
    if area.is_empty() || preview_width == 0 || preview_height == 0 {
        return Rect::default();
    }

    let width = preview_width.min(area.width);
    let height = preview_height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn render_attachment_details(frame: &mut Frame, area: Rect, item: &AttachmentViewerItem) {
    let lines = vec![
        Line::from(vec![
            Span::styled("File: ", Style::default().fg(DIM)),
            Span::raw(item.filename.clone()),
        ]),
        Line::from(vec![
            Span::styled("Size: ", Style::default().fg(DIM)),
            Span::raw(format_byte_size(item.size_bytes)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn attachment_viewer_title(item: &AttachmentViewerItem) -> String {
    format!(
        "Attachment {}/{} - {}",
        item.index, item.total, item.filename
    )
}
