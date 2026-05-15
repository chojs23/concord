use ratatui::{
    Frame,
    layout::{Alignment, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};
use ratatui_image::Image as RatatuiImage;
use unicode_width::UnicodeWidthStr;

use crate::discord::{
    ActivityInfo, ActivityKind, ChannelUnreadState, MessageState, PresenceStatus,
};

use super::super::{
    format::{sanitize_for_display_width, truncate_display_width, truncate_display_width_from},
    message_format::{EMOJI_REACTION_IMAGE_WIDTH, format_attachment_summary, wrap_text_lines},
    state::{
        ChannelPaneEntry, DashboardState, EmojiPickerEntry, FocusPane, GuildPaneEntry,
        MAX_MENTION_PICKER_VISIBLE, MemberEntry, MemberGroup, MentionPickerEntry, discord_color,
        folder_color, presence_color, presence_marker,
    },
};
use super::{
    active_text_style,
    activity::{ActivityLeading, ActivityRender, build_activity_render},
    channel_prefix, channel_unread_decoration, dm_presence_dot_span, highlight_style,
    layout::{
        composer_inner_width, panel_scrollbar_area, prefixed_composer_input,
        vertical_scrollbar_visible,
    },
    panel_block, panel_block_line, panel_content_height, render_vertical_scrollbar,
    selection_marker, styled_list_item,
    types::{ACCENT, DIM, EmojiImage, MessageAreas},
};

pub(super) fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let focused = state.focus() == FocusPane::Guilds;
    let filter_query = state.guild_pane_filter_query();

    // When the filter is active split off one row at the bottom for the search
    // bar, rendering the border block separately so we can carve up the inner.
    let (list_area, filter_area) = if filter_query.is_some() && area.height >= 4 {
        let block = panel_block("Servers", focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let list_h = inner.height.saturating_sub(1);
        let list_rect = Rect {
            height: list_h,
            ..inner
        };
        let filter_rect = Rect {
            y: inner.y + list_h,
            height: 1,
            ..inner
        };
        (list_rect, Some(filter_rect))
    } else {
        (area, None)
    };

    let entries = state.visible_guild_pane_entries();
    let max_width = list_area.width.saturating_sub(6) as usize;
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

    let list = List::new(items).highlight_style(highlight_style());
    let list = if filter_area.is_none() {
        list.block(panel_block("Servers", focused))
    } else {
        list
    };
    frame.render_widget(list, list_area);

    if let Some(filter_rect) = filter_area {
        let query = filter_query.unwrap_or_default();
        let cursor = state.guild_pane_filter_cursor().unwrap_or(0);
        let cursor_x = render_pane_filter_bar(frame, filter_rect, query, cursor, focused);
        if focused {
            frame.set_cursor_position(Position {
                x: filter_rect.x.saturating_add(cursor_x as u16),
                y: filter_rect.y,
            });
        }
    }

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

/// Renders a one-line search bar at `area` and returns the visual column offset
/// of the cursor within that area (column 0 = leftmost cell of `area`).
fn render_pane_filter_bar(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    cursor_byte: usize,
    focused: bool,
) -> usize {
    let prompt = "/ ";
    let prompt_width = prompt.width();
    let available = (area.width as usize).saturating_sub(prompt_width).max(1);

    // Scroll the visible window so the cursor is always in view.
    let cursor_byte = cursor_byte.min(query.len());
    let mut start = 0usize;
    while query[start..cursor_byte].width() > available {
        // Advance start by one char boundary
        start = query[start..]
            .char_indices()
            .nth(1)
            .map(|(off, _)| start + off)
            .unwrap_or(query.len());
    }
    let mut end = cursor_byte;
    while end < query.len() {
        let next = query[end..]
            .char_indices()
            .nth(1)
            .map(|(off, _)| end + off)
            .unwrap_or(query.len());
        if query[start..next].width() > available {
            break;
        }
        end = next;
    }
    let visible = &query[start..end];
    let cursor_col = prompt_width + query[start..cursor_byte].width();

    let accent = if focused { ACCENT } else { Color::DarkGray };
    let shown_query = if query.is_empty() {
        Span::styled("search...", Style::default().fg(DIM))
    } else {
        Span::raw(visible.to_owned())
    };
    let line = Line::from(vec![
        Span::styled(prompt, Style::default().fg(accent)),
        shown_query,
    ]);
    frame.render_widget(Paragraph::new(line), area);
    cursor_col
}

pub(super) fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let focused = state.focus() == FocusPane::Channels;
    let filter_query = state.channel_pane_filter_query();
    let block = panel_block("Channels", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let header_area = Rect {
        height: inner.height.min(1),
        ..inner
    };
    if header_area.height > 0 {
        let server_name = selected_channel_server_label(state);
        let label = truncate_display_width(&server_name, header_area.width as usize);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                label,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))),
            header_area,
        );
    }

    let channels_area = Rect {
        y: inner.y.saturating_add(header_area.height),
        height: inner.height.saturating_sub(header_area.height),
        ..inner
    };

    let (list_area, filter_area) = if filter_query.is_some() && channels_area.height >= 2 {
        let list_h = channels_area.height.saturating_sub(1);
        let list_rect = Rect {
            height: list_h,
            ..channels_area
        };
        let filter_rect = Rect {
            y: channels_area.y + list_h,
            height: 1,
            ..channels_area
        };
        (list_rect, Some(filter_rect))
    } else {
        (channels_area, None)
    };

    let channel_entry_count = state.channel_pane_filtered_entries().len();
    let all_channel_entries = state.channel_pane_entries();
    let entries = state.visible_channel_pane_entries();
    let scrollbar_width = usize::from(vertical_scrollbar_visible(
        list_area,
        list_area.height as usize,
        channel_entry_count,
    ));
    let max_width = (list_area.width as usize)
        .saturating_sub(selection_marker(false).content.width())
        .saturating_sub(scrollbar_width);
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
                        let dm_prefix_span = dm_presence_dot_span(state);
                        let channel_prefix = channel_prefix(&state.kind);
                        let prefix_width = dm_prefix_span
                            .as_ref()
                            .map_or_else(|| channel_prefix.width(), |span| span.content.width());
                        let populated_voice_channel = state.is_voice()
                            && all_channel_entries.windows(2).any(|window| {
                                matches!(
                                    (&window[0], &window[1]),
                                    (
                                        ChannelPaneEntry::Channel { state: channel, .. },
                                        ChannelPaneEntry::VoiceParticipant { .. }
                                    ) if channel.id == state.id
                                )
                            });
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
                        if let Some(prefix_span) = dm_prefix_span {
                            spans.push(prefix_span);
                        } else if populated_voice_channel {
                            spans.push(Span::styled("🔊", Style::default().fg(Color::Cyan)));
                            spans.push(Span::styled(" ", Style::default().fg(DIM)));
                        } else {
                            spans.push(Span::styled(channel_prefix, Style::default().fg(DIM)));
                        }
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
                    ChannelPaneEntry::VoiceParticipant {
                        participant,
                        parent_branch,
                        ..
                    } => {
                        let branch_prefix = parent_branch.participant_prefix();
                        let mut label = participant.display_name.clone();
                        if participant.self_stream {
                            label.push_str(" 🔴 LIVE");
                        }
                        if participant.mute || participant.self_mute {
                            label.push_str(" 🔇");
                        }
                        if participant.deaf || participant.self_deaf {
                            label.push_str(" 🎧");
                        }
                        let prefix = "  • ";
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(false),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                            Span::styled(prefix, Style::default().fg(DIM)),
                            Span::styled(
                                truncate_display_width_from(&label, horizontal_scroll, label_width),
                                Style::default().fg(DIM),
                            ),
                        ]))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items).highlight_style(highlight_style());
    frame.render_widget(list, list_area);

    if let Some(filter_rect) = filter_area {
        let query = filter_query.unwrap_or_default();
        let cursor = state.channel_pane_filter_cursor().unwrap_or(0);
        let cursor_x = render_pane_filter_bar(frame, filter_rect, query, cursor, focused);
        if focused {
            frame.set_cursor_position(Position {
                x: filter_rect.x.saturating_add(cursor_x as u16),
                y: filter_rect.y,
            });
        }
    }

    render_vertical_scrollbar(
        frame,
        list_area,
        state.channel_scroll(),
        list_area.height as usize,
        channel_entry_count,
    );
}

