use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{
            Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError,
            sync_channel,
        },
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fast_image_resize::{
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
    images::{CroppedImageMut, Image, ImageRef},
};
use image::RgbaImage;
use tokio::sync::mpsc;
use yuv::{
    YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix,
    rgba_to_yuv420,
};

use super::{
    StreamCaptureTarget,
    preview::{StreamPreviewCadence, StreamPreviewFrame},
};
use crate::logging;

#[path = "capture/encoder.rs"]
mod encoder;
use encoder::{I420Frame, StreamEncoder};

#[path = "capture/conversion.rs"]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod conversion;

#[cfg(target_os = "linux")]
#[path = "capture/linux.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "capture/macos.rs"]
mod platform;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "capture/unsupported.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "capture/windows.rs"]
mod platform;

pub(super) const STREAM_CAPTURE_WIDTH: u32 = 1280;
pub(super) const STREAM_CAPTURE_HEIGHT: u32 = 720;
pub(super) const STREAM_CAPTURE_FPS: u32 = 30;
pub(super) const STREAM_ENCODER_BITRATE: u32 = 6_000_000;
// Keep headroom for RTP and DAVE overhead, plus short IDR bursts, instead of
// making the encoder and transport pacer compete for the same bitrate limit.
pub(super) const STREAM_TRANSPORT_BITRATE: u32 = 8_000_000;
// Explicit feedback can still request immediate recovery frames, while this
// fallback period bounds recovery latency without producing an IDR every second.
const STREAM_INTRA_FRAME_PERIOD_FRAMES: u32 = STREAM_CAPTURE_FPS * 2;
const STREAM_CAPTURE_FRAME_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000 / STREAM_CAPTURE_FPS as u64);
const STREAM_CAPTURE_STATS_INTERVAL: Duration = Duration::from_secs(5);
const STREAM_RECORDER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const STREAM_CAPTURE_PREPARATION_TIMEOUT: Duration = Duration::from_secs(60);
const STREAM_CAPTURE_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const STREAM_CAPTURE_SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CAPTURE_FRAME_BUFFER_POOL_CAPACITY: usize = 4;
const ENCODED_STREAM_FRAME_QUEUE_CAPACITY: usize = 8;

#[derive(Debug)]
pub(super) struct EncodedStreamFrame {
    pub(super) timestamp: u32,
    pub(super) annex_b: Vec<u8>,
    pub(super) is_keyframe: bool,
}

pub(super) struct StreamCaptureHandle {
    stop: Arc<AtomicBool>,
    force_keyframe: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

pub(super) use super::capture_cancellation::StreamCaptureCancellation;

pub(super) struct PreparedStreamCapture {
    pub(super) handle: StreamCaptureHandle,
    pub(super) frames: mpsc::Receiver<Result<EncodedStreamFrame, String>>,
    pub(super) preview_frames: Option<mpsc::Receiver<StreamPreviewFrame>>,
    pub(super) errors: mpsc::UnboundedReceiver<String>,
}

pub(super) struct CaptureFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
    buffer_pool: CaptureFrameBufferPool,
}

pub(super) struct CaptureOutput {
    pub(super) frames: Receiver<CaptureFrame>,
    pub(super) errors: Receiver<String>,
}

struct CaptureSource {
    session: platform::CaptureSession,
    frames: Receiver<CaptureFrame>,
    errors: Receiver<String>,
}

#[derive(Clone, Default)]
pub(super) struct CaptureFrameBufferPool {
    buffers: Arc<Mutex<Vec<Vec<u8>>>>,
}

struct CapturedImage {
    image: RgbaImage,
    buffer_pool: CaptureFrameBufferPool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureFrameOutcome {
    Queued,
    QueueFull,
    QueueClosed,
    EncoderSkipped,
}

struct CaptureFrameTimings {
    capture: Duration,
    prepare: Duration,
    resize: Duration,
    color_convert: Duration,
    encode: Duration,
    total: Duration,
}

struct FramePreparationTimings {
    resize: Duration,
    color_convert: Duration,
}

struct PreparedStreamFrame {
    timings: FramePreparationTimings,
    preview: Option<StreamPreviewFrame>,
}

struct CapturePerformanceStats {
    window_started_at: Instant,
    captured_frames: u64,
    queued_frames: u64,
    queue_full_frames: u64,
    encoder_skipped_frames: u64,
    encoded_bytes: u64,
    capture_duration: Duration,
    prepare_duration: Duration,
    resize_duration: Duration,
    color_convert_duration: Duration,
    encode_duration: Duration,
    total_duration: Duration,
    max_frame_duration: Duration,
}

struct StreamFramePacer {
    next_deadline: Instant,
}

impl StreamFramePacer {
    fn new(started_at: Instant) -> Self {
        Self {
            next_deadline: started_at + STREAM_CAPTURE_FRAME_INTERVAL,
        }
    }

    fn wait_for_next_frame(&mut self) {
        let deadline = next_stream_frame_deadline(self.next_deadline, Instant::now());
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(remaining);
        }
        self.next_deadline = deadline + STREAM_CAPTURE_FRAME_INTERVAL;
    }
}

