use super::message_list::render_image_preview;
use super::*;
use crate::tui::state::MuteActionDurationItem;
use ratatui::layout::Position;

const LEADER_POPUP_WIDTH: u16 = 74;
const LEADER_POPUP_ROWS: usize = 4;
const LEADER_POPUP_COLUMN_GAP: usize = 4;
const CHANNEL_SWITCHER_POPUP_WIDTH: u16 = 74;

pub(super) fn render_leader_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_leader_active() {
        return;
    }

    let lines = leader_popup_lines(state, area.height.saturating_sub(2) as usize);
    let popup = leader_popup_area(area, lines.len() as u16);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(truncate_leader_lines(
            lines,
            popup.width.saturating_sub(2) as usize,
        ))
        .block(panel_block_owned(leader_popup_title(state), true))
        .wrap(Wrap { trim: false }),
        popup,
    );
}

fn leader_popup_area(area: Rect, line_count: u16) -> Rect {
    let width = LEADER_POPUP_WIDTH.min(area.width).max(1);
    let height = line_count.saturating_add(2).min(area.height).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    }
}

fn leader_popup_title(state: &DashboardState) -> String {
    if state.is_leader_action_mode() {
        "Leader Actions".to_owned()
    } else {
        "Leader".to_owned()
    }
}

fn leader_popup_lines(state: &DashboardState, max_lines: usize) -> Vec<Line<'static>> {
    let mut lines = if state.is_leader_action_mode() {
        leader_action_lines(state)
    } else {
        vec![
            leader_shortcut_line('1', "toggle Servers", true),
            leader_shortcut_line('2', "toggle Channels", true),
            leader_shortcut_line('4', "toggle Members", true),
            leader_shortcut_line('a', "Actions", true),
            leader_shortcut_line('o', "Options", true),
            leader_shortcut_text_line("Space", "Switch channels", true),
        ]
    };
    lines.push(Line::from(Span::styled(
        "Esc cancel",
        Style::default().fg(DIM),
    )));
    leader_shortcut_grid_lines(lines, max_lines)
}

fn leader_shortcut_grid_lines(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return lines;
    }
    let row_count = lines.len().min(LEADER_POPUP_ROWS).min(max_lines.max(1));
    let column_count = lines.len().div_ceil(row_count);
    let column_widths: Vec<usize> = (0..column_count)
        .map(|column| {
            (0..row_count)
                .filter_map(|row| lines.get(column * row_count + row))
                .map(leader_line_width)
                .max()
                .unwrap_or(0)
        })
        .collect();

    (0..row_count)
        .map(|row| {
            let mut spans = Vec::new();
            for (column, width) in column_widths.iter().enumerate() {
                let Some(line) = lines.get(column * row_count + row) else {
                    continue;
                };
                let line_width = leader_line_width(line);
                spans.extend(line.spans.iter().cloned());
                if column + 1 < column_count {
                    spans.push(Span::raw(" ".repeat(
                        width.saturating_sub(line_width) + LEADER_POPUP_COLUMN_GAP,
                    )));
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn leader_line_width(line: &Line<'_>) -> usize {
    line.spans.iter().map(|span| span.content.width()).sum()
}

fn leader_action_lines(state: &DashboardState) -> Vec<Line<'static>> {
    if state.is_message_action_menu_open() {
        let actions = state.selected_message_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_line(
                    message_action_shortcut(&actions, index).unwrap_or(' '),
                    &action.label,
                    action.enabled,
                )
            })
            .collect();
    }
    if state.is_guild_action_menu_open() {
        if state.is_guild_action_mute_duration_phase() {
            return state
                .selected_guild_mute_duration_items()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    leader_shortcut_line(indexed_shortcut(index).unwrap_or(' '), item.label, true)
                })
                .collect();
        }
        let actions = state.selected_guild_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_line(
                    guild_action_shortcut(&actions, index).unwrap_or(' '),
                    &action.label,
                    action.enabled,
                )
            })
            .collect();
    }
    if state.is_channel_action_threads_phase() {
        return state
            .channel_action_thread_items()
            .into_iter()
            .enumerate()
            .map(|(index, thread)| {
                leader_shortcut_line(indexed_shortcut(index).unwrap_or(' '), &thread.label, true)
            })
            .collect();
    }
    if state.is_channel_action_menu_open() {
        if state.is_channel_action_mute_duration_phase() {
            return state
                .selected_channel_mute_duration_items()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    leader_shortcut_line(indexed_shortcut(index).unwrap_or(' '), item.label, true)
                })
                .collect();
        }
        let actions = state.selected_channel_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_line(
                    channel_action_shortcut(&actions, index).unwrap_or(' '),
                    &action.label,
                    action.enabled,
                )
            })
            .collect();
    }
    if state.is_member_action_menu_open() {
        let actions = state.selected_member_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_line(
                    member_action_shortcut(&actions, index).unwrap_or(' '),
                    &action.label,
                    action.enabled,
                )
            })
            .collect();
    }
    vec![Line::from(Span::styled(
        "No actions available",
        Style::default().fg(DIM),
    ))]
}

