use super::*;
use crate::tui::selection;
use crate::tui::state::{ForumPostEditField, ForumPostEditTagView, ForumPostEditView};

const FORUM_POST_EDIT_POPUP_WIDTH: u16 = 78;
const FORUM_POST_EDIT_POPUP_HEIGHT: u16 = 18;
/// Tags always shown on the summary, even before any are selected.
const TAG_SUMMARY_MIN_VISIBLE: usize = 3;
/// Width of the floating tag picker popup.
const TAG_PICKER_WIDTH: u16 = 46;
/// Tag rows shown at once in the floating tag picker before it scrolls.
const TAG_PICKER_VISIBLE_ITEMS: usize = 10;

/// The settings form laid out as a flat list of rows, with the row index of
/// each focusable cell recorded so the renderer can scroll the focused cell
/// into view.
struct EditLayout {
    lines: Vec<Line<'static>>,
    title_row: usize,
    tags_row: usize,
    slow_mode_row: usize,
    auto_archive_row: usize,
    submit_row: usize,
    cancel_row: usize,
    cursor: Option<(usize, usize)>,
}

pub(in crate::tui::ui) fn render_forum_post_edit(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ForumPostEdit) {
        return;
    }
    let Some(view) = state.forum_post_edit_view() else {
        return;
    };

    let popup = forum_post_edit_popup_area(area);
    frame.render_widget(Clear, popup);
    let block = panel_block("Edit Forum Post", true);
    let inner = block.inner(popup);
    // Reserve the rightmost column for the scrollbar so long content never
    // collides with it.
    let content_width = usize::from(inner.width.saturating_sub(1)).max(1);

    let layout = build_edit_layout(&view, content_width);
    let total = layout.lines.len();
    let viewport = inner.height as usize;
    let scroll = state
        .forum_post_edit_scroll()
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

pub(in crate::tui::ui) fn forum_post_edit_popup_area(area: Rect) -> Rect {
    centered_rect(
        area,
        FORUM_POST_EDIT_POPUP_WIDTH
            .min(area.width.saturating_sub(2))
            .max(12),
        FORUM_POST_EDIT_POPUP_HEIGHT
            .min(area.height.saturating_sub(2))
            .max(10),
    )
}

fn build_edit_layout(view: &ForumPostEditView, width: usize) -> EditLayout {
    let mut lines = Vec::new();

    let title_row = lines.len();
    lines.push(field_line(
        "title",
        &view.title,
        view.active_field == ForumPostEditField::Title,
        view.editing_title,
        width,
        "(empty)",
    ));

    lines.push(Line::from(""));
    let tags_row = lines.len();
    let tag_label = if view.requires_tag {
        "tags: required"
    } else {
        "tags:"
    };
    lines.push(section_line(
        tag_label,
        view.active_field == ForumPostEditField::Tags,
    ));
    push_tag_summary(&mut lines, &view.tags, width);

    lines.push(Line::from(""));
    let slow_mode_row = lines.len();
    lines.push(selector_line(
        "slow mode",
        &view.slow_mode_label,
        view.active_field == ForumPostEditField::SlowMode,
        view.can_set_slow_mode,
        width,
    ));

    let auto_archive_row = lines.len();
    lines.push(selector_line(
        "auto-archive",
        &view.auto_archive_label,
        view.active_field == ForumPostEditField::AutoArchive,
        true,
        width,
    ));

    lines.push(Line::from(""));
    let submit_row = lines.len();
    lines.push(button_line(
        "submit",
        view.active_field == ForumPostEditField::Submit,
    ));
    let cancel_row = lines.len();
    lines.push(button_line(
        "cancel",
        view.active_field == ForumPostEditField::Cancel,
    ));

    if let Some(status) = view.status.as_deref() {
        push_wrapped_styled_popup_text(&mut lines, status, width, Style::default().fg(Color::Red));
    }

    let cursor = view.editing_title.then(|| {
        (
            title_row,
            "› title: ".width() + cursor_column(&view.title, view.title_cursor),
        )
    });

    EditLayout {
        lines,
        title_row,
        tags_row,
        slow_mode_row,
        auto_archive_row,
        submit_row,
        cancel_row,
        cursor,
    }
}

/// The [start, end) row range that must be brought into view for the currently
/// focused cell.
fn focus_rows(view: &ForumPostEditView, layout: &EditLayout) -> (usize, usize) {
    match view.active_field {
        ForumPostEditField::Title => (layout.title_row, layout.title_row + 1),
        ForumPostEditField::Tags => (layout.tags_row, layout.slow_mode_row),
        ForumPostEditField::SlowMode => (layout.slow_mode_row, layout.auto_archive_row),
        ForumPostEditField::AutoArchive => (layout.auto_archive_row, layout.submit_row),
        // Anchor the buttons to the end of the content so the other button and
        // any error status below them stay on screen instead of being clipped.
        ForumPostEditField::Submit => (layout.submit_row, layout.lines.len()),
        ForumPostEditField::Cancel => (layout.cancel_row, layout.lines.len()),
    }
}

