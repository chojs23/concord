use super::*;
use crate::tui::state::{
    ForumPostAttachmentPreviewView, ForumPostComposerField, ForumPostComposerView,
};
use crate::tui::ui::{LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH};

const FORUM_POST_POPUP_WIDTH: u16 = 78;
const FORUM_POST_POPUP_HEIGHT: u16 = 24;
const BODY_VISIBLE_LINES: usize = 6;

pub(in crate::tui::ui) fn render_forum_post_composer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ForumPostComposer) {
        return;
    }
    let Some(view) = state.forum_post_composer_view() else {
        return;
    };
    let popup = centered_rect(
        area,
        FORUM_POST_POPUP_WIDTH
            .min(area.width.saturating_sub(2))
            .max(12),
        FORUM_POST_POPUP_HEIGHT
            .min(area.height.saturating_sub(2))
            .max(10),
    );
    frame.render_widget(Clear, popup);
    let inner = panel_block("Create Forum Post", true).inner(popup);
    frame.render_widget(
        Paragraph::new(forum_post_composer_lines(&view, inner.width as usize))
            .block(panel_block("Create Forum Post", true))
            .wrap(Wrap { trim: false }),
        popup,
    );

    if let Some(upload_preview) = state.forum_post_attachment_preview()
        && let Some(preview_area) = forum_post_upload_preview_area(&view, inner)
    {
        render_forum_post_upload_preview(frame, preview_area, upload_preview);
    }

    if let Some((line, column)) = forum_post_composer_cursor(&view) {
        let x = inner
            .x
            .saturating_add(column as u16)
            .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        let y = inner
            .y
            .saturating_add(line as u16)
            .min(inner.y.saturating_add(inner.height.saturating_sub(1)));
        frame.set_cursor_position(Position::new(x, y));
    }
}

fn forum_post_composer_lines(view: &ForumPostComposerView, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(field_line(
        "title",
        &view.title,
        view.active_field == ForumPostComposerField::Title,
        view.editing_field == Some(ForumPostComposerField::Title),
        width,
        "(empty)",
    ));
    lines.push(section_line(
        "body:",
        view.active_field == ForumPostComposerField::Body,
        view.editing_field == Some(ForumPostComposerField::Body),
    ));
    let (body_lines, _) = visible_body_window(view);
    for line in &body_lines {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  {line}"), width),
            editing_value_style(view.editing_field == Some(ForumPostComposerField::Body)),
        )));
    }
    if body_lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(DIM),
        )));
    }

    lines.push(section_line(
        "attachments:",
        view.active_field == ForumPostComposerField::Attachments,
        false,
    ));
    if view.attachments.is_empty() {
        let text = if view.paste_pending {
            "  upload: processing clipboard attachment..."
        } else {
            "  (empty)"
        };
        lines.push(Line::from(Span::styled(text, Style::default().fg(DIM))));
    } else {
        for attachment in &view.attachments {
            let marker = if attachment.active { "▸" } else { " " };
            let style = if attachment.active {
                highlight_style()
            } else {
                Style::default()
            };
            let label = format!(
                "{marker} • {} ({})",
                attachment.filename,
                format_byte_size(attachment.size_bytes)
            );
            lines.push(Line::from(Span::styled(
                truncate_display_width(&label, width),
                style,
            )));
            if attachment.active {
                lines.push(Line::from(Span::styled(
                    truncate_display_width("    preview:", width),
                    Style::default().fg(DIM),
                )));
                for _ in 0..LOCAL_UPLOAD_PREVIEW_HEIGHT {
                    lines.push(Line::from(""));
                }
            }
        }
    }

    let tag_label = if view.requires_tag {
        "tags: required"
    } else {
        "tags:"
    };
    lines.push(section_line(
        tag_label,
        view.active_field == ForumPostComposerField::Tags,
        view.editing_field == Some(ForumPostComposerField::Tags),
    ));
    if view.tags.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no tags available",
            Style::default().fg(DIM),
        )));
    } else {
        for tag in &view.tags {
            let marker = if tag.active { "▸" } else { " " };
            let checkbox = if tag.selected { "[x]" } else { "[ ]" };
            let emoji = tag
                .emoji
                .as_deref()
                .map(|emoji| format!(" {emoji}"))
                .unwrap_or_default();
            let style = if tag.active {
                highlight_style()
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                truncate_display_width(&format!("{marker} {checkbox}{emoji} {}", tag.name), width),
                style,
            )));
        }
    }

    let status = view.status.clone().unwrap_or_else(|| {
        popup_shortcut_help_text(&[
            ("Tab", "fields"),
            ("Enter", "open/toggle"),
            ("Ctrl+v", "paste"),
            ("Del/Bksp", "remove"),
            ("s", "create"),
            ("Esc", "close/cancel"),
        ])
    });
    push_wrapped_styled_popup_text(
        &mut lines,
        &status,
        width,
        Style::default().fg(if view.status.is_some() {
            Color::Red
        } else {
            DIM
        }),
    );
    lines
}

