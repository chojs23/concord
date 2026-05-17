use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    Result, config,
    discord::{AppCommand, AppEvent},
    logging,
};

use super::{clipboard::ClipboardService, input, state::DashboardState};

pub(super) struct TerminalEventOutcome {
    pub(super) dirty: bool,
    pub(super) command: Option<AppCommand>,
}

pub(super) fn handle_terminal_event(
    state: &mut DashboardState,
    clipboard: &mut ClipboardService,
    event: TerminalEvent,
    last_frame_area: &mut Rect,
    mouse_clicks: &mut input::MouseClickTracker,
) -> Result<TerminalEventOutcome> {
    let mut outcome = TerminalEventOutcome {
        dirty: false,
        command: None,
    };

    match event {
        TerminalEvent::Key(key) => {
            if key.kind == KeyEventKind::Press && handle_native_paste_key(state, clipboard, key) {
                outcome.dirty = true;
            } else {
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
        _ => {}
    }

    Ok(outcome)
}

fn handle_native_paste_key(
    state: &mut DashboardState,
    clipboard: &mut ClipboardService,
    key: KeyEvent,
) -> bool {
    if !is_native_paste_key(key) || !state.is_composing() {
        return false;
    }

    if state.composer_accepts_attachments() {
        match clipboard.clipboard_file_uploads() {
            Ok(attachments) => {
                state.add_pending_composer_attachments(attachments);
                state.show_success_toast("Clipboard file attached", std::time::Instant::now());
                return true;
            }
            Err(error) => logging::error("tui", format!("native file paste failed: {error}")),
        }

        match clipboard.clipboard_image_upload() {
            Ok(attachment) => {
                state.add_pending_composer_attachments(vec![attachment]);
                state.show_success_toast("Clipboard image attached", std::time::Instant::now());
                return true;
            }
            Err(error) => logging::error("tui", format!("native image paste failed: {error}")),
        }
    }

    match clipboard.clipboard_text() {
        Ok(text) if input::handle_paste(state, &text) => true,
        Ok(_) => false,
        Err(error) => {
            logging::error("tui", format!("native text paste failed: {error}"));
            state.show_error_toast("No clipboard content", std::time::Instant::now());
            true
        }
    }
}

fn is_native_paste_key(key: KeyEvent) -> bool {
    let paste_modifier = key.modifiers.contains(KeyModifiers::CONTROL);
    matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V')) && paste_modifier
}

fn save_options_if_needed(state: &mut DashboardState) {
    let Some(options) = state.take_options_save_request() else {
        return;
    };

    match config::save_options(&options) {
        Ok(()) => {}
        Err(error) => state.push_effect(AppEvent::GatewayError {
            message: format!("save options failed: {error}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::is_native_paste_key;

    #[test]
    fn recognizes_native_clipboard_paste_keys() {
        assert!(is_native_paste_key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL,
        )));
        assert!(is_native_paste_key(KeyEvent::new(
            KeyCode::Char('V'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )));
    }

    #[test]
    fn ignores_unmodified_v_as_native_paste_key() {
        assert!(!is_native_paste_key(KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
        )));
    }
}
