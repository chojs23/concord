#[cfg(feature = "voice-playback")]
use std::path::Path;
#[cfg(feature = "voice-playback")]
use std::sync::Arc;
#[cfg(any(test, feature = "voice-playback"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(test, feature = "voice-playback"))]
use std::time::Duration;

#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[cfg(any(test, feature = "voice-playback"))]
use crate::discord::VoiceSoundKind;
#[cfg(feature = "voice-playback")]
use crate::logging;
#[cfg(feature = "voice-playback")]
use crate::support::audio_output::{self, F32OutputSource};

#[cfg(any(test, feature = "voice-playback"))]
const GENERATED_VOICE_SOUND_SAMPLE_RATE: u32 = 48_000;
#[cfg(any(test, feature = "voice-playback"))]
const GENERATED_VOICE_SOUND_CHANNELS: u16 = 2;
#[cfg(any(test, feature = "voice-playback"))]
const GENERATED_VOICE_SOUND_DURATION: Duration = Duration::from_millis(180);
#[cfg(feature = "voice-playback")]
const NOTIFICATION_SOUND_STREAM_PADDING: Duration = Duration::from_millis(40);
#[cfg(feature = "voice-playback")]
const MAX_NOTIFICATION_SOUND_BYTES: usize = 4 * 1024 * 1024;

#[cfg(any(test, feature = "voice-playback"))]
#[derive(Debug)]
struct NotificationAudio {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
}

#[cfg(feature = "voice-playback")]
struct NotificationOutputStream {
    stream: cpal::Stream,
    failed: Arc<AtomicBool>,
}

#[cfg(feature = "voice-playback")]
pub(super) fn play_voice_sound(
    kind: VoiceSoundKind,
    custom_path: Option<&Path>,
) -> std::result::Result<(), String> {
    let audio = match custom_path {
        Some(path) => load_notification_wav(path)?,
        None => generated_voice_sound(kind),
    };
    play_notification_audio(audio)
}

#[cfg(feature = "voice-playback")]
fn play_notification_audio(audio: NotificationAudio) -> std::result::Result<(), String> {
    if audio.channels == 0 || audio.sample_rate == 0 || audio.samples.is_empty() {
        return Err("notification sound is empty".to_owned());
    }

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no default audio output device is available".to_owned())?;
    let supported_config = device
        .default_output_config()
        .map_err(|error| format!("notification audio output config failed: {error}"))?;
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();
    let output_sample_rate = stream_config.sample_rate;
    let total_output_frames = notification_output_frame_count(&audio, output_sample_rate);
    let play_duration = notification_play_duration(total_output_frames, output_sample_rate);
    let stream = build_notification_output_stream(
        &device,
        &stream_config,
        sample_format,
        audio,
        total_output_frames,
    )?;
    stream
        .stream
        .play()
        .map_err(|error| format!("notification audio output stream start failed: {error}"))?;
    std::thread::sleep(play_duration);
    notification_stream_result(&stream.failed)
}

#[cfg(feature = "voice-playback")]
fn build_notification_output_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    audio: NotificationAudio,
    total_output_frames: usize,
) -> std::result::Result<NotificationOutputStream, String> {
    let failed = Arc::new(AtomicBool::new(false));
    let callback_failed = Arc::clone(&failed);
    let stream = audio_output::build_f32_output_stream(
        device,
        config,
        sample_format,
        NotificationOutputSource {
            audio,
            total_output_frames,
            output_sample_rate: config.sample_rate.max(1),
            output_frame: 0,
        },
        move |error| {
            callback_failed.store(true, Ordering::Relaxed);
            log_notification_output_stream_error(error);
        },
        "notification audio output",
        "notification audio",
    )?;
    Ok(NotificationOutputStream { stream, failed })
}

#[cfg(feature = "voice-playback")]
struct NotificationOutputSource {
    audio: NotificationAudio,
    total_output_frames: usize,
    output_sample_rate: u32,
    output_frame: usize,
}

#[cfg(feature = "voice-playback")]
impl F32OutputSource for NotificationOutputSource {
    fn fill<T>(&mut self, output: &mut [T], output_channels: usize, convert: fn(f32) -> T)
    where
        T: Default + Copy,
    {
        fill_notification_output(
            output,
            output_channels,
            self.output_sample_rate,
            &self.audio,
            self.total_output_frames,
            &mut self.output_frame,
            convert,
        );
    }
}

#[cfg(feature = "voice-playback")]
fn fill_notification_output<T>(
    output: &mut [T],
    output_channels: usize,
    output_sample_rate: u32,
    audio: &NotificationAudio,
    total_output_frames: usize,
    output_frame: &mut usize,
    convert: fn(f32) -> T,
) where
    T: Default + Copy,
{
    for frame in output.chunks_mut(output_channels) {
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = if *output_frame < total_output_frames {
                convert(notification_output_sample(
                    audio,
                    *output_frame,
                    output_sample_rate,
                    channel,
                ))
            } else {
                convert(0.0)
            };
        }
        *output_frame = output_frame.saturating_add(1);
    }
}

