#[cfg(feature = "voice-playback")]
use super::devices;
#[cfg(feature = "voice-playback")]
use super::noise::VoiceNoiseSuppressor;
use super::*;

#[cfg(feature = "voice-playback")]
impl VoiceMicrophoneCapture {
    pub(super) fn start(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        microphone_buffer_ms: Option<MicrophoneBufferMs>,
    ) -> Result<Self, String> {
        let buffer_mode = microphone_buffer_ms.map_or(
            VoiceMicrophoneBufferMode::PlatformFixed,
            VoiceMicrophoneBufferMode::UserFixed,
        );
        Self::start_with_policy(samples_tx, input_source, buffer_mode)
    }

    fn start_with_policy(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        buffer_mode: VoiceMicrophoneBufferMode,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = Self::start_with_cpal(samples_tx, input_source, buffer_mode);

        #[cfg(target_os = "linux")]
        log_captured_alsa_errors(&alsa_error_output);

        result
    }

    pub(super) fn start_with_cpal(
        samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        input_source: Option<&str>,
        buffer_mode: VoiceMicrophoneBufferMode,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = devices::resolve_input_device(&host, input_source)?;
        let stats = Arc::new(VoiceMicrophoneCaptureStats::default());
        let input_stream = build_preferred_voice_input_stream(
            &device,
            Arc::clone(&stats),
            samples_tx.clone(),
            buffer_mode,
        )
        .or_else(|preferred_error| {
            logging::debug(
                "voice",
                format!("voice preferred microphone input stream failed: {preferred_error}"),
            );
            build_default_voice_input_stream(&device, Arc::clone(&stats), samples_tx, buffer_mode)
        })?;
        input_stream
            .stream
            .play()
            .map_err(|error| format!("voice microphone input stream start failed: {error}"))?;
        let stream_reported_buffer_ms = input_stream
            .stream_reported_buffer_frames
            .map(|frames| voice_buffer_duration_ms(frames, input_stream.stream_config.sample_rate));
        logging::debug(
            "voice",
            format!(
                "voice microphone capture started: host={} sample_rate={} channels={} format={:?} buffer_mode={:?} requested_buffer={:?} stream_reported_buffer_frames={} stream_reported_buffer_ms={}",
                host.id(),
                input_stream.stream_config.sample_rate,
                input_stream.stream_config.channels,
                input_stream.sample_format,
                input_stream.buffer_mode,
                input_stream.stream_config.buffer_size,
                input_stream
                    .stream_reported_buffer_frames
                    .map_or_else(|| "unknown".to_owned(), |frames| frames.to_string()),
                stream_reported_buffer_ms
                    .map_or_else(|| "unknown".to_owned(), |millis| millis.to_string()),
            ),
        );
        Ok(Self {
            _stream: input_stream.stream,
            _processor: input_stream.processor,
            stats,
        })
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_preferred_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let supported_config = select_voice_input_config(device)?;
    let supported_buffer_size = *supported_config.buffer_size();
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    build_configured_voice_input_stream(
        device,
        &mut stream_config,
        &supported_buffer_size,
        sample_format,
        stats,
        samples_tx,
        buffer_mode,
    )
}

#[cfg(feature = "voice-playback")]
pub(super) fn build_default_voice_input_stream(
    device: &cpal::Device,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("voice microphone default input config failed: {error}"))?;
    let supported_buffer_size = *supported_config.buffer_size();
    let sample_format = supported_config.sample_format();
    let mut stream_config = supported_config.config();
    build_configured_voice_input_stream(
        device,
        &mut stream_config,
        &supported_buffer_size,
        sample_format,
        stats,
        samples_tx,
        buffer_mode,
    )
}

