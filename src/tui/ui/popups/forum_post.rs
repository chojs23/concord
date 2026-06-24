use super::*;
use crate::tui::selection;
use crate::tui::state::{
    ForumPostComposerField, ForumPostComposerTagView, ForumPostComposerView, LocalUploadPreviewView,
};
use crate::tui::ui::{LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH};

const FORUM_POST_POPUP_WIDTH: u16 = 78;
const FORUM_POST_POPUP_HEIGHT: u16 = 24;
/// Tags always shown on the composer summary, even before any are selected.
const TAG_SUMMARY_MIN_VISIBLE: usize = 3;
/// Width of the floating tag picker popup.
const TAG_PICKER_WIDTH: u16 = 46;
/// Tag rows shown at once in the floating tag picker before it scrolls.
const TAG_PICKER_VISIBLE_ITEMS: usize = 10;

/// The composer content laid out as a flat list of rows, with the row index of
/// each focusable cell recorded so the renderer can scroll the focused cell into
/// view. Image preview tiles are painted on top of the reserved `preview_row`.
struct ComposerLayout {
    lines: Vec<Line<'static>>,
    title_row: usize,
    body_row: usize,
    body_content_row: usize,
    attachments_row: usize,
    tags_row: usize,
    submit_row: usize,
    cancel_row: usize,
    preview_row: Option<usize>,
    cursor: Option<(usize, usize)>,
}

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
    let previews = state.forum_post_attachment_previews();

    let popup = forum_post_composer_popup_area(area);
    frame.render_widget(Clear, popup);
    let block = panel_block("Create Forum Post", true);
    let inner = block.inner(popup);
    // Reserve the rightmost column for the scrollbar so long content never
    // collides with it.
    let content_width = usize::from(inner.width.saturating_sub(1)).max(1);

    let layout = build_composer_layout(&view, content_width, previews.len());
    let total = layout.lines.len();
    let viewport = inner.height as usize;
    // The scroll offset is owned by state (driven by the scroll keys and the
    // reveal-on-focus pass in `sync_view_heights`); clamp defensively here.
    let scroll = state
        .forum_post_composer_scroll()
        .min(total.saturating_sub(viewport));

    let visible: Vec<Line<'static>> = layout
        .lines
        .iter()
        .skip(scroll)
        .take(viewport)
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible).block(block), popup);
    render_vertical_scrollbar(frame, inner, scroll, viewport, total);

    // Paint preview tiles over the reserved blank rows, offset by the scroll.
    if let Some(preview_row) = layout.preview_row
        && !previews.is_empty()
        && preview_row >= scroll
    {
        let row_in_view = preview_row - scroll;
        if row_in_view < viewport {
            render_forum_post_attachment_previews(frame, inner, row_in_view as u16, previews);
        }
    }

    if let Some((row, column)) = layout.cursor
        && row >= scroll
        && row - scroll < viewport
    {
        let x = inner
            .x
            .saturating_add(column as u16)
            .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        let y = inner.y.saturating_add((row - scroll) as u16);
        frame.set_cursor_position(Position::new(x, y));
    }
}

pub(in crate::tui::ui) fn forum_post_composer_popup_area(area: Rect) -> Rect {
    centered_rect(
        area,
        FORUM_POST_POPUP_WIDTH
            .min(area.width.saturating_sub(2))
            .max(12),
        FORUM_POST_POPUP_HEIGHT
            .min(area.height.saturating_sub(2))
            .max(10),
    )
}

