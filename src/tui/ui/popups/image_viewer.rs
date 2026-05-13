use super::*;

const IMAGE_VIEWER_DOWNLOAD_HINT: &str = "[d] download image";

pub(in crate::tui::ui) fn render_image_viewer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    image_preview: Option<ImagePreview<'_>>,
) {
    let Some(item) = state.selected_image_viewer_item() else {
        return;
    };

    let popup = image_viewer_popup(area);
    let title_width = usize::from(popup.width.saturating_sub(4)).max(1);
    let title = truncate_display_width(&image_viewer_title(&item), title_width);
    frame.render_widget(Clear, popup);
    let block = panel_block_owned(title, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let image_area = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let download_area = Rect {
        y: inner.y + inner.height.saturating_sub(1),
        height: inner.height.min(1),
        ..inner
    };
    let hint_y = popup.y.saturating_add(popup.height);
    let hint_area = (hint_y < area.y.saturating_add(area.height)).then_some(Rect {
        y: hint_y,
        height: 1,
        ..popup
    });

    if let Some(image_preview) = image_preview {
        render_image_preview(frame, image_area, image_preview.state);
    } else {
        frame.render_widget(
            Paragraph::new(format!("loading {}...", item.filename))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            image_area,
        );
    }
    if let Some(message) = state.image_viewer_download_message() {
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
            Paragraph::new(IMAGE_VIEWER_DOWNLOAD_HINT)
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            hint_area,
        );
    }
}

fn image_viewer_title(item: &ImageViewerItem) -> String {
    format!("Image {}/{} - {}", item.index, item.total, item.filename)
}
