use super::*;
use crate::tui::keybindings::KeyBindings;
use crate::tui::state::ActionItem;

const LEADER_POPUP_MIN_WIDTH: u16 = 74;
const LEADER_POPUP_ROWS: usize = 4;
const LEADER_POPUP_COLUMN_GAP: usize = 4;

// ============================================================================
// Shared action-menu family
// ============================================================================
// Message, thread/post, server, channel, and member action menus (and their
// mute-duration/notification submenus) all render as the same centered popup:
// one row per action, selection marker + [shortcut] + label.

struct ActionMenuRow {
    shortcut: String,
    label: String,
    enabled: bool,
    disabled_reason: Option<String>,
}

/// Builds the menu rows for one scope from its action items and the
/// keybindings lookups for that scope.
fn action_menu_rows<K>(
    actions: &[ActionItem<K>],
    shortcut: impl Fn(&[ActionItem<K>], usize) -> String,
    label: impl Fn(&ActionItem<K>) -> String,
) -> Vec<ActionMenuRow> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| ActionMenuRow {
            shortcut: shortcut(actions, index),
            label: label(action),
            enabled: action.is_enabled(),
            disabled_reason: action.disabled_reason().map(str::to_owned),
        })
        .collect()
}

fn action_menu_lines(rows: &[ActionMenuRow], selected: usize) -> Vec<Line<'static>> {
    let prefixes: Vec<String> = rows
        .iter()
        .map(|row| shortcut_label_prefix(&row.shortcut))
        .collect();
    let prefix_width = prefixes
        .iter()
        .map(|prefix| prefix.width())
        .max()
        .unwrap_or(0);
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let is_selected = index == selected;
            let shortcut = padded_shortcut_prefix(&prefixes[index], prefix_width);
            let label = match (row.enabled, row.disabled_reason.as_deref()) {
                (false, Some(reason)) => format!("{} ({reason})", row.label),
                _ => row.label.clone(),
            };
            let style = selectable_popup_label_style(is_selected, row.enabled);
            selected_row_line(
                Line::from(vec![
                    selectable_popup_marker(is_selected),
                    selectable_popup_shortcut_span(shortcut),
                    Span::styled(label, style),
                ]),
                is_selected,
            )
        })
        .collect()
}

/// Rows for the submenus (mute durations, notification levels), which are
/// activated by their list position via the `[1]`..`[9]` indexed shortcuts.
fn indexed_action_menu_rows(labels: impl IntoIterator<Item = String>) -> Vec<ActionMenuRow> {
    labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| ActionMenuRow {
            shortcut: KeyBindings::indexed_shortcut(index)
                .map(|shortcut| shortcut.to_string())
                .unwrap_or_default(),
            label,
            enabled: true,
            disabled_reason: None,
        })
        .collect()
}

fn render_action_menu(
    frame: &mut Frame,
    area: Rect,
    title: impl Into<String>,
    lines: Vec<Line<'static>>,
) {
    let popup = action_menu_area(area, lines.len());
    let lines = truncate_popup_lines(lines, popup.width.saturating_sub(2).max(1) as usize);
    render_modal_paragraph(frame, popup, title, lines);
}

pub(in crate::tui::ui) fn action_menu_area(area: Rect, action_count: usize) -> Rect {
    centered_rect(area, 54, (action_count as u16).saturating_add(2))
}

fn shortcut_label_prefix(label: &str) -> String {
    if label.is_empty() {
        return "[]".to_owned();
    }
    format!("[{label}] ")
}

fn padded_shortcut_prefix(prefix: &str, width: usize) -> String {
    if prefix == "[]" {
        "[] ".to_owned()
    } else {
        format!("{prefix:<width$}")
    }
}

// ============================================================================
// Leader popup
// ============================================================================
// The leader popup is the bottom hint window listing the key bindings
// reachable from the pressed prefix. The action menus it can lead into are
// standalone modals rendered by the family renderers below.

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
    let lines = truncate_popup_lines(lines, popup.width.saturating_sub(2).max(1) as usize);
    render_modal_paragraph(frame, popup, state.leader_keymap_title(), lines);
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

// ============================================================================
// Server / channel / member action menus
// ============================================================================

pub(in crate::tui::ui) fn render_guild_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::GuildActionMenu) {
        return;
    }
    let Some((title, lines)) = guild_action_menu_content(state) else {
        return;
    };
    render_action_menu(frame, area, title, lines);
}

fn guild_action_menu_content(state: &DashboardState) -> Option<(&'static str, Vec<Line<'static>>)> {
    let selected = state.selected_guild_action_index().unwrap_or(0);
    if state.is_guild_action_mute_duration_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_guild_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        return Some(("Mute server", action_menu_lines(&rows, selected)));
    }
    let actions = state.selected_guild_action_items();
    if actions.is_empty() {
        return None;
    }
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .guild_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().guild_action_label(action),
    );
    Some(("Server actions", action_menu_lines(&rows, selected)))
}

