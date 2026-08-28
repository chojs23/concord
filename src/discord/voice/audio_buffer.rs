use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver as StdReceiver, TryRecvError},
};
use std::time::{Duration, Instant};

use super::{
    DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE, VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES,
    VOICE_OUTPUT_UNDERRUN_FADE_MILLIS,
};

// Keep the callback lock-free while approximating whether PCM production is still active.
const RECENT_PCM_WINDOW: Duration = Duration::from_millis(100);

pub(super) struct VoiceAudioBuffer {
    samples_rx: StdReceiver<Vec<f32>>,
    stats: Arc<VoiceAudioOutputStats>,
    current: Vec<f32>,
    offset: usize,
    output_sample_rate: u32,
    source_position: f64,
    last_frame: [f32; 2],
    fade_remaining_frames: usize,
    fade_total_frames: usize,
    received_audio: bool,
    underrunning: bool,
    buffering: bool,
}

pub(super) struct VoiceAudioOutputStats {
    pub(super) queue_full_drops: AtomicU64,
    pub(super) output_underruns: AtomicU64,
    pub(super) recent_pcm_underruns: AtomicU64,
    pub(super) callback_max_gap_ms: AtomicU64,
    pub(super) callback_count: AtomicU64,
    pub(super) callback_requested_frames: AtomicU64,
    pub(super) callback_frames_min: AtomicU64,
    pub(super) callback_frames_max: AtomicU64,
    pub(super) queued_frames: AtomicU64,
    pub(super) queued_frames_max: AtomicU64,
    pub(super) pcm_enqueued_chunks: AtomicU64,
    pub(super) pcm_enqueued_frames: AtomicU64,
    pub(super) pcm_enqueue_max_gap_ms: AtomicU64,
    pub(super) prebuffer_target_frames: AtomicU64,
    started_at: Instant,
    last_pcm_enqueue_ms: AtomicU64,
}

impl Default for VoiceAudioOutputStats {
    fn default() -> Self {
        Self {
            queue_full_drops: AtomicU64::new(0),
            output_underruns: AtomicU64::new(0),
            recent_pcm_underruns: AtomicU64::new(0),
            callback_max_gap_ms: AtomicU64::new(0),
            callback_count: AtomicU64::new(0),
            callback_requested_frames: AtomicU64::new(0),
            callback_frames_min: AtomicU64::new(u64::MAX),
            callback_frames_max: AtomicU64::new(0),
            queued_frames: AtomicU64::new(0),
            queued_frames_max: AtomicU64::new(0),
            pcm_enqueued_chunks: AtomicU64::new(0),
            pcm_enqueued_frames: AtomicU64::new(0),
            pcm_enqueue_max_gap_ms: AtomicU64::new(0),
            prebuffer_target_frames: AtomicU64::new(VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES),
            started_at: Instant::now(),
            last_pcm_enqueue_ms: AtomicU64::new(0),
        }
    }
}