fn build_composer_layout(
    view: &ForumPostComposerView,
    width: usize,
    preview_count: usize,
) -> ComposerLayout {
    let editing_title = view.editing_field == Some(ForumPostComposerField::Title);
    let editing_body = view.editing_field == Some(ForumPostComposerField::Body);
    let mut lines = Vec::new();

    let title_row = lines.len();
    lines.push(field_line(
        "title",
        &view.title,
        view.active_field == ForumPostComposerField::Title,
        editing_title,
        width,
        "(empty)",
    ));

    let body_row = lines.len();
    lines.push(section_line(
        "body:",
        view.active_field == ForumPostComposerField::Body,
        editing_body,
    ));
    let body_content_row = lines.len();
    // The body grows to fit every line so long, multi-line posts stay fully
    // visible; the whole popup scrolls instead of clipping the body.
    let body_lines = visible_body_lines(&view.body);
    for line in &body_lines {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  {line}"), width),
            editing_value_style(editing_body),
        )));
    }
    if body_lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(DIM),
        )));
    }

    let attachments_row = lines.len();
    lines.push(section_line(
        "attachments:",
        view.active_field == ForumPostComposerField::Attachments,
        false,
    ));
    if view.attachments.is_empty() && !view.paste_pending {
        lines.push(Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(DIM),
        )));
    } else {
        for attachment in &view.attachments {
            lines.push(Line::from(Span::styled(
                truncate_display_width(
                    &format!(
                        "  upload: {} ({})",
                        attachment.filename,
                        format_byte_size(attachment.size_bytes)
                    ),
                    width,
                ),
                Style::default().fg(ACCENT),
            )));
        }
        if view.paste_pending {
            lines.push(Line::from(Span::styled(
                truncate_display_width("  upload: processing clipboard attachment...", width),
                Style::default().fg(DIM),
            )));
        }
    }
    // Blank rows reserved for the image preview tiles painted on top.
    let preview_row = (preview_count > 0).then(|| {
        let row = lines.len();
        for _ in 0..LOCAL_UPLOAD_PREVIEW_HEIGHT {
            lines.push(Line::from(""));
        }
        row
    });

    let tags_row = lines.len();
    let tag_label = if view.requires_tag {
        "tags: required"
    } else {
        "tags:"
    };
    lines.push(section_line(
        tag_label,
        view.active_field == ForumPostComposerField::Tags,
        false,
    ));
    push_tag_summary(&mut lines, &view.tags, width);

    lines.push(Line::from(""));
    let submit_row = lines.len();
    lines.push(button_line(
        "submit",
        view.active_field == ForumPostComposerField::Submit,
    ));
    let cancel_row = lines.len();
    lines.push(button_line(
        "cancel",
        view.active_field == ForumPostComposerField::Cancel,
    ));

    if let Some(status) = view.status.as_deref() {
        push_wrapped_styled_popup_text(&mut lines, status, width, Style::default().fg(Color::Red));
    }

    let cursor = if editing_title {
        Some((
            title_row,
            "› title: ".width() + cursor_column(&view.title, view.title_cursor),
        ))
    } else if editing_body {
        let (line, column) = body_cursor_line_column(&view.body, view.body_cursor);
        Some((body_content_row + line, 2 + column))
    } else {
        None
    };

    ComposerLayout {
        lines,
        title_row,
        body_row,
        body_content_row,
        attachments_row,
        tags_row,
        submit_row,
        cancel_row,
        preview_row,
        cursor,
    }
}

/// The [start, end) row range that must be brought into view for the currently
/// focused cell. Editing the body follows the cursor row.
fn focus_rows(view: &ForumPostComposerView, layout: &ComposerLayout) -> (usize, usize) {
    match view.active_field {
        ForumPostComposerField::Title => (layout.title_row, layout.body_row),
        ForumPostComposerField::Body => {
            if view.editing_field == Some(ForumPostComposerField::Body) {
                let row = layout
                    .cursor
                    .map(|(row, _)| row)
                    .unwrap_or(layout.body_content_row);
                (row, row + 1)
            } else {
                (layout.body_row, layout.attachments_row)
            }
        }
        ForumPostComposerField::Attachments => (layout.attachments_row, layout.tags_row),
        ForumPostComposerField::Tags => (layout.tags_row, layout.submit_row),
        // Anchor the buttons to the end of the content so the other button and
        // any error status below them stay on screen instead of being clipped.
        ForumPostComposerField::Submit => (layout.submit_row, layout.lines.len()),
        ForumPostComposerField::Cancel => (layout.cancel_row, layout.lines.len()),
    }
}

/// The row range the renderer should keep visible: the text cursor while
/// editing, otherwise the focused field.
fn reveal_target(view: &ForumPostComposerView, layout: &ComposerLayout) -> (usize, usize) {
    if let Some((row, _)) = layout.cursor {
        (row, row + 1)
    } else {
        focus_rows(view, layout)
    }
}

/// Total content height and the row range to reveal, for `sync_view_heights` to
/// drive the composer scroll state without rebuilding the layout itself.
pub(in crate::tui::ui) struct ForumPostComposerMetrics {
    pub total_lines: usize,
    pub reveal_start: usize,
    pub reveal_end: usize,
}

pub(in crate::tui::ui) fn forum_post_composer_metrics(
    view: &ForumPostComposerView,
    content_width: usize,
    preview_count: usize,
) -> ForumPostComposerMetrics {
    let layout = build_composer_layout(view, content_width, preview_count);
    let (reveal_start, reveal_end) = reveal_target(view, &layout);
    ForumPostComposerMetrics {
        total_lines: layout.lines.len(),
        reveal_start,
        reveal_end,
    }
}

