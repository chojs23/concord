use ratatui::{
    Frame,
    layout::{Alignment, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::Image as RatatuiImage;
use unicode_width::UnicodeWidthStr;

use crate::discord::{
    ActivityInfo, ActivityKind, ChannelUnreadState, MessageState, PresenceStatus,
};

use super::super::{
    format::{
        sanitize_for_display_width, truncate_display_width, truncate_display_width_from,
        truncate_text,
    },
    message_format::{EMOJI_REACTION_IMAGE_WIDTH, format_attachment_summary, wrap_text_lines},
    state::{
        ChannelPaneEntry, DashboardState, EmojiPickerEntry, FocusPane, GuildPaneEntry,
        MAX_MENTION_PICKER_VISIBLE, MemberEntry, MemberGroup, MentionPickerEntry, discord_color,
        folder_color, presence_color, presence_marker,
    },
};
use super::{
    active_text_style, channel_prefix, channel_unread_decoration, dm_presence_dot_span,
    highlight_style,
    layout::{composer_inner_width, panel_scrollbar_area},
    panel_block, panel_block_line, panel_content_height, render_vertical_scrollbar,
    selection_marker, styled_list_item,
    types::{ACCENT, DIM, EmojiReactionImage, MessageAreas},
};

pub(super) fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let entries = state.visible_guild_pane_entries();
    let max_width = area.width.saturating_sub(6) as usize;
    let horizontal_scroll = state.guild_horizontal_scroll();
    let selected = state.focused_guild_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = state.is_active_guild_entry(entry);
            styled_list_item(
                match entry {
                    GuildPaneEntry::DirectMessages => {
                        let base_style = active_text_style(
                            is_active,
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        );
                        let unread_count = state.direct_message_unread_count();
                        let badge = (unread_count > 0).then(|| {
                            notification_count_badge(ChannelUnreadState::Notified(
                                u32::try_from(unread_count).unwrap_or(u32::MAX),
                            ))
                        });
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width.saturating_sub(badge_width);
                        let mut spans = vec![selection_marker(is_selected)];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                entry.label(),
                                horizontal_scroll,
                                label_width,
                            ),
                            base_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                    GuildPaneEntry::FolderHeader { folder, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let icon = if *collapsed { "📁" } else { "📂" };
                        let color = folder_color(folder.color);
                        let label = folder.name.as_deref().unwrap_or_default();
                        let title = if label.is_empty() {
                            icon.to_owned()
                        } else {
                            format!("{icon} {label}")
                        };
                        let label_width = max_width.saturating_sub(arrow.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(color)),
                            Span::styled(
                                truncate_display_width_from(&title, horizontal_scroll, label_width),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    GuildPaneEntry::Guild {
                        state: guild,
                        branch,
                    } => {
                        let prefix = branch.prefix();
                        let base_style = active_text_style(is_active, Style::default());
                        let is_muted = dashboard.guild_notification_muted(guild.id);
                        let unread = dashboard.sidebar_guild_unread(guild.id);
                        let (badge, mut name_style) = if is_active {
                            let (badge, _) = channel_unread_decoration(unread, base_style, false);
                            (badge, base_style)
                        } else if unread == ChannelUnreadState::Seen {
                            (None, base_style)
                        } else {
                            channel_unread_decoration(unread, base_style, false)
                        };
                        if is_muted {
                            name_style = name_style.add_modifier(Modifier::DIM);
                        }
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(prefix.width())
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                guild.name.as_str(),
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("Servers", state.focus() == FocusPane::Guilds))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.guild_scroll(),
        panel_content_height(area, "Servers"),
        state.guild_pane_entries().len(),
    );
}

fn notification_count_badge(unread: ChannelUnreadState) -> Span<'static> {
    let (badge, _) = channel_unread_decoration(unread, Style::default(), false);
    badge.expect("numeric unread state always renders a badge")
}

pub(super) fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let entries = state.visible_channel_pane_entries();
    let max_width = area.width.saturating_sub(8) as usize;
    let horizontal_scroll = state.channel_horizontal_scroll();
    let selected = state.focused_channel_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = dashboard.is_active_channel_entry(entry);
            styled_list_item(
                match entry {
                    ChannelPaneEntry::CategoryHeader { state, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let label_width = max_width.saturating_sub(arrow.width());
                        let mut label_style =
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
                        if dashboard.channel_notification_muted(state.id) {
                            label_style = label_style.add_modifier(Modifier::DIM);
                        }
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(ACCENT)),
                            Span::styled(
                                truncate_display_width_from(
                                    &state.name,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                label_style,
                            ),
                        ]))
                    }
                    ChannelPaneEntry::Channel { state, branch } => {
                        let branch_prefix = branch.prefix();
                        let prefix_span = dm_presence_dot_span(state).unwrap_or_else(|| {
                            Span::styled(channel_prefix(&state.kind), Style::default().fg(DIM))
                        });
                        let prefix_width = prefix_span.content.width();
                        let base_style = active_text_style(is_active, Style::default());
                        let is_muted = dashboard.channel_notification_muted(state.id);
                        let unread = dashboard.sidebar_channel_unread(state.id);
                        let (badge, mut name_style) =
                            channel_unread_decoration(unread, base_style, is_active);
                        if is_muted {
                            name_style = name_style.add_modifier(Modifier::DIM);
                        }
                        let badge = if state.guild_id.is_none()
                            && !is_active
                            && unread != ChannelUnreadState::Seen
                        {
                            let message_count = dashboard.channel_unread_message_count(state.id);
                            if message_count > 0 {
                                let count = u32::try_from(message_count).unwrap_or(u32::MAX);
                                Some(notification_count_badge(ChannelUnreadState::Notified(
                                    count,
                                )))
                            } else if unread == ChannelUnreadState::Unread {
                                Some(notification_count_badge(ChannelUnreadState::Notified(1)))
                            } else {
                                badge
                            }
                        } else {
                            badge
                        };
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix_width)
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(prefix_span);
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                &state.name,
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items)
        .block(panel_block(
            "Channels",
            state.focus() == FocusPane::Channels,
        ))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.channel_scroll(),
        panel_content_height(area, "Channels"),
        state.channel_pane_entries().len(),
    );
}