fn selected_channel_server_label(state: &DashboardState) -> String {
    state
        .selected_guild_id()
        .and_then(|guild_id| state.guild_name(guild_id))
        .unwrap_or("Direct Messages")
        .to_owned()
}

pub(super) fn render_composer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
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
                    .title(state.composer_title())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(Color::White).bold()),
            ),
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

fn ready_custom_emoji_urls(emoji_images: &[EmojiImage<'_>]) -> Vec<String> {
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
    let text_before_cursor = &display_input.input[..display_cursor];
    let prefixed = prefixed_composer_input(text_before_cursor);
    let wrapped = wrap_text_lines(&prefixed, inner_width);
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
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        window_start,
        visible_count,
        candidates.len(),
    );
}

pub(super) fn render_composer_emoji_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
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
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        window_start,
        visible_count,
        candidates.len(),
    );
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
    emoji_images: &[EmojiImage<'_>],
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
    if state.is_composing()
        || !state.composer_input().is_empty()
        || !state.pending_composer_attachments().is_empty()
    {
        let mut lines = pending_upload_lines(state, width);
        let display_input = composer_display_input(state, loaded_custom_emoji_urls);
        if state.is_composing()
            && let Some(message) = state.reply_target_message_state()
        {
            lines.push(Line::from(Span::styled(
                reply_target_hint(message, state, width),
                Style::default().fg(DIM),
            )));
        }
        let prefixed_input = prefixed_composer_input(&display_input.input);
        let wrapped = wrap_text_lines(&prefixed_input, width as usize);
        for subline in wrapped {
            lines.push(Line::from(subline));
        }
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
    emoji_images: &[EmojiImage<'_>],
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
    let before = prefixed_composer_input(&input[..byte_start]);
    let through = prefixed_composer_input(&input[..byte_end]);
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
        let input = prefixed_composer_input(state.composer_input());
        if let Some(message) = state.reply_target_message_state() {
            lines.push(reply_target_hint(message, state, width));
        }
        lines.push(input);
        return lines.join("\n");
    }

    if !state.composer_input().is_empty() || !state.pending_composer_attachments().is_empty() {
        let mut lines = pending_upload_texts(state, width);
        lines.push(prefixed_composer_input(state.composer_input()));
        return lines.join("\n");
    }

    if let Some(channel) = state.selected_channel_state() {
        let label = match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        };
        // Tell the user up-front if the shortcut won't open the composer here,
        // so they don't repeatedly press `i` and wonder why nothing happens.
        if !state.can_send_in_selected_channel() {
            return format!("read-only · cannot send messages in {label}");
        }
        // SEND is allowed but ATTACH is not. Tell the user uploads will be
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

pub(super) fn render_members(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    let groups = state.members_grouped();
    let mut lines: Vec<Line<'static>> = Vec::new();
    // (absolute_line_index, cdn_url) for activity rows that have a loaded emoji image.
    let mut emoji_line_urls: Vec<(usize, String)> = Vec::new();
    let content_width = (area.width as usize).saturating_sub(2);
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
        lines.push(member_group_header(group, content_width));
        line_index += 1;
        for member in &group.entries {
            let member = *member;
            let is_selected = focused && selected_line == Some(line_index);
            let marker_style = Style::default().fg(presence_color(member.status()));
            let name_style =
                member_name_style(member, state.member_role_color(member), is_selected);

            let display_name = state.member_display_name(member);
            let display = member_display_label(
                member,
                &display_name,
                state.member_horizontal_scroll(),
                max_name_width,
            );
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", presence_marker(member.status())),
                    marker_style,
                ),
                Span::styled(display, name_style),
            ]));
            line_index += 1;

            if !matches!(
                member.status(),
                PresenceStatus::Offline | PresenceStatus::Unknown
            ) {
                let activities = state.user_activities(member.user_id());
                if let Some(render) = primary_activity_summary(activities, emoji_images) {
                    let h_scroll = state.member_horizontal_scroll();
                    let line = match render.leading {
                        ActivityLeading::Image(url) => {
                            let body = truncate_display_width_from(
                                &render.body,
                                h_scroll,
                                max_name_width.saturating_sub(3),
                            );
                            emoji_line_urls.push((line_index, url));
                            Line::from(vec![
                                Span::raw("     "),
                                Span::styled(body, Style::default().fg(DIM)),
                            ])
                        }
                        ActivityLeading::Icon(icon) => {
                            let body = truncate_display_width_from(
                                &render.body,
                                h_scroll,
                                max_name_width.saturating_sub(2),
                            );
                            Line::from(vec![
                                Span::raw("   "),
                                Span::styled(icon.to_string(), Style::default().fg(Color::Green)),
                                Span::raw(" "),
                                Span::styled(body, Style::default().fg(DIM)),
                            ])
                        }
                        ActivityLeading::None => {
                            let body =
                                truncate_display_width_from(&render.body, h_scroll, max_name_width);
                            Line::from(vec![
                                Span::raw("   "),
                                Span::styled(body, Style::default().fg(DIM)),
                            ])
                        }
                    };
                    lines.push(line);
                    line_index += 1;
                }
            }
        }
    }

    let scroll = state.member_scroll();
    let content_height = state.member_content_height();
    let lines: Vec<_> = lines
        .into_iter()
        .skip(scroll)
        .take(content_height)
        .collect();

    let block = panel_block_line(state.member_panel_title(), focused);
    let content_area = block.inner(area);
    frame.render_widget(Paragraph::new(lines).block(block), area);

    // Overlay custom emoji images on top of their placeholder cells.
    if state.show_custom_emoji() {
        for (line_idx, url) in &emoji_line_urls {
            let Some(image) = emoji_images.iter().find(|img| img.url == *url) else {
                continue;
            };
            let Some(visible_offset) = line_idx.checked_sub(scroll) else {
                continue;
            };
            if visible_offset >= content_height {
                continue;
            }
            let y = content_area.y.saturating_add(visible_offset as u16);
            frame.render_widget(
                ratatui_image::Image::new(image.protocol),
                Rect::new(content_area.x.saturating_add(3), y, 2, 1),
            );
        }
    }

    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        scroll,
        content_height,
        state.member_line_count(),
    );
}