fn leader_shortcut_line(key: char, label: &str, enabled: bool) -> Line<'static> {
    leader_shortcut_text_line(&key.to_string(), label, enabled)
}

fn leader_shortcut_text_line(key: &str, label: &str, enabled: bool) -> Line<'static> {
    let style = if enabled {
        Style::default()
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![
        Span::styled(format!("[{key}] "), Style::default().fg(DIM)),
        Span::raw(" "),
        Span::styled(label.to_owned(), style),
    ])
}

pub(super) fn render_channel_switcher_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_channel_switcher_open() {
        return;
    }

    let query = state.channel_switcher_query().unwrap_or_default();
    let query_cursor = state
        .channel_switcher_query_cursor_byte_index()
        .unwrap_or(query.len());
    let items = state.channel_switcher_items();
    let selected = state.selected_channel_switcher_index().unwrap_or(0);
    let popup = channel_switcher_popup_area(area);
    let max_result_lines = usize::from(popup.height.saturating_sub(6)).max(1);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(channel_switcher_lines(
            &items,
            selected,
            query,
            query_cursor,
            max_result_lines,
            popup.width.saturating_sub(2) as usize,
        ))
        .block(panel_block("Channel Switcher", true))
        .wrap(Wrap { trim: false }),
        popup,
    );
    if let Some(position) = channel_switcher_cursor_position(area, state) {
        frame.set_cursor_position(position);
    }
}

pub(super) fn channel_switcher_popup_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(2).clamp(8, 22);
    centered_rect(area, CHANNEL_SWITCHER_POPUP_WIDTH, height)
}

pub(super) fn channel_switcher_item_index_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<usize> {
    if !state.is_channel_switcher_open() {
        return None;
    }
    let popup = channel_switcher_popup_area(area);
    let inner = panel_block("", false).inner(popup);
    if column < inner.x
        || column >= inner.x.saturating_add(inner.width)
        || row < inner.y
        || row >= inner.y.saturating_add(inner.height)
    {
        return None;
    }
    let line = row.saturating_sub(inner.y) as usize;
    let result_line = line.checked_sub(2)?;
    let items = state.channel_switcher_items();
    let selected = state.selected_channel_switcher_index().unwrap_or(0);
    let max_result_lines = usize::from(popup.height.saturating_sub(6)).max(1);
    channel_switcher_visible_result_rows(&items, selected, max_result_lines)
        .get(result_line)
        .and_then(|row| match row {
            ChannelSwitcherResultRow::Item(index) => Some(*index),
            ChannelSwitcherResultRow::Group(_) => None,
        })
}

pub(super) fn channel_switcher_cursor_position(
    area: Rect,
    state: &DashboardState,
) -> Option<Position> {
    if !state.is_channel_switcher_open() {
        return None;
    }
    let query = state.channel_switcher_query().unwrap_or_default();
    let cursor = state
        .channel_switcher_query_cursor_byte_index()?
        .min(query.len());
    let popup = channel_switcher_popup_area(area);
    let inner_width = usize::from(popup.width.saturating_sub(2)).max(1);
    let (_, cursor_offset) = visible_channel_switcher_query(query, cursor, inner_width);
    Some(Position::new(
        popup
            .x
            .saturating_add(1)
            .saturating_add(cursor_offset as u16),
        popup.y.saturating_add(1),
    ))
}

