use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::state::{DashboardState, EventFilter};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) {
    if key.kind != KeyEventKind::Press {
        return;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('c') => state.clear(),
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
}
