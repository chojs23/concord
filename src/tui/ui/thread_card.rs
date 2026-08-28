use super::message::list::message_author_style;
use super::*;
use crate::tui::message::format::wrap_plain_text_at_words;
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};
use crate::tui::ui::loading_indicator::AsciiLoadingIndicator;

const THREAD_CARD_IMAGE_GAP: usize = 2;
const THREAD_CARD_IMAGE_MAX_WIDTH: usize = 20;
const THREAD_CARD_IMAGE_MAX_HEIGHT: u16 = 4;
const THREAD_CARD_IMAGE_MIN_WIDTH: usize = 10;
const THREAD_CARD_IMAGE_MIN_TEXT_WIDTH: usize = 24;
const THREAD_CARD_REACTION_LIMIT: usize = 3;

#[derive(Clone, Copy)]
enum ThreadCardTitlePart {
    Title,
    Pinned,
    State,
}

struct ThreadCardTitleRow {
    parts: Vec<(ThreadCardTitlePart, String)>,
    width: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThreadCardLayout {
    card_height: usize,
    tag_row: Option<usize>,
    metadata_row: usize,
}

pub(in crate::tui) struct ThreadCardHeightInput<'a> {
    pub(in crate::tui) label: &'a str,
    pub(in crate::tui) pinned: bool,
    pub(in crate::tui) archived: bool,
    pub(in crate::tui) locked: bool,
    pub(in crate::tui) has_tags: bool,
    pub(in crate::tui) has_preview_image: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ThreadCardImageSlot {
    pub(in crate::tui) column: u16,
    pub(in crate::tui) width: u16,
    pub(in crate::tui) height: u16,
}

#[cfg(test)]
pub(super) fn thread_card_viewport_lines(
    posts: &[ChannelThreadItem],
    selected: Option<usize>,
    width: usize,
    is_loading: bool,
) -> Vec<Line<'static>> {
    thread_card_viewport_lines_with_custom_emoji_images(
        posts, selected, width, is_loading, 0, true, true,
    )
}

pub(super) fn thread_card_viewport_lines_with_custom_emoji_images(
    posts: &[ChannelThreadItem],
    selected: Option<usize>,
    width: usize,
    is_loading: bool,
    animation_frame: usize,
    show_custom_emoji: bool,
    show_images: bool,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    if posts.is_empty() {
        // Shared by the forum post list and a channel's thread list; "threads"
        // reads correctly for both since forum posts are themselves threads.
        if is_loading {
            return AsciiLoadingIndicator::new(
                "Loading threads...",
                theme::current().style(theme::HighlightGroup::Loading),
            )
            .lines(animation_frame)
            .into_iter()
            .collect();
        }
        return vec![Line::from(Span::styled(
            "No threads yet.",
            theme::current().style(theme::HighlightGroup::Placeholder),
        ))];
    }

    let mut lines = Vec::with_capacity(
        posts
            .iter()
            .map(|post| thread_card_rendered_height(post, width, show_images))
            .sum(),
    );
    for (index, post) in posts.iter().enumerate() {
        if let Some(label) = post.section_label.as_deref() {
            lines.push(thread_card_section_header_line(label, width));
        }
        lines.extend(thread_card_lines(
            post,
            selected == Some(index),
            width,
            show_custom_emoji,
            show_images,
        ));
    }
    lines
}

pub(super) fn thread_card_scrollbar_visible_count(list_height: u16) -> usize {
    usize::from(list_height).max(1)
}