fn next_stream_frame_deadline(current_deadline: Instant, now: Instant) -> Instant {
    let Some(overdue) = now.checked_duration_since(current_deadline) else {
        return current_deadline;
    };
    let missed_intervals = overdue.as_nanos() / STREAM_CAPTURE_FRAME_INTERVAL.as_nanos() + 1;
    let Ok(missed_intervals) = u32::try_from(missed_intervals) else {
        return now + STREAM_CAPTURE_FRAME_INTERVAL;
    };

    current_deadline
        .checked_add(STREAM_CAPTURE_FRAME_INTERVAL * missed_intervals)
        .unwrap_or(now + STREAM_CAPTURE_FRAME_INTERVAL)
}

impl CapturePerformanceStats {
    fn new() -> Self {
        Self {
            window_started_at: Instant::now(),
            captured_frames: 0,
            queued_frames: 0,
            queue_full_frames: 0,
            encoder_skipped_frames: 0,
            encoded_bytes: 0,
            capture_duration: Duration::ZERO,
            prepare_duration: Duration::ZERO,
            resize_duration: Duration::ZERO,
            color_convert_duration: Duration::ZERO,
            encode_duration: Duration::ZERO,
            total_duration: Duration::ZERO,
            max_frame_duration: Duration::ZERO,
        }
    }

    fn record_frame(
        &mut self,
        outcome: CaptureFrameOutcome,
        encoded_bytes: usize,
        timings: CaptureFrameTimings,
        target: &str,
    ) {
        self.captured_frames += 1;
        match outcome {
            CaptureFrameOutcome::Queued => self.queued_frames += 1,
            CaptureFrameOutcome::QueueFull => self.queue_full_frames += 1,
            CaptureFrameOutcome::QueueClosed => {}
            CaptureFrameOutcome::EncoderSkipped => self.encoder_skipped_frames += 1,
        }
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes as u64);
        self.capture_duration += timings.capture;
        self.prepare_duration += timings.prepare;
        self.resize_duration += timings.resize;
        self.color_convert_duration += timings.color_convert;
        self.encode_duration += timings.encode;
        self.total_duration += timings.total;
        self.max_frame_duration = self.max_frame_duration.max(timings.total);
        self.log_if_due(target);
    }

    fn log_if_due(&mut self, target: &str) {
        let elapsed = self.window_started_at.elapsed();
        if elapsed < STREAM_CAPTURE_STATS_INTERVAL {
            return;
        }

        logging::debug(
            "stream",
            format!(
                "broadcast capture stats: target={} elapsed_ms={} input_fps={:.1} queued_fps={:.1} queue_full_frames={} encoder_skipped_frames={} encoded_mbps={:.2} avg_capture_ms={:.1} avg_prepare_ms={:.1} avg_resize_ms={:.1} avg_color_convert_ms={:.1} avg_encode_ms={:.1} avg_frame_ms={:.1} max_frame_ms={:.1}",
                target,
                elapsed.as_millis(),
                rate_per_second(self.captured_frames, elapsed),
                rate_per_second(self.queued_frames, elapsed),
                self.queue_full_frames,
                self.encoder_skipped_frames,
                bits_per_second(self.encoded_bytes, elapsed) / 1_000_000.0,
                average_millis(self.capture_duration, self.captured_frames),
                average_millis(self.prepare_duration, self.captured_frames),
                average_millis(self.resize_duration, self.captured_frames),
                average_millis(self.color_convert_duration, self.captured_frames),
                average_millis(self.encode_duration, self.captured_frames),
                average_millis(self.total_duration, self.captured_frames),
                self.max_frame_duration.as_secs_f64() * 1_000.0,
            ),
        );

        *self = Self::new();
    }
}

impl CaptureFrameBufferPool {
    pub(super) fn take(&self, length: usize) -> Vec<u8> {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut buffer = buffers.pop().unwrap_or_default();
        buffer.resize(length, 0);
        buffer
    }

    fn recycle(&self, buffer: Vec<u8>) {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buffers.len() < CAPTURE_FRAME_BUFFER_POOL_CAPACITY {
            buffers.push(buffer);
        }
    }
}

impl CaptureFrame {
    pub(super) fn new(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        buffer_pool: CaptureFrameBufferPool,
    ) -> Self {
        Self {
            width,
            height,
            rgba,
            buffer_pool,
        }
    }

    fn into_image(mut self) -> Result<CapturedImage, String> {
        let expected_length = usize::try_from(self.width)
            .ok()
            .and_then(|width| width.checked_mul(self.height as usize))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "capture backend returned overflowing RGBA dimensions".to_owned())?;
        if self.rgba.len() != expected_length {
            return Err("capture backend returned an invalid RGBA frame".to_owned());
        }

        let rgba = std::mem::take(&mut self.rgba);
        let image = RgbaImage::from_raw(self.width, self.height, rgba)
            .expect("validated RGBA frame dimensions");
        Ok(CapturedImage {
            image,
            buffer_pool: self.buffer_pool.clone(),
        })
    }
}

impl Drop for CaptureFrame {
    fn drop(&mut self) {
        if !self.rgba.is_empty() {
            self.buffer_pool.recycle(std::mem::take(&mut self.rgba));
        }
    }
}

impl CapturedImage {
    fn image(&self) -> &RgbaImage {
        &self.image
    }

    fn replace(&mut self, frame: CaptureFrame) -> Result<(), String> {
        let mut replacement = frame.into_image()?;
        std::mem::swap(self, &mut replacement);
        Ok(())
    }
}

impl Drop for CapturedImage {
    fn drop(&mut self) {
        let image = std::mem::replace(&mut self.image, RgbaImage::new(0, 0));
        self.buffer_pool.recycle(image.into_raw());
    }
}