pub(super) fn render_composer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiReactionImage<'_>],
) {
    let inner_width = composer_inner_width(area.width);
    let ready_urls = ready_custom_emoji_urls(emoji_images);
    let prompt = composer_lines_with_loaded_custom_emoji_urls(state, inner_width, &ready_urls);
    let border_color = if state.is_composing() { ACCENT } else { DIM };

    frame.render_widget(
        Paragraph::new(prompt)
            .style(if state.is_composing() {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(DIM)
            })
            .block(
                Block::default()
                    .title(" Message Input ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(Color::White).bold()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
    if state.show_custom_emoji() {
        render_composer_custom_emoji_images(frame, area, state, emoji_images);
    }
    if let Some(position) =
        composer_cursor_position_with_loaded_custom_emoji_urls(area, state, &ready_urls)
    {
        frame.set_cursor_position(position);
    }
}

fn ready_custom_emoji_urls(emoji_images: &[EmojiReactionImage<'_>]) -> Vec<String> {
    emoji_images.iter().map(|image| image.url.clone()).collect()
}

#[cfg(test)]
pub(super) fn composer_cursor_position(area: Rect, state: &DashboardState) -> Option<Position> {
    composer_cursor_position_with_loaded_custom_emoji_urls(area, state, &[])
}

fn composer_cursor_position_with_loaded_custom_emoji_urls(
    area: Rect,
    state: &DashboardState,
    loaded_custom_emoji_urls: &[String],
) -> Option<Position> {
    if !state.is_composing() || area.width < 3 || area.height < 3 {
        return None;
    }

    let inner_width = composer_inner_width(area.width) as usize;
    let cursor = state.composer_cursor_byte_index();
    let display_input = composer_display_input(state, loaded_custom_emoji_urls);
    let display_cursor = display_input
        .map_byte_index(cursor)
        .min(display_input.input.len());
    let prompt_prefix = format!("> {}", &display_input.input[..display_cursor]);
    let wrapped = wrap_text_lines(&prompt_prefix, inner_width);
    let mut prompt_row = wrapped.len().saturating_sub(1);
    let mut prompt_column = wrapped.last().map(|line| line.width()).unwrap_or_default();
    if prompt_column >= inner_width {
        prompt_row = prompt_row.saturating_add(1);
        prompt_column = 0;
    }

    let mut content_row = state.pending_composer_attachments().len();
    if state.reply_target_message_state().is_some() {
        content_row = content_row.saturating_add(1);
    }
    content_row = content_row.saturating_add(prompt_row);

    let x = area
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(prompt_column).unwrap_or(u16::MAX));
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(u16::try_from(content_row).unwrap_or(u16::MAX));
    let inner_right = area.x.saturating_add(area.width.saturating_sub(1));
    let inner_bottom = area.y.saturating_add(area.height.saturating_sub(1));
    if x >= inner_right || y >= inner_bottom {
        return None;
    }

    Some(Position { x, y })
}

pub(super) fn render_composer_mention_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
) {
    if state.composer_mention_query().is_none() {
        return;
    }
    let candidates = state.composer_mention_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(area) = mention_picker_area(message_areas, candidates.len()) else {
        return;
    };
    frame.render_widget(Clear, area);
    let visible_count = picker_visible_count(area, candidates.len());
    let selected = state.composer_mention_selected().min(candidates.len() - 1);
    let window_start = picker_window_start(candidates.len(), selected, visible_count);
    let visible_candidates = &candidates[window_start..window_start + visible_count];
    let shows_scrollbar = candidates.len() > visible_count;
    let inner_width = picker_inner_width(area, shows_scrollbar);
    let lines = mention_picker_lines(
        visible_candidates,
        selected.saturating_sub(window_start),
        inner_width,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" mention ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), area);
    render_picker_scrollbar(frame, area, window_start, visible_count, candidates.len());
}

pub(super) fn render_composer_emoji_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
    emoji_images: &[EmojiReactionImage<'_>],
) {
    if state.composer_emoji_query().is_none() {
        return;
    }
    let candidates = state.composer_emoji_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(area) = mention_picker_area(message_areas, candidates.len()) else {
        return;
    };
    frame.render_widget(Clear, area);
    let visible_count = picker_visible_count(area, candidates.len());
    let selected = state.composer_emoji_selected().min(candidates.len() - 1);
    let window_start = picker_window_start(candidates.len(), selected, visible_count);
    let visible_candidates = &candidates[window_start..window_start + visible_count];
    let shows_scrollbar = candidates.len() > visible_count;
    let inner_width = picker_inner_width(area, shows_scrollbar);
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let lines = emoji_picker_lines(
        visible_candidates,
        selected.saturating_sub(window_start),
        inner_width,
        &ready_urls,
        state.show_custom_emoji(),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" emoji ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), area);
    if state.show_custom_emoji() {
        render_composer_emoji_picker_images(frame, area, visible_candidates, emoji_images);
    }
    render_picker_scrollbar(frame, area, window_start, visible_count, candidates.len());
}

/// Picks a rectangle directly above the composer for the picker. Returns
/// `None` when there isn't enough room (very short terminal) so the caller
/// can silently skip drawing.
fn mention_picker_area(message_areas: MessageAreas, candidate_count: usize) -> Option<Rect> {
    let composer = message_areas.composer;
    let messages = message_areas.list;
    if composer.x < messages.x || composer.width == 0 {
        return None;
    }
    // 1 row per candidate + 2 for the bordered block.
    let desired_height = (candidate_count.min(MAX_MENTION_PICKER_VISIBLE) as u16).saturating_add(2);
    let available_above = composer.y.saturating_sub(messages.y);
    let height = desired_height.min(available_above);
    if height < 3 {
        return None;
    }
    let width = composer.width.clamp(20, 48).min(messages.width);
    let x = composer.x;
    let y = composer.y.saturating_sub(height);
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn picker_visible_count(area: Rect, candidate_count: usize) -> usize {
    usize::from(area.height.saturating_sub(2))
        .min(candidate_count)
        .max(1)
}

fn picker_window_start(total: usize, selected: usize, visible_count: usize) -> usize {
    if total <= visible_count {
        return 0;
    }
    selected
        .saturating_add(1)
        .saturating_sub(visible_count)
        .min(total.saturating_sub(visible_count))
}

fn picker_inner_width(area: Rect, shows_scrollbar: bool) -> usize {
    area.width
        .saturating_sub(2)
        .saturating_sub(u16::from(shows_scrollbar)) as usize
}

fn render_picker_scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    visible_count: usize,
    total_count: usize,
) {
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        position,
        visible_count,
        total_count,
    );
}

fn mention_picker_lines(
    candidates: &[MentionPickerEntry],
    selected: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let max_label_width = width.saturating_sub(4).max(1);
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let cursor = if index == selected { "› " } else { "  " };
            let bot_marker = if entry.is_bot { " [BOT]" } else { "" };
            // Show the raw username next to the alias when they differ so the
            // user can see which row matched their query when they typed
            // against the username instead of the alias.
            let username_hint = entry
                .username
                .as_deref()
                .filter(|name| !name.eq_ignore_ascii_case(&entry.display_name))
                .map(|name| format!(" @{name}"))
                .unwrap_or_default();
            let label = format!("{}{bot_marker}{username_hint}", entry.display_name);
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = Style::default().fg(presence_color(entry.status));
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(cursor, Style::default().fg(ACCENT)),
                Span::styled(presence_marker(entry.status).to_string(), row_style),
                Span::styled(" ", row_style),
                Span::styled(label, row_style),
            ])
        })
        .collect()
}

