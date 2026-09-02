use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use crate::logging;

use super::{
    AudioFrameAssembler, AudioProcessMode, DISCORD_OPUS_20MS_STEREO_SAMPLES, SYSTEM_AUDIO_CHANNELS,
    SYSTEM_AUDIO_SAMPLE_RATE, StreamCaptureTarget,
};

pub(super) const BACKEND_NAME: &str = "pipewire-process";

const START_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROCESS_ANCESTORS: usize = 32;
const CPAL_PULSEAUDIO_APPLICATION_PREFIX: &str = "cpal-pulseaudio-";
const PIPEWIRE_AUDIO_REQUESTED_LATENCY: &str = "960/48000";
const PIPEWIRE_AUDIO_MAX_LATENCY: &str = "8192/48000";
const PIPEWIRE_AUDIO_MAX_QUANTUM_FRAMES: usize = 8192;
// Some PipeWire graphs ignore the requested 20 ms quantum and deliver 4096 or
// 8192 frames at once. Two maximum-size callbacks avoid dropping part of a
// callback without making that capacity an intentional playback delay.
const AUDIO_SAMPLE_RING_CAPACITY: usize =
    PIPEWIRE_AUDIO_MAX_QUANTUM_FRAMES * SYSTEM_AUDIO_CHANNELS as usize * 2 + 1;
const AUDIO_ASSEMBLER_IDLE_WAIT: Duration = Duration::from_millis(1);

pub(super) struct CaptureSession {
    stop_tx: pw::channel::Sender<()>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

struct PipeWireAudioState {
    format: spa::param::audio::AudioInfoRaw,
    samples: Arc<AudioSampleRing>,
}

// PipeWire runs the process callback on its realtime thread. This SPSC ring
// keeps that callback allocation-free while conversion and Tokio delivery run
// on a normal worker thread.
struct AudioSampleRing {
    samples: Box<[AtomicU32]>,
    read: AtomicUsize,
    write: AtomicUsize,
    dropped: AtomicU64,
    maximum_push_samples: AtomicUsize,
}

struct AudioAssemblerWorker {
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
struct OutputNode {
    client_id: Option<u32>,
    application_pid: Option<u32>,
    security_pid: Option<u32>,
}

#[derive(Clone)]
struct AudioPort {
    node_id: u32,
    direction: String,
    channel: String,
}

struct LinkedOutput {
    pairs: Vec<(u32, u32)>,
    _links: Vec<pw::link::Link>,
}

struct OutputNodePartition {
    captured: Vec<(u32, u32)>,
    excluded: Vec<(u32, u32)>,
}

#[derive(Default)]
struct RegistryState {
    clients: HashMap<u32, u32>,
    nodes: HashMap<u32, OutputNode>,
    ports: HashMap<u32, AudioPort>,
    links: HashMap<u32, LinkedOutput>,
}

impl AudioSampleRing {
    fn new(capacity: usize) -> Self {
        assert!(capacity >= 2, "audio sample ring needs usable capacity");
        Self {
            samples: (0..capacity)
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            maximum_push_samples: AtomicUsize::new(0),
        }
    }