#[cfg(feature = "voice-playback")]
fn log_notification_output_stream_error(error: cpal::StreamError) {
    logging::error(
        "voice",
        format!("notification audio output stream failed: {error}"),
    );
}

#[cfg(any(test, feature = "voice-playback"))]
fn notification_stream_result(stream_failed: &AtomicBool) -> std::result::Result<(), String> {
    if stream_failed.load(Ordering::Relaxed) {
        Err("notification audio output stream failed during playback".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(feature = "voice-playback")]
fn notification_output_sample(
    audio: &NotificationAudio,
    output_frame: usize,
    output_sample_rate: u32,
    output_channel: usize,
) -> f32 {
    let source_channels = usize::from(audio.channels.max(1));
    let source_frames = audio.samples.len() / source_channels;
    if source_frames == 0 {
        return 0.0;
    }
    let source_frame = output_frame.saturating_mul(audio.sample_rate as usize)
        / output_sample_rate.max(1) as usize;
    if source_frame >= source_frames {
        return 0.0;
    }
    let source_channel = match (source_channels, output_channel) {
        (1, _) => 0,
        (_, 0) => 0,
        (_, 1) => 1,
        _ => return 0.0,
    };
    audio.samples[source_frame * source_channels + source_channel].clamp(-1.0, 1.0)
}

#[cfg(feature = "voice-playback")]
fn notification_output_frame_count(audio: &NotificationAudio, output_sample_rate: u32) -> usize {
    let source_channels = usize::from(audio.channels.max(1));
    let source_frames = audio.samples.len() / source_channels;
    let output_sample_rate = output_sample_rate.max(1) as u128;
    let source_sample_rate = u128::from(audio.sample_rate.max(1));
    ((source_frames as u128 * output_sample_rate).div_ceil(source_sample_rate)) as usize
}

#[cfg(feature = "voice-playback")]
fn notification_play_duration(total_output_frames: usize, output_sample_rate: u32) -> Duration {
    let audio_duration =
        Duration::from_secs_f64(total_output_frames as f64 / f64::from(output_sample_rate.max(1)));
    audio_duration + NOTIFICATION_SOUND_STREAM_PADDING
}

#[cfg(any(test, feature = "voice-playback"))]
fn generated_voice_sound(kind: VoiceSoundKind) -> NotificationAudio {
    let frame_count = (GENERATED_VOICE_SOUND_SAMPLE_RATE as f32
        * GENERATED_VOICE_SOUND_DURATION.as_secs_f32()) as usize;
    let mut samples = Vec::with_capacity(frame_count * usize::from(GENERATED_VOICE_SOUND_CHANNELS));
    for frame in 0..frame_count {
        let progress = frame as f32 / frame_count.max(1) as f32;
        let frequency = generated_voice_sound_frequency(kind, progress);
        let phase = frame as f32 * frequency * std::f32::consts::TAU
            / GENERATED_VOICE_SOUND_SAMPLE_RATE as f32;
        let envelope = generated_voice_sound_envelope(progress);
        let sample = phase.sin() * 0.18 * envelope;
        samples.push(sample);
        samples.push(sample);
    }
    NotificationAudio {
        sample_rate: GENERATED_VOICE_SOUND_SAMPLE_RATE,
        channels: GENERATED_VOICE_SOUND_CHANNELS,
        samples,
    }
}

#[cfg(any(test, feature = "voice-playback"))]
fn generated_voice_sound_frequency(kind: VoiceSoundKind, progress: f32) -> f32 {
    match (kind, progress < 0.5) {
        (VoiceSoundKind::Join, true) => 660.0,
        (VoiceSoundKind::Join, false) => 880.0,
        (VoiceSoundKind::Leave, true) => 880.0,
        (VoiceSoundKind::Leave, false) => 660.0,
    }
}

#[cfg(any(test, feature = "voice-playback"))]
fn generated_voice_sound_envelope(progress: f32) -> f32 {
    const FADE: f32 = 0.08;
    progress.min(1.0 - progress).min(FADE) / FADE
}

#[cfg(feature = "voice-playback")]
fn load_notification_wav(path: &Path) -> std::result::Result<NotificationAudio, String> {
    let bytes = std::fs::read(path).map_err(|error| {
        format!(
            "notification sound read failed: {}: {error}",
            path.display()
        )
    })?;
    if bytes.len() > MAX_NOTIFICATION_SOUND_BYTES {
        return Err(format!(
            "notification sound is too large: {} bytes",
            bytes.len()
        ));
    }
    decode_notification_wav(&bytes)
}

#[cfg(any(test, feature = "voice-playback"))]
fn decode_notification_wav(bytes: &[u8]) -> std::result::Result<NotificationAudio, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("notification sound must be a RIFF/WAVE file".to_owned());
    }

    let mut format = None;
    let mut data = None;
    let mut cursor = 12usize;
    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        let chunk_start = cursor + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| "notification sound chunk size overflowed".to_owned())?;
        if chunk_end > bytes.len() {
            return Err("notification sound chunk exceeds file length".to_owned());
        }
        match chunk_id {
            b"fmt " => format = Some(parse_wav_format(&bytes[chunk_start..chunk_end])?),
            b"data" => data = Some(&bytes[chunk_start..chunk_end]),
            _ => {}
        }
        cursor = chunk_end + usize::from(chunk_size % 2 == 1);
    }

    let format = format.ok_or_else(|| "notification sound has no fmt chunk".to_owned())?;
    let data = data.ok_or_else(|| "notification sound has no data chunk".to_owned())?;
    decode_wav_samples(format, data)
}

