use super::*;
use crate::tui::selection;
use crate::tui::state::{
    ForumPostComposerField, ForumPostComposerTagView, ForumPostComposerView, LocalUploadPreviewView,
};
use crate::tui::ui::{LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH};

const FORUM_POST_POPUP_WIDTH: u16 = 78;
const FORUM_POST_POPUP_HEIGHT: u16 = 24;
/// The body grows with short drafts, then keeps a six-row text viewport. The
/// remaining form fields stay close enough to reach without crossing the full
/// draft document.
const BODY_EDITOR_MAX_VISIBLE_LINES: usize = 6;
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
    body_viewport_len: usize,
    body_total_lines: usize,
    body_cursor_row: usize,
    body_boxed: bool,
    attachments_row: usize,
    tags_row: usize,
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
    let areas = render_popup_form_frame(frame, popup, "Create post", &view.channel_label);
    // Reserve the rightmost column for the scrollbar so long content never
    // collides with it.
    let content_width = usize::from(areas.content.width.saturating_sub(1)).max(1);

    let layout = build_composer_layout(&view, content_width, previews.len());
    let total = layout.lines.len();
    let viewport = usize::from(areas.content.height);
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
    frame.render_widget(Paragraph::new(visible), areas.content);
    render_vertical_scrollbar(frame, areas.content, scroll, viewport, total);
    render_body_scrollbar(
        frame,
        areas.content,
        scroll,
        viewport,
        &layout,
        view.body_scroll,
    );

    // Paint preview tiles over the reserved blank rows, offset by the scroll.
    if let Some(preview_row) = layout.preview_row
        && !previews.is_empty()
        && preview_row >= scroll
    {
        let row_in_view = preview_row - scroll;
        if row_in_view < viewport {
            render_forum_post_attachment_previews(
                frame,
                Rect {
                    width: areas.content.width.saturating_sub(1),
                    ..areas.content
                },
                row_in_view as u16,
                previews,
            );
        }
    }

    if let Some((row, column)) = layout.cursor
        && row >= scroll
        && row - scroll < viewport
    {
        let x = areas.content.x.saturating_add(column as u16).min(
            areas
                .content
                .x
                .saturating_add(areas.content.width.saturating_sub(2)),
        );
        let y = areas.content.y.saturating_add((row - scroll) as u16);
        frame.set_cursor_position(Position::new(x, y));
    }

    render_popup_form_footer(
        frame,
        areas.footer,
        PopupFormActions {
            cancel_active: view.active_field == ForumPostComposerField::Cancel,
            primary_shortcut: "s",
            primary_label: "Create",
            primary_active: view.active_field == ForumPostComposerField::Submit,
        },
    );
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

fn render_body_scrollbar(
    frame: &mut Frame,
    content: Rect,
    form_scroll: usize,
    form_viewport: usize,
    layout: &ComposerLayout,
    body_scroll: usize,
) {
    if !layout.body_boxed || layout.body_total_lines <= layout.body_viewport_len {
        return;
    }

    let body_start = layout.body_content_row;
    let body_end = body_start.saturating_add(layout.body_viewport_len);
    let form_end = form_scroll.saturating_add(form_viewport);
    // Avoid painting through the fixed footer on very short terminals. The
    // body scrollbar appears as soon as its complete text viewport is visible.
    if body_start < form_scroll || body_end > form_end || content.width <= 3 {
        return;
    }

    render_vertical_scrollbar(
        frame,
        Rect {
            x: content.x.saturating_add(2),
            y: content
                .y
                .saturating_add((body_start.saturating_sub(form_scroll)) as u16),
            width: content.width.saturating_sub(3),
            height: layout.body_viewport_len as u16,
        },
        body_scroll,
        layout.body_viewport_len,
        layout.body_total_lines,
    );
}

