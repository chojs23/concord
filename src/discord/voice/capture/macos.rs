use std::{
    slice,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
    },
    time::{Duration, Instant},
};

use block2::{DynBlock, RcBlock};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
    kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};

use super::{
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS, send_capture_result,
};
use crate::discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind};

const FRAME_QUEUE_CAPACITY: usize = 2;
const CALLBACK_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CALLBACK_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const EXCLUDED_WINDOW_BUNDLE_IDENTIFIERS: [&str; 3] = [
    "com.apple.dock",
    "com.apple.controlcenter",
    "com.apple.systemuiserver",
];

pub(super) struct CaptureSession {
    active: Option<ActiveStream>,
}

struct ActiveStream {
    stream: Retained<SCStream>,
    _output: Retained<MacCaptureOutput>,
    _queue: DispatchRetained<DispatchQueue>,
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let content = shareable_content(None)?;
    let mut targets = Vec::new();

    let displays = unsafe { content.displays() };
    for index in 0..displays.count() {
        let display = displays.objectAtIndex(index);
        let id = u64::from(unsafe { display.displayID() });
        let width = usize::try_from(unsafe { display.width() }).unwrap_or_default();
        let height = usize::try_from(unsafe { display.height() }).unwrap_or_default();
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Display,
            id,
            title: format!("Screen: Display {} ({width}x{height})", index + 1),
        });
    }

    let windows = unsafe { content.windows() };
    for index in 0..windows.count() {
        let window = windows.objectAtIndex(index);
        if !unsafe { window.isOnScreen() } {
            continue;
        }
        let owning_application = unsafe { window.owningApplication() };
        let bundle_identifier = owning_application
            .as_ref()
            .map(|application| unsafe { application.bundleIdentifier() }.to_string());
        if !is_default_shareable_window(
            unsafe { window.windowLayer() },
            bundle_identifier.as_deref(),
        ) {
            continue;
        }
        let frame = unsafe { window.frame() };
        if frame.size.width < 2.0 || frame.size.height < 2.0 {
            continue;
        }
        let Some(title) = (unsafe { window.title() }) else {
            continue;
        };
        let title = title.to_string().trim().to_owned();
        if title.is_empty() {
            continue;
        }
        let app_name = owning_application
            .as_ref()
            .map(|application| unsafe { application.applicationName() }.to_string())
            .unwrap_or_default();
        let label = if app_name.is_empty() || title.starts_with(&app_name) {
            title
        } else {
            format!("{app_name}: {title}")
        };
        targets.push(StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Window,
            id: u64::from(unsafe { window.windowID() }),
            title: format!("Window: {label}"),
        });
    }

    Ok(targets)
}

fn is_default_shareable_window(window_layer: isize, bundle_identifier: Option<&str>) -> bool {
    if window_layer != 0 {
        return false;
    }

    let Some(bundle_identifier) = bundle_identifier else {
        return true;
    };
    let bundle_identifier = bundle_identifier.to_ascii_lowercase();
    !EXCLUDED_WINDOW_BUNDLE_IDENTIFIERS.iter().any(|excluded| {
        bundle_identifier == *excluded
            || bundle_identifier
                .strip_prefix(excluded)
                .is_some_and(|suffix| suffix.starts_with('.'))
    })
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    stop: &AtomicBool,
) -> Result<(CaptureSession, CaptureOutput), String> {
    let content = shareable_content(Some(stop))?;
    let (filter, width, height) = capture_filter(&content, target)?;

    let configuration = unsafe { SCStreamConfiguration::new() };
    unsafe {
        configuration.setWidth(width);
        configuration.setHeight(height);
        configuration.setMinimumFrameInterval(CMTime::new(1, STREAM_CAPTURE_FPS as i32));
        configuration.setPixelFormat(kCVPixelFormatType_32BGRA);
        configuration.setShowsCursor(true);
        configuration.setQueueDepth(3);
    }

    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let (errors_tx, errors_rx) = mpsc::channel();
    let output = MacCaptureOutput::new(frames_tx, errors_tx, CaptureFrameBufferPool::default());
    let delegate = ProtocolObject::<dyn SCStreamDelegate>::from_ref(&*output);
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &configuration,
            Some(delegate),
        )
    };
    let output_protocol = ProtocolObject::<dyn SCStreamOutput>::from_ref(&*output);
    let queue = DispatchQueue::new("concord.stream-video", DispatchQueueAttr::SERIAL);
    unsafe {
        stream.addStreamOutput_type_sampleHandlerQueue_error(
            output_protocol,
            SCStreamOutputType::Screen,
            Some(&queue),
        )
    }
    .map_err(|error| format!("ScreenCaptureKit output setup failed: {error}"))?;

    let active = ActiveStream {
        stream,
        _output: output,
        _queue: queue,
    };
    wait_for_completion(Some(stop), "capture start", |completion| unsafe {
        active
            .stream
            .startCaptureWithCompletionHandler(Some(completion));
    })?;

    Ok((
        CaptureSession {
            active: Some(active),
        },
        CaptureOutput {
            frames: frames_rx,
            errors: errors_rx,
        },
    ))
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };
        wait_for_completion(None, "capture stop", |completion| unsafe {
            active
                .stream
                .stopCaptureWithCompletionHandler(Some(completion));
        })
    }
}

