use super::*;
use crate::tui::selection;
use crate::tui::state::{ThreadEditField, ThreadEditTagView, ThreadEditView};
use crate::tui::ui::emoji_overlay::overlay_emoji_column;

const FORUM_POST_EDIT_POPUP_WIDTH: u16 = 78;
const FORUM_POST_EDIT_POPUP_HEIGHT: u16 = 18;
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
    cursor: Option<(usize, usize)>,
}

pub(in crate::tui::ui) fn render_thread_edit(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ThreadEdit) {
        return;
    }
    let Some(view) = state.thread_edit_view() else {
        return;
    };

    let popup = thread_edit_popup_area(area);
    let title = if view.is_forum_post {
        "Edit post settings"
    } else {
        "Edit thread settings"
    };
    let areas = render_popup_form_frame(frame, popup, title, &view.channel_label);
    // Reserve the rightmost column for the scrollbar so long content never
    // collides with it.
    let content_width = usize::from(areas.content.width.saturating_sub(1)).max(1);

    let layout = build_edit_layout(&view, content_width);
    let total = layout.lines.len();
    let viewport = usize::from(areas.content.height);
    let scroll = state
        .thread_edit_scroll()
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
            cancel_active: view.active_field == ThreadEditField::Cancel,
            primary_shortcut: "s",
            primary_label: "Save",
            primary_active: view.active_field == ThreadEditField::Submit,
        },
    );
}

