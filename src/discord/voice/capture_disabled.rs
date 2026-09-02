use tokio::sync::mpsc;

use super::{STREAM_BROADCAST_FEATURE_DISABLED, StreamCaptureTarget, preview::StreamPreviewFrame};

pub(super) const STREAM_CAPTURE_WIDTH: u32 = 1280;
pub(super) const STREAM_CAPTURE_HEIGHT: u32 = 720;
pub(super) const STREAM_CAPTURE_FPS: u32 = 30;
pub(super) const STREAM_TRANSPORT_BITRATE: u32 = 8_000_000;

// Keep the broadcast runtime interface stable in reduced builds. Capture
// startup always fails before either of these values can be constructed.
#[derive(Debug)]
pub(super) struct EncodedStreamFrame {
    pub(super) timestamp: u32,
    pub(super) annex_b: Vec<u8>,
    pub(super) is_keyframe: bool,
}

pub(super) struct StreamCaptureHandle;

pub(super) use super::capture_cancellation::StreamCaptureCancellation;

pub(super) struct PreparedStreamCapture {
    pub(super) handle: StreamCaptureHandle,
    pub(super) frames: mpsc::Receiver<Result<EncodedStreamFrame, String>>,
    pub(super) preview_frames: Option<mpsc::Receiver<StreamPreviewFrame>>,
    pub(super) errors: mpsc::UnboundedReceiver<String>,
}

impl StreamCaptureHandle {
    pub(super) fn request_keyframe(&self) {
        // Disabled builds never construct a capture handle.
    }

    pub(super) fn set_keyframe_interval(&self, _milliseconds: Option<u64>) {
        // Disabled builds never construct a capture handle.
    }

    pub(super) async fn shutdown(self) {
        // Disabled builds never construct a capture handle.
    }
}

pub(crate) fn list_stream_capture_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
}

pub(super) fn prepare_stream_capture(
    _target: StreamCaptureTarget,
    _cancellation: StreamCaptureCancellation,
) -> Result<PreparedStreamCapture, String> {
    Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_targets_report_the_disabled_feature() {
        assert_eq!(
            list_stream_capture_targets(),
            Err(STREAM_BROADCAST_FEATURE_DISABLED.to_owned())
        );
    }
}