fn member_group_header(group: &MemberGroup<'_>, content_width: usize) -> Line<'static> {
    let count_suffix = format!(" - {}", group.entries.len());
    let label_max = content_width.saturating_sub(count_suffix.width());
    let label = truncate_display_width(&sanitize_for_display_width(&group.label), label_max);
    Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(discord_color(group.color, DIM))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(count_suffix, Style::default().fg(DIM)),
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
    display_name: &str,
    horizontal_scroll: usize,
    max_width: usize,
) -> String {
    let display_name = sanitize_for_display_width(display_name);
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
/// Returns `(display_text, Option<cdn_url>)`. When the cdn_url is `Some`, the
/// text contains a 2-space placeholder at the start for the image overlay.
pub(super) fn primary_activity_summary(
    activities: &[ActivityInfo],
    emoji_images: &[EmojiImage<'_>],
) -> Option<ActivityRender> {
    let mut sorted: Vec<&ActivityInfo> = activities.iter().collect();
    sorted.sort_by_key(|a| activity_priority(a.kind));
    let mut image_only_fallback: Option<ActivityRender> = None;
    for activity in sorted {
        let render = build_activity_render(activity, emoji_images, true);
        if !render.body.trim().is_empty() {
            return Some(render);
        }
        if matches!(render.leading, ActivityLeading::Image(_)) && image_only_fallback.is_none() {
            image_only_fallback = Some(render);
        }
    }
    image_only_fallback
}

/// Member-list ordering. Intentionally differs from
/// `popups::activity_priority`: see [`primary_activity_summary`].
fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Streaming => 0,
        ActivityKind::Playing => 1,
        ActivityKind::Listening => 2,
        ActivityKind::Watching => 3,
        ActivityKind::Competing => 4,
        ActivityKind::Custom => 5,
        ActivityKind::Unknown => 6,
    }
}

pub(super) fn render_header(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    let mut spans = vec![Span::styled(title, Style::default().fg(Color::Cyan).bold())];
    if let Some(user) = state.current_user() {
        spans.push(Span::styled(" Connected as ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            format!("{user} "),
            Style::default().fg(Color::Green).bold(),
        ));
    } else {
        spans.push(Span::styled(
            " Loading... ",
            Style::default().fg(Color::Yellow).bold(),
        ));
    }
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
