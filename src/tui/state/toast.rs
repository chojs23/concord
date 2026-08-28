use std::time::{Duration, Instant};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::discord::{MediaPlaybackRequestId, VoiceScope};

use super::{
    DashboardState, MediaPlaybackPreparingUiState, StreamBroadcastUiTarget, StreamPlaybackUiTarget,
    ToastKind, ToastMessage, ToastView,
};

const TOAST_DURATION: Duration = Duration::from_secs(2);
/// Captcha guidance points the user to another client, so it lingers longer
/// than the usual status toast.
const CAPTCHA_TOAST_DURATION: Duration = Duration::from_secs(8);
const MEDIA_PLAYBACK_DISABLED_TEXT: &str = "Media playback is disabled in Display options.";
const MEDIA_PLAYBACK_PREPARING_TEXT: &str = "Preparing media playback...";
const STREAM_PLAYBACK_PREPARING_TEXT: &str = "Preparing stream playback...";
const STREAM_BROADCAST_PREPARING_TEXT: &str = "Preparing screen share...";
const STREAM_CAPTURE_TARGETS_LOADING_TEXT: &str = "Loading screens and windows...";

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

    pub(in crate::tui) fn show_media_playback_disabled_toast(&mut self, now: Instant) {
        self.show_toast(MEDIA_PLAYBACK_DISABLED_TEXT, ToastKind::Info, now);
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

    pub(in crate::tui) fn show_stream_playback_preparing_toast(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> bool {
        let target = StreamPlaybackUiTarget {
            scope,
            channel_id,
            user_id,
        };
        if self.runtime.active_stream_playback == Some(target) {
            return false;
        }

        self.runtime.media_playback_preparing = None;
        self.runtime.active_stream_playback = None;
        self.runtime.stream_playback_preparing = Some(target);
        self.runtime.toast_message = Some(ToastMessage {
            text: STREAM_PLAYBACK_PREPARING_TEXT.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        });
        true
    }

    pub(in crate::tui) fn clear_stream_playback_preparing(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> bool {
        let target = StreamPlaybackUiTarget {
            scope,
            channel_id,
            user_id,
        };
        if self.runtime.stream_playback_preparing.as_ref() != Some(&target) {
            return false;
        }

        self.runtime.stream_playback_preparing = None;
        self.runtime.active_stream_playback = Some(target);
        self.clear_owned_preparing_toast(STREAM_PLAYBACK_PREPARING_TEXT)
    }

    pub(in crate::tui) fn record_stream_playback_ended(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        reconnecting: bool,
    ) -> bool {
        let target = StreamPlaybackUiTarget {
            scope,
            channel_id,
            user_id,
        };
        let tracked = self.runtime.active_stream_playback == Some(target)
            || self.runtime.stream_playback_preparing == Some(target);
        if !tracked {
            return false;
        }

        self.runtime.active_stream_playback = None;
        if reconnecting {
            return self.show_stream_playback_preparing_toast(scope, channel_id, user_id);
        }

        let was_preparing = self.runtime.stream_playback_preparing == Some(target);
        if was_preparing {
            self.runtime.stream_playback_preparing = None;
        }
        was_preparing && self.clear_owned_preparing_toast(STREAM_PLAYBACK_PREPARING_TEXT)
    }

    pub(in crate::tui) fn show_media_playback_preparing_toast(
        &mut self,
        request_id: MediaPlaybackRequestId,
        url: String,
    ) {
        self.runtime.stream_playback_preparing = None;
        self.runtime.media_playback_preparing =
            Some(MediaPlaybackPreparingUiState { request_id, url });
        self.runtime.toast_message = Some(ToastMessage {
            text: MEDIA_PLAYBACK_PREPARING_TEXT.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        });
    }

    pub(in crate::tui) fn show_stream_broadcast_preparing_toast(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        if self
            .runtime
            .active_stream_broadcast
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id))
        {
            return false;
        }
        let target = StreamBroadcastUiTarget { scope, channel_id };

        self.runtime.media_playback_preparing = None;
        self.runtime.active_stream_broadcast = None;
        self.runtime.stream_broadcast_preparing = Some(target);
        self.runtime.toast_message = Some(ToastMessage {
            text: STREAM_BROADCAST_PREPARING_TEXT.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        });
        true
    }

    pub(in crate::tui) fn clear_stream_broadcast_preparing(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        if !self
            .runtime
            .stream_broadcast_preparing
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id))
        {
            return false;
        }

        let target = self
            .runtime
            .stream_broadcast_preparing
            .take()
            .expect("matching broadcast preparation exists");
        self.runtime.active_stream_broadcast = Some(target);
        self.clear_owned_preparing_toast(STREAM_BROADCAST_PREPARING_TEXT)
    }

    pub(in crate::tui) fn cancel_stream_broadcast_preparing(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        if !self
            .runtime
            .stream_broadcast_preparing
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id))
        {
            return false;
        }

        self.runtime.stream_broadcast_preparing = None;
        self.clear_owned_preparing_toast(STREAM_BROADCAST_PREPARING_TEXT)
    }

    pub(in crate::tui) fn record_stream_broadcast_ended(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        let active_matches = self
            .runtime
            .active_stream_broadcast
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id));
        let preparing_matches = self
            .runtime
            .stream_broadcast_preparing
            .as_ref()
            .is_some_and(|target| target.matches(scope, channel_id));
        let tracked = active_matches || preparing_matches;
        if !tracked {
            return false;
        }

        if active_matches {
            self.runtime.active_stream_broadcast = None;
        }
        let was_preparing = preparing_matches;
        if preparing_matches {
            self.runtime.stream_broadcast_preparing = None;
        }
        was_preparing && self.clear_owned_preparing_toast(STREAM_BROADCAST_PREPARING_TEXT)
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
        self.clear_owned_preparing_toast(MEDIA_PLAYBACK_PREPARING_TEXT)
    }

    pub(in crate::tui) fn show_stream_capture_targets_loading_toast(&mut self) {
        self.runtime.toast_message = Some(ToastMessage {
            text: STREAM_CAPTURE_TARGETS_LOADING_TEXT.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        });
    }

    pub(in crate::tui) fn clear_stream_capture_targets_loading_toast(&mut self) -> bool {
        self.clear_owned_preparing_toast(STREAM_CAPTURE_TARGETS_LOADING_TEXT)
    }

    pub(in crate::tui) fn clear_expired_toast(&mut self, now: Instant) -> bool {
        if self
            .runtime
            .toast_message
            .as_ref()
            .and_then(|message| message.expires_at)
            .is_some_and(|expires_at| expires_at <= now)
        {
            self.runtime.toast_message = self.pending_activity_toast();
            return true;
        }
        false
    }

    fn clear_owned_preparing_toast(&mut self, text: &str) -> bool {
        if !self
            .runtime
            .toast_message
            .as_ref()
            .is_some_and(|message| message.expires_at.is_none() && message.text == text)
        {
            return false;
        }

        self.runtime.toast_message = self.pending_activity_toast();
        true
    }

    fn pending_activity_toast(&self) -> Option<ToastMessage> {
        let text = if self.runtime.stream_capture_targets_request.is_some() {
            STREAM_CAPTURE_TARGETS_LOADING_TEXT
        } else if self.runtime.stream_broadcast_preparing.is_some() {
            STREAM_BROADCAST_PREPARING_TEXT
        } else if self.runtime.stream_playback_preparing.is_some() {
            STREAM_PLAYBACK_PREPARING_TEXT
        } else if self.runtime.media_playback_preparing.is_some() {
            MEDIA_PLAYBACK_PREPARING_TEXT
        } else {
            return None;
        };
        Some(ToastMessage {
            text: text.to_owned(),
            kind: ToastKind::Info,
            expires_at: None,
        })
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
    fn unrelated_error_does_not_cancel_stream_preparation() {
        let now = Instant::now();
        let scope = VoiceScope::Guild(Id::new(1));
        let channel_id = Id::new(2);
        let user_id = Id::new(3);

        let mut playback_state = DashboardState::new();
        assert!(playback_state.show_stream_playback_preparing_toast(scope, channel_id, user_id));
        playback_state.show_error_toast("Failed to copy message", now);
        assert!(playback_state.clear_expired_toast(now + TOAST_DURATION));
        assert_eq!(
            playback_state
                .toast_message()
                .expect("playback preparation remains tracked")
                .text,
            STREAM_PLAYBACK_PREPARING_TEXT
        );

        let mut broadcast_state = DashboardState::new();
        assert!(broadcast_state.show_stream_broadcast_preparing_toast(scope, channel_id));
        broadcast_state.show_error_toast("Failed to copy message", now);
        assert!(broadcast_state.clear_expired_toast(now + TOAST_DURATION));
        assert_eq!(
            broadcast_state
                .toast_message()
                .expect("broadcast preparation remains tracked")
                .text,
            STREAM_BROADCAST_PREPARING_TEXT
        );

        let mut capture_targets_state = DashboardState::new();
        capture_targets_state.begin_stream_capture_targets_request(scope, channel_id);
        capture_targets_state.show_error_toast("Failed to copy message", now);
        assert!(capture_targets_state.clear_expired_toast(now + TOAST_DURATION));
        assert_eq!(
            capture_targets_state
                .toast_message()
                .expect("capture target loading remains tracked")
                .text,
            STREAM_CAPTURE_TARGETS_LOADING_TEXT
        );
    }

    #[test]
    fn capture_target_loading_failure_replaces_the_loading_toast() {
        let mut state = DashboardState::new();
        let scope = VoiceScope::Guild(Id::new(1));
        let channel_id = Id::new(2);
        let request_id = state.begin_stream_capture_targets_request(scope, channel_id);

        state.push_effect(AppEvent::StreamCaptureTargetsLoaded {
            request_id,
            scope,
            channel_id,
            targets: Vec::new(),
            error: Some("ScreenCaptureKit target loading failed".to_owned()),
        });

        let toast = state
            .toast_message()
            .expect("capture target error is visible");
        assert_eq!(toast.text, "ScreenCaptureKit target loading failed");
        assert_eq!(toast.kind, ToastKind::Error);
        assert!(state.next_toast_deadline().is_some());
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

    #[test]
    fn stream_playback_preparing_toast_waits_for_matching_ready_event() {
        let mut state = DashboardState::new();
        let now = Instant::now();
        let scope = VoiceScope::Guild(Id::new(1));
        let channel_id = Id::new(2);
        let user_id = Id::new(3);
        let media_request_id = MediaPlaybackRequestId::new(1);

        state.show_media_playback_preparing_toast(
            media_request_id,
            "https://example.com/video.mp4".to_owned(),
        );
        assert!(state.show_stream_playback_preparing_toast(scope, channel_id, user_id));

        let toast = state.toast_message().expect("preparing toast is visible");
        assert_eq!(toast.text, STREAM_PLAYBACK_PREPARING_TEXT);
        assert_eq!(toast.kind, ToastKind::Info);
        assert_eq!(state.next_toast_deadline(), None);
        state.show_success_toast("Voice connected", now);
        assert!(state.clear_expired_toast(now + TOAST_DURATION));
        assert_eq!(
            state
                .toast_message()
                .expect("preparing toast is restored")
                .text,
            STREAM_PLAYBACK_PREPARING_TEXT
        );
        state.push_event(AppEvent::MediaPlaybackWindowReady {
            request_id: media_request_id,
            url: "https://example.com/video.mp4".to_owned(),
        });
        assert!(state.toast_message().is_some());
        state.push_event(AppEvent::StreamPlaybackWindowReady {
            scope,
            channel_id,
            user_id: Id::new(4),
        });
        assert!(state.toast_message().is_some());
        state.push_event(AppEvent::StreamPlaybackWindowReady {
            scope,
            channel_id,
            user_id,
        });
        assert!(state.toast_message().is_none());
        assert!(!state.show_stream_playback_preparing_toast(scope, channel_id, user_id));
        assert!(state.toast_message().is_none());
        state.push_event(AppEvent::StreamPlaybackEnded {
            scope,
            channel_id,
            user_id,
            reconnecting: false,
        });
        assert!(state.show_stream_playback_preparing_toast(scope, channel_id, user_id));
        state.push_event(AppEvent::StreamPlaybackWindowReady {
            scope,
            channel_id,
            user_id,
        });
        state.push_event(AppEvent::StreamPlaybackEnded {
            scope,
            channel_id,
            user_id,
            reconnecting: true,
        });
        assert_eq!(
            state
                .toast_message()
                .expect("reconnect preparing toast is visible")
                .text,
            STREAM_PLAYBACK_PREPARING_TEXT
        );
    }

    #[test]
    fn stream_broadcast_preparing_toast_tracks_matching_broadcast_lifecycle() {
        let mut state = DashboardState::new();
        let scope = VoiceScope::Guild(Id::new(1));
        let channel_id = Id::new(2);

        assert!(state.show_stream_broadcast_preparing_toast(scope, channel_id));
        assert_eq!(
            state
                .toast_message()
                .expect("broadcast preparing toast is visible")
                .text,
            STREAM_BROADCAST_PREPARING_TEXT
        );
        state.push_event(AppEvent::StreamBroadcastStartFailed {
            scope,
            channel_id: Id::new(3),
        });
        assert!(state.toast_message().is_some());
        state.push_event(AppEvent::StreamBroadcastStartFailed { scope, channel_id });
        assert!(state.toast_message().is_none());

        assert!(state.show_stream_broadcast_preparing_toast(scope, channel_id));
        state.push_event(AppEvent::StreamBroadcastStarted {
            scope,
            channel_id: Id::new(3),
        });
        assert!(state.toast_message().is_some());
        state.push_event(AppEvent::StreamBroadcastStarted { scope, channel_id });
        assert!(state.toast_message().is_none());
        assert!(!state.show_stream_broadcast_preparing_toast(scope, channel_id));
        state.push_event(AppEvent::StreamBroadcastStartFailed { scope, channel_id });
        assert!(!state.show_stream_broadcast_preparing_toast(scope, channel_id));

        state.push_event(AppEvent::StreamBroadcastEnded { scope, channel_id });
        assert!(state.show_stream_broadcast_preparing_toast(scope, channel_id));
    }
}