impl CaptureSource {
    fn wait_for_initial_image(&self, stop: &AtomicBool) -> Result<Option<CapturedImage>, String> {
        loop {
            self.check_backend_error()?;
            match self.frames.recv_timeout(STREAM_RECORDER_POLL_INTERVAL) {
                Ok(frame) => {
                    self.check_backend_error()?;
                    let mut frame = frame;
                    while let Ok(newer_frame) = self.frames.try_recv() {
                        frame = newer_frame;
                    }
                    self.check_backend_error()?;
                    return frame.into_image().map(Some);
                }
                Err(RecvTimeoutError::Timeout) if stop.load(Ordering::Acquire) => return Ok(None),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("capture backend stopped unexpectedly".to_owned());
                }
            }
        }
    }

    fn refresh_image(&self, image: &mut CapturedImage) -> Result<bool, String> {
        self.check_backend_error()?;
        let refreshed = refresh_capture_image(&self.frames, image)?;
        self.check_backend_error()?;
        Ok(refreshed)
    }

    fn check_backend_error(&self) -> Result<(), String> {
        match self.errors.try_recv() {
            Ok(error) => Err(error),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => Ok(()),
        }
    }
}

fn send_capture_result(
    frames_tx: &SyncSender<CaptureFrame>,
    errors_tx: &Sender<String>,
    frame: Result<CaptureFrame, String>,
) {
    match frame {
        Ok(frame) => match frames_tx.try_send(frame) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        },
        Err(error) => {
            let _ = errors_tx.send(error);
        }
    }
}

#[cfg(any(test, target_os = "windows"))]
fn capture_copy_dimensions(
    content_width: u32,
    content_height: u32,
    texture_width: u32,
    texture_height: u32,
) -> (u32, u32) {
    (
        content_width.min(texture_width),
        content_height.min(texture_height),
    )
}

fn refresh_capture_image(
    frames: &Receiver<CaptureFrame>,
    image: &mut CapturedImage,
) -> Result<bool, String> {
    let mut newest = None;
    loop {
        match frames.try_recv() {
            Ok(frame) => newest = Some(frame),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                return Err("capture backend stopped unexpectedly".to_owned());
            }
        }
    }

    let Some(frame) = newest else {
        return Ok(false);
    };
    image.replace(frame)?;
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamFrameGeometry {
    source_dimensions: (u32, u32),
    scaled_dimensions: (u32, u32),
    offsets: (u32, u32),
}

impl StreamFrameGeometry {
    fn for_source(source_dimensions: (u32, u32)) -> Result<Self, String> {
        let (source_width, source_height) = source_dimensions;
        if source_width == 0 || source_height == 0 {
            return Err("capture source returned an empty frame".to_owned());
        }

        let scale = (STREAM_CAPTURE_WIDTH as f64 / source_width as f64)
            .min(STREAM_CAPTURE_HEIGHT as f64 / source_height as f64);
        // Multiples of four keep the scaled content and letterbox offsets
        // aligned to the YUV420 chroma grid used after RGBA resizing.
        let scaled_width = align_yuv420_dimension(
            (source_width as f64 * scale).round() as u32,
            STREAM_CAPTURE_WIDTH,
        );
        let scaled_height = align_yuv420_dimension(
            (source_height as f64 * scale).round() as u32,
            STREAM_CAPTURE_HEIGHT,
        );

        Ok(Self {
            source_dimensions,
            scaled_dimensions: (scaled_width, scaled_height),
            offsets: (
                (STREAM_CAPTURE_WIDTH - scaled_width) / 2,
                (STREAM_CAPTURE_HEIGHT - scaled_height) / 2,
            ),
        })
    }
}

fn align_yuv420_dimension(value: u32, maximum: u32) -> u32 {
    value.clamp(4, maximum) & !3
}

struct StreamFrameProcessor {
    resizer: Resizer,
    resize_options: ResizeOptions,
    rgba: Image<'static>,
    yuv: YuvPlanarImageMut<'static, u8>,
    geometry: Option<StreamFrameGeometry>,
}

