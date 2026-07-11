use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[cfg(feature = "voice-playback")]
use std::sync::Arc;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(feature = "voice-playback")]
use std::sync::mpsc::{Receiver as StdReceiver, SyncSender, sync_channel};

#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[cfg(feature = "voice-playback")]
use super::VOICE_AUDIO_OUTPUT_QUEUE;
#[cfg(all(feature = "voice-playback", target_os = "linux"))]
use super::VOICE_PULSE_OUTPUT_BUFFER_FRAMES;
#[cfg(feature = "voice-playback")]
use super::audio_buffer::{VoiceAudioBuffer, VoiceAudioOutputStats};
#[cfg(all(feature = "voice-playback", target_os = "linux"))]
use super::log_captured_alsa_errors;
use super::{
    DISCORD_OPUS_TIMESTAMP_INCREMENT, DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE,
    VOICE_OUTPUT_LOW_PASS_CUTOFF_HZ, VOICE_PLAYBACK_JITTER_BUFFER_DELAY,
    VOICE_PLAYBACK_MAX_BUFFERED_FRAMES_PER_SSRC, VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES,
};
use crate::discord::VoiceVolumePercent;
#[cfg(feature = "voice-playback")]
use crate::logging;
#[cfg(feature = "voice-playback")]
use crate::support::audio_output::{self, F32OutputSource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VoicePlaybackFrame {
    pub(super) ssrc: u32,
    pub(super) user_id: Option<u64>,
    pub(super) sequence: u16,
    pub(super) timestamp: u32,
    pub(super) opus: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum VoicePlayoutFrame {
    Audio(VoicePlaybackFrame),
    PacketLoss {
        ssrc: u32,
        user_id: Option<u64>,
        sequence: u16,
        timestamp_step: u32,
    },
}

#[derive(Clone, Copy)]
pub(super) struct VoicePlaybackPostProcess {
    low_pass: VoiceStereoLowPass,
}

#[derive(Clone, Copy)]
struct VoiceStereoLowPass {
    alpha: f32,
    previous: [f32; 2],
    initialized: bool,
}

#[derive(Default)]
pub(super) struct VoicePlaybackPlayoutBuffers {
    buffers: HashMap<u32, VoicePlaybackPlayoutBuffer>,
}

#[derive(Default)]
pub(super) struct VoicePlaybackPlayoutBuffer {
    ssrc: Option<u32>,
    frames: Vec<VoicePlaybackFrame>,
    next_sequence: Option<u16>,
    next_playout_at: Option<Instant>,
    first_buffered_at: Option<Instant>,
    started: bool,
    last_user_id: Option<u64>,
    last_timestamp_step: Option<u32>,
    consecutive_missing: usize,
}

#[cfg(feature = "voice-playback")]
pub(super) struct VoiceAudioOutput {
    pub(super) samples_tx: SyncSender<Vec<f32>>,
    pub(super) stats: Arc<VoiceAudioOutputStats>,
    _stream: cpal::Stream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoicePlaybackGate {
    pub(super) enabled: bool,
    pub(super) volume: VoiceVolumePercent,
}

impl VoicePlayoutFrame {
    pub(super) fn ssrc(&self) -> u32 {
        match self {
            Self::Audio(frame) => frame.ssrc,
            Self::PacketLoss { ssrc, .. } => *ssrc,
        }
    }

    pub(super) fn user_id(&self) -> Option<u64> {
        match self {
            Self::Audio(frame) => frame.user_id,
            Self::PacketLoss { user_id, .. } => *user_id,
        }
    }

    pub(super) fn sequence(&self) -> u16 {
        match self {
            Self::Audio(frame) => frame.sequence,
            Self::PacketLoss { sequence, .. } => *sequence,
        }
    }

    pub(super) fn opus(&self) -> &[u8] {
        match self {
            Self::Audio(frame) => &frame.opus,
            Self::PacketLoss { .. } => &[],
        }
    }

    pub(super) fn is_packet_loss(&self) -> bool {
        matches!(self, Self::PacketLoss { .. })
    }

    pub(super) fn packet_loss_samples_per_channel(&self) -> usize {
        match self {
            Self::Audio(_) => DISCORD_OPUS_TIMESTAMP_INCREMENT as usize,
            Self::PacketLoss { timestamp_step, .. } => *timestamp_step as usize,
        }
    }
}

impl Default for VoicePlaybackPostProcess {
    fn default() -> Self {
        Self {
            low_pass: VoiceStereoLowPass::new(
                VOICE_OUTPUT_LOW_PASS_CUTOFF_HZ,
                DISCORD_VOICE_SAMPLE_RATE,
            ),
        }
    }
}

impl VoicePlaybackPostProcess {
    pub(super) fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(usize::from(DISCORD_VOICE_CHANNELS)) {
            let filtered = self.low_pass.process([frame[0], frame[1]]);
            frame[0] = filtered[0];
            frame[1] = filtered[1];
        }
    }
}

impl VoiceStereoLowPass {
    fn new(cutoff_hz: f32, sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1) as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz.max(1.0));
        let dt = 1.0 / sample_rate;
        Self {
            alpha: dt / (rc + dt),
            previous: [0.0, 0.0],
            initialized: false,
        }
    }

    pub(super) fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        if !self.initialized {
            self.previous = frame;
            self.initialized = true;
            return frame;
        }
        self.previous[0] += self.alpha * (frame[0] - self.previous[0]);
        self.previous[1] += self.alpha * (frame[1] - self.previous[1]);
        self.previous
    }
}

