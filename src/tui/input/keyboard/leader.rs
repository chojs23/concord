use crossterm::event::KeyEvent;

use crate::discord::AppCommand;
use crate::tui::keybindings::KeyMapLookup;
use crate::tui::state::DashboardState;

use super::execute_ui_action;

pub(super) fn handle_leader_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if let Some(command) = handle_leader_keymap_key(state, key) {
        return command;
    }

    state.close_leader();

    None
}

fn handle_leader_keymap_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    let focus = state.focus();
    let lookup = state
        .key_bindings()
        .keymap_lookup_with_key(state.leader_keymap_prefix(), key);
    match lookup {
        Some(KeyMapLookup::Pending) => {
            let chord = state.key_bindings().keymap_chord_for_event(key);
            state.push_leader_keymap_key(chord);
            Some(None)
        }
        Some(KeyMapLookup::Action(action)) => {
            state.close_leader();
            Some(execute_ui_action(state, focus, action))
        }
        None if state.leader_keymap_prefix().len() > 1 => {
            state.close_leader();
            Some(None)
        }
        None => None,
    }
}
