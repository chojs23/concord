use std::sync::atomic::AtomicBool;

use super::CaptureOutput;
use crate::{
    discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

#[path = "linux/portal.rs"]
mod portal;
#[path = "linux/x11.rs"]
mod x11;

pub(super) struct CaptureSession(CaptureSessionInner);

enum CaptureSessionInner {
    Portal(portal::CaptureSession),
    X11(x11::CaptureSession),
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let mut targets = Vec::new();

    if is_native_x11_session() {
        match x11::list_targets() {
            Ok(x11_targets) => targets.extend(x11_targets),
            Err(error) => logging::debug(
                "stream",
                format!("native X11 capture target discovery failed: {error}"),
            ),
        }
    }

    targets.extend(portal::list_targets()?);
    Ok(targets)
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    stop: &AtomicBool,
) -> Result<(CaptureSession, CaptureOutput), String> {
    match target.kind {
        StreamCaptureTargetKind::Portal => {
            portal::start_capture(target, stop).map(|(session, output)| {
                (CaptureSession(CaptureSessionInner::Portal(session)), output)
            })
        }
        StreamCaptureTargetKind::Display | StreamCaptureTargetKind::Window => {
            x11::start_capture(target, stop).map(|(session, output)| {
                (CaptureSession(CaptureSessionInner::X11(session)), output)
            })
        }
    }
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        match &mut self.0 {
            CaptureSessionInner::Portal(session) => session.stop(),
            CaptureSessionInner::X11(session) => session.stop(),
        }
    }
}

fn is_native_x11_session() -> bool {
    is_native_x11_environment(
        std::env::var_os("DISPLAY").is_some(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
    )
}

fn is_native_x11_environment(
    has_display: bool,
    session_type: Option<&str>,
    has_wayland_display: bool,
) -> bool {
    if !has_display {
        return false;
    }

    match session_type {
        Some(session_type) if session_type.eq_ignore_ascii_case("x11") => true,
        Some(session_type) if session_type.eq_ignore_ascii_case("wayland") => false,
        _ => !has_wayland_display,
    }
}

#[cfg(test)]
mod tests {
    use super::is_native_x11_environment;

    #[test]
    fn native_x11_detection_excludes_wayland_and_xwayland_sessions() {
        let cases = [
            (true, Some("x11"), false, true),
            (true, None, false, true),
            (true, Some("tty"), false, true),
            (true, Some("wayland"), true, false),
            (true, None, true, false),
            (false, Some("x11"), false, false),
        ];

        for (has_display, session_type, has_wayland_display, expected) in cases {
            assert_eq!(
                is_native_x11_environment(has_display, session_type, has_wayland_display),
                expected,
                "display={has_display} session={session_type:?} wayland={has_wayland_display}",
            );
        }
    }
}