impl StreamFrameProcessor {
    fn new() -> Self {
        let mut rgba = Image::new(STREAM_CAPTURE_WIDTH, STREAM_CAPTURE_HEIGHT, PixelType::U8x4);
        fill_opaque_black(rgba.buffer_mut());

        Self {
            resizer: Resizer::new(),
            // Box filtering is close to the previous thumbnail behavior and
            // avoids the higher cost of the library's default Lanczos filter.
            resize_options: ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(FilterType::Box))
                .use_alpha(false),
            rgba,
            yuv: YuvPlanarImageMut::alloc(
                STREAM_CAPTURE_WIDTH,
                STREAM_CAPTURE_HEIGHT,
                YuvChromaSubsampling::Yuv420,
            ),
            geometry: None,
        }
    }

    fn prepare(
        &mut self,
        image: &RgbaImage,
        include_preview: bool,
    ) -> Result<PreparedStreamFrame, String> {
        let original_dimensions = image.dimensions();
        let geometry = StreamFrameGeometry::for_source(image.dimensions())?;
        self.update_geometry(geometry);

        let resize_started_at = Instant::now();
        let source = ImageRef::new(
            geometry.source_dimensions.0,
            geometry.source_dimensions.1,
            image.as_raw(),
            PixelType::U8x4,
        )
        .map_err(|error| format!("captured RGBA frame is invalid: {error}"))?;
        let mut destination = CroppedImageMut::new(
            &mut self.rgba,
            geometry.offsets.0,
            geometry.offsets.1,
            geometry.scaled_dimensions.0,
            geometry.scaled_dimensions.1,
        )
        .map_err(|error| format!("stream RGBA destination crop is invalid: {error}"))?;
        self.resizer
            .resize(&source, &mut destination, &self.resize_options)
            .map_err(|error| format!("stream RGBA resize failed: {error}"))?;
        let resize = resize_started_at.elapsed();

        let color_convert_started_at = Instant::now();
        // The measured Apple Silicon path is faster when conversion runs on
        // the final 720p buffer instead of the full-resolution capture.
        rgba_to_yuv420(
            &mut self.yuv,
            self.rgba.buffer(),
            STREAM_CAPTURE_WIDTH * 4,
            YuvRange::Limited,
            YuvStandardMatrix::Bt709,
            YuvConversionMode::Fast,
        )
        .map_err(|error| format!("RGBA to YUV conversion failed: {error}"))?;
        let color_convert = color_convert_started_at.elapsed();

        Ok(PreparedStreamFrame {
            timings: FramePreparationTimings {
                resize,
                color_convert,
            },
            preview: include_preview.then(|| StreamPreviewFrame {
                width: original_dimensions.0,
                height: original_dimensions.1,
                rgba: image.as_raw().clone(),
            }),
        })
    }

    fn i420_source(&self) -> I420Frame<'_> {
        I420Frame::new(
            self.yuv.y_plane.borrow(),
            self.yuv.u_plane.borrow(),
            self.yuv.v_plane.borrow(),
            STREAM_CAPTURE_WIDTH as usize,
            STREAM_CAPTURE_HEIGHT as usize,
            self.yuv.y_stride as usize,
            self.yuv.u_stride as usize,
            self.yuv.v_stride as usize,
        )
    }

    fn update_geometry(&mut self, geometry: StreamFrameGeometry) {
        if self.geometry == Some(geometry) {
            return;
        }

        fill_opaque_black(self.rgba.buffer_mut());
        self.geometry = Some(geometry);
    }
}

fn fill_opaque_black(rgba: &mut [u8]) {
    rgba.fill(0);
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
}

impl Drop for CaptureSource {
    fn drop(&mut self) {
        logging::debug(
            "stream",
            "stopping native capture backend because the capture source ended",
        );
        if let Err(error) = self.session.stop() {
            logging::debug(
                "stream",
                format!("capture backend stop failed during shutdown: {error}"),
            );
        }
    }
}

impl Drop for StreamCaptureHandle {
    fn drop(&mut self) {
        logging::debug(
            "stream",
            "stream capture handle dropped; signaling the capture worker to stop",
        );
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            reap_capture_worker(worker);
        }
    }
}

fn reap_capture_worker(worker: JoinHandle<()>) {
    if let Err(error) = thread::Builder::new()
        .name("stream-capture-reaper".to_owned())
        .spawn(move || {
            if let Err(error) = worker.join() {
                logging::debug(
                    "stream",
                    format!("stream capture worker panicked during shutdown: {error:?}"),
                );
            }
        })
    {
        // Dropping the join handle detaches the worker, so a reaper spawn
        // failure still leaves shutdown nonblocking.
        logging::debug(
            "stream",
            format!("stream capture reaper spawn failed: {error}"),
        );
    }
}

impl StreamCaptureHandle {
    pub(super) fn request_keyframe(&self) {
        self.force_keyframe.store(true, Ordering::Release);
    }

    pub(super) async fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return;
        };
        let deadline = Instant::now() + STREAM_CAPTURE_GRACEFUL_SHUTDOWN_TIMEOUT;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                // Native capture shutdown can wait for an operating-system
                // callback. Move ownership outside Tokio so runtime shutdown
                // stays bounded.
                logging::debug(
                    "stream",
                    "stream capture worker did not stop before graceful shutdown timeout; reaping outside Tokio",
                );
                reap_capture_worker(worker);
                return;
            }
            tokio::time::sleep(STREAM_CAPTURE_SHUTDOWN_POLL_INTERVAL).await;
        }
        if let Err(error) = worker.join() {
            logging::debug(
                "stream",
                format!("stream capture worker panicked during shutdown: {error:?}"),
            );
        }
    }
}

pub(crate) fn list_stream_capture_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let mut targets = platform::list_targets()?;

    targets.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    targets.dedup_by(|left, right| left.kind == right.kind && left.id == right.id);
    if targets.is_empty() {
        return Err("no capturable screens or windows were found".to_owned());
    }
    Ok(targets)
}

pub(super) fn prepare_stream_capture(
    target: StreamCaptureTarget,
    cancellation: StreamCaptureCancellation,
) -> Result<PreparedStreamCapture, String> {
    let (frames_tx, frames) = mpsc::channel(ENCODED_STREAM_FRAME_QUEUE_CAPACITY);
    let (preview_frames_tx, preview_frames) = mpsc::channel(1);
    let (errors_tx, errors) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = sync_channel(1);
    let stop = cancellation.flag();
    let force_keyframe = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let worker_force_keyframe = Arc::clone(&force_keyframe);
    let worker = thread::Builder::new()
        .name("stream-capture".to_owned())
        .spawn(move || {
            run_capture_worker(
                target,
                frames_tx,
                preview_frames_tx,
                errors_tx,
                worker_stop,
                worker_force_keyframe,
                ready_tx,
            );
        })
        .map_err(|error| format!("stream capture worker spawn failed: {error}"))?;
    let handle = StreamCaptureHandle {
        stop,
        force_keyframe,
        worker: Some(worker),
    };

    if let Err(error) =
        wait_for_capture_ready(&ready_rx, &handle.stop, STREAM_CAPTURE_PREPARATION_TIMEOUT)
    {
        logging::debug(
            "stream",
            format!("stream capture preparation failed while waiting for readiness: {error}"),
        );
        return Err(error);
    }
    logging::debug("stream", "stream capture encoder is ready");
    Ok(PreparedStreamCapture {
        handle,
        frames,
        preview_frames: Some(preview_frames),
        errors,
    })
}