    fn push_bytes(&self, bytes: &[u8]) {
        self.maximum_push_samples
            .fetch_max(bytes.len() / size_of::<f32>(), Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let mut write = self.write.load(Ordering::Relaxed);
        let mut dropped = 0u64;

        for sample in bytes.as_chunks::<4>().0 {
            let next = (write + 1) % self.samples.len();
            if next == read {
                dropped = dropped.saturating_add(1);
                continue;
            }
            let sample = f32::from_le_bytes(*sample);
            self.samples[write].store(sample.to_bits(), Ordering::Relaxed);
            write = next;
        }

        self.write.store(write, Ordering::Release);
        self.dropped.fetch_add(dropped, Ordering::Relaxed);
    }

    fn pop_into(&self, output: &mut Vec<f32>, maximum: usize) {
        let mut read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);

        while read != write && output.len() < maximum {
            output.push(f32::from_bits(self.samples[read].load(Ordering::Relaxed)));
            read = (read + 1) % self.samples.len();
        }

        self.read.store(read, Ordering::Release);
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn maximum_push_samples(&self) -> usize {
        self.maximum_push_samples.load(Ordering::Relaxed)
    }
}

impl AudioAssemblerWorker {
    fn start(
        samples: Arc<AudioSampleRing>,
        mut assembler: AudioFrameAssembler,
    ) -> Result<Self, String> {
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        let worker = thread::Builder::new()
            .name("stream-pipewire-audio-assembler".to_owned())
            .spawn(move || {
                let mut batch = Vec::with_capacity(DISCORD_OPUS_20MS_STEREO_SAMPLES);
                while !worker_stopping.load(Ordering::Acquire) {
                    batch.clear();
                    samples.pop_into(&mut batch, DISCORD_OPUS_20MS_STEREO_SAMPLES);
                    if batch.is_empty() {
                        thread::sleep(AUDIO_ASSEMBLER_IDLE_WAIT);
                        continue;
                    }
                    if !assembler.push(&batch) {
                        return;
                    }
                }
            })
            .map_err(|error| format!("PipeWire audio assembler worker spawn failed: {error}"))?;
        Ok(Self {
            stopping,
            worker: Some(worker),
        })
    }

    fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AudioAssemblerWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn start_capture(
    _target: &StreamCaptureTarget,
    target_pid: u32,
    mode: AudioProcessMode,
    assembler: AudioFrameAssembler,
) -> Result<CaptureSession, String> {
    let (stop_tx, stop_rx) = pw::channel::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let stopping = Arc::new(AtomicBool::new(false));
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::Builder::new()
        .name("stream-pipewire-system-audio".to_owned())
        .spawn(move || {
            run_capture(
                target_pid,
                mode,
                assembler,
                stop_rx,
                ready_tx,
                worker_stopping,
            )
        })
        .map_err(|error| format!("PipeWire system audio worker spawn failed: {error}"))?;

    match ready_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(())) => Ok(CaptureSession {
            stop_tx,
            stopping,
            worker: Some(worker),
        }),
        Ok(Err(error)) => {
            let _ = worker.join();
            Err(error)
        }
        Err(_) => {
            stopping.store(true, Ordering::Release);
            let _ = stop_tx.send(());
            let _ = worker.join();
            Err("PipeWire system audio capture did not start in time".to_owned())
        }
    }
}

pub(super) fn target_process_id(_target: &StreamCaptureTarget) -> Result<Option<u32>, String> {
    Ok(None)
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        let _ = self.stop_tx.send(());
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|panic| format!("PipeWire system audio worker panicked: {panic:?}"))?
    }
}

