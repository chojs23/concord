use super::message_list::message_author_style;
use super::*;

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
        let message = if is_loading {
            "Loading posts…"
        } else {
            "No forum posts."
        };
        return vec![Line::from(Span::styled(message, Style::default().fg(DIM)))];
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

fn forum_post_card_lines(
    post: &ChannelThreadItem,
    selected: bool,
    width: usize,
    show_custom_emoji: bool,
) -> [Line<'static>; FORUM_POST_CARD_HEIGHT] {
    let marker = if selected { "› " } else { "  " };
    let card_width = width.saturating_sub(marker.width()).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);
    let border_style = forum_post_border_style(selected);

    [
        Line::from(vec![
            Span::styled(marker, forum_post_marker_style(selected)),
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
        forum_post_inner_line(
            "  ",
            forum_post_metadata_spans(post, inner_width, show_custom_emoji),
            inner_width,
            selected,
        ),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
                border_style,
            ),
        ]),
    ]
}

fn forum_post_section_header_line(label: &str, width: usize) -> Line<'static> {
    let label = truncate_display_width(label, width);
    let padding = width.saturating_sub(label.width());
    Line::from(Span::styled(
        format!("{label}{}", " ".repeat(padding)),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))
}

fn forum_post_title_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    let title_style = Style::default().fg(Color::White).bold();
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
        Span::styled(badge, Style::default().fg(Color::Yellow).bold()),
    ]
}

fn forum_post_preview_spans(post: &ChannelThreadItem, inner_width: usize) -> Vec<Span<'static>> {
    let preview_style = Style::default().fg(Color::White);
    let Some(author) = post.preview_author.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            Style::default().fg(DIM),
        )];
    };
    let Some(content) = post.preview_content.as_deref() else {
        return vec![Span::styled(
            "Preview unavailable",
            Style::default().fg(DIM),
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
    let primary_style = Style::default().fg(Color::White);
    let reaction_style = Style::default().fg(ACCENT);
    let muted_style = Style::default().fg(DIM);
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
        spans.push(Span::styled(separator, Style::default().fg(DIM)));
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
        spans.push(Span::styled(separator, Style::default().fg(DIM)));
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
    emoji_images: &[EmojiReactionImage<'_>],
) {
    if emoji_images.is_empty() || list.height == 0 || list.width == 0 {
        return;
    }

    let list_left = list.x as isize;
    let list_right = list_left + list.width as isize;
    let content_start = 4isize;
    let card_width = width.saturating_sub(2).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);

    let mut rendered_row = 0usize;
    for post in posts {
        if post.section_label.is_some() {
            rendered_row = rendered_row.saturating_add(1);
        }
        let row = rendered_row.saturating_add(3);
        if row >= list.height as usize {
            break;
        }
        let Some((reaction_start_col, layout)) = forum_post_reaction_layout(post, inner_width)
        else {
            continue;
        };
        for slot in layout.slots.into_iter().filter(|slot| slot.line == 0) {
            let slot_col = reaction_start_col.saturating_add(slot.col as usize);
            if slot_col >= inner_width {
                continue;
            }
            let Some(image) = emoji_images.iter().find(|img| img.url == slot.url) else {
                continue;
            };
            let absolute_col = list_left + content_start + slot_col as isize;
            if absolute_col >= list_right {
                continue;
            }
            let remaining_content_width = inner_width.saturating_sub(slot_col) as u16;
            let remaining_list_width = (list_right - absolute_col).max(0) as u16;
            let image_width = EMOJI_REACTION_IMAGE_WIDTH
                .min(remaining_content_width)
                .min(remaining_list_width);
            if image_width == 0 {
                continue;
            }
            frame.render_widget(
                RatatuiImage::new(image.protocol),
                Rect {
                    x: absolute_col as u16,
                    y: list.y.saturating_add(row as u16),
                    width: image_width,
                    height: 1,
                },
            );
        }
        rendered_row = rendered_row.saturating_add(FORUM_POST_CARD_HEIGHT);
    }
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
    let border_style = forum_post_border_style(selected);
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

fn forum_post_border_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(SELECTED_FORUM_POST_BORDER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    }
}

fn forum_post_marker_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(SELECTED_FORUM_POST_BORDER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    }
}