fn button_line(label: &str, active: bool) -> Line<'static> {
    let style = if active {
        highlight_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        Span::raw(field_marker(active)),
        Span::styled(format!("[{label}]"), style),
    ])
}

fn tag_line(tag: &ForumPostComposerTagView, width: usize, thumbnail_ready: bool) -> Line<'static> {
    let marker = if tag.active { "▸" } else { " " };
    let checkbox = if tag.selected { "[x]" } else { "[ ]" };
    let emoji = super::thread_edit::tag_emoji_text(
        tag.unicode_emoji.as_deref(),
        tag.custom_emoji_url.as_deref(),
        tag.custom_emoji_label.as_deref(),
        thumbnail_ready,
    );
    // Unselectable tags (the cap is reached and this one is not yet selected)
    // are dimmed. The active row keeps its highlight so the cursor stays visible
    // even while sitting on a dimmed tag.
    let style = if tag.active {
        highlight_style()
    } else if !tag.selectable {
        Style::default().fg(DIM)
    } else {
        Style::default()
    };
    Line::from(Span::styled(
        truncate_display_width(&format!("{marker} {checkbox}{emoji} {}", tag.name), width),
        style,
    ))
}

/// The applied-tag summary on the composer. Every selected tag is shown (never
/// reduced), and at least [`TAG_SUMMARY_MIN_VISIBLE`] rows appear by default;
/// the rest fold into `...(+N more)`. Tags arrive selected-first.
fn push_tag_summary(
    lines: &mut Vec<Line<'static>>,
    tags: &[ForumPostComposerTagView],
    width: usize,
) {
    if tags.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no tags available",
            Style::default().fg(DIM),
        )));
        return;
    }
    let selected_count = tags.iter().filter(|tag| tag.selected).count();
    let shown = selected_count.max(TAG_SUMMARY_MIN_VISIBLE).min(tags.len());
    for tag in tags.iter().take(shown) {
        let checkbox = if tag.selected { "[x]" } else { "[ ]" };
        // The collapsed summary is part of the static composer form (no image
        // overlay), so custom emoji fall back to their `:name:` label here.
        let emoji = super::thread_edit::tag_emoji_text(
            tag.unicode_emoji.as_deref(),
            tag.custom_emoji_url.as_deref(),
            tag.custom_emoji_label.as_deref(),
            false,
        );
        let style = if tag.selected {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        };
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  {checkbox}{emoji} {}", tag.name), width),
            style,
        )));
    }
    let remaining = tags.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  ...(+{remaining} more)"), width),
            Style::default().fg(DIM),
        )));
    }
}

/// Floating tag picker drawn on top of the composer, in the style of the emoji
/// reaction picker. Tags are listed with checkboxes, scrolled to keep the
/// cursor (the active tag) in view, selected tags sorted to the top.
pub(in crate::tui::ui) fn render_forum_post_tag_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_forum_post_tag_picker_active() {
        return;
    }
    let Some(view) = state.forum_post_composer_view() else {
        return;
    };
    if view.tags.is_empty() {
        return;
    }
    let tags = &view.tags;
    let selected = tags.iter().position(|tag| tag.active).unwrap_or(0);
    let popup = forum_post_tag_picker_popup_area(area, tags.len());
    let block = panel_block("Choose tags", true);
    let content = block.inner(popup);
    let visible_items = usize::from(content.height)
        .min(TAG_PICKER_VISIBLE_ITEMS)
        .min(tags.len())
        .max(1);
    let visible_range = selection::visible_item_range(tags.len(), selected, visible_items);
    let ready_urls = super::thread_edit::ready_emoji_urls(emoji_images);
    frame.render_widget(Clear, popup);
    let rows: Vec<Line<'static>> = tags[visible_range.clone()]
        .iter()
        .map(|tag| {
            tag_line(
                tag,
                usize::from(content.width),
                super::thread_edit::tag_custom_emoji_ready(
                    tag.custom_emoji_url.as_deref(),
                    &ready_urls,
                ),
            )
        })
        .collect();
    frame.render_widget(
        Paragraph::new(rows).block(block).wrap(Wrap { trim: false }),
        popup,
    );
    if state.show_custom_emoji() {
        super::thread_edit::render_tag_picker_emojis(
            frame,
            content,
            tags[visible_range.clone()]
                .iter()
                .map(|tag| tag.custom_emoji_url.as_deref()),
            emoji_images,
        );
    }
    render_vertical_scrollbar(
        frame,
        Rect {
            height: visible_items as u16,
            ..content
        },
        visible_range.start,
        visible_items,
        tags.len(),
    );
}