pub(super) fn emoji_picker_lines(
    candidates: &[EmojiPickerEntry],
    selected: usize,
    width: usize,
    ready_custom_emoji_urls: &[String],
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let cursor = if index == selected { "› " } else { "  " };
            let custom_image_ready = show_custom_emoji
                && entry
                    .custom_image_url
                    .as_ref()
                    .is_some_and(|url| ready_custom_emoji_urls.iter().any(|ready| ready == url));
            let prefix_width = emoji_picker_entry_prefix_width(entry, custom_image_ready);
            let max_label_width = width.saturating_sub(2).saturating_sub(prefix_width).max(1);
            let label = format!(":{}: {}", entry.shortcode, entry.name);
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = if entry.available {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(DIM).add_modifier(Modifier::CROSSED_OUT)
            };
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let mut spans = vec![Span::styled(cursor, Style::default().fg(ACCENT))];
            spans.extend(emoji_picker_entry_prefix(
                entry,
                custom_image_ready,
                row_style,
            ));
            spans.push(Span::styled(label, row_style));
            Line::from(spans)
        })
        .collect()
}

fn emoji_picker_entry_prefix_width(entry: &EmojiPickerEntry, custom_image_ready: bool) -> usize {
    if entry.custom_image_url.is_some() {
        usize::from(custom_image_ready) * usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))
    } else {
        entry.emoji.as_str().width().saturating_add(1)
    }
}

