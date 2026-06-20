use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::tui::keybindings::{
    GlobalAction, KeyMapLookup, PaneFilterAction, SelectionAction, SelectionKeySet,
};

use super::super::state::{DashboardState, FocusPane};
use crate::discord::AppCommand;

mod composer;
mod dashboard;
mod leader;
mod paste;
mod popups;

use composer::handle_composer_key;
use dashboard::{execute_ui_action, handle_dashboard_action};
pub use paste::{handle_paste, handle_pasted_file_attachments, handle_pasted_user_profile_avatar};
use popups::{PopupKeyPhase, handle_popup_key};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if let Some(command) = handle_popup_key(state, key, PopupKeyPhase::Priority) {
        return command;
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    // The debug log is intentionally available from regular dashboard modes,
    // but popups and the composer get first chance to handle their own keys.
    if matches!(
        state.key_bindings().global_action(key),
        Some(GlobalAction::ToggleDebugLog)
    ) {
        state.toggle_debug_log_popup();
        return None;
    }

    if let Some(command) = handle_popup_key(state, key, PopupKeyPhase::Deferred) {
        return command;
    }

    let focus = state.focus();

    if key.code == KeyCode::Esc && state.has_active_pane_filter() {
        state.close_active_pane_filters();
        return None;
    }

    // Only intercept filter input when the pane that owns the filter is still
    // focused. Moving the mouse to another pane should let normal shortcuts
    // work (e.g. pressing `i` after clicking Messages).
    if state.is_pane_filter_active(focus) {
        if let Some(command) = handle_pane_filter_key(state, key, focus) {
            return command;
        }
    }

    if state.is_folder_settings_open() {
        return handle_folder_settings_key(state, key);
    }

    if is_keymap_help_key(key) {
        state.open_keymap_help_popup();
        return None;
    }

    match state.key_bindings().keymap_lookup_root_key(key) {
        Some(KeyMapLookup::Action(action)) => return execute_ui_action(state, focus, action),
        Some(KeyMapLookup::Pending) => {
            let prefix = vec![state.key_bindings().keymap_chord_for_event(key)];
            state.open_keymap_prefix(prefix);
            return None;
        }
        None => {}
    }

    if let Some(action) = state.key_bindings().dashboard_action(key) {
        return handle_dashboard_action(state, focus, action);
    }

    if state.key_bindings().is_leader_key(key) {
        state.open_leader();
        return None;
    }

    None
}

fn handle_folder_settings_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.is_folder_settings_editing() {
        return handle_folder_settings_edit_key(state, key);
    }

    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next => state.next_folder_settings_field(),
            SelectionAction::Previous => state.previous_folder_settings_field(),
        }
        return None;
    }

    match key.code {
        KeyCode::Esc => state.close_folder_settings(),
        KeyCode::Enter => state.start_or_commit_folder_settings_edit(),
        KeyCode::Char('s')
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            return state.commit_folder_settings_command();
        }
        KeyCode::Tab => {
            state.next_folder_settings_field();
        }
        KeyCode::BackTab => {
            state.previous_folder_settings_field();
        }
        _ => {}
    }
    None
}

fn handle_folder_settings_edit_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => {
            state.cancel_folder_settings_edit();
        }
        KeyCode::Enter => state.start_or_commit_folder_settings_edit(),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {}
        KeyCode::Backspace
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            state.delete_previous_folder_settings_word();
        }
        KeyCode::Backspace => state.pop_folder_settings_char(),
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_folder_settings_cursor_word_left();
        }
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_folder_settings_cursor_word_right();
        }
        KeyCode::Left => state.move_folder_settings_cursor_left(),
        KeyCode::Right => state.move_folder_settings_cursor_right(),
        KeyCode::Home => state.move_folder_settings_cursor_home(),
        KeyCode::End => state.move_folder_settings_cursor_end(),
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.delete_previous_folder_settings_word();
        }
        KeyCode::Char(value)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            state.push_folder_settings_char(value);
        }
        _ => {}
    }
    None
}

fn handle_pane_filter_key(
    state: &mut DashboardState,
    key: KeyEvent,
    focus: FocusPane,
) -> Option<Option<AppCommand>> {
    if !state.is_pane_filter_editing(focus) {
        return match key.code {
            KeyCode::Esc => {
                state.close_pane_filter(focus);
                Some(None)
            }
            KeyCode::Enter => Some(state.activate_pane_filter_selection(focus)),
            _ => None,
        };
    }

    match state.key_bindings().pane_filter_action(key) {
        Some(PaneFilterAction::Select(SelectionAction::Next)) => {
            state.move_down();
            Some(None)
        }
        Some(PaneFilterAction::Select(SelectionAction::Previous)) => {
            state.move_up();
            Some(None)
        }
        Some(PaneFilterAction::Close) => {
            state.close_pane_filter(focus);
            Some(None)
        }
        Some(PaneFilterAction::Confirm) => {
            state.commit_pane_filter(focus);
            Some(None)
        }
        Some(PaneFilterAction::DeleteChar) => {
            state.pop_pane_filter_char(focus);
            Some(None)
        }
        Some(PaneFilterAction::MoveCursorLeft) => {
            state.move_pane_filter_cursor_left(focus);
            Some(None)
        }
        Some(PaneFilterAction::MoveCursorRight) => {
            state.move_pane_filter_cursor_right(focus);
            Some(None)
        }
        Some(PaneFilterAction::Ignore) => Some(None),
        Some(PaneFilterAction::InsertChar(value)) => {
            state.push_pane_filter_char(focus, value);
            Some(None)
        }
        None => None, // fall through to normal navigation (arrows, j/k etc.)
    }
}

fn is_keymap_help_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('?')
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}
