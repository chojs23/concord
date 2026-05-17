use std::{env, fmt, io::stdout};

use crossterm::clipboard::CopyToClipboard;

#[derive(Default)]
pub(super) struct ClipboardService {
    native: Option<arboard::Clipboard>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyTextBackend {
    Native,
    Osc52,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ClipboardError {
    details: String,
}

impl ClipboardService {
    pub(super) fn copy_text(&mut self, content: &str) -> Result<CopyTextBackend, ClipboardError> {
        let mut failures = Vec::new();
        for backend in copy_text_backend_order(is_remote_session()) {
            let result = match backend {
                CopyTextBackend::Native => self.copy_text_native(content),
                CopyTextBackend::Osc52 => copy_text_osc52(content),
            };
            match result {
                Ok(()) => return Ok(backend),
                Err(error) => failures.push(error),
            }
        }

        Err(ClipboardError {
            details: failures.join("; "),
        })
    }

    fn copy_text_native(&mut self, content: &str) -> Result<(), String> {
        if self.native.is_none() {
            self.native = Some(
                arboard::Clipboard::new()
                    .map_err(|error| format!("native clipboard unavailable: {error}"))?,
            );
        }

        let clipboard = self
            .native
            .as_mut()
            .expect("native clipboard was initialized above");
        if let Err(error) = clipboard.set_text(content) {
            self.native = None;
            return Err(format!("native clipboard write failed: {error}"));
        }
        Ok(())
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.details)
    }
}

fn copy_text_osc52(content: &str) -> Result<(), String> {
    crossterm::execute!(stdout(), CopyToClipboard::to_clipboard_from(content))
        .map_err(|error| format!("OSC52 clipboard write failed: {error}"))
}

fn copy_text_backend_order(remote_session: bool) -> [CopyTextBackend; 2] {
    if remote_session {
        [CopyTextBackend::Osc52, CopyTextBackend::Native]
    } else {
        [CopyTextBackend::Native, CopyTextBackend::Osc52]
    }
}

fn is_remote_session() -> bool {
    env::var_os("SSH_CONNECTION").is_some() || env::var_os("SSH_TTY").is_some()
}

#[cfg(test)]
mod tests {
    use super::{CopyTextBackend, copy_text_backend_order};

    #[test]
    fn local_sessions_try_native_clipboard_before_osc52() {
        assert_eq!(
            copy_text_backend_order(false),
            [CopyTextBackend::Native, CopyTextBackend::Osc52]
        );
    }

    #[test]
    fn remote_sessions_try_osc52_before_native_clipboard() {
        assert_eq!(
            copy_text_backend_order(true),
            [CopyTextBackend::Osc52, CopyTextBackend::Native]
        );
    }
}