fn emoji_picker_entry_prefix(
    entry: &EmojiPickerEntry,
    custom_image_ready: bool,
    row_style: Style,
) -> Vec<Span<'static>> {
    if entry.custom_image_url.is_some() {
        if custom_image_ready {
            vec![Span::styled(
                " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))),
                row_style,
            )]
        } else {
            Vec::new()
        }
    } else {
        vec![
            Span::styled(entry.emoji.clone(), row_style),
            Span::styled(" ", row_style),
        ]
    }
}

fn render_composer_emoji_picker_images(
    frame: &mut Frame,
    area: Rect,
    candidates: &[EmojiPickerEntry],
    emoji_images: &[EmojiReactionImage<'_>],
) {
    let content = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    if content.width <= EMOJI_REACTION_IMAGE_WIDTH || content.height == 0 {
        return;
    }

    for (offset, entry) in candidates.iter().enumerate() {
        let Some(url) = entry.custom_image_url.as_deref() else {
            continue;
        };
        let Some(image) = emoji_images.iter().find(|image| image.url == url) else {
            continue;
        };
        let y = content
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= content.y.saturating_add(content.height) {
            continue;
        }
        let image_area = Rect::new(
            content.x.saturating_add(2),
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(content.width.saturating_sub(2)),
            1,
        );
        if image_area.width > 0 {
            frame.render_widget(RatatuiImage::new(image.protocol), image_area);
        }
    }
}

#[cfg(test)]
pub(super) fn composer_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
    composer_lines_with_loaded_custom_emoji_urls(state, width, &[])
}