fn reveal_target(view: &ForumPostEditView, layout: &EditLayout) -> (usize, usize) {
    if let Some((row, _)) = layout.cursor {
        (row, row + 1)
    } else {
        focus_rows(view, layout)
    }
}

/// Total content height and the row range to reveal, for `sync_view_heights` to
/// drive the popup scroll state without rebuilding the layout itself.
pub(in crate::tui::ui) struct ForumPostEditMetrics {
    pub total_lines: usize,
    pub reveal_start: usize,
    pub reveal_end: usize,
}

pub(in crate::tui::ui) fn forum_post_edit_metrics(
    view: &ForumPostEditView,
    content_width: usize,
) -> ForumPostEditMetrics {
    let layout = build_edit_layout(view, content_width);
    let (reveal_start, reveal_end) = reveal_target(view, &layout);
    ForumPostEditMetrics {
        total_lines: layout.lines.len(),
        reveal_start,
        reveal_end,
    }
}

/// Floating tag picker drawn on top of the editor, reusing the composer's
/// visual style. Tags are listed with checkboxes, scrolled to keep the active
/// tag in view.
pub(in crate::tui::ui) fn render_forum_post_edit_tag_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_forum_post_edit_tag_picker_active() {
        return;
    }
    let Some(view) = state.forum_post_edit_view() else {
        return;
    };
    if view.tags.is_empty() {
        return;
    }
    let tags = &view.tags;
    let selected = tags.iter().position(|tag| tag.active).unwrap_or(0);
    let popup = forum_post_edit_tag_picker_popup_area(area, tags.len());
    let block = panel_block("Choose tags", true);
    let content = block.inner(popup);
    let visible_items = usize::from(content.height)
        .min(TAG_PICKER_VISIBLE_ITEMS)
        .min(tags.len())
        .max(1);
    let visible_range = selection::visible_item_range(tags.len(), selected, visible_items);
    frame.render_widget(Clear, popup);
    let rows: Vec<Line<'static>> = tags[visible_range.clone()]
        .iter()
        .map(|tag| tag_line(tag, usize::from(content.width)))
        .collect();
    frame.render_widget(
        Paragraph::new(rows).block(block).wrap(Wrap { trim: false }),
        popup,
    );
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

fn forum_post_edit_tag_picker_popup_area(area: Rect, tag_count: usize) -> Rect {
    let visible = tag_count.clamp(1, TAG_PICKER_VISIBLE_ITEMS) as u16;
    centered_rect(area, TAG_PICKER_WIDTH, visible.saturating_add(2))
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

fn tag_line(tag: &ForumPostEditTagView, width: usize) -> Line<'static> {
    let marker = if tag.active { "▸" } else { " " };
    let checkbox = if tag.selected { "[x]" } else { "[ ]" };
    let emoji = tag
        .emoji
        .as_deref()
        .map(|emoji| format!(" {emoji}"))
        .unwrap_or_default();
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

fn push_tag_summary(lines: &mut Vec<Line<'static>>, tags: &[ForumPostEditTagView], width: usize) {
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
        let emoji = tag
            .emoji
            .as_deref()
            .map(|emoji| format!(" {emoji}"))
            .unwrap_or_default();
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

/// A selector cell: `label: ‹ value ›`. Dimmed when not changeable (slow mode
/// without the manage permission), so it reads as read-only.
fn selector_line(
    label: &str,
    value: &str,
    active: bool,
    changeable: bool,
    width: usize,
) -> Line<'static> {
    let marker = field_marker(active);
    let prefix = format!("{marker}{label}: ");
    let value_style = if !changeable {
        Style::default().fg(DIM)
    } else if active {
        highlight_style()
    } else {
        Style::default().fg(ACCENT)
    };
    let arrows = if active && changeable {
        format!("‹ {value} ›")
    } else {
        value.to_owned()
    };
    Line::from(vec![
        Span::styled(prefix, field_label_style(active, false)),
        Span::styled(truncate_display_width(&arrows, width.max(1)), value_style),
    ])
}

fn section_line(label: &str, active: bool) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}{}", field_marker(active), label),
        field_label_style(active, false),
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

fn cursor_column(value: &str, cursor: usize) -> usize {
    let mut end = cursor.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].width()
}
