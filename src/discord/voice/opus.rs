use std::{collections::HashMap, time::Instant};

#[cfg(feature = "voice-playback")]
use std::sync::Arc;
#[cfg(feature = "voice-playback")]
use std::sync::atomic::{AtomicBool, AtomicU8};
#[cfg(feature = "voice-playback")]
use std::sync::mpsc::SyncSender;

use ::opus::{
    Application as OpusApplication, Channels, Decoder as OpusDecoder, Encoder as OpusEncoder,
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

#[cfg(feature = "voice-playback")]
use super::playback::VoiceAudioOutput;
use super::playback::{
    VoicePlaybackFrame, VoicePlaybackGate, VoicePlaybackPlayoutBuffers, VoicePlaybackPostProcess,
    VoicePlayoutFrame, clamp_voice_sample,
};
use super::{
    DISCORD_OPUS_20MS_STEREO_SAMPLES, DISCORD_OPUS_FRAME_SAMPLES_PER_CHANNEL,
    DISCORD_VOICE_CHANNELS, DISCORD_VOICE_SAMPLE_RATE, OPUS_MAX_ENCODED_FRAME_BYTES,
    OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL, VOICE_PLAYBACK_FRAME_DURATION, VOICE_PLAYBACK_FRAME_QUEUE,
};
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
}

impl VoiceOpusDecode {
    #[cfg(not(feature = "voice-playback"))]
    pub(super) fn start(
        playback_gate: VoicePlaybackGate,
        audio_handle: &tokio::runtime::Handle,
    ) -> Self {
        let _ = playback_gate;
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let task = audio_handle.spawn(run_voice_playback_decode(
            frames_rx,
            VoiceDecodedAudio::decode_only(),
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
        audio_handle: &tokio::runtime::Handle,
    ) -> Self {
        let (frames_tx, frames_rx) = mpsc::channel(VOICE_PLAYBACK_FRAME_QUEUE);
        let playback_enabled = Arc::new(AtomicBool::new(playback_gate.enabled));
        let playback_volume = Arc::new(AtomicU8::new(playback_gate.volume.value()));
        match VoiceAudioOutput::start(Arc::clone(&playback_enabled), Arc::clone(&playback_volume)) {
            Ok(audio_output) => {
                let decoded_audio = VoiceDecodedAudio::output(audio_output.samples_tx.clone());
                let task = audio_handle.spawn(run_voice_playback_decode(frames_rx, decoded_audio));
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
        }
    }

    #[cfg(feature = "voice-playback")]
    fn output(samples_tx: SyncSender<Vec<f32>>) -> Self {
        Self {
            samples_tx: Some(samples_tx),
        }
    }

    fn try_send(&self, samples: Vec<f32>) {
        #[cfg(feature = "voice-playback")]
        if let Some(samples_tx) = self.samples_tx.as_ref() {
            let _ = samples_tx.try_send(samples);
        }
        #[cfg(not(feature = "voice-playback"))]
        {
            let _ = samples;
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
) {
    let mut decoders = HashMap::new();
    let mut playout_buffers = VoicePlaybackPlayoutBuffers::default();
    let mut post_process = VoicePlaybackPostProcess::default();
    let mut playout_tick = interval(VOICE_PLAYBACK_FRAME_DURATION);
    playout_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut decoded_frames = 0u64;
    loop {
        tokio::select! {
            frame = frames_rx.recv() => {
                let Some(frame) = frame else {
                    break;
                };
                playout_buffers.push(frame, Instant::now());
            }
            _ = playout_tick.tick() => {
                let frames = playout_buffers.next_frames(Instant::now());
                if let Some(mut samples) = decode_voice_playout_frames(frames, &mut decoders) {
                    post_process.process(&mut samples);
                    let pcm_samples = samples.len();
                    decoded_audio.try_send(samples);
                    decoded_frames = decoded_frames.saturating_add(1);
                    if decoded_frames == 1 || decoded_frames % 500 == 0 {
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
        }
    }
}

pub(super) fn decode_voice_playout_frames(
    frames: Vec<VoicePlayoutFrame>,
    decoders: &mut HashMap<u32, OpusDecoder>,
) -> Option<Vec<f32>> {
    let mut decoded_frames = Vec::new();
    for frame in frames {
        if let Some(samples) = decode_voice_playout_frame(frame, decoders) {
            decoded_frames.push(samples);
        }
    }
    mix_voice_decoded_samples(&decoded_frames)
}

pub(super) fn decode_voice_playout_frame(
    frame: VoicePlayoutFrame,
    decoders: &mut HashMap<u32, OpusDecoder>,
) -> Option<Vec<f32>> {
    let ssrc = frame.ssrc();
    if frame.is_packet_loss() && !decoders.contains_key(&ssrc) {
        return Some(vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES]);
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
        DISCORD_OPUS_20MS_STEREO_SAMPLES
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
            return Some(vec![0.0f32; DISCORD_OPUS_20MS_STEREO_SAMPLES]);
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
        *sample = clamp_voice_sample(*sample * gain);
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
