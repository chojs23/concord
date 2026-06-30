use crate::discord::AppCommand;

use super::super::{DashboardState, PollVotePickerItem, PollVotePickerState};
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup};

impl DashboardState {
    pub fn poll_vote_picker_items(&self) -> Option<&[PollVotePickerItem]> {
        self.popups
            .poll_vote_picker()
            .map(PollVotePickerState::answers)
    }

    pub fn close_poll_vote_picker(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::PollVotePicker) {
            self.popups.clear_modal();
        }
    }

    pub fn move_poll_vote_picker_down(&mut self) {
        if let Some(picker) = self.popups.poll_vote_picker_mut() {
            picker.selection.move_down(picker.answers.len());
        }
    }

    pub fn move_poll_vote_picker_up(&mut self) {
        if let Some(picker) = self.popups.poll_vote_picker_mut() {
            picker.selection.move_up();
        }
    }

    pub fn toggle_selected_poll_vote_answer(&mut self) {
        if let Some(picker) = self.popups.poll_vote_picker_mut() {
            let index = picker.selection.selected_for_len(picker.answers.len());
            toggle_poll_answer_selection(picker, index);
        }
    }

    pub fn toggle_poll_vote_answer_shortcut(&mut self, shortcut: char) {
        let shortcut = shortcut.to_ascii_lowercase();
        let Some(picker) = self.popups.poll_vote_picker_mut() else {
            return;
        };
        let Some(index) = picker.answers.iter().enumerate().position(|(index, _)| {
            crate::tui::keybindings::KeyBindings::indexed_shortcut(index) == Some(shortcut)
        }) else {
            return;
        };
        picker.selection.select(index);
        toggle_poll_answer_selection(picker, index);
    }

    pub fn selected_poll_vote_picker_index(&self) -> Option<usize> {
        self.popups
            .poll_vote_picker()
            .map(|picker| picker.selection.selected_for_len(picker.answers.len()))
    }

    pub fn activate_poll_vote_picker(&mut self) -> Option<AppCommand> {
        let picker = self.popups.take_poll_vote_picker()?;
        let answer_ids = picker
            .answers
            .iter()
            .filter(|answer| answer.selected)
            .map(|answer| answer.answer_id)
            .collect::<Vec<_>>();
        Some(AppCommand::VotePoll {
            channel_id: picker.channel_id,
            message_id: picker.message_id,
            answer_ids,
        })
    }

    pub(super) fn open_poll_vote_picker(&mut self) {
        if let Some(message) = self.selected_message_state()
            && let Some(poll) = &message.poll
        {
            self.popups.modal = Some(ModalPopup::PollVotePicker(PollVotePickerState {
                selection: Default::default(),
                allow_multiselect: poll.allow_multiselect,
                channel_id: message.channel_id,
                message_id: message.id,
                answers: normalized_poll_vote_picker_answers(
                    poll.allow_multiselect,
                    poll.answers
                        .iter()
                        .map(|answer| PollVotePickerItem {
                            answer_id: answer.answer_id,
                            label: answer.text.clone(),
                            selected: answer.me_voted,
                        })
                        .collect(),
                ),
            }));
        }
    }
}

fn normalized_poll_vote_picker_answers(
    allow_multiselect: bool,
    mut answers: Vec<PollVotePickerItem>,
) -> Vec<PollVotePickerItem> {
    if allow_multiselect {
        return answers;
    }

    let mut seen_selected = false;
    for answer in &mut answers {
        if answer.selected && seen_selected {
            answer.selected = false;
        }
        seen_selected |= answer.selected;
    }
    answers
}

fn toggle_poll_answer_selection(picker: &mut PollVotePickerState, index: usize) {
    if picker.allow_multiselect {
        if let Some(answer) = picker.answers.get_mut(index) {
            answer.selected = !answer.selected;
        }
        return;
    }

    let was_selected = picker
        .answers
        .get(index)
        .is_some_and(|answer| answer.selected);
    for answer in &mut picker.answers {
        answer.selected = false;
    }
    if !was_selected && let Some(answer) = picker.answers.get_mut(index) {
        answer.selected = true;
    }
}