pub(super) fn channel_switcher_lines(
    items: &[ChannelSwitcherItem],
    selected: usize,
    query: &str,
    query_cursor: usize,
    max_result_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        channel_switcher_search_line(query, query_cursor, width),
        Line::from(Span::styled(
            "─".repeat(width.max(1)),
            Style::default().fg(DIM),
        )),
    ];

    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "No channels found",
            Style::default().fg(DIM),
        )));
    } else {
        lines.extend(channel_switcher_result_lines(
            items,
            selected,
            max_result_lines,
        ));
    }

    lines.push(Line::from(Span::styled(
        "Enter open · Ctrl+n/p move · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn channel_switcher_search_line(query: &str, query_cursor: usize, width: usize) -> Line<'static> {
    let shown_query = if query.is_empty() {
        Span::styled("search channels", Style::default().fg(DIM))
    } else {
        Span::raw(visible_channel_switcher_query(query, query_cursor, width).0)
    };
    Line::from(vec![
        Span::styled("🔎 ", Style::default().fg(ACCENT)),
        shown_query,
    ])
}

fn visible_channel_switcher_query(query: &str, cursor: usize, width: usize) -> (String, usize) {
    let prefix_width = "🔎 ".width();
    let available = width.saturating_sub(prefix_width).max(1);
    let cursor = clamp_query_cursor(query, cursor);
    let mut start = 0usize;
    while query[start..cursor].width() > available {
        start = next_query_boundary(query, start);
    }

    let mut end = cursor;
    while end < query.len() {
        let next = next_query_boundary(query, end);
        if query[start..next].width() > available {
            break;
        }
        end = next;
    }

    let cursor_offset = prefix_width
        .saturating_add(query[start..cursor].width())
        .min(width.saturating_sub(1));
    (query[start..end].to_owned(), cursor_offset)
}

fn clamp_query_cursor(query: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(query.len());
    while cursor > 0 && !query.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn next_query_boundary(query: &str, cursor: usize) -> usize {
    let cursor = clamp_query_cursor(query, cursor);
    query[cursor..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| cursor + offset)
        .unwrap_or(query.len())
}

fn channel_switcher_result_lines(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> Vec<Line<'static>> {
    let selected = selected.min(items.len().saturating_sub(1));
    let rows = channel_switcher_visible_result_rows(items, selected, max_result_lines);
    rows.into_iter()
        .map(|row| match row {
            ChannelSwitcherResultRow::Item(index) => {
                channel_switcher_item_line(&items[index], index == selected)
            }
            ChannelSwitcherResultRow::Group(label) => Line::from(Span::styled(
                label,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
        })
        .collect()
}

enum ChannelSwitcherResultRow {
    Group(String),
    Item(usize),
}

fn channel_switcher_visible_result_rows(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> Vec<ChannelSwitcherResultRow> {
    let selected = selected.min(items.len().saturating_sub(1));
    let start = channel_switcher_visible_start(items, selected, max_result_lines);
    let end = items.len().min(start.saturating_add(max_result_lines));
    let mut rows = Vec::new();
    let mut last_group: Option<&str> = None;
    for (index, item) in items.iter().enumerate().skip(start).take(end - start) {
        if last_group != Some(item.group_label.as_str()) {
            rows.push(ChannelSwitcherResultRow::Group(item.group_label.clone()));
            last_group = Some(item.group_label.as_str());
        }
        rows.push(ChannelSwitcherResultRow::Item(index));
    }
    rows.truncate(max_result_lines.max(1));
    rows
}

fn channel_switcher_visible_start(
    items: &[ChannelSwitcherItem],
    selected: usize,
    max_result_lines: usize,
) -> usize {
    if items.is_empty() || max_result_lines == 0 {
        return 0;
    }
    selected.saturating_sub(max_result_lines / 2)
}

fn channel_switcher_item_line(item: &ChannelSwitcherItem, selected: bool) -> Line<'static> {
    let style = if selected {
        highlight_style()
    } else {
        Style::default()
    };
    let badge = channel_switcher_unread_badge(item);
    let (_, name_style) = channel_unread_decoration(item.unread, style, false);
    let marker = if selected { "› " } else { "  " };
    let indent = "  ".repeat(item.depth.saturating_add(1));
    let parent = item
        .parent_label
        .as_ref()
        .map(|label| format!("{label} / "))
        .unwrap_or_default();
    let mut spans = vec![
        Span::styled(marker, Style::default().fg(ACCENT)),
        Span::raw(indent),
        Span::styled(parent, Style::default().fg(DIM)),
    ];
    if let Some(badge) = badge {
        spans.push(badge);
    }
    spans.push(Span::styled(item.channel_label.clone(), name_style));
    Line::from(spans)
}

fn channel_switcher_unread_badge(item: &ChannelSwitcherItem) -> Option<Span<'static>> {
    let (badge, _) = channel_unread_decoration(item.unread, Style::default(), false);
    if item.guild_id.is_none() && item.unread != ChannelUnreadState::Seen {
        if item.unread_message_count > 0 {
            let count = u32::try_from(item.unread_message_count).unwrap_or(u32::MAX);
            return channel_unread_decoration(
                ChannelUnreadState::Notified(count),
                Style::default(),
                false,
            )
            .0;
        }
        if item.unread == ChannelUnreadState::Unread {
            return channel_unread_decoration(
                ChannelUnreadState::Notified(1),
                Style::default(),
                false,
            )
            .0;
        }
    }
    badge
}

fn truncate_leader_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| truncate_line_to_display_width(line, width.max(1)))
        .collect()
}

pub(super) fn render_image_viewer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    image_preview: Option<ImagePreview<'_>>,
) {
    let Some(item) = state.selected_image_viewer_item() else {
        return;
    };

    let popup = image_viewer_popup(area);
    let title_width = usize::from(popup.width.saturating_sub(4)).max(1);
    let title = truncate_display_width(&image_viewer_title(&item), title_width);
    frame.render_widget(Clear, popup);
    let block = panel_block_owned(title, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [image_area, hint_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    if let Some(image_preview) = image_preview {
        render_image_preview(frame, image_area, image_preview.state);
    } else {
        frame.render_widget(
            Paragraph::new(format!("loading {}...", item.filename))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            image_area,
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "h/← previous · l/→ next · Enter/Space actions · Esc close",
            Style::default().fg(DIM),
        )))
        .alignment(Alignment::Center),
        hint_area,
    );
}

fn image_viewer_title(item: &ImageViewerItem) -> String {
    format!("Image {}/{} — {}", item.index, item.total, item.filename)
}

pub(super) fn render_image_viewer_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_image_viewer_action_menu_open() {
        return;
    }

    let actions = state.selected_image_viewer_action_items();
    if actions.is_empty() {
        return;
    }
    let Some(selected) = state.selected_image_viewer_action_index() else {
        return;
    };
    let popup = centered_rect(area, 42, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected))
            .block(panel_block("Image actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_message_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_message_action_menu_open() {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }

    let selected = state.selected_message_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected))
            .block(panel_block("Message actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_options_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_options_popup_open() {
        return;
    }

    let items = state.display_option_items();
    let selected = state.selected_option_index().unwrap_or(0);
    let popup = centered_rect(area, 66, (items.len() as u16).saturating_add(5));
    let block = panel_block("Options", true);
    let inner = block.inner(popup);
    let visible_items = usize::from(inner.height.saturating_sub(1)).max(1);
    let inner_width = usize::from(inner.width).max(1);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(options_popup_lines(
            &items,
            selected,
            visible_items,
            inner_width,
        ))
        .block(block),
        popup,
    );
}

