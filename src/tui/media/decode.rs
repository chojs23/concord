use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

use image::{
    AnimationDecoder as _, DynamicImage, Frame, ImageDecoder as _, ImageFormat, ImageReader,
    Limits, RgbaImage,
    codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder},
};
use thorvg::{ColorSpace, EngineOption, Paint as _, Thorvg};
use tokio::{sync::mpsc, task};

use super::{
    preview::ImagePreviewKey,
    work::{MediaWorkError, MediaWorkResult, media_image_job_permits, media_image_work_permits},
};

/// The surface caches hold clones of these images, and `DecodedMediaImage`
/// shares its frames through an `Arc`, so this cache is what actually pins
/// decoded pixels in memory: an entry here keeps them alive long after the
/// preview that asked for them scrolled away. It was the largest single store
/// in the client.
const MAX_SHARED_DECODED_MEDIA_IMAGES: usize = 24;
const MAX_SHARED_DECODED_MEDIA_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_DECODED_IMAGE_WIDTH: u32 = 4096;
pub(super) const MAX_DECODED_IMAGE_HEIGHT: u32 = 4096;
const MAX_DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ANIMATION_SOURCE_FRAMES: usize = 4096;
pub(super) const MAX_RETAINED_ANIMATION_FRAMES: usize = 32;
/// Peak allocation while one animation is decoding, before compaction trims it
/// back. Transient, but it still shows up as resident memory and the allocator
/// is in no hurry to return it.
const MAX_ANIMATION_DECODE_WORK_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_LOTTIE_JSON_BYTES: usize = 1024 * 1024;
const MIN_ANIMATION_FRAME_DELAY: Duration = Duration::from_millis(50);
const MAX_UNDERSPECIFIED_FRAME_DELAY: Duration = Duration::from_millis(10);
const DEFAULT_UNDERSPECIFIED_FRAME_DELAY: Duration = Duration::from_millis(100);
const MAX_ANIMATION_FRAME_DELAY: Duration = Duration::from_secs(10);

struct DecodedMediaFrame {
    image: Arc<DynamicImage>,
    delay: Duration,
}

struct SampledAnimationFrame {
    frame: DecodedMediaFrame,
    representative_offset: Duration,
    decoded_bytes: u64,
}

#[derive(Clone)]
pub(in crate::tui) struct DecodedMediaImage {
    frames: Arc<[DecodedMediaFrame]>,
    retained_bytes: u64,
    current_frame_index: usize,
    next_frame_deadline: Option<Instant>,
}

impl DecodedMediaImage {
    fn still(image: DynamicImage) -> Self {
        let retained_bytes = u64::try_from(image.as_bytes().len()).unwrap_or(u64::MAX);
        Self {
            frames: vec![DecodedMediaFrame {
                image: Arc::new(image),
                delay: MIN_ANIMATION_FRAME_DELAY,
            }]
            .into(),
            retained_bytes,
            current_frame_index: 0,
            next_frame_deadline: None,
        }
    }

    #[cfg(test)]
    pub(in crate::tui) fn current_frame(&self) -> &DynamicImage {
        self.frames
            .get(self.current_frame_index)
            .expect("decoded media always has a current frame")
            .image
            .as_ref()
    }

    pub(in crate::tui) fn current_frame_shared(&self) -> Arc<DynamicImage> {
        self.frame_shared(self.current_frame_index)
    }

    pub(in crate::tui) fn frame_shared(&self, frame_index: usize) -> Arc<DynamicImage> {
        self.frames
            .get(frame_index)
            .expect("decoded media frame index should be valid")
            .image
            .clone()
    }

    pub(in crate::tui) fn current_frame_index(&self) -> usize {
        self.current_frame_index
    }

    pub(in crate::tui) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub(in crate::tui) fn frame_index_with_offset(&self, offset: usize) -> usize {
        (self.current_frame_index + offset) % self.frames.len()
    }

    pub(in crate::tui) fn is_animated(&self) -> bool {
        self.frame_count() > 1
    }

    pub(in crate::tui) fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    pub(in crate::tui) fn start_animation(&mut self, now: Instant) {
        if !self.is_animated() || self.next_frame_deadline.is_some() {
            return;
        }
        self.next_frame_deadline = now.checked_add(self.current_frame_delay());
    }

