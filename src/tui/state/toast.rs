use std::time::{Duration, Instant};

use crate::discord::MediaPlaybackRequestId;

use super::{DashboardState, MediaPlaybackPreparingUiState, ToastKind, ToastMessage, ToastView};

const TOAST_DURATION: Duration = Duration::from_secs(2);
/// Captcha guidance points the user to another client, so it lingers longer
/// than the usual status toast.
const CAPTCHA_TOAST_DURATION: Duration = Duration::from_secs(8);
const MEDIA_PLAYBACK_PREPARING_TEXT: &str = "Preparing media playback...";

impl DashboardState {
    pub(in crate::tui) fn show_success_toast(&mut self, text: impl Into<String>, now: Instant) {
        self.show_toast(text, ToastKind::Success, now);
    }

    pub(in crate::tui) fn show_error_toast(&mut self, text: impl Into<String>, now: Instant) {
        self.show_toast(text, ToastKind::Error, now);
    }

    pub(in crate::tui) fn show_captcha_toast(&mut self, text: impl Into<String>, now: Instant) {
        self.show_toast_for(text, ToastKind::Error, CAPTCHA_TOAST_DURATION, now);
    }

    fn show_toast(&mut self, text: impl Into<String>, kind: ToastKind, now: Instant) {
        self.show_toast_for(text, kind, TOAST_DURATION, now);
    }

    fn show_toast_for(
        &mut self,
        text: impl Into<String>,
        kind: ToastKind,
        duration: Duration,
        now: Instant,
    ) {
        self.runtime.media_playback_preparing = None;
        self.runtime.toast_message = Some(ToastMessage {
            text: text.into(),
            kind,
            expires_at: Some(now + duration),
        });
    }

    pub(in crate::tui) fn show_media_playback_preparing_toast(
        &mut self,
        request_id: MediaPlaybackRequestId,
        url: String,
    ) {
        self.runtime.media_playback_preparing =
            Some(MediaPlaybackPreparingUiState { request_id, url });
        self.runtime.toast_message = Some(ToastMessage {
            text: MEDIA_PLAYBACK_PREPARING_TEXT.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        });
    }

    pub(in crate::tui) fn clear_media_playback_preparing(
        &mut self,
        request_id: MediaPlaybackRequestId,
    ) -> bool {
        if self
            .runtime
            .media_playback_preparing
            .as_ref()
            .map(|preparing| preparing.request_id)
            != Some(request_id)
        {
            return false;
        }

        self.runtime.media_playback_preparing = None;
        if self
            .runtime
            .toast_message
            .as_ref()
            .is_some_and(|message| message.expires_at.is_none())
        {
            self.runtime.toast_message = None;
            return true;
        }
        false
    }

    pub(in crate::tui) fn clear_expired_toast(&mut self, now: Instant) -> bool {
        if self
            .runtime
            .toast_message
            .as_ref()
            .and_then(|message| message.expires_at)
            .is_some_and(|expires_at| expires_at <= now)
        {
            self.runtime.toast_message = None;
            return true;
        }
        false
    }

    pub(in crate::tui) fn next_toast_deadline(&self) -> Option<Instant> {
        self.runtime
            .toast_message
            .as_ref()
            .and_then(|message| message.expires_at)
    }

    pub fn toast_message(&self) -> Option<ToastView<'_>> {
        self.runtime
            .toast_message
            .as_ref()
            .map(|message| ToastView {
                text: &message.text,
                kind: message.kind,
            })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::discord::AppEvent;

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
    fn captcha_required_event_shows_guidance_toast_without_gateway_banner() {
        let mut state = DashboardState::new();

        state.push_event(AppEvent::CaptchaRequired {
            action: "send message".to_owned(),
        });

        let toast = state.toast_message().expect("captcha toast is visible");
        assert!(toast.text.contains("CAPTCHA"));
        assert_eq!(toast.kind, ToastKind::Error);
        // A captcha is per-action, not a connection failure, so the persistent
        // gateway-error banner must stay clear.
        assert!(state.gateway_error().is_none());
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

    #[test]
    fn media_playback_preparing_toast_waits_for_matching_ready_event() {
        let mut state = DashboardState::new();
        let now = Instant::now();
        let first_request_id = MediaPlaybackRequestId::new(1);
        let second_request_id = MediaPlaybackRequestId::new(2);

        state.show_media_playback_preparing_toast(
            second_request_id,
            "https://example.com/video.mp4".to_owned(),
        );

        let toast = state.toast_message().expect("preparing toast is visible");
        assert_eq!(toast.text, MEDIA_PLAYBACK_PREPARING_TEXT);
        assert_eq!(toast.kind, ToastKind::Info);
        assert_eq!(state.next_toast_deadline(), None);
        assert!(!state.clear_expired_toast(now + TOAST_DURATION));
        state.push_event(AppEvent::MediaPlaybackWindowReady {
            request_id: first_request_id,
            url: "https://example.com/video.mp4".to_owned(),
        });
        assert!(state.toast_message().is_some());
        state.push_event(AppEvent::MediaPlaybackWindowReady {
            request_id: second_request_id,
            url: "https://example.com/video.mp4".to_owned(),
        });
        assert!(state.toast_message().is_none());
    }
}