fn wait_for_capture_ready(
    ready_rx: &Receiver<Result<(), String>>,
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("stream capture preparation was cancelled".to_owned());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("stream capture did not become ready in time".to_owned());
        }
        let wait = (deadline - now).min(STREAM_RECORDER_POLL_INTERVAL);
        match ready_rx.recv_timeout(wait) {
            Ok(result) => return result,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err("stream capture stopped before becoming ready".to_owned());
            }
        }
    }
}

fn run_capture_worker(
    target: StreamCaptureTarget,
    frames_tx: mpsc::Sender<Result<EncodedStreamFrame, String>>,
    preview_frames_tx: mpsc::Sender<StreamPreviewFrame>,
    errors_tx: mpsc::UnboundedSender<String>,
    stop: Arc<AtomicBool>,
    force_keyframe: Arc<AtomicBool>,
    ready_tx: SyncSender<Result<(), String>>,
) {
    let result = run_capture_loop(
        &target,
        &frames_tx,
        &preview_frames_tx,
        &stop,
        &force_keyframe,
        &ready_tx,
    );
    match result {
        Ok(()) => logging::debug(
            "stream",
            format!(
                "stream capture worker stopped cleanly: target_kind={:?} cancelled={}",
                target.kind,
                stop.load(Ordering::Acquire),
            ),
        ),
        Err(error) => {
            logging::debug(
                "stream",
                format!(
                    "stream capture worker failed: target_kind={:?} cancelled={} error={error}",
                    target.kind,
                    stop.load(Ordering::Acquire),
                ),
            );
            let _ = ready_tx.try_send(Err(error.clone()));
            let _ = errors_tx.send(error);
        }
    }
}

fn run_capture_loop(
    target: &StreamCaptureTarget,
    frames_tx: &mpsc::Sender<Result<EncodedStreamFrame, String>>,
    preview_frames_tx: &mpsc::Sender<StreamPreviewFrame>,
    stop: &AtomicBool,
    force_keyframe: &AtomicBool,
    ready_tx: &SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let source = resolve_capture_source(target, stop)?;
    let Some(mut image) = source.wait_for_initial_image(stop)? else {
        logging::debug(
            "stream",
            "stream capture stopped before receiving an initial image",
        );
        return Ok(());
    };
    logging::debug(
        "stream",
        format!(
            "stream capture received its initial image: width={} height={}",
            image.image().width(),
            image.image().height(),
        ),
    );
    let mut encoder = StreamEncoder::new_auto()?;
    if ready_tx.send(Ok(())).is_err() {
        logging::debug(
            "stream",
            "stream capture readiness receiver closed before encoder startup completed",
        );
        return Ok(());
    }
    let started_at = Instant::now();
    let mut frame_pacer = StreamFramePacer::new(started_at);
    let mut stats = CapturePerformanceStats::new();
    let mut frame_processor = StreamFrameProcessor::new();
    let mut preview_cadence = StreamPreviewCadence::default();

    while !stop.load(Ordering::Acquire) {
        let frame_started_at = Instant::now();

        let capture_started_at = Instant::now();
        source.refresh_image(&mut image)?;
        let capture_time = capture_started_at.elapsed();

        let preview_now = Instant::now();
        let preview_permit = preview_cadence
            .is_due(preview_now)
            .then(|| preview_frames_tx.try_reserve().ok())
            .flatten();
        let prepare_started_at = Instant::now();
        let prepared = frame_processor.prepare(image.image(), preview_permit.is_some())?;
        let prepare_time = prepare_started_at.elapsed();
        if let (Some(permit), Some(preview)) = (preview_permit, prepared.preview) {
            permit.send(preview);
            preview_cadence.record_queued(preview_now);
        }

        let frame_slot = match try_reserve_encoded_frame_slot(frames_tx) {
            Ok(permit) => permit,
            Err(CaptureFrameOutcome::QueueFull) => {
                stats.record_frame(
                    CaptureFrameOutcome::QueueFull,
                    0,
                    CaptureFrameTimings {
                        capture: capture_time,
                        prepare: prepare_time,
                        resize: prepared.timings.resize,
                        color_convert: prepared.timings.color_convert,
                        encode: Duration::ZERO,
                        total: frame_started_at.elapsed(),
                    },
                    &target.title,
                );
                frame_pacer.wait_for_next_frame();
                continue;
            }
            Err(CaptureFrameOutcome::QueueClosed) => return Ok(()),
            Err(_) => unreachable!("frame reservation only reports queue availability"),
        };

        let encode_started_at = Instant::now();
        let force_keyframe = force_keyframe.swap(false, Ordering::AcqRel);
        let encoded = encoder.encode(frame_processor.i420_source(), force_keyframe)?;
        let encode_time = encode_started_at.elapsed();
        let (outcome, encoded_bytes) = match encoded {
            Some(encoded) => {
                let encoded_bytes = encoded.annex_b.len();
                let timestamp = stream_rtp_timestamp(started_at.elapsed());
                frame_slot.send(Ok(EncodedStreamFrame {
                    timestamp,
                    annex_b: encoded.annex_b,
                    is_keyframe: encoded.is_keyframe,
                }));
                (CaptureFrameOutcome::Queued, encoded_bytes)
            }
            None => (CaptureFrameOutcome::EncoderSkipped, 0),
        };
        stats.record_frame(
            outcome,
            encoded_bytes,
            CaptureFrameTimings {
                capture: capture_time,
                prepare: prepare_time,
                resize: prepared.timings.resize,
                color_convert: prepared.timings.color_convert,
                encode: encode_time,
                total: frame_started_at.elapsed(),
            },
            &target.title,
        );

        frame_pacer.wait_for_next_frame();
    }
    Ok(())
}