pub(in crate::tui::ui) fn thread_edit_popup_area(area: Rect) -> Rect {
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

fn build_edit_layout(view: &ThreadEditView, width: usize) -> EditLayout {
    let status_field = view.status_field;
    let mut lines = Vec::new();

    lines.push(popup_form_section_heading("POST"));
    let title_row = lines.len();
    lines.push(popup_form_text_field_line(
        "Title",
        true,
        &view.title,
        view.active_field == ThreadEditField::Title,
        view.editing_title,
        width,
    ));
    if status_field == Some(ThreadEditField::Title)
        && let Some(status) = view.status.as_deref()
    {
        push_popup_form_inline_status(&mut lines, status, width);
    }

    // Tags only exist on forum posts. For a regular thread the whole Tags
    // section is omitted, and `tags_row` collapses onto the slow-mode row so the
    // (then-unreachable) Tags focus range stays valid.
    let tags_row = if view.is_forum_post {
        let tags_row = lines.len();
        lines.push(popup_form_summary_line(
            "Tags",
            view.requires_tag,
            &tag_summary(&view.tags, width),
            (!view.tags.is_empty()).then_some("Enter ›"),
            view.active_field == ThreadEditField::Tags,
            !view.tags.is_empty(),
            width,
        ));
        if status_field == Some(ThreadEditField::Tags)
            && let Some(status) = view.status.as_deref()
        {
            push_popup_form_inline_status(&mut lines, status, width);
        }
        tags_row
    } else {
        lines.len()
    };

    lines.push(Line::from(""));
    lines.push(popup_form_section_heading("BEHAVIOR"));
    let slow_mode_row = lines.len();
    lines.push(popup_form_summary_line(
        "Slow mode",
        false,
        &view.slow_mode_label,
        Some(if view.can_set_slow_mode {
            "← →"
        } else {
            "Read only"
        }),
        view.active_field == ThreadEditField::SlowMode,
        view.can_set_slow_mode,
        width,
    ));

    let auto_archive_row = lines.len();
    lines.push(popup_form_summary_line(
        "Auto-archive",
        false,
        &view.auto_archive_label,
        Some("← →"),
        view.active_field == ThreadEditField::AutoArchive,
        true,
        width,
    ));

    if view.is_forum_post {
        lines.push(Line::from(Span::styled(
            truncate_display_width("  The first message is edited from the post itself.", width),
            theme::current().style(theme::HighlightGroup::MessageSecondary),
        )));
    }
    if status_field.is_none()
        && let Some(status) = view.status.as_deref()
    {
        lines.push(Line::from(""));
        push_popup_form_inline_status(&mut lines, status, width);
    }

    let cursor = view.editing_title.then(|| {
        (
            title_row,
            popup_form_text_value_column("Title", true, true)
                + cursor_column(&view.title, view.title_cursor),
        )
    });

    EditLayout {
        lines,
        title_row,
        tags_row,
        slow_mode_row,
        auto_archive_row,
        cursor,
    }
}

/// The [start, end) row range that must be brought into view for the currently
/// focused cell.
fn focus_rows(view: &ThreadEditView, layout: &EditLayout) -> (usize, usize) {
    match view.active_field {
        ThreadEditField::Title => (layout.title_row, layout.title_row + 1),
        ThreadEditField::Tags => (layout.tags_row, layout.slow_mode_row),
        ThreadEditField::SlowMode => (layout.slow_mode_row, layout.auto_archive_row),
        ThreadEditField::AutoArchive => (layout.auto_archive_row, layout.lines.len()),
        ThreadEditField::Submit | ThreadEditField::Cancel => {
            (layout.lines.len().saturating_sub(1), layout.lines.len())
        }
    }
}

fn reveal_target(view: &ThreadEditView, layout: &EditLayout) -> (usize, usize) {
    if let Some((row, _)) = layout.cursor {
        (row, row + 1)
    } else {
        focus_rows(view, layout)
    }
}

/// Total content height and the row range to reveal, for `sync_view_heights` to
/// drive the popup scroll state without rebuilding the layout itself.
pub(in crate::tui::ui) struct ThreadEditMetrics {
    pub total_lines: usize,
    pub reveal_start: usize,
    pub reveal_end: usize,
}

pub(in crate::tui::ui) fn thread_edit_metrics(
    view: &ThreadEditView,
    content_width: usize,
) -> ThreadEditMetrics {
    let layout = build_edit_layout(view, content_width);
    let (reveal_start, reveal_end) = reveal_target(view, &layout);
    ThreadEditMetrics {
        total_lines: layout.lines.len(),
        reveal_start,
        reveal_end,
    }
}

/// Floating tag picker drawn on top of the editor, reusing the composer's
/// visual style. Tags are listed with checkboxes, scrolled to keep the active
/// tag in view.
pub(in crate::tui::ui) fn render_thread_edit_tag_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_thread_edit_tag_picker_active() {
        return;
    }
    let Some(view) = state.thread_edit_view() else {
        return;
    };
    if view.tags.is_empty() {
        return;
    }
    let tags = &view.tags;
    let popup = thread_edit_tag_picker_popup_area(area, tags.len());
    let content = render_modal_frame(frame, popup, "Choose tags");
    let visible_items = usize::from(content.height)
        .min(TAG_PICKER_VISIBLE_ITEMS)
        .min(tags.len())
        .max(1);
    let visible_range = selection::visible_window(view.tag_scroll, visible_items, tags.len());
    let ready_urls = ready_emoji_urls(emoji_images);
    let rows: Vec<Line<'static>> = tags[visible_range.clone()]
        .iter()
        .map(|tag| {
            tag_line(
                tag,
                usize::from(content.width),
                tag_custom_emoji_ready(tag.custom_emoji_url.as_deref(), &ready_urls),
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(rows).wrap(Wrap { trim: false }), content);
    if state.show_custom_emoji() {
        render_tag_picker_emojis(
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

/// Overlays custom tag-emoji images in a tag picker, one per visible row, at the
/// fixed column where `tag_line` reserves the blank emoji gap. Shared by the
/// thread-edit and composer pickers (each passes its own urls per row).
pub(super) fn render_tag_picker_emojis<'a>(
    frame: &mut Frame,
    area: Rect,
    row_custom_emoji_urls: impl IntoIterator<Item = Option<&'a str>>,
    emoji_images: &[EmojiImage<'_>],
) {
    overlay_emoji_column(
        frame,
        area,
        tag_line_emoji_column(),
        row_custom_emoji_urls.into_iter(),
        emoji_images,
    );
}

fn thread_edit_tag_picker_popup_area(area: Rect, tag_count: usize) -> Rect {
    let visible = tag_count.clamp(1, TAG_PICKER_VISIBLE_ITEMS) as u16;
    centered_rect(area, TAG_PICKER_WIDTH, visible.saturating_add(2))
}

pub(in crate::tui::ui) fn thread_edit_tag_picker_list_layout(
    area: Rect,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    SelectablePopupLayout::fixed(
        snapshot.target,
        thread_edit_tag_picker_popup_area(area, snapshot.item_count),
        snapshot,
    )
}

fn tag_line(tag: &ThreadEditTagView, width: usize, thumbnail_ready: bool) -> Line<'static> {
    let marker = if tag.active { "▸" } else { " " };
    let checkbox = if tag.selected { "[x]" } else { "[ ]" };
    let emoji = tag_emoji_text(
        tag.unicode_emoji.as_deref(),
        tag.custom_emoji_url.as_deref(),
        tag.custom_emoji_label.as_deref(),
        thumbnail_ready,
    );
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

/// The emoji portion of a tag row (with a leading space). A custom emoji reserves
/// a blank gap for the overlaid image once ready, else its `:name:` label.
pub(super) fn tag_emoji_text(
    unicode_emoji: Option<&str>,
    custom_emoji_url: Option<&str>,
    custom_emoji_label: Option<&str>,
    thumbnail_ready: bool,
) -> String {
    if let Some(emoji) = unicode_emoji {
        return format!(" {emoji}");
    }
    if custom_emoji_url.is_some() {
        if thumbnail_ready {
            return format!(
                " {}",
                " ".repeat(usize::from(EmojiImageSize::Compact.width()))
            );
        }
        if let Some(label) = custom_emoji_label {
            return format!(" {label}");
        }
    }
    String::new()
}

/// Column of the reserved custom-emoji gap within a picker row, measured from
/// the row start: marker + space + `[x]` + space.
fn tag_line_emoji_column() -> u16 {
    "  [x] ".width() as u16
}

pub(super) fn ready_emoji_urls(emoji_images: &[EmojiImage<'_>]) -> Vec<String> {
    emoji_images.iter().map(|image| image.url.clone()).collect()
}

/// Whether a custom tag emoji's image has loaded (so the row reserves the gap).
pub(super) fn tag_custom_emoji_ready(
    custom_emoji_url: Option<&str>,
    ready_urls: &[String],
) -> bool {
    custom_emoji_url.is_some_and(|url| ready_urls.iter().any(|ready| ready == url))
}

fn tag_summary(tags: &[ThreadEditTagView], width: usize) -> String {
    if tags.is_empty() {
        return "None".to_owned();
    }
    let selected: Vec<String> = tags
        .iter()
        .filter(|tag| tag.selected)
        .map(|tag| {
            let emoji = tag_emoji_text(
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

fn cursor_column(value: &str, cursor: usize) -> usize {
    let mut end = cursor.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].width()
}
