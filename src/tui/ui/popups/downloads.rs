use super::*;

const MAX_VISIBLE_DOWNLOADS: usize = 3;
const DOWNLOADS_POPUP_MAX_WIDTH: u16 = 48;

pub(in crate::tui::ui) fn render_downloads_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let downloads = state.attachment_downloads();
    if downloads.is_empty() {
        return;
    }

    let line_count = downloads_popup_line_count(downloads.len());
    let popup = downloads_popup_area(area, line_count);
    if popup.is_empty() {
        return;
    }

    let lines = downloads_popup_lines(&downloads, popup.width.saturating_sub(2));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block("Downloads", true)),
        popup,
    );
}

pub(in crate::tui::ui) fn downloads_popup_line_count(download_count: usize) -> usize {
    download_count.min(MAX_VISIBLE_DOWNLOADS) + usize::from(download_count > MAX_VISIBLE_DOWNLOADS)
}

pub(in crate::tui::ui) fn downloads_popup_area(area: Rect, line_count: usize) -> Rect {
    if area.width < 3 || area.height < 3 || line_count == 0 {
        return Rect::default();
    }

    let width = area.width.min(DOWNLOADS_POPUP_MAX_WIDTH);
    let height = u16::try_from(line_count)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
        .min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width),
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    }
}

pub(in crate::tui::ui) fn downloads_popup_lines(
    downloads: &[AttachmentDownloadProgressView],
    width: u16,
) -> Vec<Line<'static>> {
    let visible_count = downloads.len().min(MAX_VISIBLE_DOWNLOADS);
    let start = downloads.len().saturating_sub(visible_count);
    let mut lines: Vec<Line<'static>> = downloads[start..]
        .iter()
        .map(|download| download_progress_line(download, usize::from(width)))
        .collect();
    let hidden_count = downloads.len().saturating_sub(visible_count);
    if hidden_count > 0 {
        lines.push(Line::from(Span::styled(
            format!("+{hidden_count} more"),
            Style::default().fg(DIM),
        )));
    }
    lines
}

fn download_progress_line(
    download: &AttachmentDownloadProgressView,
    width: usize,
) -> Line<'static> {
    let status = match download.total_bytes.filter(|total| *total > 0) {
        Some(total) => {
            let percent = download
                .downloaded_bytes
                .saturating_mul(100)
                .checked_div(total)
                .unwrap_or_default()
                .min(100);
            format!(
                "{}% {}/{}",
                percent,
                format_byte_size(download.downloaded_bytes),
                format_byte_size(total)
            )
        }
        None => format!("{} downloaded", format_byte_size(download.downloaded_bytes)),
    };
    let text = format!("{} {status}", download.filename);
    Line::from(Span::raw(truncate_display_width(&text, width)))
}