impl VoicePlaybackPlayoutBuffers {
    pub(super) fn push(&mut self, frame: VoicePlaybackFrame, now: Instant) {
        self.buffers.entry(frame.ssrc).or_default().push(frame, now);
    }

    pub(super) fn next_frames(&mut self, now: Instant) -> Vec<VoicePlayoutFrame> {
        let mut frames = Vec::new();
        for buffer in self.buffers.values_mut() {
            if let Some(frame) = buffer.next_frame(now) {
                frames.push(frame);
            }
        }
        self.buffers.retain(|_, buffer| !buffer.is_idle());
        frames
    }
}

impl VoicePlaybackPlayoutBuffer {
    pub(super) fn push(&mut self, frame: VoicePlaybackFrame, now: Instant) -> bool {
        self.ssrc = Some(frame.ssrc);
        if let Some(next_sequence) = self.next_sequence {
            if voice_sequence_before(frame.sequence, next_sequence) {
                if self.started {
                    return false;
                }
                self.next_sequence = Some(frame.sequence);
            }
        } else {
            self.next_sequence = Some(frame.sequence);
            self.first_buffered_at = Some(now);
        }

        self.last_user_id = frame.user_id;
        if self
            .frames
            .iter()
            .any(|queued| queued.sequence == frame.sequence)
        {
            return false;
        }

        self.frames.push(frame);
        if self.frames.len() > VOICE_PLAYBACK_MAX_BUFFERED_FRAMES_PER_SSRC {
            self.drop_farthest_buffered_frame();
        }
        true
    }

    pub(super) fn next_frame(&mut self, now: Instant) -> Option<VoicePlayoutFrame> {
        let next_sequence = self.next_sequence?;
        if !self.started {
            let aged_enough = self.first_buffered_at.is_some_and(|started_at| {
                now.duration_since(started_at) >= VOICE_PLAYBACK_JITTER_BUFFER_DELAY
            });
            let buffered_enough = self.buffered_contiguous_duration(next_sequence)
                >= VOICE_PLAYBACK_JITTER_BUFFER_DELAY;
            if !buffered_enough && !aged_enough {
                return None;
            }
            self.started = true;
        }
        if self
            .next_playout_at
            .is_some_and(|playout_at| now < playout_at)
        {
            return None;
        }

        self.drop_stale_frames(next_sequence);

        if let Some(position) = self
            .frames
            .iter()
            .position(|frame| frame.sequence == next_sequence)
        {
            let frame = self.frames.remove(position);
            self.advance_after_audio(&frame, now);
            return Some(VoicePlayoutFrame::Audio(frame));
        }

        if self.frames.is_empty() {
            return self.next_packet_loss_frame_or_stop(next_sequence, now);
        }

        if self.consecutive_missing < VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES {
            return Some(self.packet_loss_frame(next_sequence, now));
        }

        self.skip_to_next_buffered_frame(next_sequence, now)
    }

