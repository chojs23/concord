use super::*;
use crate::tui::keybindings::KeyChord;

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

pub(in crate::tui::ui) fn leader_popup_area(area: Rect, lines: &[Line<'_>]) -> Rect {
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

pub(in crate::tui::ui) fn leader_popup_area_for_state(area: Rect, state: &DashboardState) -> Rect {
    let lines = leader_popup_lines(state, area.height.saturating_sub(2) as usize);
    leader_popup_area(area, &lines)
}

fn leader_popup_title(state: &DashboardState) -> String {
    if state.is_leader_action_mode() {
        if state.is_message_action_context_active() {
            return "Message Actions".to_owned();
        }
        if state.is_guild_leader_action_active() {
            return "Server Actions".to_owned();
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
    if state.is_message_action_context_active() {
        let actions = state.selected_message_action_items();
        return leader_action_label_lines(
            actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    (
                        state
                            .key_bindings()
                            .message_action_shortcut_label(&actions, index),
                        state.key_bindings().message_action_label(action),
                        action.enabled,
                    )
                })
                .collect(),
        );
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
        return leader_action_key_lines(
            actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    (
                        state.key_bindings().guild_action_shortcuts(&actions, index),
                        state.key_bindings().guild_action_label(action),
                        action.enabled,
                    )
                })
                .collect(),
        );
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
        return leader_action_key_lines(
            actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    (
                        state
                            .key_bindings()
                            .channel_action_shortcuts(&actions, index),
                        state.key_bindings().channel_action_label(action),
                        action.enabled,
                    )
                })
                .collect(),
        );
    }
    if state.is_member_leader_action_active() {
        let actions = state.selected_member_action_items();
        return leader_action_key_lines(
            actions
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    (
                        state
                            .key_bindings()
                            .member_action_shortcuts(&actions, index),
                        state.key_bindings().member_action_label(action),
                        action.enabled,
                    )
                })
                .collect(),
        );
    }
    vec![Line::from(Span::styled(
        "No actions available",
        Style::default().fg(DIM),
    ))]
}

#[cfg(test)]
pub(in crate::tui::ui) fn leader_action_lines_for_test(
    state: &DashboardState,
) -> Vec<Line<'static>> {
    leader_action_lines(state)
}

fn leader_shortcut_line(key: char, label: &str, enabled: bool) -> Line<'static> {
    leader_shortcut_text_line(&key.to_string(), label, enabled)
}

// Action shortcuts can carry modifiers, so key labels vary in width (`[t]` vs
// `[Ctrl+u]`). Pad every prefix to the widest one so the label column stays
// aligned across rows.
fn leader_action_key_lines(rows: Vec<(Vec<KeyChord>, String, bool)>) -> Vec<Line<'static>> {
    leader_action_label_lines(
        rows.into_iter()
            .map(|(keys, label, enabled)| (leader_shortcut_key_label(&keys), label, enabled))
            .collect(),
    )
}

fn leader_action_label_lines(rows: Vec<(String, String, bool)>) -> Vec<Line<'static>> {
    let prefixes: Vec<String> = rows
        .iter()
        .map(|(keys, _, _)| {
            if keys.is_empty() {
                "[ ]".to_owned()
            } else {
                format!("[{keys}]")
            }
        })
        .collect();
    let width = prefixes
        .iter()
        .map(|prefix| prefix.width())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    rows.into_iter()
        .zip(prefixes)
        .map(|((_, label, enabled), prefix)| {
            leader_shortcut_prefix_line(&format!("{prefix:<width$}"), &label, enabled)
        })
        .collect()
}

fn leader_shortcut_key_label(keys: &[KeyChord]) -> String {
    if keys.is_empty() {
        " ".to_owned()
    } else {
        keys.iter()
            .map(|key| key.label())
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn leader_shortcut_text_line(key: &str, label: &str, enabled: bool) -> Line<'static> {
    leader_shortcut_prefix_line(&format!("[{key}] "), label, enabled)
}

fn leader_shortcut_prefix_line(prefix: &str, label: &str, enabled: bool) -> Line<'static> {
    let style = if enabled {
        Style::default()
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![
        Span::styled(prefix.to_owned(), Style::default().fg(DIM)),
        Span::raw(" "),
        Span::styled(label.to_owned(), style),
    ])
}
fn truncate_leader_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    truncate_popup_lines(lines, width.max(1))
}

