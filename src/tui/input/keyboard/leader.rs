use crossterm::event::KeyEvent;

use crate::discord::AppCommand;
use crate::tui::keybindings::KeyMapLookup;
use crate::tui::state::DashboardState;

use super::{execute_ui_action, is_key_sequence_cancel_key};

pub(super) fn handle_dashboard_key_sequence(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if is_key_sequence_cancel_key(key) {
        state.close_key_sequence();
        return None;
    }

    let focus = state.focus();
    let lookup = state
        .key_bindings()
        .keymap_lookup_with_key(state.leader_keymap_prefix(), key);
    match lookup {
        Some(KeyMapLookup::Pending) => {
            let chord = state.key_bindings().keymap_chord_for_event(key);
            state.push_key_sequence_key(chord);
            None
        }
        Some(KeyMapLookup::Action(action)) => {
            state.close_key_sequence();
            execute_ui_action(state, focus, action)
        }
        None => {
            state.close_key_sequence();
            None
        }
    }
}