#[cfg(feature = "voice-playback")]
fn build_configured_voice_input_stream(
    device: &cpal::Device,
    stream_config: &mut cpal::StreamConfig,
    supported_buffer_size: &cpal::SupportedBufferSize,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    match buffer_mode {
        VoiceMicrophoneBufferMode::UserFixed(duration) => {
            stream_config.buffer_size =
                voice_input_buffer_size(duration, stream_config.sample_rate);
            build_voice_input_stream_with_mode(
                device,
                stream_config,
                sample_format,
                stats,
                samples_tx,
                buffer_mode,
            )
        }
        VoiceMicrophoneBufferMode::PlatformFixed => {
            let Some(buffer_size) =
                automatic_voice_input_buffer_size(supported_buffer_size, stream_config.sample_rate)
            else {
                logging::debug(
                    "voice",
                    "voice microphone buffer range is unknown, using host default buffer",
                );
                return build_host_default_voice_input_stream(
                    device,
                    stream_config,
                    sample_format,
                    stats,
                    samples_tx,
                );
            };

            stream_config.buffer_size = buffer_size;
            match build_voice_input_stream_with_mode(
                device,
                stream_config,
                sample_format,
                Arc::clone(&stats),
                samples_tx.clone(),
                VoiceMicrophoneBufferMode::PlatformFixed,
            ) {
                Ok(input_stream) => Ok(input_stream),
                Err(fixed_error) => {
                    logging::debug(
                        "voice",
                        format!(
                            "voice fixed microphone input buffer failed, retrying host default buffer: {fixed_error}"
                        ),
                    );
                    build_host_default_voice_input_stream(
                        device,
                        stream_config,
                        sample_format,
                        stats,
                        samples_tx,
                    )
                    .map_err(|default_error| {
                        format!(
                            "voice fixed microphone input buffer failed ({fixed_error}); host default fallback failed ({default_error})"
                        )
                    })
                }
            }
        }
        VoiceMicrophoneBufferMode::HostDefaultFallback => build_host_default_voice_input_stream(
            device,
            stream_config,
            sample_format,
            stats,
            samples_tx,
        ),
    }
}

#[cfg(feature = "voice-playback")]
fn build_host_default_voice_input_stream(
    device: &cpal::Device,
    stream_config: &mut cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
) -> Result<VoiceMicrophoneInputStream, String> {
    stream_config.buffer_size = cpal::BufferSize::Default;
    build_voice_input_stream_with_mode(
        device,
        stream_config,
        sample_format,
        stats,
        samples_tx,
        VoiceMicrophoneBufferMode::HostDefaultFallback,
    )
}

#[cfg(feature = "voice-playback")]
fn build_voice_input_stream_with_mode(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    buffer_mode: VoiceMicrophoneBufferMode,
) -> Result<VoiceMicrophoneInputStream, String> {
    let (stream, processor) =
        build_voice_input_stream(device, stream_config, sample_format, stats, samples_tx)?;
    let stream_reported_buffer_frames = voice_stream_reported_buffer_frames(&stream);
    Ok(VoiceMicrophoneInputStream {
        stream,
        processor,
        stream_config: *stream_config,
        sample_format,
        buffer_mode,
        stream_reported_buffer_frames,
    })
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
pub(super) fn voice_input_buffer_size(
    microphone_buffer_ms: MicrophoneBufferMs,
    sample_rate: u32,
) -> cpal::BufferSize {
    cpal::BufferSize::Fixed(microphone_buffer_ms.frames(sample_rate))
}

#[cfg(feature = "voice-playback")]
pub(super) fn automatic_voice_input_buffer_size(
    supported: &cpal::SupportedBufferSize,
    sample_rate: u32,
) -> Option<cpal::BufferSize> {
    match supported {
        cpal::SupportedBufferSize::Range { min, max } => {
            let callback_period_frames = sample_rate
                .checked_div(VOICE_MIC_AUTOMATIC_CALLBACKS_PER_SECOND)
                .unwrap_or(0)
                .max(1);
            Some(cpal::BufferSize::Fixed(
                callback_period_frames.clamp(*min, *max),
            ))
        }
        cpal::SupportedBufferSize::Unknown => None,
    }
}

#[cfg(feature = "voice-playback")]
fn duration_for_audio_frames(frames: u64, sample_rate: u32) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(frames).saturating_mul(1_000_000_000) / u128::from(sample_rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

#[cfg(feature = "voice-playback")]
fn voice_buffer_duration_ms(frames: u32, sample_rate: u32) -> u128 {
    duration_for_audio_frames(u64::from(frames), sample_rate).as_millis()
}

#[cfg(feature = "voice-playback")]
fn voice_stream_reported_buffer_frames(stream: &cpal::Stream) -> Option<u32> {
    match stream.buffer_size() {
        Ok(frames) => Some(frames),
        Err(error) => {
            logging::debug(
                "voice",
                format!("voice microphone reported buffer query failed: {error}"),
            );
            None
        }
    }
}

#[cfg(feature = "voice-playback")]
impl Default for VoiceMicrophoneCaptureStats {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            chunks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            min_callback_frames: AtomicU64::new(u64::MAX),
            max_callback_frames: AtomicU64::new(0),
            queued_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            peak_sample: AtomicU64::new(0),
            clipped_samples: AtomicU64::new(0),
            last_callback_elapsed_us: AtomicU64::new(0),
            max_callback_gap_ms: AtomicU64::new(0),
            callback_queue_drops: AtomicU64::new(0),
            stream_errors: AtomicU64::new(0),
            stream_xruns: AtomicU64::new(0),
            max_capture_latency_us: AtomicU64::new(0),
            max_capture_delivery_age_us: AtomicU64::new(0),
        }
    }
}

