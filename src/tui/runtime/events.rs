use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::{Result, config, discord::AppEvent};

use super::super::{input, state::DashboardState};
use crate::discord::AppCommand;

#[derive(Default)]
pub(super) struct TerminalEventOutcome {
    pub(super) dirty: bool,
    pub(super) command: Option<AppCommand>,
}

pub(super) fn handle_terminal_event(
    state: &mut DashboardState,
    event: TerminalEvent,
    last_frame_area: &mut Rect,
    mouse_clicks: &mut input::MouseClickTracker,
) -> Result<TerminalEventOutcome> {
    let mut outcome = TerminalEventOutcome::default();

    match event {
        TerminalEvent::Key(key) => {
            if key.kind == KeyEventKind::Press {
                outcome.command = input::handle_key(state, key);
            }
            if key.kind == KeyEventKind::Press {
                save_options_if_needed(state);
                outcome.dirty = true;
            }
        }
        TerminalEvent::Mouse(mouse) => {
            let mouse_outcome =
                input::handle_mouse_event(state, mouse, *last_frame_area, mouse_clicks);
            outcome.command = mouse_outcome.command;
            if mouse_outcome.handled {
                save_options_if_needed(state);
                outcome.dirty = true;
            }
        }
        TerminalEvent::Resize(width, height) => {
            *last_frame_area = Rect::new(0, 0, width, height);
            outcome.dirty = true;
        }
        TerminalEvent::Paste(text) if input::handle_paste(state, &text) => {
            outcome.dirty = true;
        }
        TerminalEvent::FocusGained => state.set_terminal_focused(true),
        TerminalEvent::FocusLost => state.set_terminal_focused(false),
        _ => {}
    }

    Ok(outcome)
}

fn save_options_if_needed(state: &mut DashboardState) {
    if let Some(options) = state.take_options_save_request() {
        if let Err(error) = config::save_options(&options) {
            state.push_effect(AppEvent::GatewayError {
                message: format!("save options failed: {error}"),
            });
        }
    }

    if let Some(ui_state) = state.take_ui_state_save_request() {
        if let Err(error) = config::save_ui_state_options(&ui_state) {
            state.push_effect(AppEvent::GatewayError {
                message: format!("save UI state failed: {error}"),
            });
        }
    }
}