pub(super) fn composer_lines_with_loaded_custom_emoji_urls(
    state: &DashboardState,
    width: u16,
    loaded_custom_emoji_urls: &[String],
) -> Vec<Line<'static>> {
    if state.is_composing() {
        let mut lines = pending_upload_lines(state, width);
        let display_input = composer_display_input(state, loaded_custom_emoji_urls);
        let input = Line::from(format!("> {}", display_input.input));
        if let Some(message) = state.reply_target_message_state() {
            lines.push(Line::from(Span::styled(
                reply_target_hint(message, state, width),
                Style::default().fg(DIM),
            )));
        }
        lines.push(input);
        return lines;
    }

    vec![Line::from(composer_text(state, width))]
}

struct ComposerDisplayInput {
    input: String,
    replacements: Vec<ComposerEmojiReplacement>,
}

struct ComposerEmojiReplacement {
    start: usize,
    end: usize,
    new_start: usize,
    new_len: usize,
}

impl ComposerDisplayInput {
    fn map_byte_index(&self, position: usize) -> usize {
        let mut delta = 0isize;
        for replacement in &self.replacements {
            if position < replacement.start {
                break;
            }
            if position < replacement.end {
                let inside = position.saturating_sub(replacement.start);
                return replacement
                    .new_start
                    .saturating_add(inside.min(replacement.new_len));
            }
            delta += replacement.new_len as isize - (replacement.end - replacement.start) as isize;
        }

        if delta < 0 {
            position.saturating_sub(delta.unsigned_abs())
        } else {
            position.saturating_add(delta as usize)
        }
    }
}

fn composer_display_input(
    state: &DashboardState,
    loaded_custom_emoji_urls: &[String],
) -> ComposerDisplayInput {
    let original = state.composer_input();
    let mut completions = state.composer_emoji_image_completions();
    completions.sort_by_key(|completion| completion.byte_start);
    if completions.is_empty() || loaded_custom_emoji_urls.is_empty() {
        return ComposerDisplayInput {
            input: original.to_owned(),
            replacements: Vec::new(),
        };
    }

    let mut input = String::with_capacity(original.len());
    let mut cursor = 0usize;
    let mut replacements = Vec::new();
    for completion in completions {
        if completion.byte_end > original.len()
            || !original.is_char_boundary(completion.byte_start)
            || !original.is_char_boundary(completion.byte_end)
        {
            continue;
        }

        let start = completion.byte_start;
        let end = completion.byte_end;
        if start < cursor {
            continue;
        }

        input.push_str(&original[cursor..start]);
        let new_start = input.len();
        if loaded_custom_emoji_urls
            .iter()
            .any(|url| url == &completion.url)
        {
            let placeholder = " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH));
            input.push_str(&placeholder);
            replacements.push(ComposerEmojiReplacement {
                start,
                end,
                new_start,
                new_len: placeholder.len(),
            });
        } else {
            input.push_str(&original[start..end]);
        }
        cursor = end;
    }
    input.push_str(&original[cursor..]);

    ComposerDisplayInput {
        input,
        replacements,
    }
}

fn render_composer_custom_emoji_images(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiReactionImage<'_>],
) {
    if !state.is_composing() || area.width < 3 || area.height < 3 {
        return;
    }

    let ready_urls = ready_custom_emoji_urls(emoji_images);
    let display_input = composer_display_input(state, &ready_urls);
    let input = display_input.input.as_str();
    let inner_width = composer_inner_width(area.width) as usize;
    let mut content_row = state.pending_composer_attachments().len();
    if state.reply_target_message_state().is_some() {
        content_row = content_row.saturating_add(1);
    }

    for completion in state.composer_emoji_image_completions() {
        let Some(image) = emoji_images
            .iter()
            .find(|image| image.url == completion.url)
        else {
            continue;
        };
        let Some((row, column)) = composer_custom_emoji_image_position(
            input,
            display_input.map_byte_index(completion.byte_start),
            display_input.map_byte_index(completion.byte_end),
            inner_width,
        ) else {
            continue;
        };
        let x = area
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
        let y = area
            .y
            .saturating_add(1)
            .saturating_add(u16::try_from(content_row.saturating_add(row)).unwrap_or(u16::MAX));
        let inner_right = area.x.saturating_add(area.width.saturating_sub(1));
        let inner_bottom = area.y.saturating_add(area.height.saturating_sub(1));
        if x >= inner_right || y >= inner_bottom {
            continue;
        }
        let image_area = Rect::new(
            x,
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(inner_right.saturating_sub(x)),
            1,
        );
        if image_area.width > 0 {
            frame.render_widget(RatatuiImage::new(image.protocol), image_area);
        }
    }
}

