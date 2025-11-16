use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::state::{DashboardState, EventFilter};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('c') => state.clear(),
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_up(),
        KeyCode::Char('g') => state.jump_top(),
        KeyCode::Char('G') => state.jump_bottom(),
        KeyCode::Tab => state.cycle_focus(),
        KeyCode::Char('1') => state.set_filter(EventFilter::All),
        KeyCode::Char('2') => state.set_filter(EventFilter::Messages),
        KeyCode::Char('3') => state.set_filter(EventFilter::Gateway),
        KeyCode::Char('4') => state.set_filter(EventFilter::Errors),
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