pub(in crate::tui) fn thread_card_lines(
    post: &ChannelThreadItem,
    selected: bool,
    width: usize,
    show_custom_emoji: bool,
    show_images: bool,
) -> Vec<Line<'static>> {
    let marker_style = if selected {
        theme::current().style(theme::HighlightGroup::ForumSelectedBorder)
    } else {
        Style::default()
    };
    let marker = selection_marker_with_style(selected, marker_style);
    let marker_width = marker.content.width();
    let marker_placeholder = " ".repeat(marker_width);
    let card_width = width.saturating_sub(marker_width).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);
    let text_width = thread_card_text_width(post, inner_width, width, show_images);
    let title_rows = thread_card_title_rows(post, text_width);
    let layout = thread_card_layout_for_title_rows(
        title_rows.len(),
        !post.applied_tags.is_empty(),
        thread_card_image_slot(post, width, show_images).is_some(),
    );
    let border_style = thread_card_accent_style(selected);
    let border = theme::current().border_set(theme::BorderSurface::Forum);

    let mut lines = vec![Line::from(vec![
        marker,
        Span::styled(
            format!(
                "{}{}{}",
                border.top_left,
                border.horizontal_top.repeat(card_width.saturating_sub(2)),
                border.top_right
            ),
            border_style,
        ),
    ])];
    lines.extend(title_rows.into_iter().map(|row| {
        thread_card_inner_line(
            &marker_placeholder,
            thread_card_title_row_spans(row),
            inner_width,
            selected,
        )
    }));
    lines.push(thread_card_inner_line(
        &marker_placeholder,
        Vec::new(),
        inner_width,
        selected,
    ));
    lines.push(thread_card_inner_line(
        &marker_placeholder,
        thread_card_preview_spans(post, text_width),
        inner_width,
        selected,
    ));
    // Untagged posts drop the tags row entirely (shrinking `card_height` by one).
    if !post.applied_tags.is_empty() {
        lines.push(thread_card_inner_line(
            &marker_placeholder,
            thread_card_tag_spans(post, text_width),
            inner_width,
            selected,
        ));
    }
    while lines.len() < layout.metadata_row {
        lines.push(thread_card_inner_line(
            &marker_placeholder,
            Vec::new(),
            inner_width,
            selected,
        ));
    }
    lines.push(thread_card_inner_line(
        &marker_placeholder,
        thread_card_metadata_spans(post, text_width, show_custom_emoji),
        inner_width,
        selected,
    ));
    lines.push(Line::from(vec![
        Span::raw(marker_placeholder),
        Span::styled(
            format!(
                "{}{}{}",
                border.bottom_left,
                border
                    .horizontal_bottom
                    .repeat(card_width.saturating_sub(2)),
                border.bottom_right
            ),
            border_style,
        ),
    ]));
    lines
}

/// Keeps an embedded card within the message content while preserving the
/// historical maximum width used by thread-created system messages.
pub(in crate::tui) fn thread_card_width_in_message(content_width: usize) -> usize {
    let marker_width = selection_marker_width();
    content_width
        .saturating_sub(marker_width)
        .clamp(4, 72)
        .saturating_add(marker_width)
}

fn thread_card_section_header_line(label: &str, width: usize) -> Line<'static> {
    let label = truncate_display_width(label, width);
    let padding = width.saturating_sub(label.width());
    Line::from(Span::styled(
        format!("{label}{}", " ".repeat(padding)),
        theme::current().style(theme::HighlightGroup::Heading),
    ))
}

fn thread_card_title_rows(post: &ChannelThreadItem, width: usize) -> Vec<ThreadCardTitleRow> {
    thread_card_title_rows_for(&post.label, post.pinned, post.archived, post.locked, width)
}

fn thread_card_title_rows_for(
    label: &str,
    pinned: bool,
    archived: bool,
    locked: bool,
    width: usize,
) -> Vec<ThreadCardTitleRow> {
    let width = width.max(1);
    let mut rows = wrap_plain_text_at_words(label, width)
        .into_iter()
        .map(|text| ThreadCardTitleRow {
            width: text.width(),
            parts: vec![(ThreadCardTitlePart::Title, text)],
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(ThreadCardTitleRow {
            parts: Vec::new(),
            width: 0,
        });
    }

    let badges = [
        pinned.then_some((ThreadCardTitlePart::Pinned, "PINNED")),
        archived.then_some((ThreadCardTitlePart::State, "(archived)")),
        locked.then_some((ThreadCardTitlePart::State, "(locked)")),
    ];
    for (part, label) in badges.into_iter().flatten() {
        let badge_width = label.width();
        let current = rows.last_mut().expect("title has at least one row");
        let separator_width = usize::from(current.width > 0);
        if current.width > 0
            && current
                .width
                .saturating_add(separator_width)
                .saturating_add(badge_width)
                > width
        {
            rows.push(ThreadCardTitleRow {
                parts: vec![(part, truncate_display_width(label, width))],
                width: badge_width.min(width),
            });
            continue;
        }

        let value = if separator_width == 0 {
            label.to_owned()
        } else {
            format!(" {label}")
        };
        current.width = current.width.saturating_add(value.width());
        current.parts.push((part, value));
    }
    rows
}

fn thread_card_title_row_spans(row: ThreadCardTitleRow) -> Vec<Span<'static>> {
    let theme = theme::current();
    row.parts
        .into_iter()
        .map(|(part, value)| {
            let style = match part {
                ThreadCardTitlePart::Title => theme.style(theme::HighlightGroup::Heading),
                ThreadCardTitlePart::Pinned => theme.style(theme::HighlightGroup::ForumPinnedBadge),
                ThreadCardTitlePart::State => theme.style(theme::HighlightGroup::ForumSecondary),
            };
            Span::styled(value, style)
        })
        .collect()
}

