mod keyboard;
mod mouse;

use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use super::state::DashboardState;

pub use self::keyboard::handle_key;
pub type MouseClickTracker = self::mouse::MouseClickTracker;
pub type MouseOutcome = self::mouse::MouseOutcome;

pub fn handle_mouse_event(
    state: &mut DashboardState,
    mouse: MouseEvent,
    area: Rect,
    clicks: &mut MouseClickTracker,
) -> MouseOutcome {
    self::mouse::handle_mouse_event(state, mouse, area, clicks)
}

#[cfg(test)]
pub fn handle_mouse(state: &mut DashboardState, mouse: MouseEvent, area: Rect) -> bool {
    self::mouse::handle_mouse(state, mouse, area)
}

#[cfg(test)]
mod tests;
