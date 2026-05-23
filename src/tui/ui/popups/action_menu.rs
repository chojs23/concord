use super::*;

const LEADER_POPUP_MIN_WIDTH: u16 = 74;
const LEADER_POPUP_ROWS: usize = 4;
const LEADER_POPUP_COLUMN_GAP: usize = 4;

pub(in crate::tui::ui) fn render_leader_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_leader_active() {
        return;
    }

    let lines = leader_popup_lines(state, area.height.saturating_sub(2) as usize);
    let popup = leader_popup_area(area, &lines);
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

fn leader_popup_area(area: Rect, lines: &[Line<'_>]) -> Rect {
    let content_width = lines.iter().map(leader_line_width).max().unwrap_or(0);
    let desired_width = content_width.saturating_add(2).min(u16::MAX as usize) as u16;
    let width = LEADER_POPUP_MIN_WIDTH
        .max(desired_width)
        .min(area.width)
        .max(1);
    let line_count = lines.len() as u16;
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
        if state.is_message_url_picker_open() {
            return "Open URL".to_owned();
        }
        if state.is_message_action_menu_open() {
            return "Message Actions".to_owned();
        }
        if state.is_guild_leader_action_active() {
            return "Server Actions".to_owned();
        }
        if state.is_channel_action_threads_phase() {
            return "Threads".to_owned();
        }
        if state.is_channel_leader_action_active() {
            return "Channel Actions".to_owned();
        }
        if state.is_member_leader_action_active() {
            return "Member Actions".to_owned();
        }
        return "Actions".to_owned();
    }

    state.leader_keymap_title()
}

fn leader_popup_lines(state: &DashboardState, max_lines: usize) -> Vec<Line<'static>> {
    if state.is_leader_action_mode() {
        return leader_shortcut_grid_lines(leader_action_lines(state), max_lines);
    }

    let lines = state
        .leader_keymap_shortcuts()
        .into_iter()
        .map(|item| {
            let label = if item.has_children {
                format!("{} ›", item.label)
            } else {
                item.label
            };
            leader_shortcut_text_line(&item.key, &label, true)
        })
        .collect::<Vec<_>>();
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
    if state.is_message_url_picker_open() {
        return state
            .selected_message_url_items()
            .iter()
            .enumerate()
            .map(|(index, item)| {
                leader_shortcut_line(
                    state.key_bindings().indexed_shortcut(index).unwrap_or(' '),
                    &item.label,
                    true,
                )
            })
            .collect();
    }
    if state.is_message_action_menu_open() {
        let actions = state.selected_message_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_line(
                    state
                        .key_bindings()
                        .message_action_shortcut(&actions, index)
                        .unwrap_or(' '),
                    &action.label,
                    action.enabled,
                )
            })
            .collect();
    }
    if state.is_guild_leader_action_active() {
        if state.is_guild_action_mute_duration_phase() {
            return state
                .selected_guild_mute_duration_items()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    leader_shortcut_line(
                        state.key_bindings().indexed_shortcut(index).unwrap_or(' '),
                        item.label,
                        true,
                    )
                })
                .collect();
        }
        let actions = state.selected_guild_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_keys_line(
                    &state.key_bindings().guild_action_shortcuts(&actions, index),
                    &state.key_bindings().guild_action_label(action),
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
                leader_shortcut_line(
                    state.key_bindings().indexed_shortcut(index).unwrap_or(' '),
                    &thread.label,
                    true,
                )
            })
            .collect();
    }
    if state.is_channel_leader_action_active() {
        if state.is_channel_action_mute_duration_phase() {
            return state
                .selected_channel_mute_duration_items()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    leader_shortcut_line(
                        state.key_bindings().indexed_shortcut(index).unwrap_or(' '),
                        item.label,
                        true,
                    )
                })
                .collect();
        }
        let actions = state.selected_channel_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_keys_line(
                    &state
                        .key_bindings()
                        .channel_action_shortcuts(&actions, index),
                    &state.key_bindings().channel_action_label(action),
                    action.enabled,
                )
            })
            .collect();
    }
    if state.is_member_leader_action_active() {
        let actions = state.selected_member_action_items();
        return actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                leader_shortcut_keys_line(
                    &state
                        .key_bindings()
                        .member_action_shortcuts(&actions, index),
                    &state.key_bindings().member_action_label(action),
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

fn leader_shortcut_keys_line(keys: &[char], label: &str, enabled: bool) -> Line<'static> {
    let key_label = if keys.is_empty() {
        " ".to_owned()
    } else {
        keys.iter()
            .map(char::to_string)
            .collect::<Vec<_>>()
            .join("/")
    };
    leader_shortcut_text_line(&key_label, label, enabled)
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
fn truncate_leader_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| truncate_line_to_display_width(line, width.max(1)))
        .collect()
}

pub(in crate::tui::ui) fn render_message_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_message_action_menu_open() || state.is_leader_action_mode() {
        return;
    }

    let (title, lines, len) = if state.is_message_url_picker_open() {
        let urls = state.selected_message_url_items();
        if urls.is_empty() {
            return;
        }
        let selected = state.selected_message_url_index().unwrap_or(0);
        (
            "Open URL",
            message_url_picker_lines(&urls, selected),
            urls.len(),
        )
    } else {
        let actions = state.selected_message_action_items();
        if actions.is_empty() {
            return;
        }
        let selected = state.selected_message_action_index().unwrap_or(0);
        (
            "Message actions",
            message_action_menu_lines_with_key_bindings(&actions, selected, state.key_bindings()),
            actions.len(),
        )
    };

    let popup = centered_rect(area, 54, (len as u16).saturating_add(2));
    let lines = truncate_action_menu_lines(lines, popup.width.saturating_sub(2) as usize);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_action_menu_lines(
    actions: &[MessageActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    message_action_menu_lines_with_key_bindings(
        actions,
        selected,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

fn message_action_menu_lines_with_key_bindings(
    actions: &[MessageActionItem],
    selected: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(key_bindings.message_action_shortcut(actions, index));
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
        .collect()
}

pub(in crate::tui::ui) fn message_url_picker_lines(
    urls: &[MessageUrlItem],
    selected: usize,
) -> Vec<Line<'static>> {
    urls.iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if index == selected { "› " } else { "  " };
            let shortcut = shortcut_prefix(
                crate::tui::keybindings::KeyBindings::default().indexed_shortcut(index),
            );
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(shortcut, Style::default().fg(DIM)),
                Span::styled(item.label.to_owned(), style),
            ])
        })
        .collect()
}

#[cfg(test)]
pub(in crate::tui::ui) fn message_url_picker_lines_for_width(
    urls: &[MessageUrlItem],
    selected: usize,
    width: usize,
) -> Vec<Line<'static>> {
    truncate_action_menu_lines(message_url_picker_lines(urls, selected), width)
}

fn truncate_action_menu_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| truncate_line_to_display_width(line, width.max(1)))
        .collect()
}
