use super::*;
use crate::tui::selection;
use crate::tui::state::{ReactionUsersEntry, ReactionUsersPopupState};
use crate::tui::ui::emoji_overlay::overlay_emoji_column;

pub(in crate::tui::ui) fn render_emoji_reaction_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker) {
        return;
    }

    let reactions = state.filtered_emoji_reaction_items_slice().unwrap_or(&[]);
    if reactions.is_empty() && !state.is_filtering_emoji_reactions() {
        return;
    }
    let filter = state.emoji_reaction_filter();
    let own_reactions = state.own_emoji_reactions();

    let selected = state
        .selected_emoji_reaction_index_for_len(reactions.len())
        .unwrap_or(0);
    let popup = emoji_reaction_picker_popup_area(area, reactions.len(), filter.is_some());
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    let visible_items =
        emoji_reaction_picker_visible_items_for_area(area, reactions.len(), filter.is_some());
    let scroll = state.emoji_reaction_picker_scroll();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(emoji_reaction_picker_lines_with_custom_emoji_images(
            reactions,
            selected,
            EmojiReactionPickerRenderOptions {
                key_bindings: state.key_bindings(),
                max_visible_items: visible_items,
                scroll,
                thumbnail_urls: &ready_urls,
                own_reactions,
                show_custom_emoji: state.show_custom_emoji(),
                filter,
                max_width: usize::from(content.width),
            },
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
            scroll,
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
        scroll,
        visible_items,
        reactions.len(),
    );
}

/// Width of the selection marker, where the custom-emoji image overlay begins.
const REACTION_LIST_IMAGE_X_OFFSET: u16 = 2;

pub(in crate::tui::ui) fn render_reaction_users_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ReactionUsers) {
        return;
    }

    let Some(popup_state) = state.reaction_users_popup() else {
        return;
    };

    // Pre-truncate every line to the eventual inner width. Without this,
    // ratatui's `Wrap` splits a long username across rows and the continuation
    // overlaps neighbouring lines (the trailing-fragment artefact).
    let popup_width = REACTION_USERS_POPUP_TARGET_WIDTH
        .min(area.width.saturating_sub(2))
        .max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));
    let max_visible = reaction_users_visible_line_count(area);

    if let Some(entry) = popup_state.viewed_entry() {
        render_reaction_user_list(
            frame,
            area,
            state,
            popup_state,
            entry,
            inner_width,
            max_visible,
        );
    } else {
        render_reaction_list(
            frame,
            area,
            state,
            popup_state,
            emoji_images,
            inner_width,
            max_visible,
        );
    }
}

fn render_reaction_list(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    popup_state: &ReactionUsersPopupState,
    emoji_images: &[EmojiImage<'_>],
    inner_width: usize,
    max_visible: usize,
) {
    let ready_urls: Vec<String> = emoji_images.iter().map(|image| image.url.clone()).collect();
    let lines = reaction_list_lines(
        popup_state.entries(),
        popup_state.list_selected(),
        popup_state.list_scroll(),
        max_visible,
        state.show_custom_emoji(),
        &ready_urls,
        inner_width,
    );

    let popup = reaction_users_popup_area(area, lines.len());
    frame.render_widget(Clear, popup);
    let block = panel_block("Reactions", true);
    let content = block.inner(popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);

    if state.show_custom_emoji() {
        render_reaction_list_images(
            frame,
            content,
            popup_state.entries(),
            popup_state.list_scroll(),
            max_visible,
            emoji_images,
        );
    }

    render_vertical_scrollbar(
        frame,
        Rect {
            height: max_visible as u16,
            ..panel_scrollbar_area(popup)
        },
        popup_state.list_scroll(),
        max_visible,
        popup_state.entries().len(),
    );
}

fn render_reaction_user_list(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    popup_state: &ReactionUsersPopupState,
    entry: &ReactionUsersEntry,
    inner_width: usize,
    max_visible: usize,
) {
    let title = format!(
        "{} · {}",
        reaction_emoji_label(entry.emoji(), state.show_custom_emoji()),
        entry.count()
    );
    let lines = reaction_user_lines(
        popup_state,
        popup_state.user_scroll(),
        max_visible,
        inner_width,
    );

    let popup = reaction_users_popup_area(area, lines.len());
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block_owned(title, true)),
        popup,
    );
    // Pad the scrollbar total by one while more pages remain so the bar never
    // quite reaches the bottom, hinting that scrolling further loads more.
    let scrollbar_total = popup_state.user_line_count() + usize::from(entry.has_more());
    render_vertical_scrollbar(
        frame,
        Rect {
            height: max_visible as u16,
            ..panel_scrollbar_area(popup)
        },
        popup_state.user_scroll(),
        max_visible,
        scrollbar_total,
    );
}