fn shareable_content(stop: Option<&AtomicBool>) -> Result<Retained<SCShareableContent>, String> {
    ensure_not_cancelled(stop, "shareable content request")?;
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let completion: RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let result: Result<usize, String> = if let Some(error) = unsafe { error.as_ref() } {
                Err(format!(
                    "ScreenCaptureKit shareable content request failed: {error}"
                ))
            } else {
                unsafe { Retained::retain(content) }
                    .map(|content| Retained::into_raw(content) as usize)
                    .ok_or_else(|| "ScreenCaptureKit returned no shareable content".to_owned())
            };
            if let Err(mpsc::SendError(Ok(content_address))) = result_tx.send(result) {
                drop(unsafe { Retained::from_raw(content_address as *mut SCShareableContent) });
            }
        },
    );
    unsafe {
        SCShareableContent::getShareableContentWithCompletionHandler(&completion);
    }

    let content_address = receive_callback(&result_rx, stop, "shareable content request")??;
    unsafe { Retained::from_raw(content_address as *mut SCShareableContent) }
        .ok_or_else(|| "ScreenCaptureKit returned invalid shareable content".to_owned())
}

fn capture_filter(
    content: &SCShareableContent,
    target: &StreamCaptureTarget,
) -> Result<(Retained<SCContentFilter>, usize, usize), String> {
    let target_id = u32::try_from(target.id)
        .map_err(|_| format!("capture target has an invalid id: {}", target.id))?;
    match target.kind {
        StreamCaptureTargetKind::Display => {
            let displays = unsafe { content.displays() };
            let display = (0..displays.count())
                .map(|index| displays.objectAtIndex(index))
                .find(|display| unsafe { display.displayID() } == target_id)
                .ok_or_else(|| format!("screen is no longer available: {}", target.title))?;
            let width = usize::try_from(unsafe { display.width() })
                .map_err(|_| format!("screen has an invalid width: {}", target.title))?;
            let height = usize::try_from(unsafe { display.height() })
                .map_err(|_| format!("screen has an invalid height: {}", target.title))?;
            validate_dimensions(width, height, target)?;
            let excluded_windows = NSArray::<SCWindow>::new();
            let filter = unsafe {
                SCContentFilter::initWithDisplay_excludingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &excluded_windows,
                )
            };
            Ok((filter, width, height))
        }
        StreamCaptureTargetKind::Window => {
            let windows = unsafe { content.windows() };
            let window = (0..windows.count())
                .map(|index| windows.objectAtIndex(index))
                .find(|window| unsafe { window.windowID() } == target_id)
                .ok_or_else(|| format!("window is no longer available: {}", target.title))?;
            let frame = unsafe { window.frame() };
            let width = point_dimension(frame.size.width, "width", target)?;
            let height = point_dimension(frame.size.height, "height", target)?;
            let filter = unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
            };
            Ok((filter, width, height))
        }
        StreamCaptureTargetKind::Portal => {
            Err("portal capture targets are only valid on Linux".to_owned())
        }
    }
}

fn point_dimension(value: f64, name: &str, target: &StreamCaptureTarget) -> Result<usize, String> {
    if !value.is_finite() || value < 2.0 || value > usize::MAX as f64 {
        return Err(format!(
            "capture target has an invalid {name}: {}",
            target.title
        ));
    }
    Ok(value.round() as usize)
}

fn validate_dimensions(
    width: usize,
    height: usize,
    target: &StreamCaptureTarget,
) -> Result<(), String> {
    if width < 2 || height < 2 {
        return Err(format!(
            "capture target has invalid dimensions: {}",
            target.title
        ));
    }
    Ok(())
}

fn wait_for_completion(
    stop: Option<&AtomicBool>,
    operation_name: &str,
    operation: impl FnOnce(&DynBlock<dyn Fn(*mut NSError)>),
) -> Result<(), String> {
    ensure_not_cancelled(stop, operation_name)?;
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result = unsafe { error.as_ref() }.map_or(Ok(()), |error| {
            Err(format!("ScreenCaptureKit operation failed: {error}"))
        });
        let _ = result_tx.send(result);
    });
    operation(&completion);
    receive_callback(&result_rx, stop, operation_name)?
}

fn ensure_not_cancelled(stop: Option<&AtomicBool>, operation_name: &str) -> Result<(), String> {
    if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
        Err(format!("ScreenCaptureKit {operation_name} was cancelled"))
    } else {
        Ok(())
    }
}

fn receive_callback<T>(
    result_rx: &Receiver<T>,
    stop: Option<&AtomicBool>,
    operation_name: &str,
) -> Result<T, String> {
    let deadline = Instant::now() + CALLBACK_WAIT_TIMEOUT;
    loop {
        if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
            return Err(format!("ScreenCaptureKit {operation_name} was cancelled"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "ScreenCaptureKit {operation_name} did not complete in time"
            ));
        }
        match result_rx.recv_timeout(remaining.min(CALLBACK_WAIT_POLL_INTERVAL)) {
            Ok(result) => return Ok(result),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "ScreenCaptureKit {operation_name} completion was cancelled"
                ));
            }
        }
    }
}