fn thread_card_tag_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    // The tags row is only rendered for tagged posts.
    debug_assert!(!post.applied_tags.is_empty());
    let mut spans = Vec::new();
    let mut used_width = 0usize;
    for tag in &post.applied_tags {
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            inner_width,
            thread_card_tag_text(tag),
            theme::current().style(theme::HighlightGroup::Tag),
        );
    }
    spans
}

/// Text for one tag chip (`# name`). A custom emoji reserves a fixed-width blank
/// gap so the overlaid image does not reflow the row when it loads.
fn thread_card_tag_text(tag: &AppliedForumTag) -> String {
    if let Some(emoji) = tag.unicode_emoji.as_deref() {
        format!("# {emoji} {}", tag.name)
    } else if tag.custom_emoji_url.is_some() {
        let placeholder = " ".repeat(usize::from(EmojiImageSize::Compact.width()));
        format!("# {placeholder} {}", tag.name)
    } else {
        format!("# {}", tag.name)
    }
}

fn thread_card_preview_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    let preview_style = Style::default();
    if post.preview_loading {
        let loading_style = theme::current().style(theme::HighlightGroup::Loading);
        let Some(author) = post.preview_author.as_deref() else {
            return vec![Span::styled(
                truncate_display_width("Loading preview...", inner_width),
                loading_style,
            )];
        };
        let author_width = (inner_width / 3).max(1);
        let author = truncate_display_width(author, author_width);
        let content_width = inner_width
            .saturating_sub(author.width())
            .saturating_sub(2)
            .max(1);
        return vec![
            Span::styled(author, message_author_style(post.preview_author_color)),
            Span::styled(": ", preview_style),
            Span::styled(
                truncate_display_width("Loading preview...", content_width),
                loading_style,
            ),
        ];
    }
    let Some(author) = post.preview_author.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            theme::current().style(theme::HighlightGroup::Placeholder),
        )];
    };
    let Some(content) = post.preview_content.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            theme::current().style(theme::HighlightGroup::Placeholder),
        )];
    };

    let author_width = (inner_width / 3).max(1);
    let author = truncate_display_width(author, author_width);
    let content_width = inner_width
        .saturating_sub(author.width())
        .saturating_sub(2)
        .max(1);
    vec![
        Span::styled(author, message_author_style(post.preview_author_color)),
        Span::styled(": ", preview_style),
        Span::styled(
            truncate_display_width(content, content_width),
            preview_style,
        ),
    ]
}

/// Returns the right-side thumbnail slot shared by text layout and image
/// placement. Narrow cards keep all width for text rather than forcing a tiny
/// body column beside an unreadable image.
pub(in crate::tui) fn thread_card_image_slot(
    post: &ChannelThreadItem,
    width: usize,
    show_images: bool,
) -> Option<ThreadCardImageSlot> {
    thread_card_image_slot_for(post.preview_image.is_some(), width, show_images)
}

fn thread_card_image_slot_for(
    has_preview_image: bool,
    width: usize,
    show_images: bool,
) -> Option<ThreadCardImageSlot> {
    if !show_images || !has_preview_image {
        return None;
    }
    let inner_width = thread_card_inner_width_for_reactions(width);
    let available = inner_width
        .saturating_sub(THREAD_CARD_IMAGE_MIN_TEXT_WIDTH)
        .saturating_sub(THREAD_CARD_IMAGE_GAP);
    let preview_width = available.min(THREAD_CARD_IMAGE_MAX_WIDTH);
    if preview_width < THREAD_CARD_IMAGE_MIN_WIDTH {
        return None;
    }
    let column = selection_marker_width()
        .saturating_add(2)
        .saturating_add(inner_width)
        .saturating_sub(preview_width);
    Some(ThreadCardImageSlot {
        column: u16::try_from(column).unwrap_or(u16::MAX),
        width: u16::try_from(preview_width).unwrap_or(u16::MAX),
        height: THREAD_CARD_IMAGE_MAX_HEIGHT,
    })
}