fn run_capture(
    target_pid: u32,
    mode: AudioProcessMode,
    assembler: AudioFrameAssembler,
    stop_rx: pw::channel::Receiver<()>,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|error| format!("PipeWire audio main loop creation failed: {error}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        format!(
            "PipeWire audio context creation failed: {error}. Ensure the PipeWire client configuration is installed"
        )
    })?;
    let core = context
        .connect_rc(None)
        .map_err(|error| format!("PipeWire audio connection failed: {error}"))?;
    let registry = core
        .get_registry_rc()
        .map_err(|error| format!("PipeWire audio registry creation failed: {error}"))?;
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        "concord-system-audio",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_CLASS => "Stream/Input/Audio",
            *pw::keys::MEDIA_ROLE => "Screen",
            *pw::keys::NODE_NAME => "concord-system-audio",
            *pw::keys::NODE_LATENCY => PIPEWIRE_AUDIO_REQUESTED_LATENCY,
            *pw::keys::NODE_MAX_LATENCY => PIPEWIRE_AUDIO_MAX_LATENCY,
        },
    )
    .map_err(|error| format!("PipeWire system audio stream creation failed: {error}"))?;
    let samples = Arc::new(AudioSampleRing::new(AUDIO_SAMPLE_RING_CAPACITY));
    let mut assembler_worker = AudioAssemblerWorker::start(Arc::clone(&samples), assembler)?;

    let runtime_error = Rc::new(RefCell::new(None::<String>));
    let error_mainloop = mainloop.clone();
    let state_error = Rc::clone(&runtime_error);
    let format_mainloop = mainloop.clone();
    let format_error = Rc::clone(&runtime_error);
    let _stream_listener = stream
        .add_local_listener_with_user_data(PipeWireAudioState {
            format: spa::param::audio::AudioInfoRaw::new(),
            samples: Arc::clone(&samples),
        })
        .state_changed(move |_, _, _, state| {
            if let pw::stream::StreamState::Error(error) = state {
                *state_error.borrow_mut() =
                    Some(format!("PipeWire system audio stream failed: {error}"));
                error_mainloop.quit();
            }
        })
        .param_changed(move |_, state, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != spa::param::format::MediaType::Audio
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            if let Err(error) = state.format.parse(param) {
                *format_error.borrow_mut() = Some(format!(
                    "PipeWire system audio format parse failed: {error}"
                ));
                format_mainloop.quit();
                return;
            }
            if state.format.format() != spa::param::audio::AudioFormat::F32LE
                || state.format.rate() != SYSTEM_AUDIO_SAMPLE_RATE
                || state.format.channels() != u32::from(SYSTEM_AUDIO_CHANNELS)
            {
                *format_error.borrow_mut() = Some(format!(
                    "PipeWire system audio negotiated an unsupported format: format={:?} rate={} channels={}",
                    state.format.format(),
                    state.format.rate(),
                    state.format.channels(),
                ));
                format_mainloop.quit();
                return;
            }
            logging::debug(
                "stream",
                format!(
                    "PipeWire system audio format negotiated: format={:?} rate={} channels={}",
                    state.format.format(),
                    state.format.rate(),
                    state.format.channels(),
                ),
            );
        })
        .process(move |stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let chunk = data.chunk();
            let offset = chunk.offset() as usize;
            let size = chunk.size() as usize;
            let Some(bytes) = data.data() else {
                return;
            };
            let Some(end) = offset.checked_add(size).filter(|end| *end <= bytes.len()) else {
                return;
            };

            state.samples.push_bytes(&bytes[offset..end]);
        })
        .register()
        .map_err(|error| format!("PipeWire system audio listener setup failed: {error}"))?;

    let values = audio_format_pod()?;
    let mut params = [Pod::from_bytes(&values).expect("serialized audio format is valid")];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|error| format!("PipeWire system audio stream connection failed: {error}"))?;

    let registry_state = Rc::new(RefCell::new(RegistryState::default()));
    let global_state = Rc::clone(&registry_state);
    let global_core = core.clone();
    let global_stream = stream.clone();
    let remove_state = Rc::clone(&registry_state);
    let remove_core = core.clone();
    let remove_stream = stream.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props else {
                return;
            };
            match global.type_ {
                pw::types::ObjectType::Client => {
                    if let Some(pid) = client_process_id_from_properties(props) {
                        global_state.borrow_mut().clients.insert(global.id, pid);
                    }
                }
                pw::types::ObjectType::Node => {
                    if props.get(*pw::keys::MEDIA_CLASS) != Some("Stream/Output/Audio") {
                        return;
                    }
                    let client_id = props
                        .get(*pw::keys::CLIENT_ID)
                        .and_then(|value| value.parse().ok());
                    let application_pid = application_process_id_from_properties(props);
                    let security_pid = property_pid(props, *pw::keys::SEC_PID);
                    global_state.borrow_mut().nodes.insert(
                        global.id,
                        OutputNode {
                            client_id,
                            application_pid,
                            security_pid,
                        },
                    );
                }
                pw::types::ObjectType::Port => {
                    let Some(node_id) = props
                        .get(*pw::keys::NODE_ID)
                        .and_then(|value| value.parse().ok())
                    else {
                        return;
                    };
                    let direction = props
                        .get(*pw::keys::PORT_DIRECTION)
                        .unwrap_or("")
                        .to_owned();
                    if direction != "in" && direction != "out" {
                        return;
                    }
                    let channel = props.get(*pw::keys::AUDIO_CHANNEL).unwrap_or("").to_owned();
                    let mut state = global_state.borrow_mut();
                    state.ports.insert(
                        global.id,
                        AudioPort {
                            node_id,
                            direction,
                            channel,
                        },
                    );
                }
                _ => return,
            }
            link_matching_outputs(
                &global_core,
                &global_stream,
                target_pid,
                mode,
                &global_state,
            );
        })
        .global_remove(move |id| {
            let self_node_id = remove_stream.node_id();
            let mut state = remove_state.borrow_mut();
            let removed_port = state.ports.remove(&id);
            let removed_node = state.nodes.remove(&id).is_some();
            let removed_client = state.clients.remove(&id).is_some();
            if removed_node {
                state.links.remove(&id);
            }
            if let Some(port) = removed_port {
                if port.node_id == self_node_id {
                    state.links.clear();
                } else {
                    state.links.remove(&port.node_id);
                }
            }
            if removed_client {
                state.links.clear();
            }
            drop(state);
            link_matching_outputs(
                &remove_core,
                &remove_stream,
                target_pid,
                mode,
                &remove_state,
            );
        })
        .register();

    let stop_mainloop = mainloop.clone();
    let _stop_listener = stop_rx.attach(mainloop.loop_(), move |_| stop_mainloop.quit());
    if ready_tx.send(Ok(())).is_err() {
        return Ok(());
    }

    mainloop.run();
    assembler_worker.stop();
    logging::debug(
        "stream",
        format!(
            "PipeWire system audio realtime ring stopped: dropped_samples={} maximum_callback_samples={}",
            samples.dropped_samples(),
            samples.maximum_push_samples(),
        ),
    );
    if let Some(error) = runtime_error.borrow_mut().take()
        && !stopping.load(Ordering::Acquire)
    {
        return Err(error);
    }
    Ok(())
}