fn build_composer_layout(
    view: &ForumPostComposerView,
    width: usize,
    preview_count: usize,
) -> ComposerLayout {
    let editing_title = view.editing_field == Some(ForumPostComposerField::Title);
    let editing_body = view.editing_field == Some(ForumPostComposerField::Body);
    let status_field = view.status_field;
    let mut lines = Vec::new();

    lines.push(popup_form_section_heading("CONTENT"));
    let title_row = lines.len();
    lines.push(popup_form_text_field_line(
        "Title",
        true,
        &view.title,
        view.active_field == ForumPostComposerField::Title,
        editing_title,
        width,
    ));
    if status_field == Some(ForumPostComposerField::Title)
        && let Some(status) = view.status.as_deref()
    {
        push_popup_form_inline_status(&mut lines, status, width);
    }

    let body_row = lines.len();
    let body_active = view.active_field == ForumPostComposerField::Body;
    lines.push(body_header_line(view, body_active, editing_body, width));
    let body_editor = body_editor_layout(
        &view.body,
        view.body_cursor,
        body_active,
        editing_body,
        width,
        view.body_scroll,
    );
    let body_content_row = lines.len().saturating_add(body_editor.content_row_offset);
    let body_viewport_len = body_editor.viewport_len;
    let body_total_lines = body_editor.total_lines;
    let body_cursor_row = body_editor.cursor_row;
    let body_boxed = body_editor.boxed;
    let body_cursor = body_editor.cursor;
    lines.extend(body_editor.lines);
    if status_field == Some(ForumPostComposerField::Body)
        && let Some(status) = view.status.as_deref()
    {
        push_popup_form_inline_status(&mut lines, status, width);
    } else if view.body_character_count > view.body_character_limit {
        let excess = view
            .body_character_count
            .saturating_sub(view.body_character_limit);
        push_popup_form_inline_status(
            &mut lines,
            &format!(
                "Remove {excess} character{} before creating this post.",
                if excess == 1 { "" } else { "s" }
            ),
            width,
        );
    }

    lines.push(Line::from(""));
    lines.push(popup_form_section_heading("DETAILS"));
    let attachments_row = lines.len();
    lines.push(popup_form_summary_line(
        "Attachments",
        false,
        &attachment_summary(view),
        None,
        view.active_field == ForumPostComposerField::Attachments,
        true,
        width,
    ));
    if status_field == Some(ForumPostComposerField::Attachments)
        && let Some(status) = view.status.as_deref()
    {
        push_popup_form_inline_status(&mut lines, status, width);
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
    lines.push(popup_form_summary_line(
        "Tags",
        view.requires_tag,
        &tag_summary(&view.tags, width),
        (!view.tags.is_empty()).then_some("Enter ›"),
        view.active_field == ForumPostComposerField::Tags,
        !view.tags.is_empty(),
        width,
    ));
    if status_field == Some(ForumPostComposerField::Tags)
        && let Some(status) = view.status.as_deref()
    {
        push_popup_form_inline_status(&mut lines, status, width);
    }
    if status_field.is_none()
        && let Some(status) = view.status.as_deref()
    {
        lines.push(Line::from(""));
        push_popup_form_inline_status(&mut lines, status, width);
    }

    let cursor = if editing_title {
        Some((
            title_row,
            popup_form_text_value_column("Title", true, true)
                + cursor_column(&view.title, view.title_cursor),
        ))
    } else if editing_body {
        body_cursor.map(|(line, column)| {
            (
                body_content_row + line,
                usize::from(body_boxed) * 4 + column,
            )
        })
    } else {
        None
    };

    ComposerLayout {
        lines,
        title_row,
        body_row,
        body_content_row,
        body_viewport_len,
        body_total_lines,
        body_cursor_row,
        body_boxed,
        attachments_row,
        tags_row,
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
        ForumPostComposerField::Tags => (layout.tags_row, layout.lines.len()),
        // Actions live in the fixed footer, so keep the final details row in
        // view without trying to scroll to a row that no longer exists.
        ForumPostComposerField::Submit | ForumPostComposerField::Cancel => {
            (layout.lines.len().saturating_sub(1), layout.lines.len())
        }
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
    pub body_viewport_lines: usize,
    pub body_total_lines: usize,
    pub body_cursor_row: usize,
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
        body_viewport_lines: layout.body_viewport_len,
        body_total_lines: layout.body_total_lines,
        body_cursor_row: layout.body_cursor_row,
    }
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
        theme::current().style(theme::HighlightGroup::Disabled)
    } else {
        Style::default()
    };
    Line::from(Span::styled(
        truncate_display_width(&format!("{marker} {checkbox}{emoji} {}", tag.name), width),
        style,
    ))
}