fn composer_custom_emoji_image_position(
    input: &str,
    byte_start: usize,
    byte_end: usize,
    inner_width: usize,
) -> Option<(usize, usize)> {
    if inner_width == 0 || byte_start > byte_end || byte_end > input.len() {
        return None;
    }
    let before = format!("> {}", &input[..byte_start]);
    let through = format!("> {}", &input[..byte_end]);
    let before_wrapped = wrap_text_lines(&before, inner_width);
    let through_wrapped = wrap_text_lines(&through, inner_width);
    if before_wrapped.len() != through_wrapped.len() {
        return None;
    }
    Some((
        before_wrapped.len().saturating_sub(1),
        before_wrapped
            .last()
            .map(|line| line.width())
            .unwrap_or_default(),
    ))
}

fn pending_upload_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
    pending_upload_texts(state, width)
        .into_iter()
        .map(|label| Line::from(Span::styled(label, Style::default().fg(ACCENT))))
        .collect()
}

fn pending_upload_texts(state: &DashboardState, width: u16) -> Vec<String> {
    let max_width = usize::from(width).max(1);
    state
        .pending_composer_attachments()
        .iter()
        .map(|attachment| {
            let label = format!(
                "upload: {} ({})",
                attachment.filename,
                format_byte_size(attachment.size_bytes)
            );
            truncate_display_width(&label, max_width)
        })
        .collect()
}

fn format_byte_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn composer_text(state: &DashboardState, width: u16) -> String {
    if state.is_composing() {
        let mut lines = pending_upload_texts(state, width);
        let input = format!("> {}", state.composer_input());
        if let Some(message) = state.reply_target_message_state() {
            lines.push(reply_target_hint(message, state, width));
        }
        lines.push(input);
        return lines.join("\n");
    }

    if let Some(channel) = state.selected_channel_state() {
        let label = match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        };
        // Tell the user up-front if the keymap won't open the composer here,
        // so they don't repeatedly press `i` and wonder why nothing happens.
        if !state.can_send_in_selected_channel() {
            return format!("read-only · cannot send messages in {label}");
        }
        // SEND is allowed but ATTACH isn't — flag it so a future attachment
        // picker has a coherent UX, and the user knows uploads will be
        // refused before they try.
        if !state.can_attach_in_selected_channel() {
            return format!("press i to write in {label} (attachments disabled)");
        }
        return format!("press i to write in {label}");
    }

    "select a channel to write a message".to_owned()
}

fn reply_target_hint(message: &MessageState, state: &DashboardState, width: u16) -> String {
    const PREFIX: &str = "reply to ";
    let excerpt_width = usize::from(width).saturating_sub(PREFIX.width()).max(1);
    format!(
        "{PREFIX}{}",
        truncate_display_width(&reply_target_excerpt(message, state), excerpt_width)
    )
}

fn reply_target_excerpt(message: &MessageState, state: &DashboardState) -> String {
    if let Some(content) = message
        .content
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let rendered = state.render_user_mentions(message.guild_id, &message.mentions, content);
        return rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    }

    if !message.attachments.is_empty() {
        return format_attachment_summary(&message.attachments);
    }

    if message.content.is_some() {
        "<empty message>".to_owned()
    } else {
        "<message content unavailable>".to_owned()
    }
}