#[derive(Clone, Copy)]
#[cfg(any(test, feature = "voice-playback"))]
struct WavFormat {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
}

#[cfg(any(test, feature = "voice-playback"))]
fn parse_wav_format(bytes: &[u8]) -> std::result::Result<WavFormat, String> {
    if bytes.len() < 16 {
        return Err("notification sound fmt chunk is too short".to_owned());
    }
    let format = WavFormat {
        audio_format: u16::from_le_bytes([bytes[0], bytes[1]]),
        channels: u16::from_le_bytes([bytes[2], bytes[3]]),
        sample_rate: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        bits_per_sample: u16::from_le_bytes([bytes[14], bytes[15]]),
    };
    if format.channels == 0 || format.sample_rate == 0 {
        return Err("notification sound has invalid channel count or sample rate".to_owned());
    }
    Ok(format)
}

#[cfg(any(test, feature = "voice-playback"))]
fn decode_wav_samples(
    format: WavFormat,
    data: &[u8],
) -> std::result::Result<NotificationAudio, String> {
    let bytes_per_sample = usize::from(format.bits_per_sample / 8);
    if bytes_per_sample == 0 || data.len() % bytes_per_sample != 0 {
        return Err("notification sound data is not sample-aligned".to_owned());
    }
    let samples = match (format.audio_format, format.bits_per_sample) {
        (1, 8) => data
            .iter()
            .map(|sample| (f32::from(*sample) - 128.0) / 128.0)
            .collect::<Vec<_>>(),
        (1, 16) => data
            .chunks_exact(2)
            .map(|sample| {
                f32::from(i16::from_le_bytes([sample[0], sample[1]])) / f32::from(i16::MAX)
            })
            .collect(),
        (1, 24) => data.chunks_exact(3).map(decode_i24_sample).collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|sample| {
                i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32
                    / i32::MAX as f32
            })
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|sample| f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]))
            .collect(),
        _ => {
            return Err(format!(
                "unsupported notification WAV format: format={} bits={}",
                format.audio_format, format.bits_per_sample
            ));
        }
    };
    let samples = samples
        .into_iter()
        .map(|sample: f32| sample.clamp(-1.0, 1.0))
        .collect();
    Ok(NotificationAudio {
        sample_rate: format.sample_rate,
        channels: format.channels,
        samples,
    })
}

#[cfg(any(test, feature = "voice-playback"))]
fn decode_i24_sample(sample: &[u8]) -> f32 {
    let sign = if sample[2] & 0x80 == 0 { 0x00 } else { 0xff };
    i32::from_le_bytes([sample[0], sample[1], sample[2], sign]) as f32 / 8_388_607.0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use crate::discord::VoiceSoundKind;

    use super::*;

    #[test]
    fn generated_voice_sound_has_stereo_samples_for_join_and_leave() {
        let join = generated_voice_sound(VoiceSoundKind::Join);
        let leave = generated_voice_sound(VoiceSoundKind::Leave);

        assert_eq!(join.sample_rate, GENERATED_VOICE_SOUND_SAMPLE_RATE);
        assert_eq!(join.channels, GENERATED_VOICE_SOUND_CHANNELS);
        assert_eq!(join.samples.len() % usize::from(join.channels), 0);
        assert_eq!(leave.samples.len(), join.samples.len());
        assert_ne!(
            generated_voice_sound_frequency(VoiceSoundKind::Join, 0.25),
            generated_voice_sound_frequency(VoiceSoundKind::Leave, 0.25)
        );
    }

    #[test]
    fn decode_notification_wav_reads_pcm16_samples() {
        let wav = pcm16_wav_bytes(48_000, 2, &[0, i16::MAX, i16::MIN, 0]);

        let audio = decode_notification_wav(&wav).expect("test wav should decode");

        assert_eq!(audio.sample_rate, 48_000);
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.samples, vec![0.0, 1.0, -1.0, 0.0]);
    }

    #[test]
    fn decode_notification_wav_rejects_non_wav_bytes() {
        let error = decode_notification_wav(b"not a wav").expect_err("invalid wav should fail");

        assert!(error.contains("RIFF/WAVE"));
    }

    #[test]
    fn notification_stream_result_reports_async_stream_error() {
        let stream_failed = AtomicBool::new(true);

        let error =
            notification_stream_result(&stream_failed).expect_err("stream error should fail");

        assert!(error.contains("failed during playback"));
    }

    fn pcm16_wav_bytes(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = samples.len() * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