    fn drop_stale_frames(&mut self, next_sequence: u16) {
        self.frames
            .retain(|frame| !voice_sequence_before(frame.sequence, next_sequence));
    }

    fn drop_farthest_buffered_frame(&mut self) {
        let Some(next_sequence) = self.next_sequence else {
            let _ = self.frames.pop();
            return;
        };
        if let Some((position, _)) = self
            .frames
            .iter()
            .enumerate()
            .max_by_key(|(_, frame)| voice_sequence_distance(next_sequence, frame.sequence))
        {
            let _ = self.frames.remove(position);
        }
    }

    fn next_packet_loss_frame_or_stop(
        &mut self,
        next_sequence: u16,
        now: Instant,
    ) -> Option<VoicePlayoutFrame> {
        if self.consecutive_missing < VOICE_PLAYBACK_MAX_CONSECUTIVE_PLC_FRAMES {
            return Some(self.packet_loss_frame(next_sequence, now));
        }
        self.reset_idle();
        None
    }

    fn packet_loss_frame(&mut self, sequence: u16, now: Instant) -> VoicePlayoutFrame {
        self.consecutive_missing += 1;
        let timestamp_step = self.last_timestamp_step();
        self.next_sequence = Some(sequence.wrapping_add(1));
        self.schedule_next_playout(voice_timestamp_step_duration(timestamp_step), now);
        VoicePlayoutFrame::PacketLoss {
            ssrc: self.ssrc.unwrap_or_default(),
            user_id: self.last_user_id,
            sequence,
            timestamp_step,
        }
    }

    fn skip_to_next_buffered_frame(
        &mut self,
        next_sequence: u16,
        now: Instant,
    ) -> Option<VoicePlayoutFrame> {
        let position = self
            .frames
            .iter()
            .enumerate()
            .min_by_key(|(_, frame)| voice_sequence_distance(next_sequence, frame.sequence))
            .map(|(position, _)| position)?;
        let frame = self.frames.remove(position);
        self.advance_after_audio(&frame, now);
        Some(VoicePlayoutFrame::Audio(frame))
    }

    fn advance_after_audio(&mut self, frame: &VoicePlaybackFrame, now: Instant) {
        self.next_sequence = Some(frame.sequence.wrapping_add(1));
        self.consecutive_missing = 0;
        self.first_buffered_at = None;
        let step = self.playout_step_duration_after(frame);
        self.schedule_next_playout(step, now);
    }

    fn playout_step_duration_after(&mut self, frame: &VoicePlaybackFrame) -> Duration {
        if let Some(timestamp_step) = self.timestamp_step_after(frame) {
            self.last_timestamp_step = Some(timestamp_step);
            return voice_timestamp_step_duration(timestamp_step);
        }
        self.last_playout_step_duration()
    }

    fn timestamp_step_after(&self, frame: &VoicePlaybackFrame) -> Option<u32> {
        self.frames
            .iter()
            .filter_map(|queued| {
                let sequence_distance =
                    u32::from(voice_sequence_distance(frame.sequence, queued.sequence));
                if sequence_distance == 0 || sequence_distance >= 0x8000 {
                    return None;
                }
                let timestamp_distance = queued.timestamp.wrapping_sub(frame.timestamp);
                if timestamp_distance % sequence_distance != 0 {
                    return None;
                }
                let timestamp_step = timestamp_distance / sequence_distance;
                valid_voice_timestamp_step(timestamp_step)
                    .then_some((sequence_distance, timestamp_step))
            })
            .min_by_key(|(sequence_distance, _)| *sequence_distance)
            .map(|(_, timestamp_step)| timestamp_step)
    }

