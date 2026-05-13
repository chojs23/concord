use crossterm::event::{Event as TerminalEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::{
    Result, config,
    discord::{AppCommand, AppEvent},
};

use super::{input, redraw::RedrawDiagnostics, state::DashboardState};

pub(super) struct TerminalEventOutcome {
    pub(super) dirty: bool,
    pub(super) command: Option<AppCommand>,
}

pub(super) fn handle_terminal_event(
    state: &mut DashboardState,
    event: TerminalEvent,
    last_frame_area: &mut Rect,
    mouse_clicks: &mut input::MouseClickTracker,
    redraw_diagnostics: &mut RedrawDiagnostics,
) -> Result<TerminalEventOutcome> {
    let mut outcome = TerminalEventOutcome {
        dirty: false,
        command: None,
    };

    match event {
        TerminalEvent::Key(key) => {
            outcome.command = input::handle_key(state, key);
            if key.kind == KeyEventKind::Press {
                save_display_options_if_needed(state);
                redraw_diagnostics.key_presses = redraw_diagnostics.key_presses.saturating_add(1);
                outcome.dirty = true;
            }
        }
        TerminalEvent::Mouse(mouse) => {
            let mouse_outcome =
                input::handle_mouse_event(state, mouse, *last_frame_area, mouse_clicks);
            outcome.command = mouse_outcome.command;
            if mouse_outcome.handled {
                redraw_diagnostics.mouse_events = redraw_diagnostics.mouse_events.saturating_add(1);
                outcome.dirty = true;
            }
        }
        TerminalEvent::Resize(width, height) => {
            *last_frame_area = Rect::new(0, 0, width, height);
            redraw_diagnostics.resizes = redraw_diagnostics.resizes.saturating_add(1);
            outcome.dirty = true;
        }
        TerminalEvent::Paste(text) if input::handle_paste(state, &text) => {
            outcome.dirty = true;
        }
        _ => {}
    }

    Ok(outcome)
}

fn save_display_options_if_needed(state: &mut DashboardState) {
    let Some(options) = state.take_display_options_save_request() else {
        return;
    };

    match config::save_display_options(&options) {
        Ok(()) => {}
        Err(error) => state.push_effect(AppEvent::GatewayError {
            message: format!("save options failed: {error}"),
        }),
    }
}