    pub(in crate::tui) fn pause_animation(&mut self) {
        self.next_frame_deadline = None;
    }

    pub(in crate::tui) fn next_frame_deadline(&self) -> Option<Instant> {
        self.next_frame_deadline
    }

    pub(in crate::tui) fn advance_frame(&mut self, now: Instant) -> bool {
        let Some(deadline) = self.next_frame_deadline else {
            return false;
        };
        if now < deadline || !self.is_animated() {
            return false;
        }

        self.current_frame_index = (self.current_frame_index + 1) % self.frames.len();
        self.next_frame_deadline = now.checked_add(self.current_frame_delay());
        true
    }

    fn current_frame_delay(&self) -> Duration {
        self.frames
            .get(self.current_frame_index)
            .expect("decoded media always has a current frame")
            .delay
    }

    #[cfg(test)]
    pub(in crate::tui) fn shares_frames_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.frames, &other.frames)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MediaImageDecodeKey {
    Preview(ImagePreviewKey),
    Avatar(String),
    Emoji(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct MediaImageDecodeRequest {
    pub(super) key: MediaImageDecodeKey,
    pub(super) generation: u64,
}

pub(in crate::tui) struct MediaImageDecodeJob {
    url: String,
    pub(super) bytes: Arc<[u8]>,
}

pub(in crate::tui) struct MediaImageDecodeResult {
    pub(in crate::tui) url: String,
    pub(in crate::tui) result: MediaWorkResult<DecodedMediaImage>,
}

pub(in crate::tui) struct MediaImageDecodeDelivery {
    pub(in crate::tui) key: MediaImageDecodeKey,
    pub(in crate::tui) generation: u64,
    pub(in crate::tui) result: MediaWorkResult<DecodedMediaImage>,
}

pub(in crate::tui) struct MediaImageDecodeCache {
    entries: HashMap<String, SharedDecodeEntry>,
    tick: u64,
}

enum SharedDecodeEntry {
    Decoding {
        requests: Vec<MediaImageDecodeRequest>,
    },
    Ready {
        image: DecodedMediaImage,
        last_used: u64,
    },
}

pub(in crate::tui) struct MediaImageDecodeRequestOutcome {
    pub(in crate::tui) job: Option<MediaImageDecodeJob>,
    pub(in crate::tui) deliveries: Vec<MediaImageDecodeDelivery>,
}

impl MediaImageDecodeCache {
    pub(in crate::tui) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
        }
    }

    pub(in crate::tui) fn request(
        &mut self,
        url: &str,
        bytes: &[u8],
        requests: Vec<MediaImageDecodeRequest>,
    ) -> MediaImageDecodeRequestOutcome {
        if requests.is_empty() {
            return MediaImageDecodeRequestOutcome {
                job: None,
                deliveries: Vec::new(),
            };
        }

        let tick = self.next_tick();
        match self.entries.get_mut(url) {
            Some(SharedDecodeEntry::Decoding { requests: pending }) => {
                pending.extend(requests);
                MediaImageDecodeRequestOutcome {
                    job: None,
                    deliveries: Vec::new(),
                }
            }
            Some(SharedDecodeEntry::Ready { image, last_used }) => {
                *last_used = tick;
                MediaImageDecodeRequestOutcome {
                    job: None,
                    deliveries: deliveries_for_requests(requests, Ok(image.clone())),
                }
            }
            None => {
                self.entries
                    .insert(url.to_owned(), SharedDecodeEntry::Decoding { requests });
                self.prune_ready_to_limit();
                MediaImageDecodeRequestOutcome {
                    job: Some(MediaImageDecodeJob {
                        url: url.to_owned(),
                        bytes: Arc::from(bytes.to_vec()),
                    }),
                    deliveries: Vec::new(),
                }
            }
        }
    }

    pub(in crate::tui) fn complete(
        &mut self,
        completed: MediaImageDecodeResult,
    ) -> Vec<MediaImageDecodeDelivery> {
        let Some(SharedDecodeEntry::Decoding { requests, .. }) =
            self.entries.remove(&completed.url)
        else {
            return Vec::new();
        };

        if let Ok(image) = &completed.result {
            let last_used = self.next_tick();
            self.entries.insert(
                completed.url,
                SharedDecodeEntry::Ready {
                    image: image.clone(),
                    last_used,
                },
            );
            self.prune_ready_to_limit();
        }

        deliveries_for_requests(requests, completed.result)
    }

    /// Ready entries and the decoded bytes they pin. These are the same `Arc`
    /// buffers the surface caches report, counted once here.
    pub(in crate::tui) fn retained_stats(&self) -> (usize, u64) {
        self.entries
            .values()
            .fold((0, 0), |(count, bytes), entry| match entry {
                SharedDecodeEntry::Ready { image, .. } => {
                    (count + 1, bytes.saturating_add(image.retained_bytes()))
                }
                SharedDecodeEntry::Decoding { .. } => (count, bytes),
            })
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn prune_ready_to_limit(&mut self) {
        let ready_count = self
            .entries
            .values()
            .filter(|entry| matches!(entry, SharedDecodeEntry::Ready { .. }))
            .count();
        let ready_bytes = self
            .entries
            .values()
            .filter_map(|entry| match entry {
                SharedDecodeEntry::Ready { image, .. } => Some(image.retained_bytes()),
                SharedDecodeEntry::Decoding { .. } => None,
            })
            .fold(0u64, u64::saturating_add);
        if ready_count <= MAX_SHARED_DECODED_MEDIA_IMAGES
            && ready_bytes <= MAX_SHARED_DECODED_MEDIA_BYTES
        {
            return;
        }

        let mut ready = self
            .entries
            .iter()
            .filter_map(|(url, entry)| match entry {
                SharedDecodeEntry::Ready { last_used, .. } => Some((url.clone(), *last_used)),
                SharedDecodeEntry::Decoding { .. } => None,
            })
            .collect::<Vec<_>>();
        ready.sort_by_key(|(_, last_used)| *last_used);
        let mut retained_count = ready_count;
        let mut retained_bytes = ready_bytes;
        for (url, _) in ready {
            if retained_count <= MAX_SHARED_DECODED_MEDIA_IMAGES
                && retained_bytes <= MAX_SHARED_DECODED_MEDIA_BYTES
            {
                break;
            }
            if let Some(SharedDecodeEntry::Ready { image, .. }) = self.entries.remove(&url) {
                retained_count = retained_count.saturating_sub(1);
                retained_bytes = retained_bytes.saturating_sub(image.retained_bytes());
            }
        }
    }
}

fn deliveries_for_requests(
    requests: Vec<MediaImageDecodeRequest>,
    result: MediaWorkResult<DecodedMediaImage>,
) -> Vec<MediaImageDecodeDelivery> {
    requests
        .into_iter()
        .map(|request| MediaImageDecodeDelivery {
            key: request.key,
            generation: request.generation,
            result: result.clone(),
        })
        .collect()
}

pub(in crate::tui) fn spawn_media_image_decode(
    job: MediaImageDecodeJob,
    tx: mpsc::UnboundedSender<MediaImageDecodeResult>,
) {
    let decode_permits = media_image_work_permits().clone();
    let Ok(job_permit) = media_image_job_permits().clone().try_acquire_owned() else {
        let _ = tx.send(MediaImageDecodeResult {
            url: job.url,
            result: Err(MediaWorkError::Busy),
        });
        return;
    };
    let url = job.url.clone();
    task::spawn(async move {
        let _job_permit = job_permit;
        let _permit = decode_permits
            .acquire_owned()
            .await
            .expect("media work semaphore stays open");
        let result = match task::spawn_blocking(move || decode_media_image(job)).await {
            Ok(result) => result,
            Err(error) => MediaImageDecodeResult {
                url,
                result: Err(MediaWorkError::Failed(format!(
                    "image decode worker failed: {error}"
                ))),
            },
        };
        let _ = tx.send(result);
    });
}

fn decode_media_image(job: MediaImageDecodeJob) -> MediaImageDecodeResult {
    let result = decode_media_image_bytes(&job.bytes).map_err(MediaWorkError::Failed);
    MediaImageDecodeResult {
        url: job.url,
        result,
    }
}

pub(in crate::tui) fn decode_image_bytes(
    bytes: &[u8],
) -> std::result::Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("decode failed: {error}"))?;
    reader.limits(decode_limits());
    reader
        .decode()
        .map_err(|error| format!("decode failed: {error}"))
}