fn tag_summary(tags: &[ForumPostComposerTagView], width: usize) -> String {
    if tags.is_empty() {
        return "None".to_owned();
    }
    let selected: Vec<String> = tags
        .iter()
        .filter(|tag| tag.selected)
        .map(|tag| {
            let emoji = super::thread_edit::tag_emoji_text(
                tag.unicode_emoji.as_deref(),
                tag.custom_emoji_url.as_deref(),
                tag.custom_emoji_label.as_deref(),
                false,
            );
            let emoji = emoji.trim();
            if emoji.is_empty() {
                format!("[{}]", tag.name)
            } else {
                format!("[{emoji} {}]", tag.name)
            }
        })
        .collect();
    if selected.is_empty() {
        return "None selected".to_owned();
    }

    let available = width.saturating_sub(20).max(1);
    let all = selected.join(" ");
    if all.width() <= available || selected.len() == 1 {
        return all;
    }
    format!("{} +{}", selected[0], selected.len().saturating_sub(1))
}

fn attachment_summary(view: &ForumPostComposerView) -> String {
    if view.attachments.is_empty() {
        return if view.paste_pending {
            "Processing...".to_owned()
        } else {
            "None".to_owned()
        };
    }

    let total_size = view.attachments.iter().fold(0u64, |total, attachment| {
        total.saturating_add(attachment.size_bytes)
    });
    let mut summary = if view.attachments.len() == 1 {
        format!(
            "{} · {}",
            view.attachments[0].filename,
            format_byte_size(total_size)
        )
    } else {
        format!(
            "{} files · {}",
            view.attachments.len(),
            format_byte_size(total_size)
        )
    };
    if view.paste_pending {
        summary.push_str(" · processing");
    }
    summary
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
    let popup = forum_post_tag_picker_popup_area(area, tags.len());
    let content = render_modal_frame(frame, popup, "Choose tags");
    let visible_items = usize::from(content.height)
        .min(TAG_PICKER_VISIBLE_ITEMS)
        .min(tags.len())
        .max(1);
    let visible_range = selection::visible_window(view.tag_scroll, visible_items, tags.len());
    let ready_urls = super::thread_edit::ready_emoji_urls(emoji_images);
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
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), content);
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

pub(in crate::tui::ui) fn forum_post_tag_picker_list_layout(
    area: Rect,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    SelectablePopupLayout::fixed(
        snapshot.target,
        forum_post_tag_picker_popup_area(area, snapshot.item_count),
        snapshot,
    )
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
                .style(theme::current().style(theme::HighlightGroup::Loading))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(theme::current().style(theme::HighlightGroup::Warning))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Ready { protocol } => {
            frame.render_widget(RatatuiImage::new(protocol), area);
        }
    }
}

fn body_header_line(
    view: &ForumPostComposerView,
    active: bool,
    editing: bool,
    width: usize,
) -> Line<'static> {
    let label = format!("{}Body *", editable_field_marker(active));
    let count = format!(
        "{} / {}",
        view.body_character_count, view.body_character_limit
    );
    let padding = width
        .saturating_sub(label.width())
        .saturating_sub(count.width());
    let count_style = if view.body_character_count > view.body_character_limit {
        theme::current().style(theme::HighlightGroup::Error)
    } else {
        theme::current().style(theme::HighlightGroup::MessageSecondary)
    };
    truncate_line_to_display_width(
        Line::from(vec![
            Span::styled(label, popup_form_field_label_style(active, editing)),
            Span::raw(" ".repeat(padding)),
            Span::styled(count, count_style),
        ]),
        width,
    )
}

fn body_editor_text_width(width: usize) -> usize {
    width.saturating_sub(6).max(1)
}

struct BodyEditorLayout {
    lines: Vec<Line<'static>>,
    content_row_offset: usize,
    viewport_len: usize,
    total_lines: usize,
    cursor_row: usize,
    boxed: bool,
    cursor: Option<(usize, usize)>,
}