#[cfg(feature = "voice-playback")]
impl Drop for VoiceMicrophoneInputProcessor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            logging::debug(
                "voice",
                format!("voice microphone input processor panicked: {error:?}"),
            );
        }
    }
}

#[cfg(feature = "voice-playback")]
impl VoiceMicrophonePcmFrames {
    pub(super) fn new(
        frames_tx: mpsc::Sender<VoiceMicrophoneFrame>,
        stats: Arc<VoiceMicrophoneCaptureStats>,
        source_sample_rate: u32,
    ) -> Self {
        Self {
            frames_tx,
            stats,
            source_sample_rate,
            source_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending: Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES),
            output_pending_at: None,
            next_source_frame: 0.0,
        }
    }

    /// Returns the oldest completed frame timestamp, including frames dropped by a full queue.
    pub(super) fn push_stereo_samples(
        &mut self,
        samples: &[i16],
        captured_at: Instant,
    ) -> Option<Instant> {
        self.align_output_timeline(captured_at);
        if self.source_sample_rate == DISCORD_VOICE_SAMPLE_RATE {
            self.output_pending.extend_from_slice(samples);
        } else {
            self.source_pending.extend_from_slice(samples);
            self.resample_pending_source();
        }
        self.flush_output_frames()
    }

    pub(super) fn apply_input_drop(&mut self, input_dropped: bool) {
        if input_dropped {
            self.reset_after_input_drop();
        }
    }

    fn reset_after_input_drop(&mut self) {
        self.source_pending.clear();
        self.output_pending.clear();
        self.output_pending_at = None;
        self.next_source_frame = 0.0;
    }

    fn align_output_timeline(&mut self, captured_at: Instant) {
        let pending_output_frames = self.output_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        let pending_source_frames = self.source_pending.len() / DISCORD_VOICE_CHANNELS_USIZE;
        let pending_source_duration = duration_for_audio_frames(
            u64::try_from(pending_source_frames).unwrap_or(u64::MAX),
            self.source_sample_rate,
        );
        let pending_output_duration = duration_for_audio_frames(
            u64::try_from(pending_output_frames).unwrap_or(u64::MAX),
            DISCORD_VOICE_SAMPLE_RATE,
        );
        let pending_duration = pending_source_duration.saturating_add(pending_output_duration);
        let pending_started_at = captured_at
            .checked_sub(pending_duration)
            .unwrap_or(captured_at);
        self.output_pending_at = Some(
            pending_started_at
                .checked_add(DISCORD_OPUS_FRAME_DURATION)
                .unwrap_or(pending_started_at),
        );
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

    pub(super) fn flush_output_frames(&mut self) -> Option<Instant> {
        let mut oldest_frame_at = None;
        while self.output_pending.len() >= DISCORD_OPUS_20MS_STEREO_SAMPLES {
            let frame = VoiceMicrophoneFrame {
                samples: self
                    .output_pending
                    .drain(..DISCORD_OPUS_20MS_STEREO_SAMPLES)
                    .collect(),
                captured_at: self.output_pending_at.unwrap_or_else(Instant::now),
            };
            oldest_frame_at.get_or_insert(frame.captured_at);
            self.output_pending_at = self
                .output_pending_at
                .and_then(|captured_at| captured_at.checked_add(DISCORD_OPUS_FRAME_DURATION));
            if self.frames_tx.try_send(frame).is_ok() {
                self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);
            } else {
                self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
        if self.source_pending.is_empty() && self.output_pending.is_empty() {
            self.output_pending_at = None;
        }
        oldest_frame_at
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
                "voice microphone capture stopped: chunks={} frames={} callback_frames_min={} callback_frames_max={} callback_max_gap_ms={} callback_queue_drops={} capture_latency_max_us={} capture_delivery_age_max_us={} stream_errors={} stream_xruns={} queued_20ms_frames={} dropped_20ms_frames={} peak_sample={} clipped_samples={}",
                self.stats.chunks.load(Ordering::Relaxed),
                self.stats.frames.load(Ordering::Relaxed),
                voice_microphone_min_callback_frames(&self.stats),
                self.stats.max_callback_frames.load(Ordering::Relaxed),
                self.stats.max_callback_gap_ms.load(Ordering::Relaxed),
                self.stats.callback_queue_drops.load(Ordering::Relaxed),
                self.stats.max_capture_latency_us.load(Ordering::Relaxed),
                self.stats
                    .max_capture_delivery_age_us
                    .load(Ordering::Relaxed),
                self.stats.stream_errors.load(Ordering::Relaxed),
                self.stats.stream_xruns.load(Ordering::Relaxed),
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
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
) -> Result<(cpal::Stream, VoiceMicrophoneInputProcessor), String> {
    match sample_format {
        cpal::SampleFormat::F32 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_f32_to_stereo_i16,
        ),
        cpal::SampleFormat::U8 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_u8_to_stereo_i16,
        ),
        cpal::SampleFormat::I16 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_i16_to_stereo_i16,
        ),
        cpal::SampleFormat::U16 => build_typed_voice_input_stream(
            device,
            config,
            stats,
            samples_tx,
            voice_input_u16_to_stereo_i16,
        ),
        other => Err(format!(
            "unsupported voice microphone input sample format: {other:?}"
        )),
    }
}