pub(in crate::tui) fn decode_media_image_bytes(
    bytes: &[u8],
) -> std::result::Result<DecodedMediaImage, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("decode failed: {error}"))?;

    match reader.format() {
        Some(ImageFormat::Gif) => decode_gif_animation(bytes),
        Some(ImageFormat::WebP) => decode_webp_animation(bytes),
        Some(ImageFormat::Png) => decode_png_animation(bytes),
        None if looks_like_json(bytes) => decode_lottie_animation(bytes),
        _ => decode_image_bytes(bytes).map(DecodedMediaImage::still),
    }
}

fn looks_like_json(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'{')
}

fn decode_lottie_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    if bytes.len() > MAX_LOTTIE_JSON_BYTES {
        return Err("decode failed: Lottie document exceeds source byte limit".to_owned());
    }

    let engine = Thorvg::init()
        .map_err(|error| format!("decode failed: initialize Lottie renderer: {error}"))?;
    let mut animation = engine
        .lottie_animation()
        .map_err(|error| format!("decode failed: create Lottie animation: {error}"))?;
    animation
        .load_data(bytes)
        .map_err(|error| format!("decode failed: load Lottie document: {error}"))?;

    let (source_width, source_height) = animation
        .picture()
        .size()
        .map_err(|error| format!("decode failed: read Lottie dimensions: {error}"))?;
    let width = checked_lottie_dimension(source_width, MAX_DECODED_IMAGE_WIDTH, "width")?;
    let height = checked_lottie_dimension(source_height, MAX_DECODED_IMAGE_HEIGHT, "height")?;
    let frame_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "decode failed: Lottie dimensions overflow".to_owned())?;
    if frame_bytes > MAX_DECODED_IMAGE_BYTES {
        return Err("decode failed: Lottie frame exceeds decoded byte limit".to_owned());
    }

    let total_frames = animation
        .total_frame()
        .map_err(|error| format!("decode failed: read Lottie frame count: {error}"))?;
    if !total_frames.is_finite()
        || total_frames <= 0.0
        || total_frames > MAX_ANIMATION_SOURCE_FRAMES as f32
    {
        return Err("decode failed: Lottie frame count is outside limits".to_owned());
    }
    let source_frame_count = total_frames.ceil() as usize;

    let duration_seconds = animation
        .duration()
        .map_err(|error| format!("decode failed: read Lottie duration: {error}"))?;
    if !duration_seconds.is_finite() || duration_seconds < 0.0 {
        return Err("decode failed: Lottie duration is invalid".to_owned());
    }

    animation
        .set_size(width as f32, height as f32)
        .map_err(|error| format!("decode failed: set Lottie render size: {error}"))?;

    let retained_frame_limit = usize::try_from(
        (MAX_DECODED_IMAGE_BYTES / frame_bytes).min(MAX_ANIMATION_DECODE_WORK_BYTES / frame_bytes),
    )
    .unwrap_or(usize::MAX)
    .min(MAX_RETAINED_ANIMATION_FRAMES);
    if source_frame_count > 1 && retained_frame_limit < 2 {
        return Err("decode failed: Lottie animation cannot retain two frames".to_owned());
    }
    let retained_frame_count =
        lottie_retained_frame_count(source_frame_count, retained_frame_limit, duration_seconds);
    let frame_delay = lottie_frame_delay(duration_seconds, retained_frame_count);
    let mut sampled_frames = Vec::with_capacity(retained_frame_count);

    for retained_index in 0..retained_frame_count {
        let source_frame_index =
            sampled_lottie_frame_index(retained_index, retained_frame_count, source_frame_count);
        if source_frame_index != 0 {
            animation
                .set_frame(source_frame_index as f32)
                .map_err(|error| {
                    format!(
                        "decode failed: select Lottie frame {}: {error}",
                        source_frame_index + 1
                    )
                })?;
        }

        // A ThorVG canvas takes ownership of its paint. Duplicating the
        // animation picture captures the selected frame while keeping the
        // animation controller available for the remaining samples.
        let picture = animation
            .picture()
            .duplicate()
            .ok_or_else(|| "decode failed: duplicate Lottie frame".to_owned())?;
        let pixel_count = usize::try_from(frame_bytes / 4)
            .map_err(|_| "decode failed: Lottie pixel count overflow".to_owned())?;
        let mut pixels = vec![0u32; pixel_count];
        let mut canvas = engine
            .sw_canvas(EngineOption::Default)
            .map_err(|error| format!("decode failed: create Lottie canvas: {error}"))?;
        unsafe { canvas.set_target(&mut pixels, width, width, height, ColorSpace::ABGR8888S) }
            .map_err(|error| format!("decode failed: set Lottie canvas target: {error}"))?;
        canvas
            .add(picture)
            .map_err(|error| format!("decode failed: add Lottie frame to canvas: {error}"))?;
        canvas
            .draw(true)
            .map_err(|error| format!("decode failed: draw Lottie frame: {error}"))?;
        canvas
            .sync()
            .map_err(|error| format!("decode failed: finish Lottie frame: {error}"))?;
        drop(canvas);

        let mut rgba = Vec::with_capacity(pixel_count.saturating_mul(4));
        for pixel in pixels {
            rgba.extend_from_slice(&pixel.to_le_bytes());
        }
        let image = RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| "decode failed: build Lottie frame image".to_owned())?;
        sampled_frames.push(SampledAnimationFrame {
            frame: DecodedMediaFrame {
                delay: frame_delay,
                image: Arc::new(DynamicImage::ImageRgba8(image)),
            },
            representative_offset: frame_delay / 2,
            decoded_bytes: frame_bytes,
        });
    }

    finish_sampled_animation(sampled_frames, source_frame_count)
}

