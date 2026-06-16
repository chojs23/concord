use super::*;

pub(in crate::tui::ui) fn render_poll_vote_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::PollVotePicker) {
        return;
    }

    let Some(answers) = state.poll_vote_picker_items() else {
        return;
    };
    if answers.is_empty() {
        return;
    }

    let selected = state.selected_poll_vote_picker_index().unwrap_or(0);
    let popup =
        clear_centered_popup_area(frame, area, 58, (answers.len() as u16).saturating_add(2));
    render_modal_paragraph(
        frame,
        popup,
        "Choose poll votes",
        poll_vote_picker_lines_with_key_bindings(answers, selected, state.key_bindings()),
    );
}

#[cfg(test)]
pub(in crate::tui::ui) fn poll_vote_picker_lines(
    answers: &[PollVotePickerItem],
    selected: usize,
) -> Vec<Line<'static>> {
    poll_vote_picker_lines_with_key_bindings(
        answers,
        selected,
        &crate::tui::keybindings::KeyBindings::default(),
    )
}

fn poll_vote_picker_lines_with_key_bindings(
    answers: &[PollVotePickerItem],
    selected: usize,
    key_bindings: &crate::tui::keybindings::KeyBindings,
) -> Vec<Line<'static>> {
    answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let selected = index == selected;
            let shortcut = shortcut_prefix(key_bindings.indexed_shortcut(index));
            let checkbox = if answer.selected { "[x]" } else { "[ ]" };
            let style = selectable_popup_label_style(selected, true);
            Line::from(vec![
                selectable_popup_marker(selected),
                selectable_popup_shortcut_span(shortcut),
                Span::styled(format!("{checkbox} {}", answer.label), style),
            ])
        })
        .collect()
}