    fn buffered_contiguous_duration(&self, next_sequence: u16) -> Duration {
        let mut sequence = next_sequence;
        let mut previous = None;
        let mut last_timestamp_step = None;
        let mut duration = Duration::ZERO;
        while let Some(frame) = self
            .frames
            .iter()
            .find(|queued| queued.sequence == sequence)
        {
            if let Some(previous_timestamp) = previous {
                let timestamp_step = frame.timestamp.wrapping_sub(previous_timestamp);
                if !valid_voice_timestamp_step(timestamp_step) {
                    break;
                }
                last_timestamp_step = Some(timestamp_step);
                duration += voice_timestamp_step_duration(timestamp_step);
            }
            previous = Some(frame.timestamp);
            sequence = sequence.wrapping_add(1);
        }
        if previous.is_some() {
            duration += voice_timestamp_step_duration(
                last_timestamp_step.unwrap_or(DISCORD_OPUS_TIMESTAMP_INCREMENT),
            );
        }
        duration
    }

    fn schedule_next_playout(&mut self, step: Duration, now: Instant) {
        let base = self.next_playout_at.unwrap_or(now);
        let schedule_base =
            if now.saturating_duration_since(base) > VOICE_PLAYBACK_JITTER_BUFFER_DELAY {
                now
            } else {
                base
            };
        self.next_playout_at = Some(schedule_base + step);
    }

    fn last_playout_step_duration(&self) -> Duration {
        voice_timestamp_step_duration(self.last_timestamp_step())
    }

    fn last_timestamp_step(&self) -> u32 {
        self.last_timestamp_step
            .unwrap_or(DISCORD_OPUS_TIMESTAMP_INCREMENT)
    }

    fn reset_idle(&mut self) {
        self.next_sequence = None;
        self.next_playout_at = None;
        self.first_buffered_at = None;
        self.started = false;
        self.last_timestamp_step = None;
        self.consecutive_missing = 0;
    }

    fn is_idle(&self) -> bool {
        self.frames.is_empty() && self.next_sequence.is_none()
    }
}

fn voice_sequence_before(sequence: u16, reference: u16) -> bool {
    let distance = reference.wrapping_sub(sequence);
    distance != 0 && distance < 0x8000
}

fn voice_sequence_distance(from: u16, to: u16) -> u16 {
    to.wrapping_sub(from)
}

fn valid_voice_timestamp_step(timestamp_step: u32) -> bool {
    timestamp_step > 0 && timestamp_step <= DISCORD_OPUS_TIMESTAMP_INCREMENT * 6
}

fn voice_timestamp_step_duration(timestamp_step: u32) -> Duration {
    Duration::from_micros(
        u64::from(timestamp_step) * 1_000_000 / u64::from(DISCORD_VOICE_SAMPLE_RATE),
    )
}