fn forum_post_tag_picker_popup_area(area: Rect, tag_count: usize) -> Rect {
    let visible = tag_count.clamp(1, TAG_PICKER_VISIBLE_ITEMS) as u16;
    centered_rect(area, TAG_PICKER_WIDTH, visible.saturating_add(2))
}

fn render_forum_post_attachment_previews(
    frame: &mut Frame,
    inner: Rect,
    row_in_view: u16,
    previews: Vec<LocalUploadPreviewView<'_>>,
) {
    let y = inner.y.saturating_add(row_in_view);
    if y >= inner.y.saturating_add(inner.height) {
        return;
    }
    let height = LOCAL_UPLOAD_PREVIEW_HEIGHT.min(inner.y.saturating_add(inner.height) - y);
    if height == 0 {
        return;
    }
    let tile_width = LOCAL_UPLOAD_PREVIEW_WIDTH.min(inner.width);
    if tile_width == 0 {
        return;
    }
    for (index, preview) in previews.into_iter().enumerate() {
        let x_offset = u16::try_from(index)
            .unwrap_or(u16::MAX)
            .saturating_mul(tile_width.saturating_add(1));
        let x = inner.x.saturating_add(x_offset);
        if x >= inner.x.saturating_add(inner.width) {
            break;
        }
        let preview_area = Rect {
            x,
            y,
            width: tile_width.min(inner.x.saturating_add(inner.width) - x),
            height,
        };
        render_forum_post_attachment_preview(frame, preview_area, preview);
    }
}

fn render_forum_post_attachment_preview(
    frame: &mut Frame,
    area: Rect,
    preview: LocalUploadPreviewView<'_>,
) {
    match preview {
        LocalUploadPreviewView::Loading { filename } => frame.render_widget(
            Paragraph::new(format!("loading {filename}..."))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Ready { protocol } => {
            frame.render_widget(RatatuiImage::new(protocol), area);
        }
    }
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
    fn body_layout_keeps_every_line_and_tracks_cursor() {
        let body = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
        let view = body_view(body, body.len());

        let layout = build_composer_layout(&view, 40, 0);

        // Body header at row 1, then all eight lines (no fixed window), so the
        // attachments header sits nine rows below the body header.
        assert_eq!(layout.attachments_row - layout.body_row, 9);
        // Cursor follows the last body line.
        assert_eq!(layout.cursor, Some((layout.body_content_row + 7, 7)));
    }

    #[test]
    fn cursor_prefix_clamps_to_char_boundary() {
        let text = "가나";

        assert_eq!(cursor_prefix(text, 1), "");
        assert_eq!(cursor_prefix(text, 3), "가");
    }

    fn tag(name: &str, selected: bool) -> ForumPostComposerTagView {
        ForumPostComposerTagView {
            name: name.to_owned(),
            unicode_emoji: None,
            custom_emoji_url: None,
            custom_emoji_label: None,
            selected,
            active: false,
            selectable: true,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn tag_summary_shows_at_least_three_then_folds_the_rest() {
        let tags: Vec<_> = (0..20)
            .map(|index| tag(&format!("t{index}"), false))
            .collect();
        let mut lines = Vec::new();

        push_tag_summary(&mut lines, &tags, 40);

        // Three tag rows are shown by default even with nothing selected.
        assert_eq!(lines.len(), 4);
        assert!(line_text(&lines[3]).contains("...(+17 more)"));
    }

    #[test]
    fn tag_summary_never_reduces_selected_tags() {
        // Five selected (listed first, the view's contract) and one spare.
        let tags: Vec<_> = (0..6)
            .map(|index| tag(&format!("t{index}"), index < 5))
            .collect();
        let mut lines = Vec::new();

        push_tag_summary(&mut lines, &tags, 40);

        // All five selected rows plus a single "...(+1 more)".
        assert_eq!(lines.len(), 6);
        assert!(line_text(&lines[5]).contains("...(+1 more)"));
    }

    #[test]
    fn metrics_reveal_target_follows_the_body_cursor() {
        let body = "one\ntwo\nthree";
        let view = body_view(body, body.len());

        let metrics = forum_post_composer_metrics(&view, 40, 0);

        // Body content begins at row 2 (title, body header), cursor on line 3.
        assert_eq!((metrics.reveal_start, metrics.reveal_end), (4, 5));
        // Title, body header, three body lines, plus the rest of the form.
        assert!(metrics.total_lines > 5);
    }
}
