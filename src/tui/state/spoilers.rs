use std::collections::HashSet;

use crate::discord::{
    MessageState,
    ids::{Id, marker::MessageMarker},
};
use crate::tui::text::spoiler_ranges;

use super::DashboardState;

/// Messages whose `||spoiler||` text and spoiler-flagged media the reader
/// has revealed this session. Ephemeral by design: spoilers re-hide on
/// restart, matching Discord.
#[derive(Debug, Default)]
pub(super) struct SpoilerUiState {
    revealed: HashSet<Id<MessageMarker>>,
}

impl SpoilerUiState {
    fn is_revealed(&self, message_id: Id<MessageMarker>) -> bool {
        self.revealed.contains(&message_id)
    }

    fn toggle(&mut self, message_id: Id<MessageMarker>) {
        if !self.revealed.remove(&message_id) {
            self.revealed.insert(message_id);
        }
    }
}

impl DashboardState {
    pub(crate) fn message_spoilers_revealed(&self, message_id: Id<MessageMarker>) -> bool {
        self.spoilers.is_revealed(message_id)
    }

    /// Toggle reveal for the selected message's text and image spoilers.
    /// Returns `true` only when that message actually has spoiler content,
    /// so clicking an ordinary message never accumulates reveal state.
    pub(in crate::tui) fn toggle_selected_message_spoilers(&mut self) -> bool {
        let Some(message) = self.selected_message_state() else {
            return false;
        };
        if !message_has_spoiler(message) {
            return false;
        }
        let message_id = message.id;
        self.spoilers.toggle(message_id);
        true
    }
}

fn message_has_spoiler(message: &MessageState) -> bool {
    message
        .content
        .as_deref()
        .is_some_and(|content| !spoiler_ranges(content).is_empty())
        || message
            .inline_previews()
            .iter()
            .any(|preview| preview.spoiler)
}