pub(in crate::tui::ui) fn emoji_reaction_picker_popup_area(
    area: Rect,
    reaction_count: usize,
    has_filter: bool,
) -> Rect {
    let desired_visible_items = emoji_reaction_picker_visible_items(reaction_count);
    let extra_lines = u16::from(has_filter);
    centered_rect(
        area,
        42,
        (desired_visible_items as u16)
            .saturating_add(extra_lines)
            .saturating_add(2),
    )
}

pub(in crate::tui::ui) fn emoji_reaction_picker_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let reactions = state.filtered_emoji_reaction_items_slice().unwrap_or(&[]);
    if reactions.is_empty() && !state.is_filtering_emoji_reactions() {
        return None;
    }
    Some(emoji_reaction_picker_popup_area(
        area,
        reactions.len(),
        state.emoji_reaction_filter().is_some(),
    ))
}

fn emoji_reaction_picker_visible_items(reaction_count: usize) -> usize {
    reaction_count.clamp(1, selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS)
}

pub(in crate::tui::ui) fn emoji_reaction_picker_visible_items_for_area(
    area: Rect,
    reaction_count: usize,
    has_filter: bool,
) -> usize {
    let desired = emoji_reaction_picker_visible_items(reaction_count);
    let popup = emoji_reaction_picker_popup_area(area, reaction_count, has_filter);
    let content = panel_block("Choose reaction", true).inner(popup);
    let filter_lines = u16::from(has_filter);
    usize::from(content.height.saturating_sub(filter_lines)).min(desired)
}

const REACTION_USERS_POPUP_TARGET_WIDTH: u16 = 58;

pub(in crate::tui::ui) fn reaction_users_popup_area(area: Rect, line_count: usize) -> Rect {
    centered_rect(
        area,
        REACTION_USERS_POPUP_TARGET_WIDTH,
        (line_count as u16).saturating_add(2),
    )
}

pub(in crate::tui::ui) fn reaction_users_popup_area_for_state(
    area: Rect,
    state: &DashboardState,
) -> Option<Rect> {
    let popup_state = state.reaction_users_popup()?;
    let popup_width = REACTION_USERS_POPUP_TARGET_WIDTH
        .min(area.width.saturating_sub(2))
        .max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));
    let max_visible = reaction_users_visible_line_count(area);
    let line_count = if popup_state.is_viewing_users() {
        reaction_user_lines(
            popup_state,
            popup_state.user_scroll(),
            max_visible,
            inner_width,
        )
        .len()
    } else {
        reaction_list_lines(
            popup_state.entries(),
            popup_state.list_selected(),
            popup_state.list_scroll(),
            max_visible,
            state.show_custom_emoji(),
            &[],
            inner_width,
        )
        .len()
    };
    Some(reaction_users_popup_area(area, line_count))
}

#[cfg(test)]
pub(in crate::tui::ui) fn reaction_users_popup_lines(
    popup: &ReactionUsersPopupState,
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    if popup.is_viewing_users() {
        reaction_user_lines(popup, scroll, max_visible_lines, inner_width)
    } else {
        reaction_list_lines(
            popup.entries(),
            popup.list_selected(),
            scroll,
            max_visible_lines,
            true,
            &[],
            inner_width,
        )
    }
}