fn audio_format_pod() -> Result<Vec<u8>, String> {
    let mut audio = spa::param::audio::AudioInfoRaw::new();
    audio.set_format(spa::param::audio::AudioFormat::F32LE);
    audio.set_rate(SYSTEM_AUDIO_SAMPLE_RATE);
    audio.set_channels(u32::from(SYSTEM_AUDIO_CHANNELS));
    let format = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format),
    )
    .map(|serialized| serialized.0.into_inner())
    .map_err(|error| format!("PipeWire system audio format serialization failed: {error}"))
}

fn property_pid(props: &spa::utils::dict::DictRef, key: &str) -> Option<u32> {
    props.get(key)?.parse().ok()
}

fn application_process_id_from_properties(props: &spa::utils::dict::DictRef) -> Option<u32> {
    property_pid(props, *pw::keys::APP_PROCESS_ID).or_else(|| {
        props
            .get(*pw::keys::APP_NAME)
            .and_then(cpal_pulseaudio_process_id)
    })
}

fn client_process_id_from_properties(props: &spa::utils::dict::DictRef) -> Option<u32> {
    application_process_id_from_properties(props)
        .or_else(|| property_pid(props, *pw::keys::SEC_PID))
}

fn cpal_pulseaudio_process_id(application_name: &str) -> Option<u32> {
    application_name
        .strip_prefix(CPAL_PULSEAUDIO_APPLICATION_PREFIX)?
        .parse()
        .ok()
}

fn resolve_node_pid(node: OutputNode, clients: &HashMap<u32, u32>) -> Option<u32> {
    node.application_pid
        .or_else(|| node.client_id.and_then(|id| clients.get(&id).copied()))
        .or(node.security_pid)
}

