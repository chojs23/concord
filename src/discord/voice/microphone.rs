use super::dave::VoiceDaveOutboundPayload;
use super::rtp::build_voice_rtp_packet;
use super::*;

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneCapture {
    pub(super) fn start(samples_tx: Option<mpsc::Sender<Vec<i16>>>) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(samples_tx);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    pub(super) fn start_with_cpal(
        samples_tx: Option<mpsc::Sender<Vec<i16>>>,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "no default microphone input device is available".to_owned())?;
        let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
        let (stream, stream_config, sample_format) =
            build_preferred_voice_input_stream(&device, Arc::clone(&stats), samples_tx.clone())
                .or_else(|preferred_error| {
                    logging::debug(
                        "voice",
                        format!(
                            "voice preferred microphone input stream failed: {preferred_error}"
                        ),
                    );
                    build_default_voice_input_stream(&device, Arc::clone(&stats), samples_tx)
                })?;
        stream
            .play()
            .map_err(|error| format!("voice microphone input stream start failed: {error}"))?;
        logging::debug(
            "voice",
            format!(
                "voice microphone capture started: sample_rate={} channels={} format={:?} buffer_size={:?}",
                stream_config.sample_rate,
                stream_config.channels,
                sample_format,
                stream_config.buffer_size,
            ),
        );
        Ok(Self {
            _stream: stream,
            stats,
        })
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_preferred_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<(cpal::Stream, cpal::StreamConfig, cpal::SampleFormat), String> {
    let supported_config = select_voice_input_config(device)?;
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    stream_config.buffer_size = voice_input_buffer_size(supported_config.buffer_size());

    match build_voice_input_stream(
        device,
        &stream_config,
        sample_format,
        Arc::clone(&stats),
        samples_tx.clone(),
    ) {
        Ok(stream) => Ok((stream, stream_config, sample_format)),
        Err(error) if stream_config.buffer_size != cpal::BufferSize::Default => {
            logging::debug(
                "voice",
                format!(
                    "voice fixed microphone input buffer failed, retrying default buffer: {error}"
                ),
            );
            stream_config.buffer_size = cpal::BufferSize::Default;
            build_voice_input_stream(device, &stream_config, sample_format, stats, samples_tx)
                .map(|stream| (stream, stream_config, sample_format))
        }
        Err(error) => Err(error),
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_default_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<(cpal::Stream, cpal::StreamConfig, cpal::SampleFormat), String> {
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("voice microphone default input config failed: {error}"))?;
    let sample_format = supported_config.sample_format();
    let stream_config = supported_config.config();
    build_voice_input_stream(device, &stream_config, sample_format, stats, samples_tx)
        .map(|stream| (stream, stream_config, sample_format))
}

#[cfg(feature = "voice-playback")]
pub(super) fn select_voice_input_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, String> {
    device
        .supported_input_configs()
        .map_err(|error| format!("voice microphone input config query failed: {error}"))?
        .filter(|config| {
            config.min_sample_rate() <= DISCORD_VOICE_SAMPLE_RATE
                && config.max_sample_rate() >= DISCORD_VOICE_SAMPLE_RATE
                && (config.channels() == 1 || config.channels() == DISCORD_VOICE_CHANNELS)
        })
        .min_by_key(voice_input_config_rank)
        .map(|config| config.with_sample_rate(DISCORD_VOICE_SAMPLE_RATE))
        .ok_or_else(|| "no Discord-friendly microphone input config found".to_owned())
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_config_rank(config: &cpal::SupportedStreamConfigRange) -> (u8, u8) {
    (
        voice_input_channel_rank(config.channels()),
        voice_input_sample_format_rank(config.sample_format()),
    )
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_channel_rank(channels: u16) -> u8 {
    match channels {
        1 => 0,
        DISCORD_VOICE_CHANNELS => 1,
        _ => 2,
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_sample_format_rank(format: cpal::SampleFormat) -> u8 {
    match format {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::I16 => 1,
        cpal::SampleFormat::U16 => 2,
        cpal::SampleFormat::U8 => 3,
        _ if format.is_uint() => 4,
        _ => 5,
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_buffer_size(supported: &cpal::SupportedBufferSize) -> cpal::BufferSize {
    match supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(VOICE_MIC_PREFERRED_BUFFER_FRAMES.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    }
}

#[cfg(feature = "voice-playback")]
impl Default for VoiceMicrophoneCaptureStats {
    fn default() -> Self {
        Self {
            chunks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            min_callback_frames: AtomicU64::new(u64::MAX),
            max_callback_frames: AtomicU64::new(0),
            queued_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            peak_sample: AtomicU64::new(0),
            clipped_samples: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophonePcmFrames {
    pub(super) fn new(
        frames_tx: mpsc::Sender<Vec<i16>>,
        stats: Arc<VoiceMicrophoneCaptureStats>,
        source_sample_rate: u32,
    ) -> Self {
        Self {
            frames_tx,
            stats,
            source_sample_rate,
            source_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            next_source_frame: 0.0,
        }
    }

    pub(super) fn push_stereo_samples(&mut self, samples: &[i16]) {
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
            self.flush_output_frames();
            return;
        }

        self.source_pending.extend_from_slice(samples);
        self.resample_pending_source();
        self.flush_output_frames();
    }

    pub(super) fn resample_pending_source(&mut self) {
        let source_frames = self.source_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        if source_frames < 2 {
            return;
        }

        let source_step = f64::from(self.source_sample_rate) / f64::from(DISCORD_VOICE_SAMPLE_RATE);
        while self.next_source_frame + 1.0 < source_frames as f64 {
            let frame_index = self.next_source_frame.floor() as usize;
            let fraction = self.next_source_frame - frame_index as f64;
            let left = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE],
                fraction,
            );
            let right = interpolate_i16(
                self.source_pending[frame_index * DISCORD_VOICE_CHANNELS_USIZE + 1],
                self.source_pending[(frame_index + 1) * DISCORD_VOICE_CHANNELS_USIZE + 1],
                fraction,
            );
            self.output_pending.push(left);
            self.output_pending.push(right);
            self.next_source_frame += source_step;
        }

        let consumed_frames = self.next_source_frame.floor() as usize;
        if consumed_frames > 0 {
            self.source_pending
                .drain(..consumed_frames * DISCORD_VOICE_CHANNELS_USIZE);
            self.next_source_frame -= consumed_frames as f64;
        }
    }

    pub(super) fn flush_output_frames(&mut self) {
        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let frame = self
                .output_pending
                .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                .collect::<Vec<_>>();
            if self.frames_tx.try_send(frame).is_ok() {
                self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);
            } else {
                self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn interpolate_i16(current: i16, next: i16, fraction: f64) -> i16 {
    let value = f64::from(current) + (f64::from(next) - f64::from(current)) * fraction;
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(feature = "voice-playback")]
impl Drop for VoiceMicrophoneCapture {
    fn drop(&mut self) {
        logging::debug(
            "voice",
            format!(
                "voice microphone capture stopped: chunks={} frames={} callback_frames_min={} callback_frames_max={} queued_20ms_frames={} dropped_20ms_frames={} peak_sample={} clipped_samples={}",
                self.stats.chunks.load(Ordering::Relaxed),
                self.stats.frames.load(Ordering::Relaxed),
                voice_microphone_min_callback_frames(&self.stats),
                self.stats.max_callback_frames.load(Ordering::Relaxed),
                self.stats.queued_frames.load(Ordering::Relaxed),
                self.stats.dropped_frames.load(Ordering::Relaxed),
                self.stats.peak_sample.load(Ordering::Relaxed),
                self.stats.clipped_samples.load(Ordering::Relaxed),
            ),
        );
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_voice_input_stream_f32(device, config, stats, samples_tx),
        cpal::SampleFormat::U8 => build_voice_input_stream_u8(device, config, stats, samples_tx),
        cpal::SampleFormat::I16 => build_voice_input_stream_i16(device, config, stats, samples_tx),
        cpal::SampleFormat::U16 => build_voice_input_stream_u16(device, config, stats, samples_tx),
        other => Err(format!(
            "unsupported voice microphone input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[f32], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_f32_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[i16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_i16_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[u16], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u16_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_voice_input_stream_u8(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: Option<mpsc::Sender<Vec<i16>>>,
) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels);
    let pcm_frames = samples_tx.map(|tx| {
        Arc::new(StdMutex::new(VoiceMicrophonePcmFrames::new(
            tx,
            Arc::clone(&stats),
            config.sample_rate,
        )))
    });
    device
        .build_input_stream(
            config,
            move |input: &[u8], _| {
                record_voice_input_chunk(input.len(), channels, &stats);
                if let Some(pcm_frames) = pcm_frames.as_ref()
                    && let Ok(mut pcm_frames) = pcm_frames.lock()
                {
                    let samples = voice_input_u8_to_stereo_i16(input, channels);
                    record_voice_input_pcm_stats(&samples, &stats);
                    pcm_frames.push_stereo_samples(&samples);
                }
            },
            log_voice_input_stream_error,
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_f32_to_stereo_i16(input: &[f32], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
    })
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_i16_to_stereo_i16(input: &[i16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| sample)
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_u16_to_stereo_i16(input: &[u16], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| {
        let shifted = i32::from(sample) - 32768;
        shifted.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    })
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_u8_to_stereo_i16(input: &[u8], channels: usize) -> Vec<i16> {
    voice_input_to_stereo_i16(input, channels, |sample| (i16::from(sample) - 128) << 8)
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_input_to_stereo_i16<T>(
    input: &[T],
    channels: usize,
    mut convert: impl FnMut(T) -> i16,
) -> Vec<i16>
where
    T: Copy,
{
    if channels == 0 {
        return Vec::new();
    }
    let frames = input.len() / channels;
    let mut stereo = Vec::with_capacity(frames * usize::from(DISCORD_VOICE_CHANNELS));
    for frame in input.chunks_exact(channels) {
        let left = convert(frame[0]);
        let right = if channels == 1 {
            left
        } else {
            convert(frame[1])
        };
        stereo.push(left);
        stereo.push(right);
    }
    stereo
}

#[cfg(feature = "voice-playback")]
pub(super) fn record_voice_input_chunk(
    sample_count: usize,
    channels: usize,
    stats: &VoiceMicrophoneCaptureStats,
) {
    let frames = sample_count / channels.max(1);
    stats.chunks.fetch_add(1, Ordering::Relaxed);
    stats
        .frames
        .fetch_add(u64::try_from(frames).unwrap_or(u64::MAX), Ordering::Relaxed);
    let frames = u64::try_from(frames).unwrap_or(u64::MAX);
    stats
        .min_callback_frames
        .fetch_min(frames, Ordering::Relaxed);
    stats
        .max_callback_frames
        .fetch_max(frames, Ordering::Relaxed);
}

#[cfg(feature = "voice-playback")]
pub(super) fn record_voice_input_pcm_stats(samples: &[i16], stats: &VoiceMicrophoneCaptureStats) {
    let peak = samples
        .iter()
        .map(|sample| i32::from(*sample).unsigned_abs() as u64)
        .max()
        .unwrap_or(0);
    let clipped = samples
        .iter()
        .filter(|sample| i32::from(**sample).abs() >= i32::from(i16::MAX) - 1)
        .count();

    stats.peak_sample.fetch_max(peak, Ordering::Relaxed);
    stats.clipped_samples.fetch_add(
        u64::try_from(clipped).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_microphone_min_callback_frames(stats: &VoiceMicrophoneCaptureStats) -> u64 {
    let min = stats.min_callback_frames.load(Ordering::Relaxed);
    if min == u64::MAX { 0 } else { min }
}

#[cfg(feature = "voice-playback")]
pub(super) fn log_voice_input_stream_error(error: cpal::StreamError) {
    logging::error(
        "voice",
        format!("voice microphone input stream failed: {error}"),
    );
}

#[cfg(all(feature = "voice-playback", target_os = "linux"))]
pub(super) fn log_captured_alsa_errors(
    alsa_error_output: &Option<std::rc::Rc<std::cell::RefCell<alsa::Output>>>,
) {
    let Some(output) = alsa_error_output else {
        return;
    };
    let message = output
        .borrow()
        .buffer_string(|bytes| String::from_utf8_lossy(bytes).replace('\0', ""));
    let message = message.trim();
    if message.is_empty() {
        return;
    }
    logging::error("voice", format!("captured ALSA diagnostics: {message}"));
}

impl VoiceFakeOutboundSendState {
    pub(super) fn new(
        mode: &str,
        secret_key: &[u8],
        rtp: VoiceOutboundRtpState,
        nonce_suffix: u32,
    ) -> Result<Self, String> {
        Ok(Self {
            rtp,
            encryptor: VoiceRtpEncryptor::new(mode, secret_key)?,
            nonce_suffix,
            allow_microphone_transmit: false,
            self_mute: true,
            dave_active: false,
            speaking: false,
            logged_block_reason: None,
            events: Vec::new(),
        })
    }

    pub(super) fn set_capture_gate(&mut self, allow_microphone_transmit: bool, self_mute: bool) {
        self.allow_microphone_transmit = allow_microphone_transmit;
        self.self_mute = self_mute;
    }

    #[allow(dead_code)]
    pub(super) fn set_dave_active(&mut self, active: bool) {
        self.dave_active = active;
    }

    #[allow(dead_code)]
    pub(super) fn events(&self) -> &[VoiceFakeOutboundEvent] {
        &self.events
    }

    #[allow(dead_code)]
    pub(super) fn take_events(&mut self) -> Vec<VoiceFakeOutboundEvent> {
        std::mem::take(&mut self.events)
    }

    #[allow(dead_code)]
    pub(super) fn record_blocked_transmit(&mut self, reason: VoiceFakeSendBlockReason) -> bool {
        if self.logged_block_reason == Some(reason) {
            return false;
        }
        self.logged_block_reason = Some(reason);
        true
    }

    #[allow(dead_code)]
    pub(super) fn take_logged_block_reason(&mut self) -> Option<VoiceFakeSendBlockReason> {
        self.logged_block_reason.take()
    }

    #[allow(dead_code)]
    pub(super) fn send_opus_frame(
        &mut self,
        opus_payload: &[u8],
    ) -> Result<VoiceFakeSendOutcome, String> {
        self.send_opus_frame_with_dave_payload(VoiceDaveOutboundPayload::Plain(
            opus_payload.to_vec(),
        ))
    }

    pub(super) fn send_opus_frame_with_dave(
        &mut self,
        opus_payload: &[u8],
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceFakeSendOutcome, String> {
        let dave_payload = dave.prepare_outbound_opus(opus_payload);
        self.send_opus_frame_with_dave_payload(dave_payload)
    }

    pub(super) fn send_opus_frame_with_dave_payload(
        &mut self,
        dave_payload: VoiceDaveOutboundPayload,
    ) -> Result<VoiceFakeSendOutcome, String> {
        if !self.capture_gate_enabled() {
            return Ok(VoiceFakeSendOutcome::Noop);
        }
        if self.dave_active {
            return Ok(VoiceFakeSendOutcome::Blocked(
                VoiceFakeSendBlockReason::DaveOutboundUnsupported,
            ));
        }
        let opus_payload = match dave_payload {
            VoiceDaveOutboundPayload::Plain(opus) | VoiceDaveOutboundPayload::Encrypted(opus) => {
                opus
            }
            VoiceDaveOutboundPayload::Blocked(reason) => {
                return Ok(VoiceFakeSendOutcome::Blocked(reason));
            }
        };

        let encrypted = self.encrypt_current_packet(&opus_payload)?;
        if !self.speaking {
            self.events.push(VoiceFakeOutboundEvent::Speaking {
                speaking: true,
                ssrc: self.rtp.ssrc,
            });
            self.speaking = true;
        }
        self.events
            .push(VoiceFakeOutboundEvent::Packet { bytes: encrypted });
        self.advance_packet_state();
        Ok(VoiceFakeSendOutcome::Sent)
    }

    #[allow(dead_code)]
    pub(super) fn stop_speaking(&mut self) -> Result<VoiceFakeSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            VoiceDaveOutboundPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec())
        })
    }

    pub(super) fn stop_speaking_with_dave(
        &mut self,
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceFakeSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            dave.prepare_outbound_opus(&DISCORD_OPUS_SILENCE_FRAME)
        })
    }

    pub(super) fn stop_speaking_with_dave_payload(
        &mut self,
        mut next_silence: impl FnMut() -> VoiceDaveOutboundPayload,
    ) -> Result<VoiceFakeSendOutcome, String> {
        if !self.speaking {
            return Ok(VoiceFakeSendOutcome::Noop);
        }
        if !self.capture_gate_enabled() {
            return Ok(self.queue_speaking_off());
        }
        if self.dave_active {
            return Ok(self.queue_speaking_off());
        }
        if self
            .ensure_nonce_capacity(DISCORD_TRAILING_SILENCE_FRAMES)
            .is_err()
        {
            return Ok(self.queue_speaking_off());
        }

        for _ in 0..DISCORD_TRAILING_SILENCE_FRAMES {
            let opus_payload = match next_silence() {
                VoiceDaveOutboundPayload::Plain(opus)
                | VoiceDaveOutboundPayload::Encrypted(opus) => opus,
                VoiceDaveOutboundPayload::Blocked(_) => {
                    return Ok(self.queue_speaking_off());
                }
            };
            let encrypted = self.encrypt_current_packet(&opus_payload)?;
            self.events
                .push(VoiceFakeOutboundEvent::Packet { bytes: encrypted });
            self.advance_packet_state();
        }
        Ok(self.queue_speaking_off())
    }

    pub(super) fn queue_speaking_off(&mut self) -> VoiceFakeSendOutcome {
        self.events.push(VoiceFakeOutboundEvent::Speaking {
            speaking: false,
            ssrc: self.rtp.ssrc,
        });
        self.speaking = false;
        VoiceFakeSendOutcome::Sent
    }

    pub(super) fn capture_gate_enabled(&self) -> bool {
        self.allow_microphone_transmit && !self.self_mute
    }

    pub(super) fn encrypt_current_packet(&self, opus_payload: &[u8]) -> Result<Vec<u8>, String> {
        let nonce_suffix = self.current_nonce_suffix()?;
        let packet = build_voice_rtp_packet(
            self.rtp.sequence,
            self.rtp.timestamp,
            self.rtp.ssrc,
            opus_payload,
        )?;
        self.encryptor.encrypt_packet(&packet, nonce_suffix)
    }

    pub(super) fn current_nonce_suffix(&self) -> Result<[u8; RTP_AEAD_NONCE_SUFFIX_BYTES], String> {
        if self.nonce_suffix == u32::MAX {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(self.nonce_suffix.to_be_bytes())
    }

    pub(super) fn ensure_nonce_capacity(&self, packets: usize) -> Result<(), String> {
        let remaining = u32::MAX - self.nonce_suffix;
        if remaining < packets as u32 {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(())
    }

    pub(super) fn advance_packet_state(&mut self) {
        self.rtp.sequence = self.rtp.sequence.wrapping_add(1);
        self.rtp.timestamp = self
            .rtp
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
        self.nonce_suffix = self.nonce_suffix.saturating_add(1);
    }
}

#[cfg(feature = "voice-playback")]
pub(super) async fn run_voice_udp_transmit(
    mut pcm_rx: mpsc::Receiver<Vec<i16>>,
    mut gate_rx: watch::Receiver<VoiceCaptureGate>,
    context: VoiceUdpTransmitContext,
) {
    let rtp = VoiceOutboundRtpState {
        sequence: 0,
        timestamp: 0,
        ssrc: context.ssrc,
    };
    let mut sender = match VoiceFakeOutboundSendState::new(
        &context.description.mode,
        &context.description.secret_key,
        rtp,
        0,
    ) {
        Ok(sender) => sender,
        Err(error) => {
            logging::error("voice", format!("voice UDP transmit init failed: {error}"));
            return;
        }
    };
    let initial_gate = *gate_rx.borrow();
    sender.set_capture_gate(initial_gate.enabled, false);
    let mut encoder = match VoiceOpusEncode::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            logging::error("voice", error);
            return;
        }
    };
    let transmit_started_at = Instant::now();
    let mut transmit_stats = VoiceUdpTransmitStats::default();
    let mut microphone_gate = VoiceMicrophoneGateState::default();
    let mut next_stats_log_at = transmit_started_at + VOICE_TRANSMIT_STATS_LOG_INTERVAL;

    loop {
        tokio::select! {
            changed = gate_rx.changed() => {
                if changed.is_err() {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    if let Err(error) = flush_voice_outbound_events(
                        &context.udp_socket,
                        &context.writer,
                        sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await),
                        &mut sender,
                        &context.local_speaking_tx,
                        &mut transmit_stats,
                    ).await {
                        logging::error("voice", error);
                    }
                    let _ = context.local_speaking_tx.send(false);
                    sender.set_capture_gate(false, false);
                    break;
                }
                let gate = *gate_rx.borrow();
                let was_enabled = sender.capture_gate_enabled();
                if !(gate.enabled && was_enabled) {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    microphone_gate.reset();
                }
                if !gate.enabled
                    && let Err(error) = flush_voice_outbound_events(
                        &context.udp_socket,
                        &context.writer,
                        sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await),
                        &mut sender,
                        &context.local_speaking_tx,
                        &mut transmit_stats,
                    ).await
                {
                    logging::error("voice", error);
                }
                if !gate.enabled {
                    let _ = context.local_speaking_tx.send(false);
                    microphone_gate.reset();
                }
                sender.set_capture_gate(gate.enabled, false);
            }
            received = pcm_rx.recv() => {
                let Some(mut frame) = received else {
                    if let Err(error) = flush_voice_outbound_events(
                        &context.udp_socket,
                        &context.writer,
                        sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await),
                        &mut sender,
                        &context.local_speaking_tx,
                        &mut transmit_stats,
                    ).await {
                        logging::error("voice", error);
                    }
                    let _ = context.local_speaking_tx.send(false);
                    sender.set_capture_gate(false, false);
                    microphone_gate.reset();
                    break;
                };
                record_voice_transmit_frame(&mut transmit_stats, Instant::now());
                let gate = *gate_rx.borrow();
                if !gate.enabled {
                    microphone_gate.reset();
                    continue;
                }
                if !microphone_gate.allows_frame(&frame, gate.microphone_sensitivity) {
                    if let Err(error) = flush_voice_outbound_events(
                        &context.udp_socket,
                        &context.writer,
                        sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await),
                        &mut sender,
                        &context.local_speaking_tx,
                        &mut transmit_stats,
                    ).await {
                        logging::error("voice", error);
                    }
                    continue;
                }
                let raw_overload_decision = voice_microphone_overload_decision(&frame);
                let overload_decision = if voice_microphone_clipped_frame_needs_blank(
                    &frame,
                    raw_overload_decision,
                ) {
                    Some(VoiceMicrophoneOverloadDecision {
                        kind: VoiceMicrophoneOverloadKind::HandlingNoise,
                        gain: VOICE_MIC_HANDLING_NOISE_GAIN,
                    })
                } else {
                    microphone_gate.overload_decision(&frame)
                };
                if let Some(decision) = overload_decision {
                    transmit_stats.overload_smoothed_frames += 1;
                    apply_voice_microphone_gain(&mut frame, decision.gain);
                }
                apply_voice_volume_to_i16_frame(&mut frame, gate.microphone_volume);
                apply_voice_microphone_gain(&mut frame, VOICE_MIC_TRANSMIT_BOOST_GAIN);
                transmit_stats.limited_samples += protect_voice_microphone_frame(&mut frame);
                let _ = context.local_speaking_tx.send(true);
                let opus = match encoder.encode_20ms_i16(&frame) {
                    Ok(opus) => opus,
                    Err(error) => {
                        logging::debug("voice", error);
                        continue;
                    }
                };
                let outcome = sender.send_opus_frame_with_dave(&opus, &mut *context.dave_state.lock().await);
                if let Err(error) = flush_voice_outbound_events(
                    &context.udp_socket,
                    &context.writer,
                    outcome,
                    &mut sender,
                    &context.local_speaking_tx,
                    &mut transmit_stats,
                ).await {
                    logging::error("voice", error);
                    break;
                }
                let now = Instant::now();
                if now >= next_stats_log_at {
                    log_voice_transmit_stats(
                        "voice UDP transmit stats",
                        &transmit_stats,
                        transmit_started_at,
                        sender.rtp.timestamp,
                    );
                    next_stats_log_at = now + VOICE_TRANSMIT_STATS_LOG_INTERVAL;
                }
            }
        }
    }
    log_voice_transmit_stats(
        "voice UDP transmit stopped",
        &transmit_stats,
        transmit_started_at,
        sender.rtp.timestamp,
    );
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneGateState {
    pub(super) fn overload_decision(
        &mut self,
        frame: &[i16],
    ) -> Option<VoiceMicrophoneOverloadDecision> {
        if let Some(decision) = voice_microphone_overload_decision(frame) {
            if decision.kind == VoiceMicrophoneOverloadKind::HandlingNoise {
                self.handling_noise_suppression_frames =
                    VOICE_MIC_HANDLING_NOISE_SUPPRESSION_FRAMES;
                self.overload_recovery_frames = 0;
                return Some(decision);
            }
            if self.handling_noise_suppression_frames > 0 {
                self.handling_noise_suppression_frames -= 1;
                return Some(VoiceMicrophoneOverloadDecision {
                    kind: VoiceMicrophoneOverloadKind::Recovery,
                    gain: VOICE_MIC_HANDLING_NOISE_GAIN,
                });
            }
            self.overload_recovery_frames = if decision.gain <= VOICE_MIC_OVERLOAD_TRANSIENT_GAIN {
                VOICE_MIC_OVERLOAD_RECOVERY_FRAMES
            } else {
                0
            };
            return Some(decision);
        }
        if self.handling_noise_suppression_frames > 0 {
            self.handling_noise_suppression_frames -= 1;
            return Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::Recovery,
                gain: VOICE_MIC_HANDLING_NOISE_GAIN,
            });
        }
        if self.overload_recovery_frames > 0 {
            let recovery_gain =
                voice_microphone_overload_recovery_gain(self.overload_recovery_frames);
            self.overload_recovery_frames -= 1;
            return Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::Recovery,
                gain: recovery_gain,
            });
        }
        None
    }

    pub(super) fn allows_frame(
        &mut self,
        frame: &[i16],
        sensitivity: MicrophoneSensitivityDb,
    ) -> bool {
        if voice_pcm_frame_reaches_sensitivity(frame, sensitivity) {
            self.hangover_frames = VOICE_MIC_GATE_HANGOVER_FRAMES;
            return true;
        }
        if self.hangover_frames > 0 {
            self.hangover_frames -= 1;
            return true;
        }
        false
    }

    pub(super) fn reset(&mut self) {
        self.hangover_frames = 0;
        self.overload_recovery_frames = 0;
        self.handling_noise_suppression_frames = 0;
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn drain_voice_microphone_pcm_queue(pcm_rx: &mut mpsc::Receiver<Vec<i16>>) {
    while pcm_rx.try_recv().is_ok() {}
}

#[cfg(feature = "voice-playback")]
pub(super) async fn flush_voice_outbound_events(
    udp_socket: &UdpSocket,
    writer: &VoiceWriter,
    outcome: Result<VoiceFakeSendOutcome, String>,
    sender: &mut VoiceFakeOutboundSendState,
    local_speaking_tx: &mpsc::UnboundedSender<bool>,
    transmit_stats: &mut VoiceUdpTransmitStats,
) -> Result<(), String> {
    match outcome? {
        VoiceFakeSendOutcome::Sent => {
            for event in sender.take_events() {
                match event {
                    VoiceFakeOutboundEvent::Speaking { speaking, ssrc } => {
                        send_voice_text(writer, voice_speaking_payload(ssrc, speaking)).await?;
                        let _ = local_speaking_tx.send(speaking);
                    }
                    VoiceFakeOutboundEvent::Packet { bytes } => {
                        udp_socket
                            .send(&bytes)
                            .await
                            .map_err(|error| format!("voice UDP transmit failed: {error}"))?;
                        transmit_stats.sent_packets += 1;
                    }
                }
            }
            if let Some(reason) = sender.take_logged_block_reason() {
                logging::debug(
                    "voice",
                    format!("voice UDP transmit resumed after block: {reason:?}"),
                );
            }
        }
        VoiceFakeSendOutcome::Noop => {
            let _ = sender.take_logged_block_reason();
        }
        VoiceFakeSendOutcome::Blocked(reason) => {
            if sender.record_blocked_transmit(reason) {
                logging::debug("voice", format!("voice UDP transmit blocked: {reason:?}"));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "voice-playback")]
pub(super) fn record_voice_transmit_frame(stats: &mut VoiceUdpTransmitStats, now: Instant) {
    if let Some(last_frame_at) = stats.last_frame_at {
        stats.max_frame_gap_ms = stats
            .max_frame_gap_ms
            .max(now.duration_since(last_frame_at).as_millis());
    }
    stats.last_frame_at = Some(now);
}

#[cfg(feature = "voice-playback")]
pub(super) fn log_voice_transmit_stats(
    label: &str,
    stats: &VoiceUdpTransmitStats,
    started_at: Instant,
    rtp_timestamp: u32,
) {
    let elapsed_ms = started_at.elapsed().as_millis();
    let rtp_elapsed_ms =
        (u128::from(rtp_timestamp) * 1_000) / u128::from(DISCORD_VOICE_SAMPLE_RATE);
    logging::debug(
        "voice",
        format!(
            "{label}: elapsed_ms={} sent_packets={} rtp_timestamp={} rtp_elapsed_ms={} overload_smoothed_frames={} limited_samples={} max_frame_gap_ms={}",
            elapsed_ms,
            stats.sent_packets,
            rtp_timestamp,
            rtp_elapsed_ms,
            stats.overload_smoothed_frames,
            stats.limited_samples,
            stats.max_frame_gap_ms,
        ),
    );
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_pcm_frame_reaches_sensitivity(
    frame: &[i16],
    sensitivity: MicrophoneSensitivityDb,
) -> bool {
    let threshold = sensitivity.peak_threshold();
    threshold == 0 || voice_pcm_peak(frame) >= threshold
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn apply_voice_volume_to_i16_frame(frame: &mut [i16], volume: VoiceVolumePercent) {
    let gain = volume.gain();
    if (gain - 1.0).abs() <= f32::EPSILON {
        return;
    }
    for sample in frame {
        *sample = (f32::from(*sample) * gain)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn apply_voice_microphone_gain(frame: &mut [i16], gain: f32) {
    if (gain - 1.0).abs() <= f32::EPSILON {
        return;
    }
    for sample in frame {
        *sample = (f32::from(*sample) * gain)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
    }
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn protect_voice_microphone_frame(frame: &mut [i16]) -> u64 {
    let mut limited = 0u64;
    for sample in frame {
        let original = *sample;
        *sample = soft_limit_voice_microphone_sample(original);
        if *sample != original {
            limited += 1;
        }
    }
    limited
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
pub(super) fn voice_microphone_frame_is_overloaded(frame: &[i16]) -> bool {
    voice_microphone_clipped_sample_count(frame) >= VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
}

#[cfg(any(test, feature = "voice-playback"))]
#[allow(dead_code)]
pub(super) fn voice_microphone_overload_gain(frame: &[i16]) -> Option<f32> {
    voice_microphone_overload_decision(frame).map(|decision| decision.gain)
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_microphone_clipped_frame_needs_blank(
    frame: &[i16],
    raw_decision: Option<VoiceMicrophoneOverloadDecision>,
) -> bool {
    voice_microphone_clipped_sample_count(frame) > 0
        && !matches!(
            raw_decision.map(|decision| decision.kind),
            Some(VoiceMicrophoneOverloadKind::HandlingNoise)
        )
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_microphone_overload_decision(
    frame: &[i16],
) -> Option<VoiceMicrophoneOverloadDecision> {
    let max_adjacent_delta = voice_microphone_max_adjacent_delta(frame);
    let clipped_samples = voice_microphone_clipped_sample_count(frame);
    if max_adjacent_delta >= VOICE_MIC_HANDLING_NOISE_DELTA {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples >= VOICE_MIC_OVERLOAD_EXTREME_CLIPPED_SAMPLES {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples > 0
        && clipped_samples < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES
        && max_adjacent_delta >= VOICE_MIC_OVERLOAD_CLIPPED_STEP_DELTA
    {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples > 0 && max_adjacent_delta >= VOICE_MIC_OVERLOAD_IMPULSE_DELTA {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::HandlingNoise,
            gain: VOICE_MIC_HANDLING_NOISE_GAIN,
        });
    }

    if clipped_samples < VOICE_MIC_OVERLOAD_MIN_CLIPPED_SAMPLES {
        return None;
    }

    if clipped_samples >= VOICE_MIC_OVERLOAD_SEVERE_CLIPPED_SAMPLES {
        return Some(VoiceMicrophoneOverloadDecision {
            kind: VoiceMicrophoneOverloadKind::Transient,
            gain: VOICE_MIC_OVERLOAD_TRANSIENT_GAIN,
        });
    }

    Some(VoiceMicrophoneOverloadDecision {
        kind: VoiceMicrophoneOverloadKind::Attenuated,
        gain: VOICE_MIC_OVERLOAD_ATTENUATION_GAIN,
    })
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_microphone_overload_recovery_gain(frames_remaining: u8) -> f32 {
    let recovery_frames = f32::from(VOICE_MIC_OVERLOAD_RECOVERY_FRAMES.max(1));
    let elapsed_frames = f32::from(VOICE_MIC_OVERLOAD_RECOVERY_FRAMES - frames_remaining);
    VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN
        + (1.0 - VOICE_MIC_OVERLOAD_RECOVERY_START_GAIN) * (elapsed_frames / recovery_frames)
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_microphone_clipped_sample_count(frame: &[i16]) -> usize {
    frame
        .iter()
        .filter(|sample| i32::from(**sample).abs() >= i32::from(i16::MAX) - 1)
        .count()
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_microphone_max_adjacent_delta(frame: &[i16]) -> i32 {
    frame
        .windows(2)
        .map(|samples| (i32::from(samples[1]) - i32::from(samples[0])).abs())
        .max()
        .unwrap_or(0)
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn soft_limit_voice_microphone_sample(sample: i16) -> i16 {
    let normalized = (f32::from(sample) / f32::from(i16::MAX)).clamp(-1.0, 1.0);
    let magnitude = normalized.abs();
    if magnitude <= VOICE_MIC_SOFT_LIMIT_THRESHOLD {
        return sample;
    }

    let excess =
        (magnitude - VOICE_MIC_SOFT_LIMIT_THRESHOLD) / (1.0 - VOICE_MIC_SOFT_LIMIT_THRESHOLD);
    let shaped = VOICE_MIC_SOFT_LIMIT_THRESHOLD
        + (VOICE_MIC_SOFT_LIMIT_CEILING - VOICE_MIC_SOFT_LIMIT_THRESHOLD)
            * (1.0 - 1.0 / (1.0 + VOICE_MIC_SOFT_LIMIT_CURVE * excess));
    let limited = normalized.signum() * shaped.min(VOICE_MIC_SOFT_LIMIT_CEILING);

    (limited * f32::from(i16::MAX))
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn voice_pcm_peak(frame: &[i16]) -> i32 {
    frame
        .iter()
        .map(|sample| i32::from(*sample).abs())
        .max()
        .unwrap_or(0)
}