fn try_reserve_encoded_frame_slot(
    frames_tx: &mpsc::Sender<Result<EncodedStreamFrame, String>>,
) -> Result<mpsc::Permit<'_, Result<EncodedStreamFrame, String>>, CaptureFrameOutcome> {
    match frames_tx.try_reserve() {
        Ok(permit) => Ok(permit),
        Err(mpsc::error::TrySendError::Full(_)) => Err(CaptureFrameOutcome::QueueFull),
        Err(mpsc::error::TrySendError::Closed(_)) => Err(CaptureFrameOutcome::QueueClosed),
    }
}

fn resolve_capture_source(
    target: &StreamCaptureTarget,
    stop: &AtomicBool,
) -> Result<CaptureSource, String> {
    let (session, output) = platform::start_capture(target, stop)?;
    logging::debug("stream", "native continuous capture started");
    Ok(CaptureSource {
        session,
        frames: output.frames,
        errors: output.errors,
    })
}

fn stream_rtp_timestamp(elapsed: Duration) -> u32 {
    let ticks = elapsed.as_micros().saturating_mul(90) / 1_000;
    ticks as u32
}

fn rate_per_second(count: u64, elapsed: Duration) -> f64 {
    count as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
}

fn bits_per_second(bytes: u64, elapsed: Duration) -> f64 {
    rate_per_second(bytes.saturating_mul(8), elapsed)
}