fn process_matches(candidate: u32, target: u32) -> bool {
    if candidate == target {
        return true;
    }

    let mut current = candidate;
    for _ in 0..MAX_PROCESS_ANCESTORS {
        let Ok(status) = fs::read_to_string(format!("/proc/{current}/status")) else {
            return false;
        };
        let Some(parent) = status.lines().find_map(|line| {
            line.strip_prefix("PPid:")
                .and_then(|value| value.trim().parse::<u32>().ok())
        }) else {
            return false;
        };
        if parent == target {
            return true;
        }
        if parent == 0 || parent == current {
            return false;
        }
        current = parent;
    }
    false
}

fn partition_output_nodes(
    state: &RegistryState,
    target_pid: u32,
    mode: AudioProcessMode,
) -> OutputNodePartition {
    let mut captured = Vec::new();
    let mut excluded = Vec::new();
    for (node_id, node) in &state.nodes {
        let Some(pid) = resolve_node_pid(*node, &state.clients) else {
            continue;
        };
        let matches = process_matches(pid, target_pid);
        let should_capture = match mode {
            AudioProcessMode::Include => matches,
            AudioProcessMode::Exclude => !matches,
        };
        if should_capture {
            captured.push((*node_id, pid));
        } else {
            excluded.push((*node_id, pid));
        }
    }
    OutputNodePartition { captured, excluded }
}

fn link_matching_outputs(
    core: &pw::core::CoreRc,
    stream: &pw::stream::StreamRc,
    target_pid: u32,
    mode: AudioProcessMode,
    state: &RefCell<RegistryState>,
) {
    let self_node_id = stream.node_id();
    if self_node_id == 0 || self_node_id == pw::constants::ID_ANY {
        return;
    }

    let (input_ports, targets, excluded) = {
        let state = state.borrow();
        let input_ports = state
            .ports
            .iter()
            .filter(|(_, port)| port.node_id == self_node_id && port.direction == "in")
            .map(|(id, port)| (*id, port.channel.clone()))
            .collect::<Vec<_>>();
        let partition = partition_output_nodes(&state, target_pid, mode);
        (input_ports, partition.captured, partition.excluded)
    };

    // PulseAudio compatibility clients may expose their application identity
    // after the output node. Remove a link as soon as that late identity shows
    // the node belongs to the process that must not be captured.
    for (node_id, pid) in excluded {
        if state.borrow_mut().links.remove(&node_id).is_some() {
            logging::debug(
                "stream",
                format!("PipeWire system audio excluded output node: node_id={node_id} pid={pid}"),
            );
        }
    }
    if input_ports.is_empty() {
        return;
    }

    for (node_id, pid) in targets {
        let output_ports = {
            let state = state.borrow();
            state
                .ports
                .iter()
                .filter(|(_, port)| port.node_id == node_id && port.direction == "out")
                .map(|(id, port)| (*id, port.channel.clone()))
                .collect::<Vec<_>>()
        };
        let mut pairs = pair_audio_ports(&output_ports, &input_ports);
        pairs.sort_unstable();
        if pairs.is_empty() {
            continue;
        }
        if state
            .borrow()
            .links
            .get(&node_id)
            .is_some_and(|linked| linked.pairs == pairs)
        {
            continue;
        }

        // Rebuild only the output whose port topology changed. Clearing every
        // link for unrelated registry events caused audible gaps.
        state.borrow_mut().links.remove(&node_id);

        let mut links = Vec::with_capacity(pairs.len());
        for (output_port, input_port) in &pairs {
            let properties = properties! {
                *pw::keys::LINK_OUTPUT_NODE => node_id.to_string(),
                *pw::keys::LINK_OUTPUT_PORT => output_port.to_string(),
                *pw::keys::LINK_INPUT_NODE => self_node_id.to_string(),
                *pw::keys::LINK_INPUT_PORT => input_port.to_string(),
            };
            let Ok(link) = core.create_object::<pw::link::Link>("link-factory", &properties) else {
                links.clear();
                break;
            };
            links.push(link);
        }
        if links.len() == pairs.len() {
            state.borrow_mut().links.insert(
                node_id,
                LinkedOutput {
                    pairs,
                    _links: links,
                },
            );
            logging::debug(
                "stream",
                format!("PipeWire system audio linked output node: node_id={node_id} pid={pid}"),
            );
        }
    }
}