pub(in crate::tui::ui) fn render_channel_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::ChannelActionMenu) {
        return;
    }
    let Some((title, lines)) = channel_action_menu_content(state) else {
        return;
    };
    render_action_menu(frame, area, title, lines);
}

fn channel_action_menu_content(
    state: &DashboardState,
) -> Option<(&'static str, Vec<Line<'static>>)> {
    let selected = state.selected_channel_action_index().unwrap_or(0);
    if state.is_channel_action_mute_duration_phase() {
        let rows = indexed_action_menu_rows(
            state
                .selected_channel_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        return Some(("Mute channel", action_menu_lines(&rows, selected)));
    }
    let actions = state.selected_channel_action_items();
    if actions.is_empty() {
        return None;
    }
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .channel_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().channel_action_label(action),
    );
    Some(("Channel actions", action_menu_lines(&rows, selected)))
}

pub(in crate::tui::ui) fn render_member_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MemberActionMenu) {
        return;
    }
    let actions = state.selected_member_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_member_action_index().unwrap_or(0);
    let rows = action_menu_rows(
        &actions,
        |actions, index| {
            state
                .key_bindings()
                .member_action_shortcut_label(actions, index)
        },
        |action| state.key_bindings().member_action_label(action),
    );
    render_action_menu(
        frame,
        area,
        "Member actions",
        action_menu_lines(&rows, selected),
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn channel_action_menu_lines_for_test(
    state: &DashboardState,
) -> Vec<Line<'static>> {
    channel_action_menu_content(state)
        .map(|(_, lines)| lines)
        .unwrap_or_default()
}

fn leader_popup_lines(state: &DashboardState, max_lines: usize) -> Vec<Line<'static>> {
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

fn leader_shortcut_text_line(key: &str, label: &str, enabled: bool) -> Line<'static> {
    let style = if enabled {
        Style::default()
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    };
    Line::from(vec![
        Span::styled(
            format!("[{key}] "),
            theme::current().style(theme::HighlightGroup::Shortcut),
        ),
        Span::raw(" "),
        Span::styled(label.to_owned(), style),
    ])
}

// ============================================================================
// Message action menu
// ============================================================================

pub(in crate::tui::ui) fn render_message_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::MessageActionMenu) {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_message_action_index().unwrap_or(0);
    let lines =
        message_action_menu_lines_with_key_bindings(&actions, selected, state.key_bindings());
    render_action_menu(frame, area, "Message actions", lines);
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

#[cfg(test)]
pub(in crate::tui::ui) fn message_action_menu_lines_with_keymap_options(
    actions: &[MessageActionItem],
    selected: usize,
    keymap_options: &crate::config::KeymapOptions,
) -> Vec<Line<'static>> {
    let key_bindings = crate::tui::keybindings::KeyBindings::try_from_options(keymap_options)
        .expect("test keymap options should parse");
    message_action_menu_lines_with_key_bindings(actions, selected, &key_bindings)
}

fn message_action_menu_lines_with_key_bindings(
    actions: &[MessageActionItem],
    selected: usize,
    key_bindings: &KeyBindings,
) -> Vec<Line<'static>> {
    let rows = action_menu_rows(
        actions,
        |actions, index| key_bindings.message_action_shortcut_label(actions, index),
        |action| key_bindings.message_action_label(action),
    );
    action_menu_lines(&rows, selected)
}

// ============================================================================
// Thread / forum-post action menu
// ============================================================================

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
        let rows = indexed_action_menu_rows(
            state
                .selected_thread_mute_duration_items()
                .iter()
                .map(|item| item.label.to_owned()),
        );
        (format!("Mute {noun}"), action_menu_lines(&rows, selected))
    } else if state.is_thread_action_notification_phase() {
        let items = state.selected_thread_notification_items();
        if items.is_empty() {
            return;
        }
        let rows = indexed_action_menu_rows(items.into_iter().map(|item| item.label));
        (
            "Notification settings".to_owned(),
            action_menu_lines(&rows, selected),
        )
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
    render_action_menu(frame, area, title, lines);
}

fn thread_action_menu_lines(
    actions: &[ThreadActionItem],
    selected: usize,
    key_bindings: &KeyBindings,
) -> Vec<Line<'static>> {
    let rows = action_menu_rows(
        actions,
        |actions, index| key_bindings.thread_action_shortcut_label(actions, index),
        |action| key_bindings.thread_action_label(action),
    );
    action_menu_lines(&rows, selected)
}
