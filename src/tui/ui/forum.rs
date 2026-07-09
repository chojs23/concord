use super::message::list::message_author_style;
use super::*;
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};

#[cfg(test)]
pub(super) fn forum_post_viewport_lines(
    posts: &[ChannelThreadItem],
    selected: Option<usize>,
    width: usize,
    is_loading: bool,
) -> Vec<Line<'static>> {
    forum_post_viewport_lines_with_custom_emoji_images(posts, selected, width, is_loading, true)
}

pub(super) fn forum_post_viewport_lines_with_custom_emoji_images(
    posts: &[ChannelThreadItem],
    selected: Option<usize>,
    width: usize,
    is_loading: bool,
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    if posts.is_empty() {
        // Shared by the forum post list and a channel's thread list; "threads"
        // reads correctly for both since forum posts are themselves threads.
        let message = if is_loading {
            "Loading threads…"
        } else {
            "No threads yet."
        };
        return vec![Line::from(Span::styled(
            message,
            Style::default().fg(theme::current().dim),
        ))];
    }

    let mut lines = Vec::new();
    for (index, post) in posts.iter().enumerate() {
        if let Some(label) = post.section_label.as_deref() {
            lines.push(forum_post_section_header_line(label, width));
        }
        lines.extend(forum_post_card_lines(
            post,
            selected == Some(index),
            width,
            show_custom_emoji,
        ));
    }
    lines
}

pub(super) fn forum_post_scrollbar_visible_count(list_height: u16) -> usize {
    usize::from(list_height).max(1)
}

pub(in crate::tui) fn forum_post_card_lines(
    post: &ChannelThreadItem,
    selected: bool,
    width: usize,
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    let marker = if selected { "› " } else { "  " };
    let card_width = width.saturating_sub(marker.width()).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);
    let border_style = forum_post_accent_style(selected);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(marker, forum_post_accent_style(selected)),
            Span::styled(
                format!("╭{}╮", "─".repeat(card_width.saturating_sub(2))),
                border_style,
            ),
        ]),
        forum_post_inner_line(
            "  ",
            forum_post_title_spans(post, inner_width),
            inner_width,
            selected,
        ),
        forum_post_inner_line(
            "  ",
            forum_post_preview_spans(post, inner_width),
            inner_width,
            selected,
        ),
    ];
    // Untagged posts drop the tags row entirely (shrinking `card_height` by one).
    if !post.applied_tags.is_empty() {
        lines.push(forum_post_inner_line(
            "  ",
            forum_post_tag_spans(post, inner_width),
            inner_width,
            selected,
        ));
    }
    lines.push(forum_post_inner_line(
        "  ",
        forum_post_metadata_spans(post, inner_width, show_custom_emoji),
        inner_width,
        selected,
    ));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
            border_style,
        ),
    ]));
    lines
}

fn forum_post_section_header_line(label: &str, width: usize) -> Line<'static> {
    let label = truncate_display_width(label, width);
    let padding = width.saturating_sub(label.width());
    Line::from(Span::styled(
        format!("{label}{}", " ".repeat(padding)),
        Style::default()
            .fg(theme::current().text)
            .add_modifier(Modifier::BOLD),
    ))
}

fn forum_post_title_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    let title_style = Style::default().fg(theme::current().text).bold();
    if !post.pinned {
        return vec![Span::styled(
            truncate_display_width(&post.label, inner_width),
            title_style,
        )];
    }

    let badge = " PINNED";
    let badge_width = badge.width();
    let title_width = inner_width.saturating_sub(badge_width).max(1);
    vec![
        Span::styled(
            truncate_display_width(&post.label, title_width),
            title_style,
        ),
        Span::styled(badge, Style::default().fg(theme::current().warning).bold()),
    ]
}

fn forum_post_tag_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    // The tags row is only rendered for tagged posts.
    debug_assert!(!post.applied_tags.is_empty());
    let mut spans = Vec::new();
    let mut used_width = 0usize;
    for tag in &post.applied_tags {
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            inner_width,
            forum_post_tag_text(tag),
            Style::default().fg(theme::current().accent),
        );
    }
    spans
}