fn thread_card_layout_for_title_rows(
    title_rows: usize,
    has_tags: bool,
    has_image_slot: bool,
) -> ThreadCardLayout {
    let content_rows = title_rows.max(1) + 1 + 1 + usize::from(has_tags);
    let body_rows = if has_image_slot {
        content_rows.max(usize::from(THREAD_CARD_IMAGE_MAX_HEIGHT))
    } else {
        content_rows
    };
    let metadata_row = 1usize.saturating_add(body_rows);
    ThreadCardLayout {
        card_height: metadata_row.saturating_add(2),
        tag_row: has_tags.then_some(title_rows.max(1).saturating_add(3)),
        metadata_row,
    }
}

fn thread_card_layout(
    post: &ChannelThreadItem,
    width: usize,
    show_images: bool,
) -> ThreadCardLayout {
    let inner_width = thread_card_inner_width_for_reactions(width);
    let text_width = thread_card_text_width(post, inner_width, width, show_images);
    thread_card_layout_for_title_rows(
        thread_card_title_rows(post, text_width).len(),
        !post.applied_tags.is_empty(),
        thread_card_image_slot(post, width, show_images).is_some(),
    )
}

pub(in crate::tui) fn thread_card_height_for(
    input: ThreadCardHeightInput<'_>,
    width: usize,
    show_images: bool,
) -> usize {
    let inner_width = thread_card_inner_width_for_reactions(width);
    let image_slot = thread_card_image_slot_for(input.has_preview_image, width, show_images);
    let text_width = image_slot
        .map(|slot| {
            inner_width
                .saturating_sub(usize::from(slot.width))
                .saturating_sub(THREAD_CARD_IMAGE_GAP)
                .max(1)
        })
        .unwrap_or(inner_width);
    thread_card_layout_for_title_rows(
        thread_card_title_rows_for(
            input.label,
            input.pinned,
            input.archived,
            input.locked,
            text_width,
        )
        .len(),
        input.has_tags,
        image_slot.is_some(),
    )
    .card_height
}

pub(in crate::tui) fn thread_card_height(
    post: &ChannelThreadItem,
    width: usize,
    show_images: bool,
) -> usize {
    thread_card_layout(post, width, show_images).card_height
}

pub(in crate::tui) fn thread_card_rendered_height(
    post: &ChannelThreadItem,
    width: usize,
    show_images: bool,
) -> usize {
    thread_card_height(post, width, show_images)
        .saturating_add(usize::from(post.section_label.is_some()))
}

pub(in crate::tui) fn thread_card_image_preview_area(
    list: Rect,
    row: isize,
    column: u16,
    width: u16,
    height: u16,
) -> Option<Rect> {
    let row = u16::try_from(row).ok()?;
    if row >= list.height || column >= list.width || width == 0 || height == 0 {
        return None;
    }
    Some(Rect {
        x: list.x.saturating_add(column),
        y: list.y.saturating_add(row),
        width: width.min(list.width.saturating_sub(column)),
        height: height.min(list.height.saturating_sub(row)),
    })
}

fn thread_card_text_width(
    post: &ChannelThreadItem,
    inner_width: usize,
    width: usize,
    show_images: bool,
) -> usize {
    thread_card_image_slot(post, width, show_images)
        .map(|slot| {
            inner_width
                .saturating_sub(usize::from(slot.width))
                .saturating_sub(THREAD_CARD_IMAGE_GAP)
                .max(1)
        })
        .unwrap_or(inner_width)
}

