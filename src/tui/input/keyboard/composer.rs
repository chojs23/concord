use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::discord::AppCommand;
use crate::tui::keybindings::{ComposerAction, ComposerCompletionAction, SelectionAction};
use crate::tui::state::DashboardState;

pub(super) fn handle_composer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.composer_has_active_picker()
        && let Some(command) = handle_active_picker_key(state, key)
    {
        return command;
    }

    match state.key_bindings().composer_action(key) {
        ComposerAction::OpenInEditor => {
            state.request_open_composer_in_editor();
            None
        }
        ComposerAction::PasteClipboard => {
            state.request_paste_clipboard();
            None
        }
        ComposerAction::InsertNewline => {
            state.push_composer_char('\n');
            None
        }
        ComposerAction::Submit => state.submit_composer(),
        ComposerAction::Close => {
            state.close_composer();
            None
        }
        ComposerAction::ClearInput => {
            state.clear_composer_input();
            None
        }
        ComposerAction::RemoveLastAttachment => {
            state.pop_pending_composer_attachment();
            None
        }
        ComposerAction::DeletePreviousChar => {
            state.pop_composer_char();
            None
        }
        ComposerAction::DeletePreviousWord => {
            state.delete_previous_composer_word();
            None
        }
        ComposerAction::MoveCursorUp => {
            state.move_composer_cursor_up();
            None
        }
        ComposerAction::MoveCursorDown => {
            state.move_composer_cursor_down();
            None
        }
        ComposerAction::MoveCursorWordLeft => {
            state.move_composer_cursor_word_left();
            None
        }
        ComposerAction::MoveCursorLeft => {
            state.move_composer_cursor_left();
            None
        }
        ComposerAction::MoveCursorWordRight => {
            state.move_composer_cursor_word_right();
            None
        }
        ComposerAction::MoveCursorRight => {
            state.move_composer_cursor_right();
            None
        }
        ComposerAction::MoveCursorHome => {
            state.move_composer_cursor_home();
            None
        }
        ComposerAction::MoveCursorEnd => {
            state.move_composer_cursor_end();
            None
        }
        ComposerAction::InsertChar(value) => {
            if value != ':' || !state.open_composer_reaction_picker_from_plus_colon() {
                state.push_composer_char(value);
            }
            None
        }
        ComposerAction::Ignore => None,
    }
}

/// Returns `Some(None)` to mean "the picker absorbed this key, don't fall
/// through to the regular composer handler", and `None` to mean "let the
/// composer handle this key normally."
fn handle_active_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if key.code == KeyCode::Enter
        && key.modifiers == KeyModifiers::NONE
        && state.active_composer_picker_is_command()
        && state.composer_command_can_submit()
        && !state.composer_command_selected_candidate_is_top_level()
    {
        return Some(state.submit_composer());
    }

    handle_composer_completion_picker_key(
        state,
        key,
        DashboardState::move_active_composer_picker_selection,
        DashboardState::confirm_active_composer_picker,
        DashboardState::cancel_active_composer_picker,
    )
}

fn handle_composer_completion_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
    mut move_selection: impl FnMut(&mut DashboardState, isize),
    mut confirm: impl FnMut(&mut DashboardState) -> bool,
    mut cancel: impl FnMut(&mut DashboardState),
) -> Option<Option<AppCommand>> {
    match state.key_bindings().composer_completion_action(key) {
        ComposerCompletionAction::Select(SelectionAction::Next) => {
            move_selection(state, 1);
            Some(None)
        }
        ComposerCompletionAction::Select(SelectionAction::Previous) => {
            move_selection(state, -1);
            Some(None)
        }
        ComposerCompletionAction::Confirm => {
            if confirm(state) {
                Some(None)
            } else {
                cancel(state);
                Some(None)
            }
        }
        ComposerCompletionAction::Cancel => {
            cancel(state);
            Some(None)
        }
        ComposerCompletionAction::FallThrough => None,
    }
}
