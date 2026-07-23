use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};

#[cfg(feature = "voice-playback")]
use std::sync::Arc;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(feature = "voice-playback")]
use std::sync::mpsc::{SyncSender, TrySendError};

use ::opus::{
    Application as OpusApplication, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder,
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

#[cfg(feature = "voice-playback")]
use super::audio_buffer::VoiceAudioOutputStats;
#[cfg(feature = "voice-playback")]
use super::playback::VoiceAudioOutput;
use super::playback::{
    VoicePlaybackFrame, VoicePlaybackGate, VoicePlaybackPlayoutBuffers, VoicePlaybackPostProcess,
    VoicePlayoutFrame,
};
use super::{
    DISCORD_OPUS_20MS_STEREO_SAMPLES, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL,
    DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE, OPUS_MAX_ENCODED_FRAME_BYTES,
    OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL, VOICE_OUTPUT_STATS_LOG_INTERVAL,
    VOICE_PLAYBACK_FRAME_QUEUE, VOICE_PLAYBACK_POLL_DURATION,
    VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL, VoiceParticipantPlaybackSettings,
};
use crate::discord::ids::{Id, marker::UserMarker};
use crate::logging;

#[allow(dead_code)]
pub(super) struct VoiceOpusEncode {
    encoder: OpusEncoder,
}

pub(super) struct VoiceOpusDecode {
    pub(super) frames_tx: mpsc::Sender<VoicePlaybackFrame>,
    pub(super) task: JoinHandle<()>,
    #[cfg(feature = "voice-playback")]
    pub(super) audio_output: Option<VoiceAudioOutput>,
    #[cfg(feature = "voice-playback")]
    pub(super) playback_enabled: Arc<AtomicBool>,
    #[cfg(feature = "voice-playback")]
    pub(super) playback_volume: Arc<AtomicU8>,
}

struct VoiceDecodedAudio {
    #[cfg(feature = "voice-playback")]
    samples_tx: Option<SyncSender<Vec<f32>>>,
    #[cfg(feature = "voice-playback")]
    stats: Option<Arc<VoiceAudioOutputStats>>,
}

#[derive(Default)]
pub(super) struct VoicePlaybackDecodeState {
    decoders: HashMap<u32, OpusDecoder>,
    pending_samples: HashMap<u32, PendingVoiceSamples>,
    participant_playback_settings: HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
}

#[derive(Default)]
struct PendingVoiceSamples {
    user_id: Option<Id<UserMarker>>,
    samples: VecDeque<f32>,
}

impl VoiceOpusDecode {
    #[cfg(not(feature = "voice-playback"))]
    pub(super) fn start(
        playback_gate: VoicePlaybackGate,
        participant_playback_rx: tokio::sync::watch::Receiver<
            HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
        >,
        audio_handle: &tokio::runtime::Handle,
    ) -> Self {
        let _ = playback_gate;
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let task = audio_handle.spawn(run_voice_playback_decode(
            frames_rx,
            VoiceDecodedAudio::decode_only(),
            participant_playback_rx,
        ));
        logging::debug(
            "voice",
            "voice Opus decode worker started without audio output device",
        );
        Self { frames_tx, task }
    }

    #[cfg(feature = "voice-playback")]
    pub(super) fn start(
        playback_gate: VoicePlaybackGate,
        participant_playback_rx: tokio::sync::watch::Receiver<
            HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
        >,
        audio_handle: &tokio::runtime::Handle,
    ) -> Self {
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let playback_enabled = Arc::new(AtomicBool::new(playback_gate.enabled));
        let playback_volume = Arc::new(AtomicU8::new(playback_gate.volume.value()));
        match VoiceAudioOutput::start(Arc::clone(&playback_enabled), Arc::clone(&playback_volume)) {
            Ok(audio_output) => {
                let decoded_audio = VoiceDecodedAudio::output(
                    audio_output.samples_tx.clone(),
                    Arc::clone(&audio_output.stats),
                );
                let task = audio_handle.spawn(run_voice_playback_decode(
                    frames_rx,
                    decoded_audio,
                    participant_playback_rx,
                ));
                logging::debug(
                    "voice",
                    "voice Opus playback worker started with audio output",
                );
                Self {
                    frames_tx,
                    task,
                    audio_output: Some(audio_output),
                    playback_enabled,
                    playback_volume,
                }
            }
            Err(error) => {
                logging::error(
                    "voice",
                    format!("voice audio output unavailable, falling back to decode-only: {error}"),
                );
                let task = audio_handle.spawn(run_voice_playback_decode(
                    frames_rx,
                    VoiceDecodedAudio::decode_only(),
                    participant_playback_rx,
                ));
                Self {
                    frames_tx,
                    task,
                    audio_output: None,
                    playback_enabled,
                    playback_volume,
                }
            }
        }
    }
}

impl VoiceDecodedAudio {
    fn decode_only() -> Self {
        Self {
            #[cfg(feature = "voice-playback")]
            samples_tx: None,
            #[cfg(feature = "voice-playback")]
            stats: None,
        }
    }

    #[cfg(feature = "voice-playback")]
    fn output(samples_tx: SyncSender<Vec<f32>>, stats: Arc<VoiceAudioOutputStats>) -> Self {
        Self {
            samples_tx: Some(samples_tx),
            stats: Some(stats),
        }
    }

    fn try_send(&self, samples: Vec<f32>) {
        #[cfg(feature = "voice-playback")]
        if let (Some(samples_tx), Some(stats)) = (self.samples_tx.as_ref(), self.stats.as_ref()) {
            let frames = samples.len() / usize::from(DISCORD_VOICE_CHANNELS);
            stats
                .queued_frames
                .fetch_add(frames as u64, Ordering::Relaxed);
            match samples_tx.try_send(samples) {
                Ok(()) => stats.record_pcm_enqueue(frames),
                Err(TrySendError::Full(_)) => {
                    stats
                        .queued_frames
                        .fetch_sub(frames as u64, Ordering::Relaxed);
                    stats.queue_full_drops.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => {
                    stats
                        .queued_frames
                        .fetch_sub(frames as u64, Ordering::Relaxed);
                }
            }
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = samples;
        }
    }

    fn log_output_stats(&self) {
        #[cfg(feature = "voice-playback")]
        if let Some(stats) = self.stats.as_ref() {
            let callback_frames_min = stats.callback_frames_min.swap(u64::MAX, Ordering::Relaxed);
            logging::debug(
                "voice",
                format!(
                    "voice audio stats: queue_full_drops={} output_underruns={} recent_pcm_underruns={} callback_count={} callback_requested_frames={} callback_frames_min={} callback_frames_max={} callback_max_gap_ms={} pcm_enqueued_chunks={} pcm_enqueued_frames={} pcm_enqueue_max_gap_ms={} queued_frames={} queued_frames_max={} prebuffer_target_frames={}",
                    stats.queue_full_drops.swap(0, Ordering::Relaxed),
                    stats.output_underruns.swap(0, Ordering::Relaxed),
                    stats.recent_pcm_underruns.swap(0, Ordering::Relaxed),
                    stats.callback_count.swap(0, Ordering::Relaxed),
                    stats.callback_requested_frames.swap(0, Ordering::Relaxed),
                    if callback_frames_min == u64::MAX {
                        0
                    } else {
                        callback_frames_min
                    },
                    stats.callback_frames_max.swap(0, Ordering::Relaxed),
                    stats.callback_max_gap_ms.swap(0, Ordering::Relaxed),
                    stats.pcm_enqueued_chunks.swap(0, Ordering::Relaxed),
                    stats.pcm_enqueued_frames.swap(0, Ordering::Relaxed),
                    stats.pcm_enqueue_max_gap_ms.swap(0, Ordering::Relaxed),
                    stats.queued_frames.load(Ordering::Relaxed),
                    stats.queued_frames_max.swap(0, Ordering::Relaxed),
                    stats.prebuffer_target_frames.load(Ordering::Relaxed),
                ),
            );
        }
    }
}

#[allow(dead_code)]
impl VoiceOpusEncode {
    pub(super) fn new() -> Result<Self, String> {
        OpusEncoder::new(
            DISCORD_VOICE_SAMPLE_RATE,
            Channels::Stereo,
            OpusApplication::Voip,
        )
        .map(|encoder| Self { encoder })
        .map_err(|error| format!("voice Opus encoder init failed: {error}"))
    }

    pub(super) fn encode_20ms_i16(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
        if pcm.len() != DISCORD_OPUS_20MS_STEREO_SAMPLES {
            return Err(format!(
                "voice Opus encoder expected {} interleaved stereo samples, got {}",
                DISCORD_OPUS_20MS_STEREO_SAMPLES,
                pcm.len()
            ));
        }
        self.encoder
            .encode_vec(pcm, OPUS_MAX_ENCODED_FRAME_BYTES)
            .map_err(|error| format!("voice Opus encode failed: {error}"))
    }
}

async fn run_voice_playback_decode(
    mut frames_rx: mpsc::Receiver<VoicePlaybackFrame>,
    decoded_audio: VoiceDecodedAudio,
    mut participant_playback_rx: tokio::sync::watch::Receiver<
        HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
    >,
) {
    let mut decode_state = VoicePlaybackDecodeState::default();
    decode_state
        .replace_participant_playback_settings(participant_playback_rx.borrow_and_update().clone());
    let mut participant_playback_open = true;
    let mut playout_buffers = VoicePlaybackPlayoutBuffers::default();
    let mut post_process = VoicePlaybackPostProcess::default();
    let mut playout_tick = interval(VOICE_PLAYBACK_POLL_DURATION);
    playout_tick.set_missed_tick_behavior(MissedTickBehavior::Burst);
    let mut output_stats_tick = interval(VOICE_OUTPUT_STATS_LOG_INTERVAL);
    output_stats_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    output_stats_tick.tick().await;
    let mut decoded_frames = 0u64;
    loop {
        tokio::select! {
            frame = frames_rx.recv() => {
                let Some(frame) = frame else {
                    break;
                };
                playout_buffers.push(frame, Instant::now());
            }
            changed = participant_playback_rx.changed(), if participant_playback_open => {
                match changed {
                    Ok(()) => decode_state.replace_participant_playback_settings(
                        participant_playback_rx.borrow_and_update().clone(),
                    ),
                    Err(_) => participant_playback_open = false,
                }
            }
            _ = playout_tick.tick() => {
                let frames = playout_buffers.next_frames(Instant::now());
                if let Some(mut samples) = decode_state.next_mixed_samples(frames) {
                    post_process.process(&mut samples);
                    let pcm_samples = samples.len();
                    decoded_audio.try_send(samples);
                    decoded_frames = decoded_frames.saturating_add(1);
                    if decoded_frames == 1 || decoded_frames.is_multiple_of(500) {
                        logging::debug(
                            "voice",
                            format!(
                                "voice Opus mixed: count={} pcm_samples={}",
                                decoded_frames, pcm_samples
                            ),
                        );
                    }
                }
            }
            _ = output_stats_tick.tick() => decoded_audio.log_output_stats(),
        }
    }
}

impl VoicePlaybackDecodeState {
    pub(super) fn replace_participant_playback_settings(
        &mut self,
        settings: HashMap<Id<UserMarker>, VoiceParticipantPlaybackSettings>,
    ) {
        self.participant_playback_settings = settings;
    }

    pub(super) fn next_mixed_samples(
        &mut self,
        frames: Vec<VoicePlayoutFrame>,
    ) -> Option<Vec<f32>> {
        for frame in frames {
            let ssrc = frame.ssrc();
            let user_id = frame.user_id();
            if let Some(samples) = decode_voice_playout_frame(frame, &mut self.decoders) {
                self.push_decoded_samples_for_user(ssrc, user_id, samples);
            }
        }
        self.next_pending_mix()
    }

    #[cfg(test)]
    pub(super) fn push_decoded_samples(&mut self, ssrc: u32, samples: Vec<f32>) {
        self.push_decoded_samples_for_user(ssrc, None, samples);
    }

    pub(super) fn push_decoded_samples_for_user(
        &mut self,
        ssrc: u32,
        user_id: Option<Id<UserMarker>>,
        samples: Vec<f32>,
    ) {
        let pending = self.pending_samples.entry(ssrc).or_default();
        if user_id.is_some() {
            pending.user_id = user_id;
        }
        pending.samples.extend(samples);
    }

    pub(super) fn next_pending_mix(&mut self) -> Option<Vec<f32>> {
        let chunk_len =
            VOICE_PLAYBACK_POLL_SAMPLES_PER_CHANNEL * usize::from(DISCORD_VOICE_CHANNELS);
        let participant_playback_settings = &self.participant_playback_settings;
        let chunks: Vec<Vec<f32>> = self
            .pending_samples
            .values_mut()
            .filter_map(|pending| {
                let mut chunk = drain_voice_pending_chunk(&mut pending.samples, chunk_len)?;
                let settings = pending
                    .user_id
                    .and_then(|user_id| participant_playback_settings.get(&user_id))
                    .copied()
                    .unwrap_or_default();
                if settings.muted || settings.volume.value() == 0 {
                    return None;
                }
                let gain = settings.volume.gain();
                if gain != 1.0 {
                    for sample in &mut chunk {
                        *sample *= gain;
                    }
                }
                Some(chunk)
            })
            .collect();
        self.pending_samples
            .retain(|_, pending| !pending.samples.is_empty());
        mix_voice_decoded_samples(&chunks)
    }
}

fn drain_voice_pending_chunk(samples: &mut VecDeque<f32>, chunk_len: usize) -> Option<Vec<f32>> {
    if samples.is_empty() {
        return None;
    }
    let len = samples.len().min(chunk_len);
    Some(samples.drain(..len).collect())
}

pub(super) fn decode_voice_playout_frame(
    frame: VoicePlayoutFrame,
    decoders: &mut HashMap<u32, OpusDecoder>,
) -> Option<Vec<f32>> {
    let ssrc = frame.ssrc();
    let packet_loss_stereo_samples =
        frame.packet_loss_samples_per_channel() * usize::from(DISCORD_VOICE_CHANNELS);
    if frame.is_packet_loss() && !decoders.contains_key(&ssrc) {
        return Some(vec![0.0f32; packet_loss_stereo_samples]);
    }
    if let std::collections::hash_map::Entry::Vacant(entry) = decoders.entry(ssrc) {
        match OpusDecoder::new(DISCORD_VOICE_SAMPLE_RATE, Channels::Stereo) {
            Ok(decoder) => {
                entry.insert(decoder);
            }
            Err(error) => {
                logging::error("voice", format!("voice Opus decoder init failed: {error}"));
                return None;
            }
        }
    }
    let decoder = decoders
        .get_mut(&ssrc)
        .expect("Opus decoder should exist after insertion");
    let decode_sample_capacity = if frame.opus().is_empty() {
        packet_loss_stereo_samples
    } else {
        OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL * usize::from(DISCORD_VOICE_CHANNELS)
    };
    let mut decoded = vec![0.0f32; decode_sample_capacity];
    let samples_per_channel = match decoder.decode_float(frame.opus(), &mut decoded, false) {
        Ok(samples) => samples,
        Err(error) => {
            logging::debug(
                "voice",
                format!(
                    "voice Opus decode failed: ssrc={} seq={} error={}",
                    frame.ssrc(),
                    frame.sequence(),
                    error
                ),
            );
            decoders.remove(&ssrc);
            return Some(vec![0.0f32; packet_loss_stereo_samples]);
        }
    };
    let decoded_len = samples_per_channel * usize::from(DISCORD_VOICE_CHANNELS);
    decoded.truncate(decoded_len);
    if samples_per_channel != DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL {
        logging::debug(
            "voice",
            format!(
                "voice Opus decoded non-20ms frame: ssrc={} user_id={:?} seq={} samples_per_channel={} pcm_samples={}",
                frame.ssrc(),
                frame.user_id(),
                frame.sequence(),
                samples_per_channel,
                decoded_len
            ),
        );
    }
    Some(decoded)
}

pub(super) fn mix_voice_decoded_samples(decoded_frames: &[Vec<f32>]) -> Option<Vec<f32>> {
    let max_len = decoded_frames.iter().map(Vec::len).max()?;
    if max_len == 0 {
        return None;
    }

    let mut mixed = vec![0.0f32; max_len];
    for decoded in decoded_frames {
        for (mixed_sample, decoded_sample) in mixed.iter_mut().zip(decoded) {
            *mixed_sample += *decoded_sample;
        }
    }

    let gain = voice_mix_gain(decoded_frames.len());
    for sample in &mut mixed {
        *sample *= gain;
    }
    Some(mixed)
}

pub(super) fn voice_mix_gain(frame_count: usize) -> f32 {
    if frame_count <= 1 {
        1.0
    } else {
        1.0 / (frame_count as f32).sqrt()
    }
}