pub(super) fn render_members(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let groups = state.members_grouped();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let max_name_width = (area.width as usize).saturating_sub(6).max(8);
    let selected_line = state
        .focused_member_selection_line()
        .map(|line| line + state.member_scroll());
    let focused = state.focus() == FocusPane::Members;
    let mut line_index = 0usize;

    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "No members loaded yet.",
            Style::default().fg(DIM),
        )));
    }

    for group in &groups {
        if !lines.is_empty() {
            lines.push(Line::from(""));
            line_index += 1;
        }
        lines.push(member_group_header(group));
        line_index += 1;
        for member in &group.entries {
            let member = *member;
            let is_selected = focused && selected_line == Some(line_index);
            let marker_style = Style::default().fg(presence_color(member.status()));
            let name_style =
                member_name_style(member, state.member_role_color(member), is_selected);

            let display =
                member_display_label(member, state.member_horizontal_scroll(), max_name_width);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", presence_marker(member.status())),
                    marker_style,
                ),
                Span::styled(display, name_style),
            ]));
            line_index += 1;

            // Three-space indent + max_name_width matches the name row's
            // envelope so the activity row never overflows or wraps.
            if !matches!(
                member.status(),
                PresenceStatus::Offline | PresenceStatus::Unknown
            ) {
                let activities = state.user_activities(member.user_id());
                if let Some(summary) = primary_activity_summary(activities) {
                    let summary = sanitize_for_display_width(&summary);
                    let summary = truncate_display_width_from(
                        &summary,
                        state.member_horizontal_scroll(),
                        max_name_width,
                    );
                    lines.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(summary, Style::default().fg(DIM)),
                    ]));
                    line_index += 1;
                }
            }
        }
    }

    let lines: Vec<_> = lines
        .into_iter()
        .skip(state.member_scroll())
        .take(state.member_content_height())
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block_line(state.member_panel_title(), focused))
            .wrap(Wrap { trim: false }),
        area,
    );
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.member_scroll(),
        state.member_content_height(),
        state.member_line_count(),
    );
}

fn member_group_header(group: &MemberGroup<'_>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            group.label.clone(),
            Style::default()
                .fg(discord_color(group.color, DIM))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", group.entries.len()),
            Style::default().fg(DIM),
        ),
    ])
}

pub(super) fn member_name_style(
    member: MemberEntry<'_>,
    role_color: Option<u32>,
    is_selected: bool,
) -> Style {
    let mut style = Style::default().fg(discord_color(role_color, Color::White));
    if matches!(
        member.status(),
        PresenceStatus::Offline | PresenceStatus::Unknown
    ) {
        style = style.add_modifier(Modifier::DIM);
    }
    if member.is_bot() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if is_selected {
        style = style
            .bg(Color::Rgb(24, 54, 65))
            .add_modifier(Modifier::BOLD);
    }
    style
}

pub(super) fn member_display_label(
    member: MemberEntry<'_>,
    horizontal_scroll: usize,
    max_width: usize,
) -> String {
    let display_name = member.display_name();
    if !member.is_bot() {
        return truncate_display_width_from(&display_name, horizontal_scroll, max_width);
    }

    const BOT_SUFFIX: &str = " [bot]";
    let suffix_width = BOT_SUFFIX.width();
    if max_width <= suffix_width {
        return truncate_display_width_from(
            &format!("{}{}", display_name, BOT_SUFFIX),
            horizontal_scroll,
            max_width,
        );
    }

    format!(
        "{}{}",
        truncate_display_width_from(
            &display_name,
            horizontal_scroll,
            max_width.saturating_sub(suffix_width),
        ),
        BOT_SUFFIX
    )
}

/// Priority: Custom > Streaming > Listening > Playing > Watching > Competing > Unknown.
pub(super) fn primary_activity_summary(activities: &[ActivityInfo]) -> Option<String> {
    activities
        .iter()
        .min_by_key(|activity| activity_priority(activity.kind))
        .map(format_activity_summary)
}

fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Custom => 0,
        ActivityKind::Streaming => 1,
        ActivityKind::Listening => 2,
        ActivityKind::Playing => 3,
        ActivityKind::Watching => 4,
        ActivityKind::Competing => 5,
        ActivityKind::Unknown => 6,
    }
}