fn thread_card_metadata_spans(
    post: &ChannelThreadItem,
    width: usize,
    show_custom_emoji: bool,
) -> Vec<Span<'static>> {
    let theme = theme::current();
    let primary_style = Style::default();
    let reaction_style = theme.style(theme::HighlightGroup::Reaction);
    let muted_style = theme.style(theme::HighlightGroup::ForumSecondary);
    let mut spans = Vec::new();
    let mut used_width = 0usize;

    if let Some(count) = post.comment_count {
        let label = if count == 1 { "comment" } else { "comments" };
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            width,
            format!("{count} {label}"),
            primary_style,
        );
    }
    if post.new_message_count > 0 {
        let label = if post.new_message_count == 1 {
            "new message"
        } else {
            "new messages"
        };
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            width,
            format!("{} {label}", post.new_message_count),
            theme.style(theme::HighlightGroup::UnreadNotice),
        );
    }
    let reactions = thread_card_visible_reactions(post)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(layout) =
        thread_card_reaction_layout_for_width(&reactions, width, show_custom_emoji)
    {
        push_forum_metadata_reaction_part(
            &mut spans,
            &mut used_width,
            width,
            reaction_style,
            layout,
        );
    }
    if let Some(message_id) = post.last_activity_message_id {
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            width,
            format_message_relative_age(message_id),
            muted_style,
        );
    }
    if spans.is_empty() {
        vec![Span::styled("No activity yet", muted_style)]
    } else {
        spans
    }
}

fn push_forum_metadata_part(
    spans: &mut Vec<Span<'static>>,
    used_width: &mut usize,
    max_width: usize,
    text: String,
    style: Style,
) {
    if *used_width >= max_width {
        return;
    }
    if !spans.is_empty() {
        let separator = " · ";
        let remaining = max_width.saturating_sub(*used_width);
        if remaining == 0 {
            return;
        }
        let separator = truncate_display_width(separator, remaining);
        *used_width = used_width.saturating_add(separator.width());
        spans.push(Span::styled(
            separator,
            theme::current().style(theme::HighlightGroup::Decoration),
        ));
    }

    let remaining = max_width.saturating_sub(*used_width);
    if remaining == 0 {
        return;
    }
    let text = truncate_display_width(&text, remaining);
    *used_width = used_width.saturating_add(text.width());
    spans.push(Span::styled(text, style));
}

fn push_forum_metadata_reaction_part(
    spans: &mut Vec<Span<'static>>,
    used_width: &mut usize,
    max_width: usize,
    style: Style,
    layout: ReactionLayout,
) {
    let Some(line) = layout.lines.first() else {
        return;
    };
    if line.is_empty() {
        return;
    }

    if *used_width > 0 {
        let separator = " · ";
        let remaining = max_width.saturating_sub(*used_width);
        if remaining == 0 {
            return;
        }
        let separator = truncate_display_width(separator, remaining);
        *used_width = used_width.saturating_add(separator.width());
        spans.push(Span::styled(
            separator,
            theme::current().style(theme::HighlightGroup::Decoration),
        ));
    }

    let remaining = max_width.saturating_sub(*used_width);
    if remaining == 0 {
        return;
    }
    let text = truncate_display_width(line, remaining);
    *used_width = used_width.saturating_add(text.width());
    spans.extend(reaction_line_spans(&text, &layout.self_ranges, 0, style));
}

fn thread_card_reaction_start_col(post: &ChannelThreadItem) -> usize {
    if let Some(count) = post.comment_count {
        let label = if count == 1 { "comment" } else { "comments" };
        format!("{count} {label} · ").width()
    } else {
        0
    }
}

#[cfg(test)]
pub(super) fn thread_card_reaction_summary(
    reactions: &[ReactionInfo],
    width: usize,
) -> Option<String> {
    thread_card_reaction_summary_with_custom_emoji_images(reactions, width, true)
}

#[cfg(test)]
fn thread_card_reaction_summary_with_custom_emoji_images(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
) -> Option<String> {
    thread_card_reaction_layout_for_width(reactions, width, show_custom_emoji)
        .and_then(|layout| layout.lines.into_iter().next())
        .filter(|line| !line.is_empty())
}

fn thread_card_reaction_layout_for_width(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
) -> Option<ReactionLayout> {
    let layout =
        lay_out_reaction_chips_with_custom_emoji_images(reactions, width, show_custom_emoji);
    if layout.lines.first().is_some_and(|line| !line.is_empty()) {
        Some(layout)
    } else {
        None
    }
}