fn body_editor_layout(
    body: &str,
    cursor: usize,
    active: bool,
    editing: bool,
    width: usize,
    scroll: usize,
) -> BodyEditorLayout {
    let boxed = width > 6;
    let text_width = if boxed {
        body_editor_text_width(width)
    } else {
        width.max(1)
    };
    let (body_lines, cursor_row, cursor_column) = wrapped_body_rows(body, cursor, text_width);
    let total_lines = body_lines.len();
    let viewport_len = total_lines.clamp(1, BODY_EDITOR_MAX_VISIBLE_LINES);
    let scroll = scroll.min(total_lines.saturating_sub(viewport_len));
    let visible_lines = body_lines
        .into_iter()
        .skip(scroll)
        .take(viewport_len)
        .collect::<Vec<_>>();
    let cursor = editing
        .then_some((cursor_row, cursor_column))
        .filter(|(row, _)| *row >= scroll && *row < scroll.saturating_add(viewport_len))
        .map(|(row, column)| (row - scroll, column));
    let value_style = popup_form_field_value_style(active, editing);

    if !boxed {
        let lines = visible_lines
            .into_iter()
            .map(|line| Line::from(Span::styled(line, value_style)))
            .collect();
        return BodyEditorLayout {
            lines,
            content_row_offset: 0,
            viewport_len,
            total_lines,
            cursor_row,
            boxed,
            cursor,
        };
    }

    let border_style = popup_form_field_label_style(active, editing);
    let horizontal = "─".repeat(width.saturating_sub(4));
    let mut lines = vec![Line::from(Span::styled(
        format!("  ┌{horizontal}┐"),
        border_style,
    ))];
    for line in visible_lines {
        let content = truncate_display_width(&line, text_width);
        let padding = text_width.saturating_sub(content.width());
        lines.push(Line::from(vec![
            Span::styled("  │ ", border_style),
            Span::styled(content, value_style),
            Span::raw(" ".repeat(padding)),
            Span::styled(" │", border_style),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("  └{horizontal}┘"),
        border_style,
    )));
    BodyEditorLayout {
        lines,
        content_row_offset: 1,
        viewport_len,
        total_lines,
        cursor_row,
        boxed,
        cursor,
    }
}

fn wrapped_body_rows(body: &str, cursor: usize, text_width: usize) -> (Vec<String>, usize, usize) {
    let mut lines = if body.is_empty() {
        vec![String::new()]
    } else {
        wrap_text_lines(body, text_width)
    };
    let (cursor_row, cursor_column) = wrapped_body_cursor(body, cursor, text_width);
    while lines.len() <= cursor_row {
        lines.push(String::new());
    }
    (lines, cursor_row, cursor_column)
}

fn wrapped_body_cursor(value: &str, cursor: usize, width: usize) -> (usize, usize) {
    let wrapped = wrap_text_lines(cursor_prefix(value, cursor), width.max(1));
    let mut row = wrapped.len().saturating_sub(1);
    let mut column = wrapped.last().map(|line| line.width()).unwrap_or_default();
    if column >= width.max(1) {
        row = row.saturating_add(1);
        column = 0;
    }
    (row, column)
}

fn cursor_column(value: &str, cursor: usize) -> usize {
    cursor_prefix(value, cursor).width()
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
            body_scroll: 0,
            body_character_count: body.chars().count(),
            body_character_limit: 2_000,
            attachments: Vec::new(),
            tags: Vec::new(),
            tag_scroll: 0,
            requires_tag: false,
            paste_pending: false,
            status: None,
            status_field: None,
        }
    }

    #[test]
    fn body_layout_caps_its_viewport_and_tracks_the_scrolled_cursor() {
        let body = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight";
        let mut view = body_view(body, body.len());
        view.body_scroll = 2;

        let layout = build_composer_layout(&view, 40, 0);

        assert_eq!(layout.body_total_lines, 8);
        assert_eq!(layout.body_viewport_len, 6);
        assert_eq!(layout.attachments_row - layout.body_row, 11);
        assert_eq!(layout.cursor, Some((layout.body_content_row + 5, 9)));
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

    #[test]
    fn details_use_compact_summaries() {
        let no_selection: Vec<_> = (0..20)
            .map(|index| tag(&format!("t{index}"), false))
            .collect();
        let tags: Vec<_> = (0..6)
            .map(|index| tag(&format!("t{index}"), index < 5))
            .collect();

        assert_eq!(tag_summary(&no_selection, 40), "None selected");
        assert_eq!(tag_summary(&tags, 40), "[t0] +4");
    }

    #[test]
    fn metrics_reveal_target_follows_the_body_cursor() {
        let body = "one\ntwo\nthree";
        let view = body_view(body, body.len());

        let metrics = forum_post_composer_metrics(&view, 40, 0);

        assert_eq!((metrics.reveal_start, metrics.reveal_end), (6, 7));
        assert_eq!(metrics.body_viewport_lines, 3);
        assert_eq!(metrics.body_total_lines, 3);
        assert_eq!(metrics.body_cursor_row, 2);
        assert!(metrics.total_lines > 7);
    }
}