fn render_forum_post_upload_preview(
    frame: &mut Frame,
    area: Rect,
    upload_preview: ForumPostAttachmentPreviewView<'_>,
) {
    match upload_preview {
        ForumPostAttachmentPreviewView::Loading { filename } => frame.render_widget(
            Paragraph::new(format!("loading {filename}..."))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ForumPostAttachmentPreviewView::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ForumPostAttachmentPreviewView::Ready {
            protocol,
            content_hash,
        } => {
            frame.render_widget(TrackedImage::new(protocol, content_hash), area);
        }
    }
}

fn forum_post_upload_preview_area(view: &ForumPostComposerView, inner: Rect) -> Option<Rect> {
    let row = forum_post_upload_preview_start_row(view)?;
    let y = inner.y.saturating_add(row as u16);
    if y >= inner.y.saturating_add(inner.height) {
        return None;
    }
    let height = LOCAL_UPLOAD_PREVIEW_HEIGHT.min(inner.y.saturating_add(inner.height) - y);
    if height == 0 {
        return None;
    }
    let x = inner.x.saturating_add(4);
    if x >= inner.x.saturating_add(inner.width) {
        return None;
    }
    Some(Rect {
        x,
        y,
        width: LOCAL_UPLOAD_PREVIEW_WIDTH.min(inner.x.saturating_add(inner.width) - x),
        height,
    })
}

fn forum_post_upload_preview_start_row(view: &ForumPostComposerView) -> Option<usize> {
    let (body_lines, _) = visible_body_window(view);
    let body_count = body_lines.len().max(1);
    let mut row = 1 + 1 + body_count + 1;
    for attachment in &view.attachments {
        row += 1;
        if attachment.active {
            return Some(row + 1);
        }
    }
    None
}

fn field_line(
    label: &str,
    value: &str,
    active: bool,
    editing: bool,
    width: usize,
    placeholder: &str,
) -> Line<'static> {
    let marker = field_marker(active);
    let prefix = format!("{marker}{label}: ");
    let available = width.saturating_sub(prefix.width()).max(1);
    let content = if value.is_empty() {
        Span::styled(
            truncate_display_width(placeholder, available),
            Style::default().fg(DIM),
        )
    } else {
        Span::styled(
            truncate_display_width(value, available),
            editing_value_style(editing),
        )
    };
    Line::from(vec![
        Span::styled(prefix, field_label_style(active, editing)),
        content,
    ])
}

fn section_line(label: &str, active: bool, editing: bool) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}{}", field_marker(active), label),
        field_label_style(active, editing),
    ))
}

fn field_marker(active: bool) -> &'static str {
    if active { "› " } else { "  " }
}

fn field_label_style(active: bool, editing: bool) -> Style {
    if editing {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn editing_value_style(editing: bool) -> Style {
    if editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

fn visible_body_lines(body: &str) -> Vec<&str> {
    if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').collect()
    }
}

fn forum_post_composer_cursor(view: &ForumPostComposerView) -> Option<(usize, usize)> {
    match view.editing_field? {
        ForumPostComposerField::Title => Some((
            0,
            "› title: ".width() + cursor_column(&view.title, view.title_cursor),
        )),
        ForumPostComposerField::Body => {
            let (_, start) = visible_body_window(view);
            let (line, column) = body_cursor_line_column(&view.body, view.body_cursor);
            Some((2 + line.saturating_sub(start), 2 + column))
        }
        ForumPostComposerField::Attachments | ForumPostComposerField::Tags => None,
    }
}

fn visible_body_window(view: &ForumPostComposerView) -> (Vec<&str>, usize) {
    let lines = visible_body_lines(&view.body);
    if lines.len() <= BODY_VISIBLE_LINES {
        return (lines, 0);
    }
    let (cursor_line, _) = body_cursor_line_column(&view.body, view.body_cursor);
    let start = cursor_line
        .saturating_add(1)
        .saturating_sub(BODY_VISIBLE_LINES)
        .min(lines.len().saturating_sub(BODY_VISIBLE_LINES));
    let window = lines
        .into_iter()
        .skip(start)
        .take(BODY_VISIBLE_LINES)
        .collect();
    (window, start)
}

fn body_cursor_line_column(value: &str, cursor: usize) -> (usize, usize) {
    let prefix = cursor_prefix(value, cursor);
    let line = prefix.chars().filter(|value| *value == '\n').count();
    let column = prefix
        .rsplit('\n')
        .next()
        .map(cursor_column_for_str)
        .unwrap_or_default();
    (line, column)
}

fn cursor_column(value: &str, cursor: usize) -> usize {
    cursor_column_for_str(cursor_prefix(value, cursor))
}

fn cursor_column_for_str(value: &str) -> usize {
    value.width()
}

fn cursor_prefix(value: &str, cursor: usize) -> &str {
    let mut end = cursor.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::ForumPostComposerView;

    fn body_view(body: &str, body_cursor: usize) -> ForumPostComposerView {
        ForumPostComposerView {
            channel_label: "#support".to_owned(),
            active_field: ForumPostComposerField::Body,
            editing_field: Some(ForumPostComposerField::Body),
            title: String::new(),
            title_cursor: 0,
            body: body.to_owned(),
            body_cursor,
            attachments: Vec::new(),
            tags: Vec::new(),
            requires_tag: false,
            paste_pending: false,
            status: None,
        }
    }

    #[test]
    fn body_window_follows_cursor_line() {
        let body = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
        let view = body_view(body, body.len());

        let (lines, start) = visible_body_window(&view);

        assert_eq!(start, 2);
        assert_eq!(
            lines,
            vec!["three", "four", "five", "six", "seven", "eight"]
        );
        assert_eq!(forum_post_composer_cursor(&view), Some((7, 7)));
    }

    #[test]
    fn cursor_prefix_clamps_to_char_boundary() {
        let text = "가나";

        assert_eq!(cursor_prefix(text, 1), "");
        assert_eq!(cursor_prefix(text, 3), "가");
    }
}