fn pair_audio_ports(outputs: &[(u32, String)], inputs: &[(u32, String)]) -> Vec<(u32, u32)> {
    if outputs.is_empty() || inputs.is_empty() {
        return Vec::new();
    }
    if outputs.len() == 1 {
        return inputs
            .iter()
            .map(|(input, _)| (outputs[0].0, *input))
            .collect();
    }

    let mut pairs = Vec::new();
    let mut used_inputs = vec![false; inputs.len()];
    for (output, output_channel) in outputs {
        if let Some((index, (input, _))) =
            inputs.iter().enumerate().find(|(index, (_, channel))| {
                !used_inputs[*index] && !output_channel.is_empty() && channel == output_channel
            })
        {
            used_inputs[index] = true;
            pairs.push((*output, *input));
        }
    }
    for (output, _) in outputs {
        if pairs.iter().any(|(paired, _)| paired == output) {
            continue;
        }
        if let Some((index, (input, _))) = inputs
            .iter()
            .enumerate()
            .find(|(index, _)| !used_inputs[*index])
        {
            used_inputs[index] = true;
            pairs.push((*output, *input));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realtime_audio_ring_keeps_capture_backlog_bounded() {
        let ring = AudioSampleRing::new(5);
        let bytes = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        ring.push_bytes(&bytes);
        let mut captured = Vec::new();
        ring.pop_into(&mut captured, 8);

        assert_eq!(captured, vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(ring.dropped_samples(), 2);
        assert_eq!(ring.maximum_push_samples(), 6);
    }

    #[test]
    fn realtime_audio_ring_accepts_a_maximum_pipewire_quantum() {
        let ring = AudioSampleRing::new(AUDIO_SAMPLE_RING_CAPACITY);
        let sample_count = PIPEWIRE_AUDIO_MAX_QUANTUM_FRAMES * SYSTEM_AUDIO_CHANNELS as usize;
        let bytes = vec![0.25f32; sample_count]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        ring.push_bytes(&bytes);
        let mut captured = Vec::new();
        ring.pop_into(&mut captured, sample_count);

        assert_eq!(captured.len(), sample_count);
        assert_eq!(ring.dropped_samples(), 0);
        assert_eq!(ring.maximum_push_samples(), sample_count);
        assert!(AUDIO_SAMPLE_RING_CAPACITY > sample_count * 2);
    }

    #[test]
    fn cpal_pulseaudio_name_recovers_the_application_process_id() {
        assert_eq!(
            cpal_pulseaudio_process_id("cpal-pulseaudio-79983"),
            Some(79_983)
        );
        assert_eq!(cpal_pulseaudio_process_id("cpal-pulseaudio-"), None);
        assert_eq!(cpal_pulseaudio_process_id("cpal-pulseaudio-invalid"), None);
        assert_eq!(cpal_pulseaudio_process_id("another-client-79983"), None);
    }

    #[test]
    fn output_partition_excludes_a_matching_process_after_identity_arrives() {
        let target_pid = std::process::id();
        let mut state = RegistryState::default();
        state.clients.insert(30, target_pid);
        state.nodes.insert(
            10,
            OutputNode {
                client_id: Some(30),
                application_pid: None,
                security_pid: Some(1),
            },
        );
        state.nodes.insert(
            20,
            OutputNode {
                client_id: None,
                application_pid: None,
                security_pid: None,
            },
        );

        let partition = partition_output_nodes(&state, target_pid, AudioProcessMode::Exclude);

        assert!(partition.captured.is_empty());
        assert_eq!(partition.excluded, vec![(10, target_pid)]);
    }
}