impl VoiceAudioOutputStats {
    pub(super) fn record_pcm_enqueue(&self, frames: usize) {
        let now_ms = self.elapsed_ms().saturating_add(1);
        let previous_ms = self.last_pcm_enqueue_ms.swap(now_ms, Ordering::Relaxed);
        if previous_ms != 0 {
            self.pcm_enqueue_max_gap_ms
                .fetch_max(now_ms.saturating_sub(previous_ms), Ordering::Relaxed);
        }
        self.pcm_enqueued_chunks.fetch_add(1, Ordering::Relaxed);
        self.pcm_enqueued_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        self.queued_frames_max.fetch_max(
            self.queued_frames.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }

    pub(super) fn record_callback(&self, frames: usize) {
        let frames = frames as u64;
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        self.callback_requested_frames
            .fetch_add(frames, Ordering::Relaxed);
        self.callback_frames_min
            .fetch_min(frames, Ordering::Relaxed);
        self.callback_frames_max
            .fetch_max(frames, Ordering::Relaxed);
    }

    fn pcm_was_recent(&self) -> bool {
        let last_pcm_enqueue_ms = self.last_pcm_enqueue_ms.load(Ordering::Relaxed);
        last_pcm_enqueue_ms != 0
            && self
                .elapsed_ms()
                .saturating_add(1)
                .saturating_sub(last_pcm_enqueue_ms)
                <= RECENT_PCM_WINDOW.as_millis() as u64
    }

    fn elapsed_ms(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

impl VoiceAudioBuffer {
    pub(super) fn new(
        samples_rx: StdReceiver<Vec<f32>>,
        output_sample_rate: u32,
        stats: Arc<VoiceAudioOutputStats>,
    ) -> Self {
        Self {
            samples_rx,
            stats,
            current: Vec::new(),
            offset: 0,
            output_sample_rate,
            source_position: 0.0,
            last_frame: [0.0, 0.0],
            fade_remaining_frames: 0,
            fade_total_frames: voice_output_underrun_fade_frames(output_sample_rate),
            received_audio: false,
            underrunning: false,
            buffering: true,
        }
    }

    pub(super) fn begin_output(&mut self, callback_frames: usize) {
        let target_frames = voice_output_prebuffer_frames(callback_frames, self.output_sample_rate);
        self.stats
            .prebuffer_target_frames
            .store(target_frames, Ordering::Relaxed);
        if self.buffering && self.stats.queued_frames.load(Ordering::Relaxed) >= target_frames {
            self.buffering = false;
        }
    }

    pub(super) fn next_stereo_frame(&mut self) -> Option<[f32; 2]> {
        if self.buffering {
            return self.next_fade_stereo_frame();
        }
        let frame = if self.output_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.next_native_stereo_frame()
        } else {
            self.next_resampled_stereo_frame()
        };
        match frame {
            Some(frame) => {
                self.underrunning = false;
                self.last_frame = frame;
                self.fade_remaining_frames = self.fade_total_frames;
                Some(frame)
            }
            None => {
                if self.received_audio && !self.underrunning {
                    self.stats.output_underruns.fetch_add(1, Ordering::Relaxed);
                    if self.stats.pcm_was_recent() {
                        self.stats
                            .recent_pcm_underruns
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    self.underrunning = true;
                }
                self.buffering = true;
                self.next_fade_stereo_frame()
            }
        }
    }

    fn next_native_stereo_frame(&mut self) -> Option<[f32; 2]> {
        loop {
            if self.offset + 1 < self.current.len() {
                let frame = [self.current[self.offset], self.current[self.offset + 1]];
                self.offset += usize::from(DISCORD_VOICE_CHANNELS);
                return Some(frame);
            }
            if !self.receive_next_samples() {
                return None;
            }
        }
    }

    fn next_resampled_stereo_frame(&mut self) -> Option<[f32; 2]> {
        loop {
            let frame_count = self.current.len() / usize::from(DISCORD_VOICE_CHANNELS);
            if frame_count == 0 || self.source_position >= frame_count as f64 {
                self.source_position -= frame_count as f64;
                if self.source_position < 0.0 {
                    self.source_position = 0.0;
                }
                if !self.receive_next_samples() {
                    return None;
                }
                continue;
            }

            let base_frame = self.source_position.floor() as usize;
            let next_frame = (base_frame + 1).min(frame_count - 1);
            let fraction = (self.source_position - base_frame as f64) as f32;
            let frame = interpolate_voice_stereo_frame(
                voice_stereo_frame_at(&self.current, base_frame),
                voice_stereo_frame_at(&self.current, next_frame),
                fraction,
            );
            self.source_position +=
                f64::from(DISCORD_VOICE_SAMPLE_RATE) / f64::from(self.output_sample_rate.max(1));
            return Some(frame);
        }
    }

    fn receive_next_samples(&mut self) -> bool {
        match self.samples_rx.try_recv() {
            Ok(samples) => {
                self.record_dequeued_samples(&samples);
                self.current = samples;
                self.offset = 0;
                self.received_audio = true;
                true
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
        }
    }

    fn record_dequeued_samples(&self, samples: &[f32]) {
        let frames = samples.len() / usize::from(DISCORD_VOICE_CHANNELS);
        self.stats
            .queued_frames
            .fetch_sub(frames as u64, Ordering::Relaxed);
    }

    fn next_fade_stereo_frame(&mut self) -> Option<[f32; 2]> {
        if self.fade_remaining_frames == 0 || self.fade_total_frames == 0 {
            return None;
        }
        let gain = self.fade_remaining_frames as f32 / (self.fade_total_frames + 1) as f32;
        self.fade_remaining_frames -= 1;
        Some([self.last_frame[0] * gain, self.last_frame[1] * gain])
    }

    pub(super) fn clear_pending(&mut self) {
        self.current.clear();
        self.offset = 0;
        self.source_position = 0.0;
        self.last_frame = [0.0, 0.0];
        self.fade_remaining_frames = 0;
        self.received_audio = false;
        self.underrunning = false;
        self.buffering = true;
        while let Ok(samples) = self.samples_rx.try_recv() {
            self.record_dequeued_samples(&samples);
        }
    }
}

pub(super) fn voice_output_prebuffer_frames(
    callback_frames: usize,
    output_sample_rate: u32,
) -> u64 {
    let callback_frames = u64::try_from(callback_frames).unwrap_or(u64::MAX);
    let source_frames = callback_frames
        .saturating_mul(u64::from(DISCORD_VOICE_SAMPLE_RATE))
        .div_ceil(u64::from(output_sample_rate.max(1)));
    VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES.saturating_add(source_frames)
}

fn voice_output_underrun_fade_frames(output_sample_rate: u32) -> usize {
    ((output_sample_rate.max(1) * VOICE_OUTPUT_UNDERRUN_FADE_MILLIS) / 1_000).max(1) as usize
}

fn voice_stereo_frame_at(samples: &[f32], frame: usize) -> [f32; 2] {
    let offset = frame * usize::from(DISCORD_VOICE_CHANNELS);
    [samples[offset], samples[offset + 1]]
}

fn interpolate_voice_stereo_frame(left: [f32; 2], right: [f32; 2], fraction: f32) -> [f32; 2] {
    [
        left[0] + (right[0] - left[0]) * fraction,
        left[1] + (right[1] - left[1]) * fraction,
    ]
}