pub(super) fn options_popup_lines(
    items: &[DisplayOptionItem],
    selected: usize,
    visible_items: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let visible_items = visible_items.max(1);
    let width = width.max(1);
    let selected = selected.min(items.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_items);
    let mut lines: Vec<Line<'static>> = items
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_items)
        .map(|(index, item)| {
            let marker = if index == selected { "› " } else { "  " };
            let control = item.value.map_or_else(
                || {
                    if item.enabled {
                        "[x]".to_owned()
                    } else {
                        "[ ]".to_owned()
                    }
                },
                |value| format!("[{value}]"),
            );
            let mut style = if item.effective || index == 0 {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{control} "), style),
                Span::styled(item.label, style),
                Span::styled(" — ", Style::default().fg(DIM)),
                Span::styled(item.description, Style::default().fg(DIM)),
            ])
        })
        .map(|line| truncate_line_to_display_width(line, width))
        .collect();
    let footer = format!(
        "Enter/Space toggle or cycle · j/k move · Esc close · saved to {}",
        crate::config::config_path_display()
    );
    lines.push(truncate_line_to_display_width(
        Line::from(Span::styled(footer, Style::default().fg(DIM))),
        width,
    ));
    lines
}

pub(super) fn render_guild_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_guild_action_menu_open() {
        return;
    }
    let actions = state.selected_guild_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_guild_action_index().unwrap_or(0);
    let is_duration_phase = state.is_guild_action_mute_duration_phase();
    let title = state.guild_action_menu_title();
    let row_count = if is_duration_phase {
        state.selected_guild_mute_duration_items().len()
    } else {
        actions.len()
    };
    let popup = centered_rect(area, 48, (row_count as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(if is_duration_phase {
            mute_duration_menu_lines(state.selected_guild_mute_duration_items(), selected)
        } else {
            guild_action_menu_lines(&actions, selected)
        })
            .block(panel_block_owned(
                if is_duration_phase {
                    format!("Mute server for… — {}", title.unwrap_or_default())
                } else {
                    title
                        .map(|name| format!("Server actions — {name}"))
                        .unwrap_or_else(|| "Server actions".to_owned())
                },
                true,
            ))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_channel_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_channel_action_menu_open() {
        return;
    }

    let title_suffix = state
        .channel_action_menu_title()
        .map(|name| format!(" — {name}"))
        .unwrap_or_default();

    if state.is_channel_action_threads_phase() {
        let threads = state.channel_action_thread_items();
        let selected = state.selected_channel_action_index().unwrap_or(0);
        let row_count = threads.len().max(1) as u16;
        let popup = centered_rect(area, 54, row_count.saturating_add(4));
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(channel_thread_menu_lines(&threads, selected))
                .block(panel_block_owned(format!("Threads{title_suffix}"), true))
                .wrap(Wrap { trim: false }),
            popup,
        );
        return;
    }

    let is_duration_phase = state.is_channel_action_mute_duration_phase();
    let actions = state.selected_channel_action_items();
    if actions.is_empty() && !is_duration_phase {
        return;
    }
    let selected = state.selected_channel_action_index().unwrap_or(0);
    let row_count = if is_duration_phase {
        state.selected_channel_mute_duration_items().len()
    } else {
        actions.len()
    };
    let popup = centered_rect(area, 54, (row_count as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(if is_duration_phase {
            mute_duration_menu_lines(state.selected_channel_mute_duration_items(), selected)
        } else {
            channel_action_menu_lines(&actions, selected)
        })
            .block(panel_block_owned(
                if is_duration_phase {
                    format!("Mute channel for…{title_suffix}")
                } else {
                    format!("Channel actions{title_suffix}")
                },
                true,
            ))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_emoji_reaction_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: Vec<EmojiReactionImage<'_>>,
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
        .clamp(1, super::super::selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
    let popup = centered_rect(area, 42, (desired_visible_items as u16).saturating_add(5));
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    let footer_lines = if filter.is_some() { 2 } else { 1 };
    let visible_items =
        usize::from(content.height.saturating_sub(footer_lines)).min(desired_visible_items);
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(emoji_reaction_picker_lines_with_custom_emoji_images(
            reactions,
            selected,
            visible_items,
            &ready_urls,
            state.show_custom_emoji(),
            filter,
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

pub(super) fn render_poll_vote_picker(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(answers) = state.poll_vote_picker_items() else {
        return;
    };
    if answers.is_empty() {
        return;
    }

    let selected = state.selected_poll_vote_picker_index().unwrap_or(0);
    let popup = centered_rect(area, 58, (answers.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(poll_vote_picker_lines(answers, selected))
            .block(panel_block("Choose poll votes", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_member_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_member_action_menu_open() {
        return;
    }
    let actions = state.selected_member_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_member_action_index().unwrap_or(0);
    let title = state
        .member_action_menu_title()
        .map(|name| format!("Member actions — {name}"))
        .unwrap_or_else(|| "Member actions".to_owned());
    let popup = centered_rect(area, 48, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(member_action_menu_lines(&actions, selected))
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn guild_action_menu_lines(
    actions: &[GuildActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(guild_action_shortcut(actions, index));
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn mute_duration_menu_lines(
    actions: &[MuteActionDurationItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(indexed_shortcut(index));
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(action.label, style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter select · Esc back",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn member_action_menu_lines(
    actions: &[MemberActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(member_action_shortcut(actions, index));
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn render_user_profile_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    avatar: Option<AvatarImage>,
) {
    if !state.is_user_profile_popup_open() {
        return;
    }

    const AVATAR_CELL_HEIGHT: u16 = 4;
    let popup = user_profile_popup_area(area);
    frame.render_widget(Clear, popup);

    let block = panel_block("Profile", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // The avatar sits inside the inner area; reserve a fixed column gutter
    // so the text section starts cleanly to its right.
    let has_avatar = user_profile_popup_has_avatar_inside(
        inner,
        state.show_avatars() && state.user_profile_popup_avatar_url().is_some(),
    );
    let text_area = user_profile_popup_text_area_inside(inner, has_avatar);

    let popup_text = user_profile_popup_text_for_render(state, text_area.width);
    let total_lines = popup_text.lines.len();
    let viewport = text_area.height as usize;
    let scroll_position = state
        .user_profile_popup_scroll()
        .min(total_lines.saturating_sub(viewport));
    let lines = popup_text
        .lines
        .into_iter()
        .skip(scroll_position)
        .take(viewport)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    render_vertical_scrollbar(frame, text_area, scroll_position, viewport, total_lines);

    if let Some(avatar) = avatar.filter(|_| has_avatar) {
        let avatar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: USER_PROFILE_POPUP_AVATAR_CELL_WIDTH.min(inner.width),
            height: AVATAR_CELL_HEIGHT.min(inner.height),
        };
        frame.render_widget(RatatuiImage::new(&avatar.protocol), avatar_area);
    }
}

const USER_PROFILE_POPUP_WIDTH: u16 = 60;
const USER_PROFILE_POPUP_HEIGHT: u16 = 24;
const USER_PROFILE_POPUP_AVATAR_CELL_WIDTH: u16 = 8;

/// Centered popup rect inside the messages area. Shared so the geometry
/// computation lives in one place and the scroll-clamping pass uses the
/// exact same width/height the renderer ends up drawing into.
pub(super) fn user_profile_popup_area(area: Rect) -> Rect {
    let width = USER_PROFILE_POPUP_WIDTH
        .min(area.width.saturating_sub(2))
        .max(8);
    let height = USER_PROFILE_POPUP_HEIGHT
        .min(area.height.saturating_sub(2))
        .max(6);
    centered_rect(area, width, height)
}

pub(super) fn user_profile_popup_has_avatar(area: Rect, has_avatar_url: bool) -> bool {
    let popup = user_profile_popup_area(area);
    let inner = panel_block("Profile", true).inner(popup);
    user_profile_popup_has_avatar_inside(inner, has_avatar_url)
}

fn user_profile_popup_has_avatar_inside(inner: Rect, has_avatar_url: bool) -> bool {
    has_avatar_url && inner.width > USER_PROFILE_POPUP_AVATAR_CELL_WIDTH + 2
}

fn user_profile_popup_text_area_inside(inner: Rect, has_avatar: bool) -> Rect {
    if has_avatar {
        let gutter = USER_PROFILE_POPUP_AVATAR_CELL_WIDTH + 2;
        Rect {
            x: inner.x + gutter,
            y: inner.y,
            width: inner.width.saturating_sub(gutter),
            height: inner.height,
        }
    } else {
        inner
    }
}

/// Geometry the scroll-clamping pass needs: the inner text rect plus the
/// available width that `user_profile_popup_text` will lay out into.
pub(super) fn user_profile_popup_text_geometry(area: Rect, has_avatar: bool) -> (u16, u16) {
    let popup = user_profile_popup_area(area);
    let inner = panel_block("Profile", true).inner(popup);
    let text_area = user_profile_popup_text_area_inside(inner, has_avatar);
    (text_area.width, text_area.height)
}

fn user_profile_popup_text_for_render(state: &DashboardState, width: u16) -> UserProfilePopupText {
    if let Some(profile) = state.user_profile_popup_data() {
        user_profile_popup_text(
            profile,
            state,
            width,
            state.user_profile_popup_status(),
            state.user_profile_popup_activities(),
        )
    } else if let Some(message) = state.user_profile_popup_load_error() {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                truncate_display_width(&format!("Failed to load profile: {message}"), width.into()),
                Style::default().fg(Color::Red),
            ))],
        }
    } else {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                "Loading profile...",
                Style::default().fg(DIM),
            ))],
        }
    }
}

/// Counts the lines the popup will draw, mirroring
/// `user_profile_popup_text_for_render` so the scroll-clamping pass in
/// `sync_view_heights` matches the eventual render exactly.
pub(super) fn user_profile_popup_total_lines(state: &DashboardState, width: u16) -> usize {
    user_profile_popup_text_for_render(state, width).lines.len()
}

#[cfg(test)]
pub(super) fn user_profile_popup_lines(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, &[]).lines
}

#[cfg(test)]
pub(super) fn user_profile_popup_lines_with_activities(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, activities).lines
}

pub(super) fn user_profile_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
) -> UserProfilePopupText {
    let is_self = state.current_user_id() == Some(profile.user_id);

    let inner_width = usize::from(width.max(8));
    let mut lines: Vec<Line<'static>> = Vec::new();

    let display_name = profile.display_name().to_owned();
    lines.push(Line::from(Span::styled(
        truncate_display_width(&display_name, inner_width),
        user_profile_display_name_style(status),
    )));
    lines.push(Line::from(Span::styled(
        truncate_display_width(&format!("@{}", profile.username), inner_width),
        Style::default().fg(DIM),
    )));

    if let Some(pronouns) = profile.pronouns.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate_display_width(pronouns, inner_width),
            Style::default().fg(DIM),
        )));
    }

    if !is_self {
        let (badge_label, badge_color) = friend_status_badge(profile.friend_status);
        lines.push(Line::from(Span::styled(
            badge_label,
            Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD),
        )));
    }

    if !activities.is_empty() {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(&mut lines, "ACTIVITY");
        for activity in activities {
            push_activity_lines(&mut lines, activity, inner_width);
        }
    }

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "ABOUT ME");
    push_wrapped_paragraph(
        &mut lines,
        profile
            .bio
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no bio)"),
        inner_width,
    );

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "NOTE");
    push_wrapped_paragraph(
        &mut lines,
        profile
            .note
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no note)"),
        inner_width,
    );

    if !is_self {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(
            &mut lines,
            &format!("MUTUAL SERVERS ({})", profile.mutual_guilds.len()),
        );
        if profile.mutual_guilds.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)".to_owned(),
                Style::default().fg(DIM),
            )));
        } else {
            for entry in &profile.mutual_guilds {
                let name = state
                    .guild_name(entry.guild_id)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("guild-{}", entry.guild_id.get()));
                let body = match entry.nick.as_deref() {
                    Some(nick) => format!("• {name} — {nick}"),
                    None => format!("• {name}"),
                };
                lines.push(Line::from(vec![
                    Span::styled("  ".to_owned(), Style::default().fg(ACCENT)),
                    Span::styled(
                        truncate_display_width(&body, inner_width.saturating_sub(2)),
                        Style::default(),
                    ),
                ]));
            }
        }
    }

    if !is_self {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(
            &mut lines,
            &format!("MUTUAL FRIENDS ({})", profile.mutual_friends_count),
        );
    }

    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "j/k scroll · Esc close",
        Style::default().fg(DIM),
    )));

    UserProfilePopupText { lines }
}

