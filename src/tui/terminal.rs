use std::io::stdout;

use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};

use crate::Result;

pub(in crate::tui) struct TerminalRestoreGuard {
    keyboard_enhancement_enabled: bool,
    mouse_capture_enabled: bool,
    bracketed_paste_enabled: bool,
}

impl TerminalRestoreGuard {
    pub(in crate::tui) fn new() -> Result<Self> {
        // Kitty progressive enhancement isn't supported on every terminal
        // (e.g. legacy Windows console). Fall back silently when unavailable
        // so the app still runs with basic key handling.
        let keyboard_enhancement_enabled = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
        let mouse_capture_enabled = execute!(stdout(), EnableMouseCapture).is_ok();
        let bracketed_paste_enabled = execute!(stdout(), EnableBracketedPaste).is_ok();
        Ok(Self {
            keyboard_enhancement_enabled,
            mouse_capture_enabled,
            bracketed_paste_enabled,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        if self.mouse_capture_enabled {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        if self.bracketed_paste_enabled {
            let _ = execute!(stdout(), DisableBracketedPaste);
        }
        ratatui::restore();
    }
}