#[cfg(feature = "voice-playback")]
fn build_typed_voice_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    stats: Arc<VoiceMicrophoneCaptureStats>,
    samples_tx: mpsc::Sender<VoiceMicrophoneFrame>,
    convert: fn(&[T], usize) -> Vec<i16>,
) -> Result<(cpal::Stream, VoiceMicrophoneInputProcessor), String>
where
    T: cpal::SizedSample + Copy + Send + 'static,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    let (input_tx, input_rx) = std::sync::mpsc::sync_channel::<VoiceMicrophoneInputChunk<T>>(
        VOICE_MIC_INPUT_CALLBACK_QUEUE,
    );
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = Arc::clone(&stopped);
    let worker_stats = Arc::clone(&stats);
    let (recycle_tx, recycle_rx) =
        std::sync::mpsc::sync_channel::<Vec<T>>(VOICE_MIC_INPUT_CALLBACK_QUEUE + 1);
    let worker = std::thread::Builder::new()
        .name("voice-mic-input".to_owned())
        .spawn(move || {
            let mut pcm_frames =
                VoiceMicrophonePcmFrames::new(samples_tx, Arc::clone(&worker_stats), sample_rate);
            while !worker_stopped.load(Ordering::Acquire) {
                let mut chunk = match input_rx.recv_timeout(Duration::from_millis(5)) {
                    Ok(chunk) => chunk,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                pcm_frames.apply_input_drop(chunk.input_dropped);
                let samples = convert(&chunk.samples, channels);
                record_voice_input_pcm_stats(&samples, &worker_stats);
                let oldest_frame_at = pcm_frames.push_stereo_samples(&samples, chunk.captured_at);
                record_voice_input_delivery_age(oldest_frame_at, Instant::now(), &worker_stats);
                chunk.samples.clear();
                let _ = recycle_tx.try_send(chunk.samples);
            }
        })
        .map_err(|error| format!("voice microphone input processor spawn failed: {error}"))?;
    let processor = VoiceMicrophoneInputProcessor {
        stopped,
        worker: Some(worker),
    };
    let mut spare = None;
    let mut input_dropped = false;
    let error_stats = Arc::clone(&stats);
    let stream = device
        .build_input_stream(
            *config,
            move |input: &[T], info| {
                let callback_at = Instant::now();
                let captured_at = voice_input_capture_instant(info, callback_at);
                record_voice_input_chunk(input.len(), channels, captured_at, callback_at, &stats);
                let mut samples = recycle_rx
                    .try_recv()
                    .ok()
                    .or_else(|| spare.take())
                    .unwrap_or_else(|| Vec::with_capacity(input.len()));
                samples.clear();
                samples.extend_from_slice(input);
                let chunk = VoiceMicrophoneInputChunk {
                    samples,
                    captured_at,
                    input_dropped: std::mem::take(&mut input_dropped),
                };
                match input_tx.try_send(chunk) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(dropped)) => {
                        stats.callback_queue_drops.fetch_add(1, Ordering::Relaxed);
                        input_dropped = true;
                        spare = Some(dropped.samples);
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(dropped)) => {
                        spare = Some(dropped.samples);
                    }
                }
            },
            move |error| record_voice_input_stream_error(error, &error_stats),
            None,
        )
        .map_err(|error| format!("voice microphone input stream build failed: {error}"))?;
    Ok((stream, processor))
}

