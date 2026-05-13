use super::*;
use crate::tui::selection;

pub(in crate::tui::ui) fn render_emoji_reaction_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: Vec<EmojiImage<'_>>,
) {
    if !state.is_emoji_reaction_picker_open() {
        return;
    }

    let reactions = state.filtered_emoji_reaction_items_slice().unwrap_or(&[]);
    if reactions.is_empty() && !state.is_filtering_emoji_reactions() {
        return;
    }
    let filter = state.emoji_reaction_filter();

    let selected = state
        .selected_emoji_reaction_index_for_len(reactions.len())
        .unwrap_or(0);
    let desired_visible_items = reactions
        .len()
        .clamp(1, selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
    let extra_lines = u16::from(filter.is_some());
    let popup = centered_rect(
        area,
        42,
        (desired_visible_items as u16)
            .saturating_add(extra_lines)
            .saturating_add(2),
    );
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    let filter_lines = u16::from(filter.is_some());
    let visible_items =
        usize::from(content.height.saturating_sub(filter_lines)).min(desired_visible_items);
    let visible_range = selection::visible_item_range(reactions.len(), selected, visible_items);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(emoji_reaction_picker_lines_with_custom_emoji_images(
            reactions,
            selected,
            visible_items,
            &ready_urls,
            state.show_custom_emoji(),
            filter,
            usize::from(content.width),
        ))
        .block(block)
        .wrap(Wrap { trim: false }),
        popup,
    );
    if state.show_custom_emoji() {
        render_emoji_reaction_images(
            frame,
            content,
            reactions,
            selected,
            visible_items,
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
        reactions.len(),
    );
}

pub(in crate::tui::ui) fn render_reaction_users_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(popup_state) = state.reaction_users_popup() else {
        return;
    };

    // Compute the popup's eventual inner width up front so we can pre-truncate
    // every line to fit. Without this, ratatui's `Wrap` would split a long
    // username across rows and the wrap continuation overlaps neighbouring
    // lines, producing the trailing-fragment artefact reported by users.
    const POPUP_TARGET_WIDTH: u16 = 58;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));

    let max_visible_lines = reaction_users_visible_line_count(area);
    let lines = reaction_users_popup_lines_with_custom_emoji_images(
        popup_state.reactions(),
        popup_state.scroll(),
        max_visible_lines,
        inner_width,
        state.show_custom_emoji(),
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block("Reacted users", true)),
        popup,
    );
    render_vertical_scrollbar(
        frame,
        Rect {
            height: max_visible_lines as u16,
            ..panel_scrollbar_area(popup)
        },
        popup_state.scroll(),
        max_visible_lines,
        popup_state.data_line_count(),
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn reaction_users_popup_lines(
    reactions: &[ReactionUsersInfo],
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    reaction_users_popup_lines_with_custom_emoji_images(
        reactions,
        scroll,
        max_visible_lines,
        inner_width,
        true,
    )
}

fn reaction_users_popup_lines_with_custom_emoji_images(
    reactions: &[ReactionUsersInfo],
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    let data_lines = reaction_users_popup_data_lines(reactions, show_custom_emoji);
    let visible_lines = max_visible_lines.min(data_lines.len());
    let scroll = scroll.min(data_lines.len().saturating_sub(visible_lines));
    data_lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .map(|line| truncate_line_to_display_width(line, inner_width))
        .collect()
}

fn reaction_users_popup_data_lines(
    reactions: &[ReactionUsersInfo],
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if reactions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No reactions found",
            Style::default().fg(DIM),
        )));
    }

    for reaction in reactions {
        let count = reaction.users.len();
        let user_label = if count == 1 { "user" } else { "users" };
        lines.push(Line::from(Span::styled(
            format!(
                "{} · {count} {user_label}",
                reaction_emoji_label(&reaction.emoji, show_custom_emoji)
            ),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        if reaction.users.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no users found",
                Style::default().fg(DIM),
            )));
        } else {
            lines.extend(
                reaction
                    .users
                    .iter()
                    .map(|user| Line::from(Span::raw(format!("  {}", user.display_name)))),
            );
        }
    }
    lines
}

fn reaction_emoji_label(emoji: &crate::discord::ReactionEmoji, show_custom_emoji: bool) -> String {
    match emoji {
        crate::discord::ReactionEmoji::Custom { id, .. } if !show_custom_emoji => {
            id.get().to_string()
        }
        _ => emoji.status_label(),
    }
}

