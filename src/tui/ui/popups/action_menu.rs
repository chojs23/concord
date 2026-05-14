use super::*;

const LEADER_POPUP_WIDTH: u16 = 74;
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
    let popup = leader_popup_area(area, lines.len() as u16);
    frame.render_widget(bg_clear(state.theme().background), popup);
    frame.render_widget(
        Paragraph::new(truncate_leader_lines(
            lines,
            popup.width.saturating_sub(2) as usize,
        ))
        .block(panel_block_owned(
            leader_popup_title(state),
            true,
            state.theme().accent,
        ))
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
    if !state.is_leader_action_mode() {
        return "Leader".to_owned();
    }

    if state.is_message_action_menu_open() {
        "Message Actions".to_owned()
    } else if state.is_channel_leader_action_active() {
        "Channel Actions".to_owned()
    } else if state.is_guild_leader_action_active() {
        "Server Actions".to_owned()
    } else if state.is_member_leader_action_active() {
        "Member Actions".to_owned()
    } else {
        "Actions".to_owned()
    }
}

fn leader_popup_lines(state: &DashboardState, max_lines: usize) -> Vec<Line<'static>> {
    let dim = state.theme().dim;
    let lines = if state.is_leader_action_mode() {
        leader_action_lines(state, dim)
    } else {
        vec![
            leader_shortcut_line('1', "toggle Servers", true, dim),
            leader_shortcut_line('2', "toggle Channels", true, dim),
            leader_shortcut_line('4', "toggle Members", true, dim),
            leader_shortcut_line('a', "Actions", true, dim),
            leader_shortcut_line('o', "Options", true, dim),
            leader_shortcut_text_line("Space", "Switch channels", true, dim),
        ]
    };
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

fn leader_action_lines(state: &DashboardState, dim: Color) -> Vec<Line<'static>> {
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
                    dim,
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
                        indexed_shortcut(index).unwrap_or(' '),
                        item.label,
                        true,
                        dim,
                    )
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
                    dim,
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
                    indexed_shortcut(index).unwrap_or(' '),
                    &thread.label,
                    true,
                    dim,
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
                        indexed_shortcut(index).unwrap_or(' '),
                        item.label,
                        true,
                        dim,
                    )
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
                    dim,
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
                leader_shortcut_line(
                    member_action_shortcut(&actions, index).unwrap_or(' '),
                    &action.label,
                    action.enabled,
                    dim,
                )
            })
            .collect();
    }
    vec![Line::from(Span::styled(
        "No actions available",
        Style::default().fg(dim),
    ))]
}

fn leader_shortcut_line(key: char, label: &str, enabled: bool, dim: Color) -> Line<'static> {
    leader_shortcut_text_line(&key.to_string(), label, enabled, dim)
}

fn leader_shortcut_text_line(key: &str, label: &str, enabled: bool, dim: Color) -> Line<'static> {
    let style = if enabled {
        Style::default()
    } else {
        Style::default().fg(dim)
    };
    Line::from(vec![
        Span::styled(format!("[{key}] "), Style::default().fg(dim)),
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
    if !state.is_message_action_menu_open() {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }

    let selected = state.selected_message_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(2));
    frame.render_widget(bg_clear(state.theme().background), popup);
    let ctx = RenderCtx::new(state.theme());
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected, &ctx))
            .block(panel_block("Message actions", true, state.theme().accent))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn message_action_menu_lines(
    actions: &[MessageActionItem],
    selected: usize,
    ctx: &RenderCtx<'_>,
) -> Vec<Line<'static>> {
    actions
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
                Style::default().fg(ctx.theme.dim)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ctx.theme.accent)),
                Span::styled(shortcut, Style::default().fg(ctx.theme.dim)),
                Span::styled(label, style),
            ])
        })
        .collect()
}
