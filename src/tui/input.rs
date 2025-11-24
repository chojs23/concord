use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_up(),
        KeyCode::Char('g') => state.jump_top(),
        KeyCode::Char('G') => state.jump_bottom(),
        KeyCode::Tab => state.cycle_focus(),
        // Folder headers act like a small tree: Enter/Space toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Guilds => {
            state.toggle_selected_folder()
        }
        KeyCode::Right if state.focus() == FocusPane::Guilds => state.open_selected_folder(),
        KeyCode::Left if state.focus() == FocusPane::Guilds => state.close_selected_folder(),
        _ => {}
    }

    None
}

fn handle_composer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Enter => state.submit_composer(),
        KeyCode::Esc => {
            state.cancel_composer();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.quit();
            None
        }
        KeyCode::Backspace => {
            state.pop_composer_char();
            None
        }
        KeyCode::Char(value) => {
            state.push_composer_char(value);
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use twilight_model::id::Id;

    use super::handle_key;
    use crate::{
        discord::{AppEvent, GuildFolder},
        tui::state::{DashboardState, FocusPane, GuildPaneEntry},
    };

    #[test]
    fn enter_and_space_toggle_selected_folder() {
        let mut state = state_with_folder();
        focus_guilds(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_selected_folder_collapsed(&state, true);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert_selected_folder_collapsed(&state, false);
    }

    fn state_with_folder() -> DashboardState {
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        let mut state = DashboardState::new();

        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![GuildFolder {
                id: Some(42),
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }],
        });
        state
    }

    fn focus_guilds(state: &mut DashboardState) {
        while state.focus() != FocusPane::Guilds {
            state.cycle_focus();
        }
    }

    fn assert_selected_folder_collapsed(state: &DashboardState, expected: bool) {
        let entries = state.guild_pane_entries();
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader { collapsed, .. } if collapsed == expected
        ));
    }
}