#[cfg(test)]
pub(in crate::tui::ui) fn reaction_list_lines_with_ready_urls(
    popup: &ReactionUsersPopupState,
    ready_urls: &[String],
    inner_width: usize,
) -> Vec<Line<'static>> {
    reaction_list_lines(
        popup.entries(),
        popup.list_selected(),
        popup.list_scroll(),
        usize::MAX,
        true,
        ready_urls,
        inner_width,
    )
}

#[allow(clippy::too_many_arguments)]
fn reaction_list_lines(
    entries: &[ReactionUsersEntry],
    selected: usize,
    scroll: usize,
    max_visible: usize,
    show_custom_emoji: bool,
    ready_urls: &[String],
    inner_width: usize,
) -> Vec<Line<'static>> {
    if entries.is_empty() {
        return vec![Line::from(Span::styled(
            "No reactions found",
            Style::default().fg(DIM),
        ))];
    }

    let range = selection::visible_window(scroll, max_visible, entries.len());
    entries[range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, entry)| {
            let index = range.start + offset;
            let is_selected = index == selected;
            let marker = if is_selected { "› " } else { "  " };
            let cell = reaction_emoji_cell(entry.emoji(), show_custom_emoji, ready_urls);
            let mut style = Style::default();
            if is_selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let line = Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{cell} {}", entry.count()), style),
            ]);
            truncate_line_to_display_width(line, inner_width)
        })
        .collect()
}

/// A ready custom-emoji thumbnail becomes a blank cell of the image's width so
/// the overlaid image sits over it. Everything else renders as text.
fn reaction_emoji_cell(
    emoji: &crate::discord::ReactionEmoji,
    show_custom_emoji: bool,
    ready_urls: &[String],
) -> String {
    let thumbnail_ready = show_custom_emoji
        && emoji
            .custom_image_url()
            .is_some_and(|url| ready_urls.iter().any(|ready| ready == &url));
    if thumbnail_ready {
        " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH))
    } else {
        reaction_emoji_label(emoji, show_custom_emoji)
    }
}

fn render_reaction_list_images(
    frame: &mut Frame,
    content: Rect,
    entries: &[ReactionUsersEntry],
    scroll: usize,
    max_visible: usize,
    emoji_images: &[EmojiImage<'_>],
) {
    let range = selection::visible_window(scroll, max_visible, entries.len());
    overlay_emoji_column(
        frame,
        content,
        REACTION_LIST_IMAGE_X_OFFSET,
        entries[range]
            .iter()
            .map(|entry| entry.emoji().custom_image_url()),
        emoji_images,
    );
}

fn reaction_user_lines(
    popup: &ReactionUsersPopupState,
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let data_lines = reaction_user_data_lines(popup);
    let visible_lines = max_visible_lines.min(data_lines.len());
    let scroll = scroll.min(data_lines.len().saturating_sub(visible_lines));
    data_lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .map(|line| truncate_line_to_display_width(line, inner_width))
        .collect()
}

fn reaction_user_data_lines(popup: &ReactionUsersPopupState) -> Vec<Line<'static>> {
    let Some(entry) = popup.viewed_entry() else {
        return vec![Line::from(Span::styled(
            "No users found",
            Style::default().fg(DIM),
        ))];
    };
    if entry.users().is_empty() {
        let text = if entry.is_loading() || !entry.loaded_once() {
            "  loading…"
        } else {
            "  no users found"
        };
        return vec![Line::from(Span::styled(text, Style::default().fg(DIM)))];
    }
    entry
        .users()
        .iter()
        .map(|user| Line::from(Span::raw(format!("  {}", user.display_name))))
        .collect()
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
    scroll: usize,
    thumbnail_urls: &[String],
) -> Vec<Line<'static>> {
    emoji_reaction_picker_lines_with_custom_emoji_images(
        reactions,
        selected,
        EmojiReactionPickerRenderOptions {
            key_bindings: &crate::tui::keybindings::KeyBindings::default(),
            max_visible_items,
            scroll,
            thumbnail_urls,
            own_reactions: &[],
            show_custom_emoji: true,
            filter: None,
            max_width: usize::MAX,
        },
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
        EmojiReactionPickerRenderOptions {
            key_bindings: &crate::tui::keybindings::KeyBindings::default(),
            max_visible_items,
            scroll: 0,
            thumbnail_urls,
            own_reactions: &[],
            show_custom_emoji: true,
            filter: None,
            max_width: width,
        },
    )
}