#[cfg(feature = "voice-playback")]
fn voice_input_capture_instant(info: &cpal::InputCallbackInfo, callback_at: Instant) -> Instant {
    let timestamp = info.timestamp();
    let capture_delay = timestamp
        .callback
        .saturating_duration_since(timestamp.capture);
    callback_at
        .checked_sub(capture_delay)
        .unwrap_or(callback_at)
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
    captured_at: Instant,
    callback_at: Instant,
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

    let elapsed_us = u64::try_from(
        callback_at
            .saturating_duration_since(stats.started_at)
            .as_micros(),
    )
    .unwrap_or(u64::MAX);
    let previous_elapsed_us = stats
        .last_callback_elapsed_us
        .swap(elapsed_us.max(1), Ordering::Relaxed);
    let callback_gap = elapsed_us.saturating_sub(previous_elapsed_us);
    if previous_elapsed_us != 0 {
        stats
            .max_callback_gap_ms
            .fetch_max(callback_gap / 1_000, Ordering::Relaxed);
    }

    let capture_latency = callback_at.saturating_duration_since(captured_at);
    let capture_latency_us = u64::try_from(capture_latency.as_micros()).unwrap_or(u64::MAX);
    stats
        .max_capture_latency_us
        .fetch_max(capture_latency_us, Ordering::Relaxed);
}