#[cfg(test)]
pub(in crate::tui::ui) fn emoji_reaction_picker_lines(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
) -> Vec<Line<'static>> {
    emoji_reaction_picker_lines_with_custom_emoji_images(
        reactions,
        selected,
        max_visible_items,
        thumbnail_urls,
        true,
        None,
        usize::MAX,
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn emoji_reaction_picker_lines_for_width(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
    width: usize,
) -> Vec<Line<'static>> {
    emoji_reaction_picker_lines_with_custom_emoji_images(
        reactions,
        selected,
        max_visible_items,
        thumbnail_urls,
        true,
        None,
        width,
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn filtered_emoji_reaction_picker_lines(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
    filter: &str,
) -> Vec<Line<'static>> {
    emoji_reaction_picker_lines_with_custom_emoji_images(
        reactions,
        selected,
        max_visible_items,
        thumbnail_urls,
        true,
        Some(filter),
        usize::MAX,
    )
}

fn emoji_reaction_picker_lines_with_custom_emoji_images(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
    show_custom_emoji: bool,
    filter: Option<&str>,
    max_width: usize,
) -> Vec<Line<'static>> {
    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_items = max_visible_items.max(1).min(reactions.len().max(1));
    let visible_range = selection::visible_item_range(reactions.len(), selected, visible_items);

    let mut lines: Vec<Line<'static>> = reactions[visible_range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, reaction)| {
            let index = visible_range.start + offset;
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(indexed_shortcut(index));
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let thumbnail_ready = show_custom_emoji
                && reaction
                    .custom_image_url()
                    .is_some_and(|url| thumbnail_urls.iter().any(|ready| ready == &url));
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(
                    format_emoji_reaction_item(reaction, thumbnail_ready, show_custom_emoji),
                    style,
                ),
            ])
        })
        .collect();

    if reactions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no matching reactions",
            Style::default().fg(DIM),
        )));
    }

    if let Some(filter) = filter {
        lines.push(Line::from(vec![
            Span::styled("Filter ", Style::default().fg(DIM)),
            Span::styled(
                format!("/{filter}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if max_width == usize::MAX {
        lines
    } else {
        lines
            .into_iter()
            .map(|line| truncate_line_to_display_width(line, max_width))
            .collect()
    }
}

fn render_emoji_reaction_images(
    frame: &mut Frame,
    area: Rect,
    reactions: &[EmojiReactionItem],
    selected: usize,
    visible_items: usize,
    emoji_images: Vec<EmojiImage<'_>>,
) {
    if area.width <= EMOJI_REACTION_IMAGE_WIDTH || area.height == 0 {
        return;
    }

    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_range = selection::visible_item_range(reactions.len(), selected, visible_items);
    for (offset, reaction) in reactions[visible_range].iter().enumerate() {
        let Some(url) = reaction.custom_image_url() else {
            continue;
        };
        let Some(image) = emoji_images.iter().find(|image| image.url == url) else {
            continue;
        };
        let y = area
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= area.y.saturating_add(area.height.saturating_sub(1)) {
            continue;
        }
        let image_area = Rect::new(
            area.x.saturating_add(2),
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(area.width.saturating_sub(2)),
            1,
        );
        frame.render_widget(RatatuiImage::new(image.protocol), image_area);
    }
}

fn format_emoji_reaction_item(
    reaction: &EmojiReactionItem,
    thumbnail_ready: bool,
    show_custom_emoji: bool,
) -> String {
    match &reaction.emoji {
        crate::discord::ReactionEmoji::Unicode(emoji) => format!("{} {}", emoji, reaction.label),
        crate::discord::ReactionEmoji::Custom { id, .. } if !show_custom_emoji => {
            format!("{} {}", id.get(), reaction.label)
        }
        crate::discord::ReactionEmoji::Custom { .. } if thumbnail_ready => format!(
            "{}{}",
            " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))),
            reaction.label
        ),
        crate::discord::ReactionEmoji::Custom { name, .. } => name
            .as_deref()
            .map(|name| format!(":{name}: {}", reaction.label))
            .unwrap_or_else(|| format!(":custom: {}", reaction.label)),
    }
}
