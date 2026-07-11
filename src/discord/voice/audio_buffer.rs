use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver as StdReceiver, TryRecvError},
};

use super::{
    DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE, VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES,
    VOICE_OUTPUT_UNDERRUN_FADE_MILLIS,
};

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

#[derive(Default)]
pub(super) struct VoiceAudioOutputStats {
    pub(super) queue_full_drops: AtomicU64,
    pub(super) output_underruns: AtomicU64,
    pub(super) callback_max_gap_ms: AtomicU64,
    pub(super) queued_frames: AtomicU64,
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

    pub(super) fn begin_output(&mut self) {
        if self.buffering
            && self.stats.queued_frames.load(Ordering::Relaxed)
                >= VOICE_AUDIO_OUTPUT_PREBUFFER_FRAMES
        {
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