fn format_activity_summary(activity: &ActivityInfo) -> String {
    match activity.kind {
        ActivityKind::Custom => {
            let emoji = activity
                .emoji
                .as_ref()
                .map(|emoji| {
                    if emoji.id.is_some() {
                        format!(":{}:", emoji.name)
                    } else {
                        emoji.name.clone()
                    }
                })
                .unwrap_or_default();
            let body = activity.state.clone().unwrap_or_default();
            match (emoji.is_empty(), body.is_empty()) {
                (true, true) => activity.name.clone(),
                (false, true) => emoji,
                (true, false) => body,
                (false, false) => format!("{emoji} {body}"),
            }
        }
        ActivityKind::Playing => format!("Playing {}", activity.name),
        ActivityKind::Streaming => format!("Streaming {}", activity.name),
        ActivityKind::Listening => match (activity.details.as_deref(), activity.state.as_deref()) {
            (Some(track), Some(artist)) => {
                format!("Listening to {} — {} by {}", activity.name, track, artist)
            }
            (Some(track), None) => format!("Listening to {} — {}", activity.name, track),
            _ => format!("Listening to {}", activity.name),
        },
        ActivityKind::Watching => format!("Watching {}", activity.name),
        ActivityKind::Competing => format!("Competing in {}", activity.name),
        ActivityKind::Unknown => activity.name.clone(),
    }
}

pub(super) fn render_header(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    let mut spans = vec![Span::styled(title, Style::default().fg(Color::Cyan).bold())];
    if let Some(version) = state.update_available_version() {
        spans.push(Span::styled(
            format!(" New version available: v{version} "),
            Style::default().fg(Color::Yellow).bold(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
}

pub(super) fn render_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let user = footer_user_label(state);
    let mut spans = vec![
        Span::styled(
            format!(" {user} "),
            Style::default().fg(Color::Green).bold(),
        ),
        Span::styled(footer_hint(state), Style::default().fg(DIM)),
    ];
    if let Some(status) = state.last_status() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            truncate_text(status, 72),
            Style::default().fg(Color::Green),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
}

pub(super) fn footer_user_label(state: &DashboardState) -> &str {
    state.current_user().unwrap_or("Loading Concord...")
}

pub(super) fn footer_hint(state: &DashboardState) -> String {
    if state.is_debug_log_popup_open() {
        "`/esc close debug logs".to_owned()
    } else if state.is_reaction_users_popup_open() {
        "esc close reacted users".to_owned()
    } else if state.is_poll_vote_picker_open() {
        "j/k choose answer | space toggle | enter vote | esc close".to_owned()
    } else if state.is_emoji_reaction_picker_open() {
        "j/k choose emoji | enter/space react | esc close".to_owned()
    } else if state.is_image_viewer_action_menu_open() {
        "enter/space download image | esc close menu".to_owned()
    } else if state.is_image_viewer_open() {
        "h/← previous image | l/→ next image | enter/space actions | esc close".to_owned()
    } else if state.is_user_profile_popup_open() {
        "j/k pick mutual server | enter open server | esc close".to_owned()
    } else if state.is_message_action_menu_open()
        || state.is_guild_action_menu_open()
        || state.is_member_action_menu_open()
    {
        if state.is_guild_action_mute_duration_phase() {
            "j/k choose duration | enter select | esc back | q quit".to_owned()
        } else {
            "j/k choose action | enter select | esc close | q quit".to_owned()
        }
    } else if state.is_channel_action_menu_open() {
        if state.is_channel_action_threads_phase() {
            "j/k choose thread | enter open | esc/← back | q quit".to_owned()
        } else if state.is_channel_action_mute_duration_phase() {
            "j/k choose duration | enter select | esc/← back | q quit".to_owned()
        } else {
            "j/k choose action | enter select | esc close | q quit".to_owned()
        }
    } else if state.focus() == FocusPane::Members {
        "tab/shift+tab/1-4 focus | alt+h/l/←/→ width | j/k move | H/L scroll name | enter profile | space leader | i write | q quit".to_owned()
    } else if state.focus() == FocusPane::Channels {
        "tab/shift+tab/1-4 focus | alt+h/l/←/→ width | j/k move | H/L scroll name | enter open | space leader | h/← close | l/→ open | ` logs | i write | q quit".to_owned()
    } else if state.focus() == FocusPane::Guilds {
        "tab/shift+tab/1-4 focus | alt+h/l/←/→ width | j/k move | J/K scroll | H/L scroll name | enter action/tree | space leader | h/← close | l/→ open | ` logs | i write | esc cancel | q quit".to_owned()
    } else {
        "tab/shift+tab/1-4 focus | j/k move | J/K scroll | enter actions | space leader | ` logs | i write | esc cancel | q quit".to_owned()
    }
}