#[cfg(feature = "voice-playback")]
impl VoiceAudioOutput {
    pub(super) fn start(
        playback_enabled: Arc<AtomicBool>,
        playback_volume: Arc<AtomicU8>,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(playback_enabled, playback_volume);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    fn start_with_cpal(
        playback_enabled: Arc<AtomicBool>,
        playback_volume: Arc<AtomicU8>,
    ) -> Result<Self, String> {
        let (samples_tx, samples_rx) = sync_channel(VOICE_AUDIO_OUTPUT_QUEUE);
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device is available".to_owned())?;
        let supported_config = select_voice_output_config(&device)?;
        let sample_format = supported_config.sample_format();
        let mut stream_config = supported_config.config();
        #[cfg(target_os = "linux")]
        if host.id() == cpal::HostId::PulseAudio {
            stream_config.buffer_size = cpal::BufferSize::Fixed(VOICE_PULSE_OUTPUT_BUFFER_FRAMES);
        }
        let stats = Arc::new(VoiceAudioOutputStats::default());
        let stream = build_voice_output_stream(
            &device,
            &stream_config,
            sample_format,
            samples_rx,
            playback_enabled,
            playback_volume,
            Arc::clone(&stats),
        )?;
        stream
            .play()
            .map_err(|error| format!("voice audio output stream start failed: {error}"))?;
        logging::debug(
            "voice",
            format!(
                "voice audio output stream started: host={} sample_rate={} channels={} format={:?} buffer_size={:?}",
                host.id(),
                stream_config.sample_rate,
                stream_config.channels,
                sample_format,
                stream_config.buffer_size,
            ),
        );
        Ok(Self {
            samples_tx,
            stats,
            _stream: stream,
        })
    }
}

#[cfg(feature = "voice-playback")]
fn select_voice_output_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, String> {
    let sample_rate = DISCORD_VOICE_SAMPLE_RATE;
    let configs = device
        .supported_output_configs()
        .map_err(|error| format!("voice audio output config query failed: {error}"))?;
    if let Some(config) = configs
        .filter(|config| {
            config.channels() == DISCORD_VOICE_CHANNELS
                && config.min_sample_rate() <= sample_rate
                && config.max_sample_rate() >= sample_rate
        })
        .min_by_key(|config| voice_output_sample_format_rank(config.sample_format()))
    {
        return Ok(config.with_sample_rate(sample_rate));
    }
    device
        .default_output_config()
        .map_err(|error| format!("voice default audio output config failed: {error}"))
}

#[cfg(feature = "voice-playback")]
fn voice_output_sample_format_rank(format: cpal::SampleFormat) -> u8 {
    match format {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::I16 => 1,
        cpal::SampleFormat::U16 => 2,
        cpal::SampleFormat::U8 => 3,
        _ => 4,
    }
}

#[cfg(feature = "voice-playback")]
fn build_voice_output_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    samples_rx: StdReceiver<Vec<f32>>,
    playback_enabled: Arc<AtomicBool>,
    playback_volume: Arc<AtomicU8>,
    stats: Arc<VoiceAudioOutputStats>,
) -> Result<cpal::Stream, String> {
    audio_output::build_f32_output_stream(
        device,
        config,
        sample_format,
        VoiceOutputSource {
            buffer: VoiceAudioBuffer::new(samples_rx, config.sample_rate, Arc::clone(&stats)),
            playback_enabled,
            playback_volume,
            stats,
            last_callback_at: None,
        },
        log_voice_output_stream_error,
        "voice audio output",
        "voice audio",
    )
}

#[cfg(feature = "voice-playback")]
struct VoiceOutputSource {
    buffer: VoiceAudioBuffer,
    playback_enabled: Arc<AtomicBool>,
    playback_volume: Arc<AtomicU8>,
    stats: Arc<VoiceAudioOutputStats>,
    last_callback_at: Option<Instant>,
}

#[cfg(feature = "voice-playback")]
impl F32OutputSource for VoiceOutputSource {
    fn fill<T>(&mut self, output: &mut [T], channels: usize, convert: fn(f32) -> T)
    where
        T: Default + Copy,
    {
        let callback_at = Instant::now();
        if let Some(previous) = self.last_callback_at.replace(callback_at) {
            let gap_ms = callback_at
                .duration_since(previous)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            self.stats
                .callback_max_gap_ms
                .fetch_max(gap_ms, Ordering::Relaxed);
        }
        if !self.playback_enabled.load(Ordering::Relaxed) {
            self.buffer.clear_pending();
            for frame in output.chunks_mut(channels) {
                write_voice_output_frame(frame, 0.0, 0.0, convert);
            }
            return;
        }
        self.buffer.begin_output();
        let gain = f32::from(self.playback_volume.load(Ordering::Relaxed).min(100)) / 100.0;
        for frame in output.chunks_mut(channels) {
            let [left, right] = self.buffer.next_stereo_frame().unwrap_or([0.0, 0.0]);
            write_voice_output_frame(frame, left * gain, right * gain, convert);
        }
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn write_voice_output_frame<T>(
    output: &mut [T],
    left: f32,
    right: f32,
    convert: fn(f32) -> T,
) where
    T: Default + Copy,
{
    match output {
        [] => {}
        [mono] => *mono = convert((left + right) * 0.5),
        [first, second, rest @ ..] => {
            *first = convert(left);
            *second = convert(right);
            for sample in rest {
                *sample = convert(0.0);
            }
        }
    }
}

pub(super) fn clamp_voice_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

#[cfg(feature = "voice-playback")]
fn log_voice_output_stream_error(error: cpal::Error) {
    logging::error(
        "voice",
        format!("voice audio output stream failed: {error}"),
    );
}
