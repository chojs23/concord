use std::time::{Duration, Instant};

use super::{DashboardState, ToastKind, ToastMessage, ToastView};

const TOAST_DURATION: Duration = Duration::from_secs(2);

impl DashboardState {
    pub(in crate::tui) fn show_success_toast(&mut self, text: impl Into<String>, now: Instant) {
        self.show_toast(text, ToastKind::Success, now);
    }

    pub(in crate::tui) fn show_error_toast(&mut self, text: impl Into<String>, now: Instant) {
        self.show_toast(text, ToastKind::Error, now);
    }

    fn show_toast(&mut self, text: impl Into<String>, kind: ToastKind, now: Instant) {
        self.toast_message = Some(ToastMessage {
            text: text.into(),
            kind,
            expires_at: now + TOAST_DURATION,
        });
    }

    pub(in crate::tui) fn clear_expired_toast(&mut self, now: Instant) -> bool {
        if self
            .toast_message
            .as_ref()
            .is_some_and(|message| message.expires_at <= now)
        {
            self.toast_message = None;
            return true;
        }
        false
    }

    pub(in crate::tui) fn next_toast_deadline(&self) -> Option<Instant> {
        self.toast_message
            .as_ref()
            .map(|message| message.expires_at)
    }

    pub fn toast_message(&self) -> Option<ToastView<'_>> {
        self.toast_message.as_ref().map(|message| ToastView {
            text: &message.text,
            kind: message.kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn toast_expires_after_two_seconds() {
        let mut state = DashboardState::new();
        let now = Instant::now();

        state.show_success_toast("Message copied", now);

        assert_eq!(
            state.toast_message().expect("toast is visible").text,
            "Message copied"
        );
        assert_eq!(state.next_toast_deadline(), Some(now + TOAST_DURATION));
        assert!(!state.clear_expired_toast(now + TOAST_DURATION - Duration::from_millis(1)));
        assert!(state.toast_message().is_some());
        assert!(state.clear_expired_toast(now + TOAST_DURATION));
        assert!(state.toast_message().is_none());
    }

    #[test]
    fn newer_toast_replaces_previous_toast() {
        let mut state = DashboardState::new();
        let now = Instant::now();

        state.show_success_toast("Message copied", now);
        state.show_error_toast("Failed to copy message", now + Duration::from_secs(1));

        let toast = state.toast_message().expect("toast is visible");
        assert_eq!(toast.text, "Failed to copy message");
        assert_eq!(toast.kind, ToastKind::Error);
        assert_eq!(
            state.next_toast_deadline(),
            Some(now + Duration::from_secs(1) + TOAST_DURATION)
        );
    }
}