#[cfg(feature = "voice-playback")]
fn record_voice_input_delivery_age(
    oldest_frame_at: Option<Instant>,
    ready_at: Instant,
    stats: &VoiceMicrophoneCaptureStats,
) {
    if let Some(captured_at) = oldest_frame_at {
        let age = ready_at.saturating_duration_since(captured_at);
        stats.max_capture_delivery_age_us.fetch_max(
            u64::try_from(age.as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
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
pub(super) fn record_voice_input_stream_error(
    error: cpal::Error,
    stats: &VoiceMicrophoneCaptureStats,
) {
    stats.stream_errors.fetch_add(1, Ordering::Relaxed);
    if error.kind() == cpal::ErrorKind::Xrun {
        stats.stream_xruns.fetch_add(1, Ordering::Relaxed);
        logging::debug(
            "voice",
            format!("voice microphone input stream reported an xrun: {error}"),
        );
    } else {
        logging::error(
            "voice",
            format!("voice microphone input stream failed: {error}"),
        );
    }
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

/// Flushes a stop-speaking notice through the outbound sender, logging
/// instead of propagating failures since the transmit loop keeps running.
#[cfg(feature = "voice-playback")]
async fn stop_voice_transmission(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    let outcome = sender.stop_speaking_with_dave(&mut *context.dave_state.lock().await);
    if let Err(error) = flush_voice_outbound_events(
        &context.udp_socket,
        &context.writer,
        outcome,
        sender,
        transmit_stats,
    )
    .await
    {
        logging::error("voice", error);
    }
}

/// [`stop_voice_transmission`] plus forcing the capture gate shut. The
/// transmit loop publishes the local silent edge on every teardown path.
#[cfg(feature = "voice-playback")]
async fn silence_voice_transmission(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    stop_voice_transmission(context, sender, transmit_stats).await;
    sender.set_capture_gate(false, false);
}

#[cfg(feature = "voice-playback")]
pub(super) fn publish_local_speaking_edge(
    local_speaking_tx: &mpsc::UnboundedSender<bool>,
    local_speaking: &mut bool,
    speaking: bool,
) {
    if *local_speaking == speaking {
        return;
    }
    *local_speaking = speaking;
    let _ = local_speaking_tx.send(speaking);
}

#[cfg(feature = "voice-playback")]
pub(super) fn voice_microphone_frame_is_active(
    gate: VoiceCaptureGate,
    microphone_gate: &mut VoiceMicrophoneGateState,
    frame: &[i16],
) -> bool {
    gate.transmit_enabled
        && (!gate.use_voice_activity
            || microphone_gate.allows_frame(frame, gate.microphone_sensitivity))
}

/// Applies overload smoothing, user volume, transmit boost, and the limiter
/// to a captured microphone frame in place.
#[cfg(feature = "voice-playback")]
pub(super) fn condition_voice_microphone_frame(
    frame: &mut [i16],
    gate: VoiceCaptureGate,
    microphone_gate: &mut VoiceMicrophoneGateState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) {
    let raw_overload_decision = voice_microphone_overload_decision(frame);
    let overload_decision =
        if voice_microphone_clipped_frame_needs_blank(frame, raw_overload_decision) {
            Some(VoiceMicrophoneOverloadDecision {
                kind: VoiceMicrophoneOverloadKind::HandlingNoise,
                gain: VOICE_MIC_HANDLING_NOISE_GAIN,
            })
        } else {
            microphone_gate.overload_decision(frame)
        };
    let overload_gain = if let Some(decision) = overload_decision {
        transmit_stats.overload_smoothed_frames += 1;
        decision.gain
    } else {
        1.0
    };
    let combined_gain =
        overload_gain * gate.microphone_volume.gain() * VOICE_MIC_TRANSMIT_BOOST_GAIN;
    transmit_stats.limited_samples += apply_voice_microphone_gain_and_limit(frame, combined_gain);
}

/// Advances past old microphone audio until no more than the live latency
/// budget remains.
#[cfg(feature = "voice-playback")]
pub(super) fn select_fresh_voice_microphone_frame(
    mut frame: VoiceMicrophoneFrame,
    pcm_rx: &mut mpsc::Receiver<VoiceMicrophoneFrame>,
    now: Instant,
) -> (Option<VoiceMicrophoneFrame>, u64) {
    let mut dropped = 0u64;
    while now.saturating_duration_since(frame.captured_at) > VOICE_MIC_MAX_FRAME_AGE {
        dropped = dropped.saturating_add(1);
        let Ok(next) = pcm_rx.try_recv() else {
            return (None, dropped);
        };
        frame = next;
    }
    (Some(frame), dropped)
}

#[cfg(any(test, feature = "voice-playback"))]
pub(super) fn advance_voice_media_clock(
    sender: &mut VoiceOutboundSendState,
    previous_frame_at: &mut Option<Instant>,
    captured_at: Instant,
) {
    let elapsed_frames = previous_frame_at
        .map(|previous| {
            let elapsed_us = captured_at.saturating_duration_since(previous).as_micros();
            let frame_us = DISCORD_OPUS_FRAME_DURATION.as_micros();
            let rounded_frames = elapsed_us.saturating_add(frame_us / 2) / frame_us;
            u32::try_from(rounded_frames).unwrap_or(u32::MAX).max(1)
        })
        .unwrap_or(1);
    sender.advance_media_clock_frames(elapsed_frames);
    *previous_frame_at = Some(captured_at);
}

#[cfg(feature = "voice-playback")]
async fn send_voice_trailing_silence_frame(
    context: &VoiceUdpTransmitContext,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
    trailing_silence: &mut VoiceTrailingSilence,
) -> Result<(), String> {
    let Some(finish_talkspurt) = trailing_silence.take_frame() else {
        return Ok(());
    };

    let mut dave_state = context.dave_state.lock().await;
    let outcome = sender.send_trailing_silence_frame_with_dave(&mut dave_state, finish_talkspurt);
    drop(dave_state);
    flush_voice_outbound_events(
        &context.udp_socket,
        &context.writer,
        outcome,
        sender,
        transmit_stats,
    )
    .await?;

    if !sender.speaking {
        trailing_silence.cancel();
    }
    Ok(())
}

#[cfg(feature = "voice-playback")]
pub(super) async fn run_voice_udp_transmit(
    mut pcm_rx: mpsc::Receiver<VoiceMicrophoneFrame>,
    mut gate_rx: watch::Receiver<VoiceCaptureGate>,
    context: VoiceUdpTransmitContext,
) -> Result<(), String> {
    let rtp = VoiceOutboundRtpState {
        sequence: 0,
        timestamp: 0,
        ssrc: context.ssrc,
    };
    let mut sender = match VoiceOutboundSendState::new(
        &context.description.mode,
        &context.description.secret_key,
        rtp,
        0,
    ) {
        Ok(sender) => sender,
        Err(error) => {
            let _ = context.local_speaking_tx.send(false);
            return Err(format!("voice UDP transmit init failed: {error}"));
        }
    };
    let initial_gate = *gate_rx.borrow();
    sender.set_capture_gate(initial_gate.transmit_enabled, false);
    let mut encoder = match VoiceOpusEncode::new() {
        Ok(encoder) => encoder,
        Err(error) => {
            let _ = context.local_speaking_tx.send(false);
            return Err(error);
        }
    };
    let transmit_started_at = Instant::now();
    let mut transmit_stats = VoiceUdpTransmitStats::default();
    let mut microphone_gate = VoiceMicrophoneGateState::default();
    let mut trailing_silence = VoiceTrailingSilence::default();
    let mut noise_suppressor = VoiceNoiseSuppressor::new();
    let mut noise_suppression_enabled = initial_gate.noise_suppression;
    let mut previous_microphone_frame_at = None;
    let mut next_stats_log_at = transmit_started_at + VOICE_TRANSMIT_STATS_LOG_INTERVAL;
    let mut local_speaking = false;
    let mut transmit_interval = tokio::time::interval(DISCORD_OPUS_FRAME_DURATION);
    transmit_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let result = loop {
        tokio::select! {
            changed = gate_rx.changed() => {
                if changed.is_err() {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    silence_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                    break Ok(());
                }
                let gate = *gate_rx.borrow();
                let was_enabled = sender.capture_gate_enabled();
                if gate.transmit_enabled != was_enabled {
                    drain_voice_microphone_pcm_queue(&mut pcm_rx);
                    microphone_gate.reset();
                    noise_suppressor.reset();
                }
                if gate.noise_suppression != noise_suppression_enabled {
                    if gate.noise_suppression {
                        noise_suppressor.reset();
                    }
                    noise_suppression_enabled = gate.noise_suppression;
                }
                if !gate.transmit_enabled {
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        false,
                    );
                    if gate.capture_enabled {
                        trailing_silence.start(sender.speaking);
                    } else {
                        trailing_silence.cancel();
                        stop_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                    }
                    microphone_gate.reset();
                } else {
                    trailing_silence.cancel();
                }
                sender.set_capture_gate(gate.transmit_enabled, false);
            }
            _ = transmit_interval.tick() => {
                let frame = match pcm_rx.try_recv() {
                    Ok(frame) => frame,
                    Err(mpsc::error::TryRecvError::Empty) => continue,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        silence_voice_transmission(&context, &mut sender, &mut transmit_stats).await;
                        microphone_gate.reset();
                        break Ok(());
                    }
                };
                transmit_stats.max_microphone_queue_depth = transmit_stats
                    .max_microphone_queue_depth
                    .max(pcm_rx.len().saturating_add(1));
                let (frame, stale_frames_dropped) = select_fresh_voice_microphone_frame(
                    frame,
                    &mut pcm_rx,
                    Instant::now(),
                );
                transmit_stats.stale_microphone_frames_dropped = transmit_stats
                    .stale_microphone_frames_dropped
                    .saturating_add(stale_frames_dropped);
                if stale_frames_dropped > 0 {
                    microphone_gate.reset();
                }
                let Some(mut frame) = frame else {
                    continue;
                };
                advance_voice_media_clock(
                    &mut sender,
                    &mut previous_microphone_frame_at,
                    frame.captured_at,
                );
                let gate = *gate_rx.borrow();
                if !gate.transmit_enabled {
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        false,
                    );
                    microphone_gate.reset();
                    if let Err(error) = send_voice_trailing_silence_frame(
                        &context,
                        &mut sender,
                        &mut transmit_stats,
                        &mut trailing_silence,
                    )
                    .await
                    {
                        break Err(error);
                    }
                } else {
                    if gate.noise_suppression {
                        let processing_started_at = Instant::now();
                        if noise_suppressor.process_20ms_stereo(&mut frame.samples) {
                            transmit_stats.noise_suppressed_frames = transmit_stats
                                .noise_suppressed_frames
                                .saturating_add(1);
                            transmit_stats.max_noise_suppression_processing_us = transmit_stats
                                .max_noise_suppression_processing_us
                                .max(processing_started_at.elapsed().as_micros());
                        }
                    }
                    let microphone_active =
                        voice_microphone_frame_is_active(gate, &mut microphone_gate, &frame.samples);
                    publish_local_speaking_edge(
                        &context.local_speaking_tx,
                        &mut local_speaking,
                        microphone_active,
                    );
                    if !microphone_active {
                        trailing_silence.start(sender.speaking);
                        if let Err(error) = send_voice_trailing_silence_frame(
                            &context,
                            &mut sender,
                            &mut transmit_stats,
                            &mut trailing_silence,
                        )
                        .await
                        {
                            break Err(error);
                        }
                    } else {
                        trailing_silence.cancel();
                        condition_voice_microphone_frame(
                            &mut frame.samples,
                            gate,
                            &mut microphone_gate,
                            &mut transmit_stats,
                        );
                        let opus = match encoder.encode_20ms_i16(&frame.samples) {
                            Ok(opus) => Some(opus),
                            Err(error) => {
                                logging::debug("voice", error);
                                None
                            }
                        };
                        if let Some(opus) = opus {
                            let now = Instant::now();
                            let frame_age = now.saturating_duration_since(frame.captured_at);
                            transmit_stats.max_microphone_queue_depth = transmit_stats
                                .max_microphone_queue_depth
                                .max(pcm_rx.len().saturating_add(1));
                            if frame_age > VOICE_MIC_MAX_FRAME_AGE {
                                transmit_stats.stale_microphone_frames_dropped = transmit_stats
                                    .stale_microphone_frames_dropped
                                    .saturating_add(1);
                                microphone_gate.reset();
                            } else {
                                transmit_stats.max_microphone_frame_age_ms = transmit_stats
                                    .max_microphone_frame_age_ms
                                    .max(frame_age.as_millis());
                                record_voice_transmit_frame(&mut transmit_stats, now);
                                let mut dave_state = context.dave_state.lock().await;
                                let outcome =
                                    sender.send_opus_frame_with_dave(&opus, &mut dave_state);
                                drop(dave_state);
                                if let Err(error) = flush_voice_outbound_events(
                                    &context.udp_socket,
                                    &context.writer,
                                    outcome,
                                    &mut sender,
                                    &mut transmit_stats,
                                )
                                .await
                                {
                                    break Err(error);
                                }
                            }
                        }
                    }
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
    };
    publish_local_speaking_edge(&context.local_speaking_tx, &mut local_speaking, false);
    sender.set_capture_gate(false, false);
    log_voice_transmit_stats(
        "voice UDP transmit stopped",
        &transmit_stats,
        transmit_started_at,
        sender.rtp.timestamp,
    );
    result
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
pub(super) fn drain_voice_microphone_pcm_queue(pcm_rx: &mut mpsc::Receiver<VoiceMicrophoneFrame>) {
    while pcm_rx.try_recv().is_ok() {}
}

#[cfg(feature = "voice-playback")]
pub(super) async fn flush_voice_outbound_events(
    udp_socket: &UdpSocket,
    writer: &VoiceWriter,
    outcome: Result<VoiceOutboundSendOutcome, String>,
    sender: &mut VoiceOutboundSendState,
    transmit_stats: &mut VoiceUdpTransmitStats,
) -> Result<(), String> {
    match outcome? {
        VoiceOutboundSendOutcome::Sent => {
            for event in sender.take_events() {
                match event {
                    VoiceOutboundSendEvent::Speaking { speaking, ssrc } => {
                        send_voice_text(writer, voice_speaking_payload(ssrc, speaking)).await?;
                    }
                    VoiceOutboundSendEvent::Packet { bytes } => {
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
        VoiceOutboundSendOutcome::Noop => {
            let _ = sender.take_logged_block_reason();
        }
        VoiceOutboundSendOutcome::Blocked(reason) => {
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
            "{label}: elapsed_ms={} sent_packets={} rtp_timestamp={} rtp_elapsed_ms={} stale_microphone_frames_dropped={} max_microphone_queue_depth={} max_microphone_frame_age_ms={} noise_suppressed_frames={} max_noise_suppression_processing_us={} overload_smoothed_frames={} limited_samples={} max_frame_gap_ms={}",
            elapsed_ms,
            stats.sent_packets,
            rtp_timestamp,
            rtp_elapsed_ms,
            stats.stale_microphone_frames_dropped,
            stats.max_microphone_queue_depth,
            stats.max_microphone_frame_age_ms,
            stats.noise_suppressed_frames,
            stats.max_noise_suppression_processing_us,
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
pub(super) fn apply_voice_microphone_gain_and_limit(frame: &mut [i16], gain: f32) -> u64 {
    let mut limited = 0u64;
    for sample in frame {
        let amplified = f32::from(*sample) * gain;
        if amplified.abs() > f32::from(i16::MAX) * VOICE_SOFT_LIMIT_THRESHOLD {
            limited += 1;
        }
        let normalized = amplified / f32::from(i16::MAX);
        *sample = (soft_limit_voice_sample(normalized) * f32::from(i16::MAX))
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
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
pub(super) fn voice_pcm_peak(frame: &[i16]) -> i32 {
    frame
        .iter()
        .map(|sample| i32::from(*sample).abs())
        .max()
        .unwrap_or(0)
}