fn checked_lottie_dimension(value: f32, maximum: u32, label: &str) -> Result<u32, String> {
    if !value.is_finite() || value <= 0.0 || value > maximum as f32 {
        return Err(format!("decode failed: Lottie {label} is outside limits"));
    }
    Ok(value.ceil() as u32)
}

fn sampled_lottie_frame_index(
    retained_index: usize,
    retained_frame_count: usize,
    source_frame_count: usize,
) -> usize {
    if retained_frame_count <= 1 {
        return 0;
    }
    retained_index.saturating_mul(source_frame_count - 1) / (retained_frame_count - 1)
}

fn lottie_retained_frame_count(
    source_frame_count: usize,
    retained_frame_limit: usize,
    duration_seconds: f32,
) -> usize {
    let playback_frame_limit = if duration_seconds == 0.0 {
        usize::MAX
    } else {
        (duration_seconds / MIN_ANIMATION_FRAME_DELAY.as_secs_f32())
            .floor()
            .max(2.0) as usize
    };
    source_frame_count
        .min(retained_frame_limit.max(1))
        .min(playback_frame_limit)
}

fn lottie_frame_delay(duration_seconds: f32, retained_frame_count: usize) -> Duration {
    if duration_seconds == 0.0 {
        return DEFAULT_UNDERSPECIFIED_FRAME_DELAY;
    }
    let maximum_duration_seconds =
        MAX_ANIMATION_FRAME_DELAY.as_secs_f32() * retained_frame_count as f32;
    let total_duration = Duration::from_secs_f32(duration_seconds.min(maximum_duration_seconds));
    (total_duration / u32::try_from(retained_frame_count).unwrap_or(u32::MAX))
        .clamp(MIN_ANIMATION_FRAME_DELAY, MAX_ANIMATION_FRAME_DELAY)
}

