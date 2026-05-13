use super::*;

pub(in crate::tui::ui) fn render_poll_vote_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(answers) = state.poll_vote_picker_items() else {
        return;
    };
    if answers.is_empty() {
        return;
    }

    let selected = state.selected_poll_vote_picker_index().unwrap_or(0);
    let popup = centered_rect(area, 58, (answers.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(poll_vote_picker_lines(answers, selected))
            .block(panel_block("Choose poll votes", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(in crate::tui::ui) fn poll_vote_picker_lines(
    answers: &[PollVotePickerItem],
    selected: usize,
) -> Vec<Line<'static>> {
    answers
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
        .collect()
}