pub(super) fn user_profile_display_name_style(status: PresenceStatus) -> Style {
    Style::default()
        .fg(presence_color(status))
        .add_modifier(Modifier::BOLD)
}

fn friend_status_badge(status: FriendStatus) -> (String, Color) {
    match status {
        FriendStatus::Friend => ("● Friend".to_owned(), Color::Green),
        FriendStatus::IncomingRequest => ("● Incoming friend request".to_owned(), Color::Yellow),
        FriendStatus::OutgoingRequest => ("● Outgoing friend request".to_owned(), Color::Yellow),
        FriendStatus::Blocked => ("● Blocked".to_owned(), Color::Red),
        FriendStatus::None => ("● Not friends".to_owned(), DIM),
    }
}

fn push_section_header(lines: &mut Vec<Line<'static>>, label: &str) {
    lines.push(Line::from(Span::styled(
        label.to_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
}

fn push_activity_lines(lines: &mut Vec<Line<'static>>, activity: &ActivityInfo, width: usize) {
    let primary = activity_primary_line(activity);
    if !primary.is_empty() {
        lines.push(Line::from(Span::raw(truncate_display_width(
            &primary, width,
        ))));
    }
    if let Some(secondary) = activity_secondary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&secondary, width),
            Style::default().fg(DIM),
        )));
    }
    if let Some(tertiary) = activity_tertiary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&tertiary, width),
            Style::default().fg(DIM),
        )));
    }
}