#[cfg(test)]
pub(in crate::tui::ui) fn emoji_reaction_picker_lines_with_own_reactions(
    reactions: &[EmojiReactionItem],
    own_reactions: &[crate::discord::ReactionEmoji],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
) -> Vec<Line<'static>> {
    emoji_reaction_picker_lines_with_custom_emoji_images(
        reactions,
        selected,
        EmojiReactionPickerRenderOptions {
            key_bindings: &crate::tui::keybindings::KeyBindings::default(),
            max_visible_items,
            scroll: 0,
            thumbnail_urls,
            own_reactions,
            show_custom_emoji: true,
            filter: None,
            max_width: usize::MAX,
        },
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
        EmojiReactionPickerRenderOptions {
            key_bindings: &crate::tui::keybindings::KeyBindings::default(),
            max_visible_items,
            scroll: 0,
            thumbnail_urls,
            own_reactions: &[],
            show_custom_emoji: true,
            filter: Some(filter),
            max_width: usize::MAX,
        },
    )
}

struct EmojiReactionPickerRenderOptions<'a> {
    key_bindings: &'a crate::tui::keybindings::KeyBindings,
    max_visible_items: usize,
    scroll: usize,
    thumbnail_urls: &'a [String],
    own_reactions: &'a [crate::discord::ReactionEmoji],
    show_custom_emoji: bool,
    filter: Option<&'a str>,
    max_width: usize,
}

fn emoji_reaction_picker_lines_with_custom_emoji_images(
    reactions: &[EmojiReactionItem],
    selected: usize,
    options: EmojiReactionPickerRenderOptions<'_>,
) -> Vec<Line<'static>> {
    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_range =
        selection::visible_window(options.scroll, options.max_visible_items, reactions.len());

    let mut lines: Vec<Line<'static>> = reactions[visible_range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, reaction)| {
            let index = visible_range.start + offset;
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(
                options
                    .key_bindings
                    .emoji_reaction_shortcut(reactions, index),
            );
            let mut style = Style::default();
            if options.own_reactions.contains(&reaction.emoji) {
                style = style.fg(Color::Yellow);
            }
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let thumbnail_ready = options.show_custom_emoji
                && reaction
                    .custom_image_url()
                    .is_some_and(|url| options.thumbnail_urls.iter().any(|ready| ready == &url));
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(
                    format_emoji_reaction_item(
                        reaction,
                        thumbnail_ready,
                        options.show_custom_emoji,
                    ),
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

    if let Some(filter) = options.filter {
        lines.push(Line::from(vec![
            Span::styled("Filter ", Style::default().fg(DIM)),
            Span::styled(
                format!(
                    "{}{filter}",
                    options.key_bindings.emoji_reaction_filter_prefix()
                ),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if options.max_width == usize::MAX {
        lines
    } else {
        lines
            .into_iter()
            .map(|line| truncate_line_to_display_width(line, options.max_width))
            .collect()
    }
}

fn render_emoji_reaction_images(
    frame: &mut Frame,
    area: Rect,
    reactions: &[EmojiReactionItem],
    scroll: usize,
    visible_items: usize,
    emoji_images: &[EmojiImage<'_>],
) {
    let visible_range = selection::visible_window(scroll, visible_items, reactions.len());
    // The picker keeps its last content row clear, so shrink the draw area.
    let area = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    overlay_emoji_column(
        frame,
        area,
        emoji_reaction_image_x_offset(),
        reactions[visible_range]
            .iter()
            .map(EmojiReactionItem::custom_image_url),
        emoji_images,
    );
}

fn emoji_reaction_image_x_offset() -> u16 {
    2 + shortcut_prefix(Some('q')).width() as u16
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