/// Text for one tag chip (`# name`). A custom emoji reserves a fixed-width blank
/// gap so the overlaid image does not reflow the row when it loads.
fn forum_post_tag_text(tag: &AppliedForumTag) -> String {
    if let Some(emoji) = tag.unicode_emoji.as_deref() {
        format!("# {emoji} {}", tag.name)
    } else if tag.custom_emoji_url.is_some() {
        let placeholder = " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH));
        format!("# {placeholder} {}", tag.name)
    } else {
        format!("# {}", tag.name)
    }
}

fn forum_post_preview_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    let preview_style = Style::default().fg(theme::current().text);
    let Some(author) = post.preview_author.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            Style::default().fg(theme::current().dim),
        )];
    };
    let Some(content) = post.preview_content.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            Style::default().fg(theme::current().dim),
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

fn forum_post_metadata_spans(
    post: &ChannelThreadItem,
    width: usize,
    show_custom_emoji: bool,
) -> Vec<Span<'static>> {
    let theme = theme::current();
    let primary_style = Style::default().fg(theme.text);
    let reaction_style = Style::default().fg(theme.accent);
    let muted_style = Style::default().fg(theme.dim);
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
            Style::default().fg(theme::current().warning).bold(),
        );
    }
    if let Some(layout) =
        forum_post_reaction_layout_for_width(&post.preview_reactions, width, show_custom_emoji)
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
            primary_style,
        );
    }
    if post.archived {
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            width,
            "archived".to_owned(),
            muted_style,
        );
    }
    if post.locked {
        push_forum_metadata_part(
            &mut spans,
            &mut used_width,
            width,
            "locked".to_owned(),
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
            Style::default().fg(theme::current().dim),
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
            Style::default().fg(theme::current().dim),
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

fn forum_post_reaction_start_col(post: &ChannelThreadItem) -> usize {
    if let Some(count) = post.comment_count {
        let label = if count == 1 { "comment" } else { "comments" };
        format!("{count} {label} · ").width()
    } else {
        0
    }
}

#[cfg(test)]
pub(super) fn forum_post_reaction_summary(
    reactions: &[ReactionInfo],
    width: usize,
) -> Option<String> {
    forum_post_reaction_summary_with_custom_emoji_images(reactions, width, true)
}

#[cfg(test)]
fn forum_post_reaction_summary_with_custom_emoji_images(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
) -> Option<String> {
    forum_post_reaction_layout_for_width(reactions, width, show_custom_emoji)
        .and_then(|layout| layout.lines.into_iter().next())
        .filter(|line| !line.is_empty())
}

fn forum_post_reaction_layout_for_width(
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

fn forum_post_reaction_layout(
    post: &ChannelThreadItem,
    width: usize,
) -> Option<(usize, ReactionLayout)> {
    let start_col = forum_post_reaction_start_col(post);
    let available_width = width.saturating_sub(start_col).max(1);
    let layout = lay_out_reaction_chips_with_custom_emoji_images(
        &post.preview_reactions,
        available_width,
        true,
    );
    if layout.lines.first().is_some_and(|line| !line.is_empty()) {
        Some((start_col, layout))
    } else {
        None
    }
}

pub(super) fn render_forum_post_reaction_emojis(
    frame: &mut Frame,
    list: Rect,
    posts: &[ChannelThreadItem],
    width: usize,
    emoji_images: &[EmojiImage<'_>],
    occlusion_areas: &[Rect],
) {
    let list_left = list.x as isize;
    let content_start = 4isize;
    let inner_width = forum_post_inner_width_for_reactions(width);

    let mut slots = Vec::new();
    for (row, reaction_start_col, layout) in
        forum_post_reaction_render_layouts(posts, width, usize::from(list.height))
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
/// `forum_post_tag_spans` so the overlay lands on the reserved gap after
/// truncation.
fn forum_post_tag_image_slots(
    post: &ChannelThreadItem,
    inner_width: usize,
) -> Vec<(usize, String)> {
    let mut slots = Vec::new();
    let mut used_width = 0usize;
    for tag in &post.applied_tags {
        let text = forum_post_tag_text(tag);
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
            if emoji_col + usize::from(EMOJI_REACTION_IMAGE_WIDTH) <= used_width {
                slots.push((emoji_col, url.to_owned()));
            }
        }
    }
    slots
}

/// Overlays custom tag-emoji images on each visible card's tags row, which sits
/// at `card_height() - 3` from the card top (it only exists for tagged posts).
pub(super) fn render_forum_post_tag_emojis(
    frame: &mut Frame,
    list: Rect,
    posts: &[ChannelThreadItem],
    width: usize,
    emoji_images: &[EmojiImage<'_>],
    occlusion_areas: &[Rect],
) {
    let list_left = list.x as isize;
    let content_start = 4isize;
    let inner_width = forum_post_inner_width_for_reactions(width);
    let list_height = usize::from(list.height);

    let mut slots = Vec::new();
    let mut rendered_row = 0usize;
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        if post.applied_tags.is_empty() {
            rendered_row = rendered_row.saturating_add(post.card_height());
            continue;
        }
        let row = rendered_row.saturating_add(post.card_height().saturating_sub(3));
        if row >= list_height {
            break;
        }
        for (slot_col, url) in forum_post_tag_image_slots(post, inner_width) {
            if slot_col >= inner_width {
                continue;
            }
            slots.push(EmojiSlot {
                row_in_list: row as isize,
                col: list_left + content_start + slot_col as isize,
                max_width: inner_width.saturating_sub(slot_col) as u16,
                url,
            });
        }
        rendered_row = rendered_row.saturating_add(post.card_height());
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
pub(super) fn forum_post_tag_rows_for_test(
    posts: &[ChannelThreadItem],
    width: usize,
    list_height: usize,
) -> Vec<(usize, Vec<usize>)> {
    let inner_width = forum_post_inner_width_for_reactions(width);
    let mut rendered_row = 0usize;
    let mut result = Vec::new();
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        if post.applied_tags.is_empty() {
            rendered_row = rendered_row.saturating_add(post.card_height());
            continue;
        }
        let row = rendered_row.saturating_add(post.card_height().saturating_sub(3));
        if row >= list_height {
            break;
        }
        let cols = forum_post_tag_image_slots(post, inner_width)
            .into_iter()
            .map(|(col, _)| col)
            .collect();
        result.push((row, cols));
        rendered_row = rendered_row.saturating_add(post.card_height());
    }
    result
}

fn forum_post_inner_width_for_reactions(width: usize) -> usize {
    let card_width = width.saturating_sub(2).max(4);
    card_width.saturating_sub(4).max(1)
}

fn forum_post_reaction_render_layouts(
    posts: &[ChannelThreadItem],
    width: usize,
    list_height: usize,
) -> Vec<(usize, usize, ReactionLayout)> {
    let inner_width = forum_post_inner_width_for_reactions(width);
    let mut rendered_row = 0usize;
    let mut layouts = Vec::new();
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        // Reactions render on the metadata line, which is the second-to-last
        // card row (its offset shifts up by one when the tags row is absent).
        let row = rendered_row.saturating_add(post.card_height().saturating_sub(2));
        if row >= list_height {
            break;
        }
        if let Some((reaction_start_col, layout)) = forum_post_reaction_layout(post, inner_width) {
            layouts.push((row, reaction_start_col, layout));
        }
        rendered_row = rendered_row.saturating_add(post.card_height());
    }
    layouts
}

fn forum_post_inner_line(
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
    let border_style = forum_post_accent_style(selected);
    let fill_style = Style::default();
    let mut spans = vec![
        Span::raw(marker.to_owned()),
        Span::styled("│ ", border_style),
    ];
    spans.append(&mut content);
    spans.push(Span::styled(" ".repeat(padding), fill_style));
    spans.push(Span::styled(" │", border_style));
    Line::from(spans)
}

fn forum_post_accent_style(selected: bool) -> Style {
    let theme = theme::current();
    if selected {
        Style::default()
            .fg(theme.selected_forum_post_border)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    }
}