fn activity_primary_line(activity: &ActivityInfo) -> String {
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
                (true, true) => String::new(),
                (false, true) => emoji,
                (true, false) => body,
                (false, false) => format!("{emoji} {body}"),
            }
        }
        ActivityKind::Playing => format!("Playing {}", activity.name),
        ActivityKind::Streaming => format!("Streaming {}", activity.name),
        ActivityKind::Listening => format!("Listening to {}", activity.name),
        ActivityKind::Watching => format!("Watching {}", activity.name),
        ActivityKind::Competing => format!("Competing in {}", activity.name),
        ActivityKind::Unknown => activity.name.clone(),
    }
}

fn activity_secondary_line(activity: &ActivityInfo) -> Option<String> {
    match activity.kind {
        ActivityKind::Custom => None,
        _ => activity.details.clone(),
    }
}

fn activity_tertiary_line(activity: &ActivityInfo) -> Option<String> {
    match activity.kind {
        ActivityKind::Custom => None,
        ActivityKind::Listening => activity
            .state
            .as_deref()
            .map(|artist| format!("by {artist}")),
        ActivityKind::Streaming => activity.url.clone(),
        _ => activity.state.clone(),
    }
}

fn push_wrapped_paragraph(lines: &mut Vec<Line<'static>>, text: &str, width: usize) {
    for line in text.split('\n') {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(Span::raw(String::new())));
        } else {
            lines.push(Line::from(Span::raw(truncate_display_width(
                trimmed, width,
            ))));
        }
    }
}

