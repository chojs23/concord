use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use wasapi::{
    AudioClient, Direction, SampleType, StreamMode, WasapiError, WaveFormat, initialize_mta,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use super::{
    AudioFrameAssembler, AudioProcessMode, SYSTEM_AUDIO_CHANNELS, SYSTEM_AUDIO_SAMPLE_RATE,
    StreamCaptureTarget,
};

pub(super) const BACKEND_NAME: &str = "wasapi-process-loopback";

const START_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_WAIT_MS: u32 = 100;

pub(super) struct CaptureSession {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

pub(super) fn start_capture(
    _target: &StreamCaptureTarget,
    target_pid: u32,
    mode: AudioProcessMode,
    assembler: AudioFrameAssembler,
) -> Result<CaptureSession, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("stream-wasapi-system-audio".to_owned())
        .spawn(move || run_capture(target_pid, mode, assembler, worker_stop, ready_tx))
        .map_err(|error| format!("WASAPI system audio worker spawn failed: {error}"))?;

    match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(())) => Ok(CaptureSession {
            stop,
            worker: Some(worker),
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, Ordering::Release);
            let _ = worker.join();
            Err("WASAPI system audio capture did not start in time".to_owned())
        }
    }
}

pub(super) fn target_process_id(target: &StreamCaptureTarget) -> Result<Option<u32>, String> {
    let mut process_id = 0;
    unsafe {
        GetWindowThreadProcessId(target.id as usize as *mut _, &mut process_id);
    }
    if process_id == 0 {
        return Err(format!(
            "window process lookup failed for capture target: {}",
            target.title
        ));
    }
    Ok(Some(process_id))
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|panic| format!("WASAPI system audio worker panicked: {panic:?}"))?
    }
}

fn run_capture(
    target_pid: u32,
    mode: AudioProcessMode,
    mut assembler: AudioFrameAssembler,
    stop: Arc<AtomicBool>,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    if let Err(error) = initialize_mta().ok() {
        let error = format!("WASAPI COM initialization failed: {error}");
        let _ = ready_tx.send(Err(error.clone()));
        return Err(error);
    }

    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        SYSTEM_AUDIO_SAMPLE_RATE as usize,
        SYSTEM_AUDIO_CHANNELS as usize,
        None,
    );
    let include_tree = mode == AudioProcessMode::Include;
    let result = (|| {
        let mut audio_client =
            AudioClient::new_application_loopback_client(target_pid, include_tree)
                .map_err(|error| format!("WASAPI process loopback activation failed: {error}"))?;
        audio_client
            .initialize_client(
                &desired_format,
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 0,
                },
            )
            .map_err(|error| format!("WASAPI process loopback initialization failed: {error}"))?;
        let event = audio_client
            .set_get_eventhandle()
            .map_err(|error| format!("WASAPI capture event creation failed: {error}"))?;
        let capture_client = audio_client
            .get_audiocaptureclient()
            .map_err(|error| format!("WASAPI capture client creation failed: {error}"))?;
        audio_client
            .start_stream()
            .map_err(|error| format!("WASAPI process loopback start failed: {error}"))?;

        if ready_tx.send(Ok(())).is_err() {
            let _ = audio_client.stop_stream();
            return Ok(());
        }

        let capture_result = capture_packets(&capture_client, &event, &stop, &mut assembler);
        let stop_result = audio_client
            .stop_stream()
            .map_err(|error| format!("WASAPI process loopback stop failed: {error}"));
        capture_result.and(stop_result)
    })();

    if let Err(error) = &result {
        let _ = ready_tx.send(Err(error.clone()));
    }
    result
}

fn capture_packets(
    capture_client: &wasapi::AudioCaptureClient,
    event: &wasapi::Handle,
    stop: &AtomicBool,
    assembler: &mut AudioFrameAssembler,
) -> Result<(), String> {
    let mut bytes = VecDeque::new();
    let mut samples = Vec::with_capacity(4_096);

    while !stop.load(Ordering::Acquire) {
        match event.wait_for_event(EVENT_WAIT_MS) {
            Ok(()) | Err(WasapiError::EventTimeout) => {}
            Err(error) => return Err(format!("WASAPI capture event failed: {error}")),
        }
        if stop.load(Ordering::Acquire) {
            break;
        }

        while capture_client
            .get_next_packet_size()
            .map_err(|error| format!("WASAPI capture packet query failed: {error}"))?
            .unwrap_or(0)
            > 0
        {
            capture_client
                .read_from_device_to_deque(&mut bytes)
                .map_err(|error| format!("WASAPI capture packet read failed: {error}"))?;
        }

        samples.clear();
        while bytes.len() >= size_of::<f32>() {
            let sample = [
                bytes.pop_front().expect("WASAPI sample byte is available"),
                bytes.pop_front().expect("WASAPI sample byte is available"),
                bytes.pop_front().expect("WASAPI sample byte is available"),
                bytes.pop_front().expect("WASAPI sample byte is available"),
            ];
            samples.push(f32::from_le_bytes(sample));
        }
        if !samples.is_empty() && !assembler.push(&samples) {
            break;
        }
    }
    Ok(())
}