fn thread_card_reaction_layout(
    post: &ChannelThreadItem,
    width: usize,
) -> Option<(usize, ReactionLayout)> {
    let start_col = thread_card_reaction_start_col(post);
    let available_width = width.saturating_sub(start_col).max(1);
    let reactions = thread_card_visible_reactions(post)
        .cloned()
        .collect::<Vec<_>>();
    let layout = lay_out_reaction_chips_with_custom_emoji_images(&reactions, available_width, true);
    if layout.lines.first().is_some_and(|line| !line.is_empty()) {
        Some((start_col, layout))
    } else {
        None
    }
}

pub(in crate::tui) fn thread_card_visible_reactions(
    post: &ChannelThreadItem,
) -> impl Iterator<Item = &ReactionInfo> {
    post.preview_reactions
        .iter()
        .filter(|reaction| reaction.count > 0)
        .take(THREAD_CARD_REACTION_LIMIT)
}

pub(super) fn render_thread_card_reaction_emojis(
    frame: &mut Frame,
    list: Rect,
    posts: &[ChannelThreadItem],
    width: usize,
    emoji_images: &[EmojiImage<'_>],
    occlusion_areas: &[Rect],
    show_images: bool,
) {
    let list_left = list.x as isize;
    let content_start =
        isize::try_from(selection_marker_width().saturating_add(2)).unwrap_or(isize::MAX);
    let inner_width = thread_card_inner_width_for_reactions(width);

    let mut slots = Vec::new();
    for (row, reaction_start_col, layout) in
        thread_card_reaction_render_layouts(posts, width, usize::from(list.height), show_images)
    {
        for slot in layout.slots.into_iter().filter(|slot| slot.line == 0) {
            let slot_col = reaction_start_col.saturating_add(slot.col as usize);
            if slot_col >= inner_width {
                continue;
            }
            slots.push(EmojiSlot {
                row_in_list: row as isize,
                col: list_left + content_start + slot_col as isize,
                max_width: inner_width.saturating_sub(slot_col) as u16,
                image_size: EmojiImageSize::Compact,
                url: slot.url,
            });
        }
    }
    overlay_emoji_slots(
        frame,
        list,
        emoji_images,
        occlusion_areas,
        slots.into_iter(),
    );
}

/// Column offsets (from the card's inner content start) and urls of each
/// custom-emoji placeholder on a post's tag row. Mirrors the width accounting of
/// `thread_card_tag_spans` so the overlay lands on the reserved gap after
/// truncation.
fn thread_card_tag_image_slots(
    post: &ChannelThreadItem,
    inner_width: usize,
) -> Vec<(usize, String)> {
    let mut slots = Vec::new();
    let mut used_width = 0usize;
    for tag in &post.applied_tags {
        let text = thread_card_tag_text(tag);
        if used_width >= inner_width {
            break;
        }
        if used_width > 0 {
            let separator = " · ";
            let remaining = inner_width.saturating_sub(used_width);
            if remaining == 0 {
                break;
            }
            let separator = truncate_display_width(separator, remaining);
            used_width = used_width.saturating_add(separator.width());
        }
        let remaining = inner_width.saturating_sub(used_width);
        if remaining == 0 {
            break;
        }
        let truncated = truncate_display_width(&text, remaining);
        let chip_start = used_width;
        used_width = used_width.saturating_add(truncated.width());
        // The placeholder gap sits at `# ` (two columns) into the chip. Only
        // record it when the truncated chip still includes that gap.
        if let Some(url) = tag.custom_emoji_url.as_deref() {
            let emoji_col = chip_start.saturating_add("# ".width());
            if emoji_col + usize::from(EmojiImageSize::Compact.width()) <= used_width {
                slots.push((emoji_col, url.to_owned()));
            }
        }
    }
    slots
}

/// Overlays custom tag-emoji images on each visible card's tags row.
pub(super) fn render_thread_card_tag_emojis(
    frame: &mut Frame,
    list: Rect,
    posts: &[ChannelThreadItem],
    width: usize,
    emoji_images: &[EmojiImage<'_>],
    occlusion_areas: &[Rect],
    show_images: bool,
) {
    let list_left = list.x as isize;
    let content_start = 4isize;
    let full_inner_width = thread_card_inner_width_for_reactions(width);
    let list_height = usize::from(list.height);

    let mut slots = Vec::new();
    let mut rendered_row = 0usize;
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        let layout = thread_card_layout(post, width, show_images);
        let Some(tag_row) = layout.tag_row else {
            rendered_row = rendered_row.saturating_add(layout.card_height);
            continue;
        };
        let inner_width = thread_card_text_width(post, full_inner_width, width, show_images);
        let row = rendered_row.saturating_add(tag_row);
        if row >= list_height {
            break;
        }
        for (slot_col, url) in thread_card_tag_image_slots(post, inner_width) {
            if slot_col >= inner_width {
                continue;
            }
            slots.push(EmojiSlot {
                row_in_list: row as isize,
                col: list_left + content_start + slot_col as isize,
                max_width: inner_width.saturating_sub(slot_col) as u16,
                image_size: EmojiImageSize::Compact,
                url,
            });
        }
        rendered_row = rendered_row.saturating_add(layout.card_height);
    }
    overlay_emoji_slots(
        frame,
        list,
        emoji_images,
        occlusion_areas,
        slots.into_iter(),
    );
}

#[cfg(test)]
pub(super) fn thread_card_tag_rows_for_test(
    posts: &[ChannelThreadItem],
    width: usize,
    list_height: usize,
) -> Vec<(usize, Vec<usize>)> {
    let inner_width = thread_card_inner_width_for_reactions(width);
    let mut rendered_row = 0usize;
    let mut result = Vec::new();
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        let layout = thread_card_layout(post, width, true);
        let Some(tag_row) = layout.tag_row else {
            rendered_row = rendered_row.saturating_add(layout.card_height);
            continue;
        };
        let row = rendered_row.saturating_add(tag_row);
        if row >= list_height {
            break;
        }
        let cols = thread_card_tag_image_slots(post, inner_width)
            .into_iter()
            .map(|(col, _)| col)
            .collect();
        result.push((row, cols));
        rendered_row = rendered_row.saturating_add(layout.card_height);
    }
    result
}