pub(super) fn render_reaction_users_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
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

pub(super) fn render_debug_log_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_debug_log_popup_open() {
        return;
    }

    const POPUP_TARGET_WIDTH: u16 = 78;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let visible_log_lines = usize::from(area.height).saturating_sub(6).max(1);
    let lines = debug_log_popup_lines(
        state.debug_log_lines(),
        state.debug_channel_visibility(),
        visible_log_lines,
        usize::from(popup_width.saturating_sub(2)),
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Debug logs", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn debug_log_popup_lines(
    entries: Vec<String>,
    channel_visibility: ChannelVisibilityStats,
    visible_log_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let visible_log_lines = visible_log_lines.max(1);
    let mut lines = Vec::new();

    // Header line: visible vs. permission-hidden channels for the active
    // scope. Helps the user diagnose "why is this channel missing" without
    // diving into the logs.
    let visibility_text = format!(
        "Channels: {} visible · {} hidden by permissions",
        channel_visibility.visible, channel_visibility.hidden,
    );
    lines.push(Line::from(Span::styled(
        visibility_text,
        Style::default().fg(ACCENT),
    )));
    lines.push(Line::from(Span::raw(String::new())));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "No errors recorded in this process.",
            Style::default().fg(DIM),
        )));
    } else {
        let wrapped = entries
            .into_iter()
            .flat_map(|entry| wrap_text_lines(&entry, width))
            .collect::<Vec<_>>();
        let start = wrapped.len().saturating_sub(visible_log_lines);
        for entry in wrapped.into_iter().skip(start) {
            lines.push(Line::from(Span::styled(
                entry,
                Style::default().fg(Color::Red),
            )));
        }
    }
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "Showing current-process ERROR logs only · ` / Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn message_action_menu_lines(
    actions: &[MessageActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(message_action_shortcut(actions, index));
            let label = if action.enabled {
                action.label.to_owned()
            } else {
                format!("{} (unavailable)", action.label)
            };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(label, style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn channel_action_menu_lines(
    actions: &[ChannelActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(channel_action_shortcut(actions, index));
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn channel_thread_menu_lines(threads: &[ChannelThreadItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = if threads.is_empty() {
        vec![Line::from(Span::styled(
            "  (no threads)".to_owned(),
            Style::default().fg(DIM),
        ))]
    } else {
        threads
            .iter()
            .enumerate()
            .map(|(index, thread)| {
                let marker = if index == selected { "› " } else { "  " };
                let shortcut = shortcut_prefix(indexed_shortcut(index));
                let mut suffix = String::new();
                if thread.archived {
                    suffix.push_str(" [archived]");
                }
                if thread.locked {
                    suffix.push_str(" [locked]");
                }
                let mut style = Style::default();
                if index == selected {
                    style = style
                        .bg(Color::Rgb(40, 45, 90))
                        .add_modifier(Modifier::BOLD);
                }
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(ACCENT)),
                    Span::styled(shortcut, Style::default().fg(DIM)),
                    Span::styled(format!("» {}", thread.label), style),
                    Span::styled(suffix, Style::default().fg(DIM)),
                ])
            })
            .collect()
    };
    lines.push(Line::from(Span::styled(
        "Shortcut/Enter open · Esc back",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn poll_vote_picker_lines(
    answers: &[PollVotePickerItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(indexed_shortcut(index));
            let checkbox = if answer.selected { "[x]" } else { "[ ]" };
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(format!("{checkbox} {}", answer.label), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Shortcut/Space toggle · Enter vote · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

#[cfg(test)]
pub(super) fn reaction_users_popup_lines(
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
    let has_hidden_before = scroll > 0;
    let has_hidden_after = scroll.saturating_add(visible_lines) < data_lines.len();
    let mut lines: Vec<Line<'static>> = data_lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .map(|line| truncate_line_to_display_width(line, inner_width))
        .collect();
    let hint = match (has_hidden_before, has_hidden_after) {
        (true, true) => "j/k scroll · more above/below · Esc close",
        (true, false) => "j/k scroll · more above · Esc close",
        (false, true) => "j/k scroll · more below · Esc close",
        (false, false) => "Esc close",
    };
    lines.push(truncate_line_to_display_width(
        Line::from(Span::styled(hint, Style::default().fg(DIM))),
        inner_width,
    ));
    lines
}

fn truncate_line_to_display_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 {
        return Line::default();
    }
    let mut remaining = max_width;
    let mut new_spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 1);
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        if span.content.width() <= remaining {
            remaining = remaining.saturating_sub(span.content.width());
            new_spans.push(span);
            continue;
        }
        let truncated = truncate_display_width(&span.content, remaining);
        remaining = remaining.saturating_sub(truncated.width());
        new_spans.push(Span::styled(truncated, span.style));
    }
    if remaining > 0 {
        new_spans.push(Span::styled(" ".repeat(remaining), line.style));
    }
    let mut truncated = Line::from(new_spans);
    truncated.style = line.style;
    truncated.alignment = line.alignment;
    truncated
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
pub(super) fn emoji_reaction_picker_lines(
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
    )
}

#[cfg(test)]
pub(super) fn filtered_emoji_reaction_picker_lines(
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
    )
}

fn emoji_reaction_picker_lines_with_custom_emoji_images(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
    show_custom_emoji: bool,
    filter: Option<&str>,
) -> Vec<Line<'static>> {
    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_items = max_visible_items.max(1).min(reactions.len().max(1));
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);

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

    lines.push(Line::from(Span::styled(
        "Shortcut/Enter/Space react · / filter · Esc close",
        Style::default().fg(DIM),
    )));
    if let Some(filter) = filter {
        lines.push(Line::from(vec![
            Span::styled("Filter ", Style::default().fg(DIM)),
            Span::styled(
                format!("/{filter}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines
}

fn shortcut_prefix(shortcut: Option<char>) -> String {
    shortcut
        .map(|shortcut| format!("[{shortcut}] "))
        .unwrap_or_else(|| "    ".to_owned())
}

fn render_emoji_reaction_images(
    frame: &mut Frame,
    area: Rect,
    reactions: &[EmojiReactionItem],
    selected: usize,
    visible_items: usize,
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    if area.width <= EMOJI_REACTION_IMAGE_WIDTH || area.height == 0 {
        return;
    }

    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);
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