fn decode_gif_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoder =
        GifDecoder::new(Cursor::new(bytes)).map_err(|error| format!("decode failed: {error}"))?;
    decoder
        .set_limits(decode_limits())
        .map_err(|error| format!("decode failed: {error}"))?;
    decode_animation_frames(decoder.into_frames())
}

fn decode_webp_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoder =
        WebPDecoder::new(Cursor::new(bytes)).map_err(|error| format!("decode failed: {error}"))?;
    decoder
        .set_limits(decode_limits())
        .map_err(|error| format!("decode failed: {error}"))?;
    if !decoder.has_animation() {
        return decode_image_bytes(bytes).map(DecodedMediaImage::still);
    }
    decode_animation_frames(decoder.into_frames())
}

fn decode_png_animation(bytes: &[u8]) -> std::result::Result<DecodedMediaImage, String> {
    let mut decoder =
        PngDecoder::new(Cursor::new(bytes)).map_err(|error| format!("decode failed: {error}"))?;
    decoder
        .set_limits(decode_limits())
        .map_err(|error| format!("decode failed: {error}"))?;
    if !decoder
        .is_apng()
        .map_err(|error| format!("decode failed: {error}"))?
    {
        return decode_image_bytes(bytes).map(DecodedMediaImage::still);
    }
    let apng = decoder
        .apng()
        .map_err(|error| format!("decode failed: {error}"))?;
    decode_animation_frames(apng.into_frames())
}