struct MacCaptureOutputIvars {
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ConcordScreenCaptureVideoOutput"]
    #[ivars = MacCaptureOutputIvars]
    struct MacCaptureOutput;

    unsafe impl SCStreamOutput for MacCaptureOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        #[allow(non_snake_case)]
        unsafe fn stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if output_type != SCStreamOutputType::Screen {
                return;
            }
            if let Some(result) = capture_frame(sample_buffer, &self.ivars().buffer_pool) {
                send_capture_result(&self.ivars().frames_tx, &self.ivars().errors_tx, result);
            }
        }
    }

    unsafe impl SCStreamDelegate for MacCaptureOutput {
        #[unsafe(method(stream:didStopWithError:))]
        #[allow(non_snake_case)]
        unsafe fn stream_didStopWithError(&self, _stream: &SCStream, error: &NSError) {
            send_capture_result(
                &self.ivars().frames_tx,
                &self.ivars().errors_tx,
                Err(format!("ScreenCaptureKit stream stopped: {error}")),
            );
        }
    }
);

unsafe impl NSObjectProtocol for MacCaptureOutput {}

impl MacCaptureOutput {
    fn new(
        frames_tx: SyncSender<CaptureFrame>,
        errors_tx: Sender<String>,
        buffer_pool: CaptureFrameBufferPool,
    ) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MacCaptureOutputIvars {
            frames_tx,
            errors_tx,
            buffer_pool,
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn capture_frame(
    sample_buffer: &CMSampleBuffer,
    buffer_pool: &CaptureFrameBufferPool,
) -> Option<Result<CaptureFrame, String>> {
    let pixel_buffer = unsafe { sample_buffer.image_buffer() }?;
    let lock_flags = CVPixelBufferLockFlags::ReadOnly;
    let lock_result = unsafe { CVPixelBufferLockBaseAddress(&pixel_buffer, lock_flags) };
    if lock_result != kCVReturnSuccess {
        return Some(Err(format!(
            "ScreenCaptureKit pixel buffer lock failed: {lock_result}"
        )));
    }
    let result = copy_bgra_frame(&pixel_buffer, buffer_pool);
    let unlock_result = unsafe { CVPixelBufferUnlockBaseAddress(&pixel_buffer, lock_flags) };
    if unlock_result != kCVReturnSuccess {
        return Some(Err(format!(
            "ScreenCaptureKit pixel buffer unlock failed: {unlock_result}"
        )));
    }
    Some(result)
}

fn copy_bgra_frame(
    pixel_buffer: &CVPixelBuffer,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<CaptureFrame, String> {
    let width = CVPixelBufferGetWidth(pixel_buffer);
    let height = CVPixelBufferGetHeight(pixel_buffer);
    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    let row_length = width
        .checked_mul(4)
        .ok_or_else(|| "ScreenCaptureKit frame width overflowed".to_owned())?;
    if width == 0 || height == 0 || bytes_per_row < row_length {
        return Err("ScreenCaptureKit returned invalid frame dimensions".to_owned());
    }
    let base_address = CVPixelBufferGetBaseAddress(pixel_buffer).cast::<u8>();
    if base_address.is_null() {
        return Err("ScreenCaptureKit returned a null pixel buffer".to_owned());
    }

    let output_length = row_length
        .checked_mul(height)
        .ok_or_else(|| "ScreenCaptureKit frame size overflowed".to_owned())?;
    let mut rgba = buffer_pool.take(output_length);
    for row in 0..height {
        let source =
            unsafe { slice::from_raw_parts(base_address.add(row * bytes_per_row), row_length) };
        let destination = &mut rgba[row * row_length..(row + 1) * row_length];
        for (bgra, rgba) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
            rgba.copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
        }
    }

    Ok(CaptureFrame::new(
        u32::try_from(width).map_err(|_| "ScreenCaptureKit frame width is too large".to_owned())?,
        u32::try_from(height)
            .map_err(|_| "ScreenCaptureKit frame height is too large".to_owned())?,
        rgba,
        buffer_pool.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shareable_window_policy_keeps_app_windows_and_rejects_system_surfaces() {
        let cases = [
            (0, Some("com.example.Terminal"), true),
            (0, None, true),
            (1, Some("com.example.Terminal"), false),
            (-1, Some("com.example.Terminal"), false),
            (0, Some("com.apple.dock"), false),
            (0, Some("com.apple.Dock.extra"), false),
            (0, Some("com.apple.controlcenter"), false),
            (0, Some("com.apple.controlcenter.helper"), false),
            (0, Some("com.apple.SystemUIServer"), false),
            (0, Some("com.apple.systemuiserver.widget"), false),
            (0, Some("com.apple.docker"), true),
        ];

        for (window_layer, bundle_identifier, expected) in cases {
            assert_eq!(
                is_default_shareable_window(window_layer, bundle_identifier),
                expected,
                "window_layer={window_layer} bundle_identifier={bundle_identifier:?}"
            );
        }
    }
}