pub(in crate::tui::ui) fn render_message_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageActionMenu)
        || state.is_leader_action_mode()
    {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_message_action_index().unwrap_or(0);
    let lines =
        message_action_menu_lines_with_key_bindings(&actions, selected, state.key_bindings());

    let popup = message_action_menu_area(area, actions.len());
    let lines = truncate_action_menu_lines(lines, popup.width.saturating_sub(2) as usize);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Message actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn message_action_menu_area(area: Rect, action_count: usize) -> Rect {
    centered_rect(area, 54, (action_count as u16).saturating_add(2))
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
    let prefixes: Vec<String> = (0..actions.len())
        .map(|index| {
            shortcut_label_prefix(&key_bindings.message_action_shortcut_label(actions, index))
        })
        .collect();
    let prefix_width = prefixes
        .iter()
        .map(|prefix| prefix.width())
        .max()
        .unwrap_or(0);
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let selected = index == selected;
            let shortcut = format!("{:<prefix_width$}", prefixes[index]);
            let label = if action.enabled {
                key_bindings.message_action_label(action)
            } else {
                format!(
                    "{} (unavailable)",
                    key_bindings.message_action_label(action)
                )
            };
            let style = selectable_popup_label_style(selected, action.enabled);
            Line::from(vec![
                selectable_popup_marker(selected),
                selectable_popup_shortcut_span(shortcut),
                Span::styled(label, style),
            ])
        })
        .collect()
}

fn thread_action_menu_lines(
    actions: &[ThreadActionItem],
    selected: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    let prefixes: Vec<String> = (0..actions.len())
        .map(|index| {
            shortcut_label_prefix(&key_bindings.thread_action_shortcut_label(actions, index))
        })
        .collect();
    let prefix_width = prefixes
        .iter()
        .map(|prefix| prefix.width())
        .max()
        .unwrap_or(0);
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let selected = index == selected;
            let shortcut = format!("{:<prefix_width$}", prefixes[index]);
            let label = if action.enabled {
                key_bindings.thread_action_label(action)
            } else {
                format!("{} (unavailable)", key_bindings.thread_action_label(action))
            };
            let style = selectable_popup_label_style(selected, action.enabled);
            Line::from(vec![
                selectable_popup_marker(selected),
                selectable_popup_shortcut_span(shortcut),
                Span::styled(label, style),
            ])
        })
        .collect()
}

pub(in crate::tui::ui) fn render_thread_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ThreadActionMenu) {
        return;
    }

    let selected = state.selected_thread_action_index().unwrap_or(0);
    let noun = state.thread_action_menu_noun();
    let (title, lines) = if state.is_thread_action_mute_duration_phase() {
        let items = state.selected_thread_mute_duration_items();
        let lines = items
            .iter()
            .enumerate()
            .map(|(index, item)| thread_action_line(item.label, index == selected, true))
            .collect::<Vec<_>>();
        (format!("Mute {noun}"), lines)
    } else if state.is_thread_action_notification_phase() {
        let items = state.selected_thread_notification_items();
        if items.is_empty() {
            return;
        }
        let lines = items
            .iter()
            .enumerate()
            .map(|(index, item)| thread_action_line(&item.label, index == selected, true))
            .collect::<Vec<_>>();
        ("Notification settings".to_owned(), lines)
    } else {
        let items = state.selected_thread_action_items();
        if items.is_empty() {
            return;
        }
        let lines = thread_action_menu_lines(&items, selected, state.key_bindings());
        // Title-case the noun: "Post actions" / "Thread actions".
        let title = format!("{}{} actions", noun[..1].to_uppercase(), &noun[1..]);
        (title, lines)
    };

    let popup = message_action_menu_area(area, lines.len());
    let lines = truncate_action_menu_lines(lines, popup.width.saturating_sub(2) as usize);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn thread_action_line(label: &str, selected: bool, enabled: bool) -> Line<'static> {
    let label = if enabled {
        label.to_owned()
    } else {
        format!("{label} (unavailable)")
    };
    let style = selectable_popup_label_style(selected, enabled);
    Line::from(vec![
        selectable_popup_marker(selected),
        Span::styled(label, style),
    ])
}

fn shortcut_label_prefix(label: &str) -> String {
    if label.is_empty() {
        return "    ".to_owned();
    }
    format!("[{label}] ")
}

fn truncate_action_menu_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    truncate_popup_lines(lines, width.max(1))
}