fn decode_animation_frames(
    frames: impl Iterator<Item = image::ImageResult<Frame>>,
) -> std::result::Result<DecodedMediaImage, String> {
    let mut sampled_frames = Vec::new();
    let mut retained_bytes = 0u64;
    let mut decode_work_bytes = 0u64;
    let mut source_frame_count = 0usize;

    for (frame_index, result) in frames.take(MAX_ANIMATION_SOURCE_FRAMES).enumerate() {
        let frame = result.map_err(|error| {
            format!(
                "decode failed at animation frame {}: {error}",
                frame_index + 1
            )
        })?;
        source_frame_count = frame_index + 1;

        let frame_bytes = u64::try_from(frame.buffer().as_raw().len()).unwrap_or(u64::MAX);
        if frame_bytes > MAX_DECODED_IMAGE_BYTES
            || decode_work_bytes.saturating_add(frame_bytes) > MAX_ANIMATION_DECODE_WORK_BYTES
        {
            return finish_sampled_animation(sampled_frames, source_frame_count);
        }

        decode_work_bytes = decode_work_bytes.saturating_add(frame_bytes);
        let delay = normalized_frame_delay(frame.delay());
        retained_bytes = retained_bytes.saturating_add(frame_bytes);
        sampled_frames.push(SampledAnimationFrame {
            frame: DecodedMediaFrame {
                delay,
                image: Arc::new(DynamicImage::ImageRgba8(frame.into_buffer())),
            },
            representative_offset: delay / 2,
            decoded_bytes: frame_bytes,
        });
        coalesce_short_animation_tail(&mut sampled_frames, &mut retained_bytes);
        compact_animation_frames(&mut sampled_frames, &mut retained_bytes);
    }

    finish_sampled_animation(sampled_frames, source_frame_count)
}

fn finish_sampled_animation(
    mut frames: Vec<SampledAnimationFrame>,
    source_frame_count: usize,
) -> std::result::Result<DecodedMediaImage, String> {
    if source_frame_count > 1 && frames.len() < 2 {
        return Err("decode failed: animation cannot retain two frames within limits".to_owned());
    }

    if frames.len() > 2
        && frames
            .last()
            .is_some_and(|frame| frame.frame.delay < MIN_ANIMATION_FRAME_DELAY)
    {
        let mut retained_bytes = frames
            .iter()
            .map(|frame| frame.decoded_bytes)
            .fold(0u64, u64::saturating_add);
        let tail_index = frames.len() - 2;
        merge_animation_frame_pair(&mut frames, tail_index, &mut retained_bytes);
    }

    // A loop shorter than two 50 ms frames cannot preserve both its exact
    // duration and the 20 FPS render cap. Keep the animation and play it slower
    // instead of collapsing distinct source frames into a still image.
    if source_frame_count > 1 {
        for frame in &mut frames {
            frame.frame.delay = frame.frame.delay.max(MIN_ANIMATION_FRAME_DELAY);
        }
    }

    let retained_bytes = frames
        .iter()
        .map(|sample| sample.decoded_bytes)
        .fold(0u64, u64::saturating_add);
    if retained_bytes > MAX_DECODED_IMAGE_BYTES {
        return Err("decode failed: animation exceeds retained byte limit".to_owned());
    }
    let decoded_frames = frames
        .into_iter()
        .map(|sample| sample.frame)
        .collect::<Vec<_>>();
    match decoded_frames.len() {
        0 => Err("decode failed: animated image has no frames".to_owned()),
        1 => first_frame_fallback(decoded_frames),
        _ => Ok(DecodedMediaImage {
            frames: decoded_frames.into(),
            retained_bytes,
            current_frame_index: 0,
            next_frame_deadline: None,
        }),
    }
}