fn thread_card_inner_width_for_reactions(width: usize) -> usize {
    let card_width = width.saturating_sub(selection_marker_width()).max(4);
    card_width.saturating_sub(4).max(1)
}

fn thread_card_reaction_render_layouts(
    posts: &[ChannelThreadItem],
    width: usize,
    list_height: usize,
    show_images: bool,
) -> Vec<(usize, usize, ReactionLayout)> {
    let full_inner_width = thread_card_inner_width_for_reactions(width);
    let mut rendered_row = 0usize;
    let mut layouts = Vec::new();
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        let layout = thread_card_layout(post, width, show_images);
        let row = rendered_row.saturating_add(layout.metadata_row);
        if row >= list_height {
            break;
        }
        let inner_width = thread_card_text_width(post, full_inner_width, width, show_images);
        if let Some((reaction_start_col, layout)) = thread_card_reaction_layout(post, inner_width) {
            layouts.push((row, reaction_start_col, layout));
        }
        rendered_row = rendered_row.saturating_add(layout.card_height);
    }
    layouts
}

fn thread_card_inner_line(
    marker: &str,
    mut content: Vec<Span<'static>>,
    inner_width: usize,
    selected: bool,
) -> Line<'static> {
    let content_width = content
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>();
    let padding = inner_width.saturating_sub(content_width);
    let border_style = thread_card_accent_style(selected);
    let border = theme::current().border_set(theme::BorderSurface::Forum);
    let fill_style = theme::current().style(theme::HighlightGroup::Normal);
    let mut spans = vec![
        Span::raw(marker.to_owned()),
        Span::styled(format!("{} ", border.vertical_left), border_style),
    ];
    spans.append(&mut content);
    spans.push(Span::styled(" ".repeat(padding), fill_style));
    spans.push(Span::styled(
        format!(" {}", border.vertical_right),
        border_style,
    ));
    Line::from(spans)
}

fn thread_card_accent_style(selected: bool) -> Style {
    let theme = theme::current();
    if selected {
        theme.style(theme::HighlightGroup::ForumSelectedBorder)
    } else {
        theme.style(theme::HighlightGroup::ForumBorder)
    }
}