fn average_millis(duration: Duration, samples: u64) -> f64 {
    duration.as_secs_f64() * 1_000.0 / samples.max(1) as f64
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn test_capture_frame(image: RgbaImage, buffer_pool: &CaptureFrameBufferPool) -> CaptureFrame {
        let (width, height) = image.dimensions();
        CaptureFrame::new(width, height, image.into_raw(), buffer_pool.clone())
    }

    #[test]
    fn capture_handle_coalesces_keyframe_requests() {
        let mut handle = StreamCaptureHandle {
            stop: Arc::new(AtomicBool::new(false)),
            force_keyframe: Arc::new(AtomicBool::new(false)),
            worker: None,
        };

        handle.request_keyframe();
        handle.request_keyframe();

        assert!(handle.force_keyframe.swap(false, Ordering::AcqRel));
        assert!(!handle.force_keyframe.swap(false, Ordering::AcqRel));
        handle.worker = None;
    }

    #[test]
    fn capture_handle_drop_does_not_wait_for_worker_shutdown() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let handle = StreamCaptureHandle {
            stop: Arc::new(AtomicBool::new(false)),
            force_keyframe: Arc::new(AtomicBool::new(false)),
            worker: Some(worker),
        };
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(handle);
            let _ = dropped_tx.send(());
        });

        dropped_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("capture handle drop must not wait for the worker");
        release_tx
            .send(())
            .expect("test capture worker should be released");
        dropper.join().expect("test handle dropper should finish");
    }

    #[test]
    fn capture_error_uses_a_separate_nonblocking_channel() {
        let (frames_tx, mut frames_rx) = mpsc::channel::<Result<EncodedStreamFrame, String>>(1);
        frames_tx
            .try_send(Ok(EncodedStreamFrame {
                timestamp: 1,
                annex_b: vec![1],
                is_keyframe: false,
            }))
            .expect("test frame should fill the queue");
        let (errors_tx, mut errors_rx) = mpsc::unbounded_channel();

        errors_tx
            .send("capture failed".to_owned())
            .expect("capture error should not depend on frame queue capacity");

        let queued = frames_rx
            .try_recv()
            .expect("the queued frame should remain available");
        assert!(queued.is_ok());
        assert_eq!(
            errors_rx
                .try_recv()
                .expect("capture error should remain available"),
            "capture failed"
        );
    }

    #[test]
    fn native_capture_error_does_not_depend_on_frame_queue_capacity() {
        let buffer_pool = CaptureFrameBufferPool::default();
        let (frames_tx, frames_rx) = sync_channel(1);
        frames_tx
            .try_send(test_capture_frame(
                RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])),
                &buffer_pool,
            ))
            .expect("test frame should fill the native queue");
        let (errors_tx, errors_rx) = std::sync::mpsc::channel();

        send_capture_result(
            &frames_tx,
            &errors_tx,
            Err("native capture failed".to_owned()),
        );

        let _queued_frame = frames_rx.try_recv().expect("queued frame should remain");
        assert_eq!(
            errors_rx
                .try_recv()
                .expect("native error should remain available"),
            "native capture failed"
        );
    }

    #[test]
    fn capture_copy_dimensions_stay_within_the_frame_pool_texture() {
        let cases = [
            ((1_920, 1_080), (1_280, 720), (1_280, 720)),
            ((640, 360), (1_280, 720), (640, 360)),
            ((1_280, 720), (1_280, 720), (1_280, 720)),
            ((1_920, 360), (1_280, 720), (1_280, 360)),
        ];

        for ((content_width, content_height), (texture_width, texture_height), expected) in cases {
            assert_eq!(
                capture_copy_dimensions(
                    content_width,
                    content_height,
                    texture_width,
                    texture_height,
                ),
                expected,
            );
        }
    }

    #[test]
    fn capture_frame_buffers_return_to_the_pool_when_dropped() {
        let buffer_pool = CaptureFrameBufferPool::default();
        let buffer = buffer_pool.take(16);
        let address = buffer.as_ptr();

        drop(CaptureFrame::new(2, 2, buffer, buffer_pool.clone()));

        let reused = buffer_pool.take(16);
        assert_eq!(reused.as_ptr(), address);
    }

    #[test]
    fn capture_readiness_wait_honors_cancellation() {
        let (_ready_tx, ready_rx) = sync_channel(1);
        let stop = AtomicBool::new(true);

        assert_eq!(
            wait_for_capture_ready(&ready_rx, &stop, Duration::from_secs(1)),
            Err("stream capture preparation was cancelled".to_owned())
        );
    }

    #[test]
    fn capture_readiness_wait_has_a_deadline() {
        let (_ready_tx, ready_rx) = sync_channel(1);
        let stop = AtomicBool::new(false);

        assert_eq!(
            wait_for_capture_ready(&ready_rx, &stop, Duration::ZERO),
            Err("stream capture did not become ready in time".to_owned())
        );
    }

    #[test]
    fn full_encoded_frame_queue_skips_encoding_without_consuming_keyframe_request() {
        let (frames_tx, mut frames_rx) = mpsc::channel(1);
        let force_keyframe = AtomicBool::new(true);
        frames_tx
            .try_send(Ok(EncodedStreamFrame {
                timestamp: 1,
                annex_b: vec![1],
                is_keyframe: false,
            }))
            .expect("first test frame should fill the queue");

        match try_reserve_encoded_frame_slot(&frames_tx) {
            Err(outcome) => assert_eq!(outcome, CaptureFrameOutcome::QueueFull),
            Ok(_) => panic!("a full frame queue must not reserve another slot"),
        }
        assert!(force_keyframe.load(Ordering::Acquire));
        assert_eq!(
            frames_rx
                .try_recv()
                .expect("the older queued frame should remain")
                .expect("the older queued frame should be valid")
                .timestamp,
            1
        );

        let permit = try_reserve_encoded_frame_slot(&frames_tx)
            .expect("a drained frame queue should reserve a slot");
        permit.send(Ok(EncodedStreamFrame {
            timestamp: 2,
            annex_b: vec![2],
            is_keyframe: true,
        }));
        assert_eq!(
            frames_rx
                .try_recv()
                .expect("the reserved frame should be queued")
                .expect("the reserved frame should be valid")
                .timestamp,
            2
        );
    }

    #[test]
    fn letterbox_preserves_source_aspect_ratio_for_common_shapes() {
        let cases = [
            ((800, 600), Some((159, 0)), (160, 0)),
            ((1280, 720), None, (0, 0)),
            ((1600, 600), Some((0, 119)), (0, 120)),
        ];

        for (dimensions, black_bar, content_edge) in cases {
            let image = RgbaImage::from_pixel(dimensions.0, dimensions.1, Rgba([255, 0, 0, 255]));
            let mut processor = StreamFrameProcessor::new();
            processor
                .prepare(&image, false)
                .expect("stream frame should be prepared");

            assert!(luma_pixel(&processor, 640, 360).abs_diff(63) <= 1);
            assert!(luma_pixel(&processor, content_edge.0, content_edge.1).abs_diff(63) <= 1);
            if let Some(black_bar) = black_bar {
                assert_eq!(luma_pixel(&processor, black_bar.0, black_bar.1), 16);
            }
        }
    }

    #[test]
    fn broadcast_color_conversion_and_vui_use_bt709_limited() {
        let mut processor = StreamFrameProcessor::new();
        let image = RgbaImage::from_pixel(
            STREAM_CAPTURE_WIDTH,
            STREAM_CAPTURE_HEIGHT,
            Rgba([255, 0, 0, 255]),
        );
        processor
            .prepare(&image, false)
            .expect("red stream frame should be prepared");

        for value in processor.yuv.y_plane.borrow() {
            assert!(
                value.abs_diff(63) <= 1,
                "unexpected BT.709 limited red luma: {value}"
            );
        }
        let u = processor.yuv.u_plane.borrow()[0];
        let v = processor.yuv.v_plane.borrow()[0];
        assert!(u.abs_diff(102) <= 1, "unexpected BT.709 limited red U: {u}");
        assert!(v.abs_diff(240) <= 1, "unexpected BT.709 limited red V: {v}");

        let config = format!("{:?}", encoder::openh264_encoder_config());
        assert!(
            config.contains("matrix_coefficients: Bt709") && config.contains("full_range: false"),
            "unexpected stream VUI configuration: {config}"
        );
    }

    #[test]
    fn frame_processor_reuses_working_buffers_for_stable_dimensions() {
        let mut processor = StreamFrameProcessor::new();
        let image = RgbaImage::from_pixel(800, 600, Rgba([12, 34, 56, 255]));
        processor
            .prepare(&image, false)
            .expect("first stream frame should be prepared");
        let addresses = (
            processor.rgba.buffer().as_ptr(),
            processor.yuv.y_plane.borrow().as_ptr(),
            processor.yuv.u_plane.borrow().as_ptr(),
            processor.yuv.v_plane.borrow().as_ptr(),
        );

        processor
            .prepare(&image, false)
            .expect("second stream frame should be prepared");

        assert_eq!(
            addresses,
            (
                processor.rgba.buffer().as_ptr(),
                processor.yuv.y_plane.borrow().as_ptr(),
                processor.yuv.u_plane.borrow().as_ptr(),
                processor.yuv.v_plane.borrow().as_ptr(),
            )
        );
    }

    #[test]
    fn preview_copies_the_latest_reusable_capture_frame() {
        let image = RgbaImage::from_pixel(800, 600, Rgba([12, 34, 56, 255]));
        let mut processor = StreamFrameProcessor::new();

        let prepared = processor
            .prepare(&image, true)
            .expect("stream frame should be prepared");
        let preview = prepared.preview.expect("preview should be returned");

        assert_eq!(preview.rgba, image.as_raw().as_slice());
        assert_eq!((preview.width, preview.height), (800, 600));
    }

    #[test]
    fn damage_driven_capture_keeps_the_latest_frame_between_updates() {
        let buffer_pool = CaptureFrameBufferPool::default();
        let (frames_tx, frames_rx) = std::sync::mpsc::sync_channel(1);
        let mut image = test_capture_frame(
            RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])),
            &buffer_pool,
        )
        .into_image()
        .expect("initial test frame should be valid");
        let initial_buffer_address = image.image().as_raw().as_ptr();

        assert!(
            !refresh_capture_image(&frames_rx, &mut image)
                .expect("an idle capture source should remain usable")
        );
        assert_eq!(image.image().get_pixel(0, 0), &Rgba([1, 2, 3, 255]));

        frames_tx
            .send(test_capture_frame(
                RgbaImage::from_pixel(2, 2, Rgba([4, 5, 6, 255])),
                &buffer_pool,
            ))
            .expect("updated capture frame should be queued");
        assert!(
            refresh_capture_image(&frames_rx, &mut image)
                .expect("an updated capture frame should be accepted")
        );
        assert_eq!(image.image().get_pixel(0, 0), &Rgba([4, 5, 6, 255]));

        assert!(
            !refresh_capture_image(&frames_rx, &mut image)
                .expect("the latest frame should remain usable without another update")
        );
        assert_eq!(image.image().get_pixel(0, 0), &Rgba([4, 5, 6, 255]));
        let recycled = buffer_pool.take(16);
        assert_eq!(recycled.as_ptr(), initial_buffer_address);
    }

    #[test]
    fn odd_capture_dimensions_keep_yuv420_output_aligned() {
        let geometry =
            StreamFrameGeometry::for_source((2057, 1329)).expect("source should be accepted");
        let mut processor = StreamFrameProcessor::new();
        let image = RgbaImage::from_pixel(801, 601, Rgba([12, 34, 56, 255]));
        processor
            .prepare(&image, false)
            .expect("odd-sized stream frame should be prepared");

        assert_eq!(geometry.source_dimensions, (2057, 1329));
        assert_eq!(geometry.scaled_dimensions.0 % 4, 0);
        assert_eq!(geometry.scaled_dimensions.1 % 4, 0);
        assert_eq!(geometry.offsets.0 % 2, 0);
        assert_eq!(geometry.offsets.1 % 2, 0);
    }

    #[test]
    fn frame_deadline_corrects_sleep_overshoot_without_drift() {
        let started_at = Instant::now();
        let deadline = started_at + STREAM_CAPTURE_FRAME_INTERVAL;
        let woke_late = deadline + Duration::from_millis(4);

        assert_eq!(
            next_stream_frame_deadline(deadline, woke_late),
            deadline + STREAM_CAPTURE_FRAME_INTERVAL
        );
    }

    #[test]
    fn frame_deadline_skips_missed_intervals_without_catch_up_bursts() {
        let started_at = Instant::now();
        let deadline = started_at + STREAM_CAPTURE_FRAME_INTERVAL;
        let second_deadline = deadline + STREAM_CAPTURE_FRAME_INTERVAL;
        let third_deadline = second_deadline + STREAM_CAPTURE_FRAME_INTERVAL;
        let finished_after_third_deadline = third_deadline + Duration::from_millis(1);

        assert_eq!(
            next_stream_frame_deadline(deadline, finished_after_third_deadline),
            third_deadline + STREAM_CAPTURE_FRAME_INTERVAL
        );
    }

    #[test]
    fn performance_rates_use_the_observed_interval() {
        let elapsed = Duration::from_secs(5);

        assert_eq!(rate_per_second(150, elapsed), 30.0);
        assert_eq!(bits_per_second(3_750_000, elapsed), 6_000_000.0);
        assert_eq!(average_millis(Duration::from_millis(150), 10), 15.0);
    }

    #[test]
    fn rtp_timestamp_uses_video_clock() {
        assert_eq!(stream_rtp_timestamp(Duration::from_millis(500)), 45_000);
        assert_eq!(stream_rtp_timestamp(Duration::from_secs(1)), 90_000);
    }

    fn luma_pixel(processor: &StreamFrameProcessor, x: u32, y: u32) -> u8 {
        processor.yuv.y_plane.borrow()[(y * processor.yuv.y_stride + x) as usize]
    }
}