/// Keeps decoded playback at no more than 20 rendered frames per second while
/// retaining the normalized source duration. Two frames are kept as the lower
/// bound so a valid animation never becomes a still image.
fn coalesce_short_animation_tail(
    frames: &mut Vec<SampledAnimationFrame>,
    retained_bytes: &mut u64,
) {
    if frames.len() < 3 {
        return;
    }

    let pending_index = frames.len() - 2;
    if frames[pending_index].frame.delay < MIN_ANIMATION_FRAME_DELAY {
        merge_animation_frame_pair(frames, pending_index, retained_bytes);
    }
}

/// Keeps a time-weighted representation of the full animation without first
/// retaining every decoded RGBA frame. Adjacent segments with the shortest
/// combined display time are merged until both the frame and byte budgets fit.
fn compact_animation_frames(frames: &mut Vec<SampledAnimationFrame>, retained_bytes: &mut u64) {
    while frames.len() > 2
        && (frames.len() > MAX_RETAINED_ANIMATION_FRAMES
            || *retained_bytes > MAX_DECODED_IMAGE_BYTES)
    {
        let Some(merge_index) = (0..frames.len().saturating_sub(1)).min_by_key(|index| {
            frames[*index]
                .frame
                .delay
                .saturating_add(frames[*index + 1].frame.delay)
        }) else {
            break;
        };
        merge_animation_frame_pair(frames, merge_index, retained_bytes);
    }
}

fn merge_animation_frame_pair(
    frames: &mut Vec<SampledAnimationFrame>,
    left_index: usize,
    retained_bytes: &mut u64,
) {
    let original_len = frames.len();
    let right = frames.remove(left_index + 1);
    let left = &mut frames[left_index];
    let left_delay = left.frame.delay;
    let merged_delay = left_delay.saturating_add(right.frame.delay);

    // Endpoint frames anchor the loop. Interior merges use the image closest
    // to the merged segment's midpoint so variable source delays influence
    // which visual frames survive sampling.
    let keep_right = if original_len == 2 || left_index == 0 {
        false
    } else if left_index + 1 == original_len - 1 {
        true
    } else {
        let midpoint = merged_delay / 2;
        let left_distance = duration_distance(left.representative_offset, midpoint);
        let right_offset = left_delay.saturating_add(right.representative_offset);
        let right_distance = duration_distance(right_offset, midpoint);
        right_distance <= left_distance
    };

    let dropped_bytes = if keep_right {
        left.frame.image = right.frame.image;
        left.representative_offset = left_delay.saturating_add(right.representative_offset);
        let dropped = left.decoded_bytes;
        left.decoded_bytes = right.decoded_bytes;
        dropped
    } else {
        right.decoded_bytes
    };
    left.frame.delay = merged_delay;
    *retained_bytes = retained_bytes.saturating_sub(dropped_bytes);
}

fn duration_distance(left: Duration, right: Duration) -> Duration {
    left.abs_diff(right)
}

fn first_frame_fallback(
    frames: Vec<DecodedMediaFrame>,
) -> std::result::Result<DecodedMediaImage, String> {
    frames
        .into_iter()
        .next()
        .map(|frame| DecodedMediaImage::still(Arc::unwrap_or_clone(frame.image)))
        .ok_or_else(|| "decode failed: animated image has no frames".to_owned())
}

fn normalized_frame_delay(delay: image::Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let nanos = u128::from(numerator)
        .saturating_mul(1_000_000)
        .checked_div(u128::from(denominator))
        .unwrap_or_default();
    let duration = Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
    if duration <= MAX_UNDERSPECIFIED_FRAME_DELAY {
        DEFAULT_UNDERSPECIFIED_FRAME_DELAY
    } else {
        duration.min(MAX_ANIMATION_FRAME_DELAY)
    }
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_IMAGE_WIDTH);
    limits.max_image_height = Some(MAX_DECODED_IMAGE_HEIGHT);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    limits
}
