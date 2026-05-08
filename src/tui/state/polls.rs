use crate::discord::AppCommand;

use super::scroll::{clamp_selected_index, move_index_down, move_index_up};
use super::{DashboardState, PollVotePickerItem, PollVotePickerState, indexed_shortcut};

impl DashboardState {
    pub fn is_poll_vote_picker_open(&self) -> bool {
        self.poll_vote_picker.is_some()
    }

    pub fn poll_vote_picker_items(&self) -> Option<&[PollVotePickerItem]> {
        self.poll_vote_picker
            .as_ref()
            .map(PollVotePickerState::answers)
    }

    pub fn close_poll_vote_picker(&mut self) {
        self.poll_vote_picker = None;
    }

    pub fn move_poll_vote_picker_down(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            move_index_down(&mut picker.selected, picker.answers.len());
        }
    }

    pub fn move_poll_vote_picker_up(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            move_index_up(&mut picker.selected);
        }
    }

    pub fn toggle_selected_poll_vote_answer(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            let index = clamp_selected_index(picker.selected, picker.answers.len());
            if let Some(answer) = picker.answers.get_mut(index) {
                answer.selected = !answer.selected;
            }
        }
    }

    pub fn toggle_poll_vote_answer_shortcut(&mut self, shortcut: char) {
        let shortcut = shortcut.to_ascii_lowercase();
        let Some(picker) = &mut self.poll_vote_picker else {
            return;
        };
        let Some(index) = picker
            .answers
            .iter()
            .enumerate()
            .position(|(index, _)| indexed_shortcut(index) == Some(shortcut))
        else {
            return;
        };
        picker.selected = index;
        if let Some(answer) = picker.answers.get_mut(index) {
            answer.selected = !answer.selected;
        }
    }

    pub fn selected_poll_vote_picker_index(&self) -> Option<usize> {
        self.poll_vote_picker
            .as_ref()
            .map(|picker| clamp_selected_index(picker.selected, picker.answers.len()))
    }

    pub fn activate_poll_vote_picker(&mut self) -> Option<AppCommand> {
        let picker = self.poll_vote_picker.clone()?;
        let answer_ids = picker
            .answers
            .iter()
            .filter(|answer| answer.selected)
            .map(|answer| answer.answer_id)
            .collect::<Vec<_>>();
        self.close_poll_vote_picker();
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
            self.poll_vote_picker = Some(PollVotePickerState {
                selected: 0,
                channel_id: message.channel_id,
                message_id: message.id,
                answers: poll
                    .answers
                    .iter()
                    .map(|answer| PollVotePickerItem {
                        answer_id: answer.answer_id,
                        label: answer.text.clone(),
                        selected: answer.me_voted,
                    })
                    .collect(),
            });
        }
    }
}
