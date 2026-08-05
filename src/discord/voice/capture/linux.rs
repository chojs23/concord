use std::{
    collections::HashMap,
    io,
    os::fd::OwnedFd,
    os::fd::RawFd,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ashpd::desktop::{
    PersistMode, Session,
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as PortalStream,
    },
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

use super::{
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS, send_capture_result,
};
use crate::{
    discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

const FRAME_QUEUE_CAPACITY: usize = 2;
const START_TIMEOUT: Duration = Duration::from_secs(5);
const CANCEL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const DRM_FORMAT_MOD_LINEAR: u64 = 0;
const DMA_BUF_SYNC_READ: u64 = 1 << 0;
const DMA_BUF_SYNC_START: u64 = 0 << 2;
const DMA_BUF_SYNC_END: u64 = 1 << 2;
const DMA_BUF_READY_TIMEOUT_MS: libc::c_int = 1_000;

// Linux's generic _IOW('b', 0, struct dma_buf_sync) encoding. These are the
// architectures supported by Concord's release targets. Other Linux targets
// use the generic encoding unless their kernel ABI overrides it.
#[cfg(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "powerpc",
    target_arch = "powerpc64"
))]
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x8008_6200;
#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips64",
    target_arch = "powerpc",
    target_arch = "powerpc64"
)))]
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;

pub(super) struct CaptureSession {
    stop_tx: pw::channel::Sender<()>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    portal_runtime: tokio::runtime::Runtime,
    portal_session: Option<Session<Screencast>>,
}

struct PipeWireState {
    format: spa::param::video::VideoInfoRaw,
    memory: PipeWireMemory,
    dma_buf_mappings: HashMap<RawFd, DmaBufMapping>,
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
    ready_tx: Option<SyncSender<Result<(), String>>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PipeWireMemory {
    #[default]
    Unknown,
    SharedMemory,
    LinearDmaBuf,
}

struct DmaBufMapping {
    pointer: NonNull<u8>,
    length: usize,
}

impl DmaBufMapping {
    fn new(fd: RawFd) -> Result<Self, String> {
        let length = dma_buf_length(fd)?;
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return Err(format!(
                "PipeWire linear DMA-BUF could not be mapped for CPU conversion: {}",
                io::Error::last_os_error()
            ));
        }
        let Some(pointer) = NonNull::new(pointer.cast::<u8>()) else {
            let _ = unsafe { libc::munmap(pointer, length) };
            return Err("PipeWire linear DMA-BUF mapping returned a null pointer".to_owned());
        };
        Ok(Self { pointer, length })
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), self.length) }
    }
}

impl Drop for DmaBufMapping {
    fn drop(&mut self) {
        if unsafe { libc::munmap(self.pointer.as_ptr().cast(), self.length) } != 0 {
            logging::debug(
                "stream",
                format!(
                    "PipeWire linear DMA-BUF unmap failed: {}",
                    io::Error::last_os_error()
                ),
            );
        }
    }
}

#[repr(C)]
struct DmaBufSync {
    flags: u64,
}

struct DmaBufReadGuard {
    fd: RawFd,
    finished: bool,
}

impl DmaBufReadGuard {
    fn begin(fd: RawFd) -> Result<Self, String> {
        wait_for_dma_buf(fd)?;
        dma_buf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ).map_err(|error| {
            format!("PipeWire DMA-BUF CPU read synchronization failed: {error}")
        })?;
        Ok(Self {
            fd,
            finished: false,
        })
    }

    fn finish(mut self) -> Result<(), String> {
        self.finished = true;
        dma_buf_sync(self.fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ)
            .map_err(|error| format!("PipeWire DMA-BUF CPU read completion failed: {error}"))
    }
}

impl Drop for DmaBufReadGuard {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = dma_buf_sync(self.fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ)
        {
            logging::debug(
                "stream",
                format!("PipeWire DMA-BUF CPU read cleanup failed: {error}"),
            );
        }
    }
}

impl PipeWireState {
    fn report_error(&mut self, error: String) {
        logging::debug(
            "stream",
            format!("PipeWire video capture reported an error: {error}"),
        );
        let _ = self.errors_tx.send(error.clone());
        if let Some(ready_tx) = self.ready_tx.take() {
            let _ = ready_tx.send(Err(error));
        }
    }

    fn queue_frame(&mut self, frame: CaptureFrame) {
        send_capture_result(&self.frames_tx, &self.errors_tx, Ok(frame));
        if let Some(ready_tx) = self.ready_tx.take() {
            logging::debug(
                "stream",
                "PipeWire video capture produced its first valid frame",
            );
            let _ = ready_tx.send(Ok(()));
        }
    }
}

struct PortalCapture {
    session: Session<Screencast>,
    stream: PortalStream,
    remote_fd: OwnedFd,
}

struct PipeWirePortal {
    stream: PortalStream,
    remote_fd: OwnedFd,
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    Ok(vec![StreamCaptureTarget {
        kind: StreamCaptureTargetKind::Portal,
        id: 0,
        title: "Screen or window...".to_owned(),
    }])
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    stop: &AtomicBool,
) -> Result<(CaptureSession, CaptureOutput), String> {
    if target.kind != StreamCaptureTargetKind::Portal {
        return Err("Linux screen sharing requires a portal capture target".to_owned());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("screen cast portal runtime creation failed: {error}"))?;
    let portal = runtime.block_on(open_portal(stop))?;
    let PortalCapture {
        session: portal_session,
        stream,
        remote_fd,
    } = portal;
    let pipewire_portal = PipeWirePortal { stream, remote_fd };

    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let (errors_tx, errors_rx) = mpsc::channel();
    let buffer_pool = CaptureFrameBufferPool::default();
    let (stop_tx, stop_rx) = pw::channel::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let stopping = Arc::new(AtomicBool::new(false));
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::Builder::new()
        .name("stream-pipewire-video".to_owned())
        .spawn(move || {
            let result = run_pipewire_capture(
                pipewire_portal,
                frames_tx.clone(),
                errors_tx.clone(),
                buffer_pool,
                stop_rx,
                ready_tx.clone(),
            );
            match result {
                Ok(()) if worker_stopping.load(Ordering::Acquire) => {
                    logging::debug("stream", "PipeWire video worker stopped on request");
                }
                Ok(()) => {
                    let error = "PipeWire video worker stopped unexpectedly".to_owned();
                    logging::debug("stream", &error);
                    let _ = ready_tx.try_send(Err(error.clone()));
                    let _ = errors_tx.send(error);
                }
                Err(error) if worker_stopping.load(Ordering::Acquire) => {
                    logging::debug(
                        "stream",
                        format!("PipeWire video worker stopped during shutdown: {error}"),
                    );
                }
                Err(error) => {
                    logging::debug("stream", format!("PipeWire video worker failed: {error}"));
                    let _ = ready_tx.try_send(Err(error.clone()));
                    let _ = errors_tx.send(error);
                }
            }
        })
        .map_err(|error| format!("PipeWire video worker spawn failed: {error}"));
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => {
            let _ = runtime.block_on(portal_session.close());
            return Err(error);
        }
    };

    match wait_for_pipewire_start(&ready_rx, stop) {
        Ok(()) => Ok((
            CaptureSession {
                stop_tx,
                stopping,
                worker: Some(worker),
                portal_runtime: runtime,
                portal_session: Some(portal_session),
            },
            CaptureOutput {
                frames: frames_rx,
                errors: errors_rx,
            },
        )),
        Err(error) => {
            stopping.store(true, Ordering::Release);
            let _ = stop_tx.send(());
            let _ = worker.join();
            let _ = runtime.block_on(portal_session.close());
            Err(error)
        }
    }
}

fn wait_for_pipewire_start(
    ready_rx: &Receiver<Result<(), String>>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("PipeWire video capture did not start in time".to_owned());
        }
        let wait = (deadline - now).min(Duration::from_millis(20));
        match ready_rx.recv_timeout(wait) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("PipeWire video capture stopped during startup".to_owned());
            }
        }
    }
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.stopping.store(true, Ordering::Release);
        let _ = self.stop_tx.send(());
        let worker_result = self.worker.take().map_or(Ok(()), |worker| {
            worker
                .join()
                .map_err(|error| format!("PipeWire video worker panicked: {error:?}"))
        });
        let portal_result = self.portal_session.take().map_or(Ok(()), |session| {
            logging::debug("stream", "closing screen cast portal session");
            let result = self
                .portal_runtime
                .block_on(session.close())
                .map_err(|error| format!("screen cast portal session close failed: {error}"));
            if result.is_ok() {
                logging::debug("stream", "screen cast portal session closed");
            }
            result
        });
        worker_result.and(portal_result)
    }
}

async fn open_portal(stop: &AtomicBool) -> Result<PortalCapture, String> {
    let cancellation = wait_for_capture_cancellation(stop);
    tokio::pin!(cancellation);
    logging::debug("stream", "connecting to screen cast portal");
    // The default ashpd proxy caches its D-Bus connection process-wide. Our
    // capture runtime is per session, so that cached connection stops being
    // driven after the first runtime is dropped. Bind a fresh connection to
    // each capture runtime so later broadcasts can open another portal.
    let connection = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = ashpd::zbus::Connection::session() => result
            .map_err(|error| format!("screen cast portal connection failed: {error}"))?,
    };
    let proxy = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = Screencast::with_connection(connection) => result
            .map_err(|error| format!("screen cast portal proxy creation failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal connected");
    let session = tokio::select! {
        _ = &mut cancellation => return Err("screen cast portal selection was cancelled".to_owned()),
        result = proxy.create_session(Default::default()) => result
            .map_err(|error| format!("screen cast portal session creation failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal session created");
    let select_sources = proxy.select_sources(
        &session,
        SelectSourcesOptions::default()
            .set_cursor_mode(CursorMode::Embedded)
            .set_sources(SourceType::Monitor | SourceType::Window)
            .set_multiple(false)
            .set_persist_mode(PersistMode::DoNot),
    );
    tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = select_sources => result
            .map_err(|error| format!("screen cast source selection failed: {error}"))?,
    };
    logging::debug("stream", "waiting for screen cast portal source selection");

    let start = proxy.start(&session, None, Default::default());
    let response = tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = start => result
            .map_err(|error| format!("screen cast portal start request failed: {error}"))?
            .response()
            .map_err(|error| format!("screen cast portal start failed: {error}"))?,
    };
    logging::debug("stream", "screen cast portal source selected");
    let stream = response
        .streams()
        .first()
        .cloned()
        .ok_or_else(|| "screen cast portal returned no selected source".to_owned())?;
    let remote_fd = tokio::select! {
        _ = &mut cancellation => {
            close_cancelled_portal_session(&session).await;
            return Err("screen cast portal selection was cancelled".to_owned());
        }
        result = proxy.open_pipe_wire_remote(&session, Default::default()) => result
            .map_err(|error| format!("screen cast PipeWire remote open failed: {error}"))?,
    };
    logging::debug("stream", "screen cast PipeWire remote opened");

    Ok(PortalCapture {
        session,
        stream,
        remote_fd,
    })
}

async fn wait_for_capture_cancellation(stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn close_cancelled_portal_session(session: &Session<Screencast>) {
    match tokio::time::timeout(CANCEL_CLOSE_TIMEOUT, session.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => logging::debug(
            "stream",
            format!("cancelled screen cast portal session close failed: {error}"),
        ),
        Err(_) => logging::debug(
            "stream",
            "cancelled screen cast portal session close timed out",
        ),
    }
}

fn run_pipewire_capture(
    portal: PipeWirePortal,
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
    stop_rx: pw::channel::Receiver<()>,
    ready_tx: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|error| format!("PipeWire main loop creation failed: {error}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        format!(
            "PipeWire context creation failed: {error}. Ensure the PipeWire client configuration is installed"
        )
    })?;
    let core = context
        .connect_fd_rc(portal.remote_fd, None)
        .map_err(|error| format!("PipeWire portal connection failed: {error}"))?;
    let stream = pw::stream::StreamRc::new(
        core,
        "concord-screen-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|error| format!("PipeWire video stream creation failed: {error}"))?;

    let stop_mainloop = mainloop.clone();
    let _stop_listener = stop_rx.attach(mainloop.loop_(), move |_| stop_mainloop.quit());
    let state_error_mainloop = mainloop.clone();
    let format_error_mainloop = mainloop.clone();
    let frame_error_mainloop = mainloop.clone();
    let state = PipeWireState {
        format: Default::default(),
        memory: PipeWireMemory::Unknown,
        dma_buf_mappings: HashMap::new(),
        frames_tx,
        errors_tx,
        buffer_pool,
        ready_tx: Some(ready_tx.clone()),
    };
    let _stream_listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_, state, old, new| {
            logging::debug(
                "stream",
                format!("PipeWire video stream state changed: {old:?} -> {new:?}"),
            );
            if let pw::stream::StreamState::Error(error) = new {
                state.report_error(format!("PipeWire video stream failed: {error}"));
                state_error_mainloop.quit();
            }
        })
        .param_changed(move |stream, state, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let (media_type, media_subtype) = match spa::param::format_utils::parse_format(param) {
                Ok(format) => format,
                Err(error) => {
                    state.report_error(format!(
                        "PipeWire video media format parse failed: {error}"
                    ));
                    format_error_mainloop.quit();
                    return;
                }
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                state.report_error(format!(
                    "PipeWire negotiated an unsupported media format: {media_type:?}/{media_subtype:?}"
                ));
                format_error_mainloop.quit();
                return;
            }
            match state.format.parse(param) {
                Ok(_) => {
                    let size = state.format.size();
                    let framerate = state.format.framerate();
                    let has_modifier = pipewire_format_has_modifier(param);
                    state.memory = if has_modifier {
                        if state.format.modifier() != DRM_FORMAT_MOD_LINEAR {
                            state.report_error(format!(
                                "PipeWire negotiated unsupported non-linear DMA-BUF modifier: {}",
                                state.format.modifier()
                            ));
                            format_error_mainloop.quit();
                            return;
                        }
                        PipeWireMemory::LinearDmaBuf
                    } else {
                        PipeWireMemory::SharedMemory
                    };
                    state.dma_buf_mappings.clear();
                    logging::debug(
                        "stream",
                        format!(
                            "PipeWire video format negotiated: format={:?} width={} height={} framerate={}/{} memory={:?} modifier={}",
                            state.format.format(),
                            size.width,
                            size.height,
                            framerate.num,
                            framerate.denom,
                            state.memory,
                            state.format.modifier(),
                        ),
                    );
                    let buffer_param = match pipewire_buffer_param(state.format, state.memory) {
                        Ok(param) => param,
                        Err(error) => {
                            state.report_error(error);
                            format_error_mainloop.quit();
                            return;
                        }
                    };
                    let values = match serialize_pipewire_object(
                        buffer_param,
                        "PipeWire video buffer parameter serialization failed",
                    ) {
                        Ok(values) => values,
                        Err(error) => {
                            state.report_error(error);
                            format_error_mainloop.quit();
                            return;
                        }
                    };
                    let mut params = [Pod::from_bytes(&values)
                        .expect("serialized PipeWire video buffer parameter is valid")];
                    if let Err(error) = stream.update_params(&mut params) {
                        state.report_error(format!(
                            "PipeWire video buffer negotiation failed: {error}"
                        ));
                        format_error_mainloop.quit();
                    }
                }
                Err(error) => {
                    state.report_error(format!("PipeWire video format parse failed: {error}"));
                    format_error_mainloop.quit();
                }
            }
        })
        .process(move |stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if state.memory == PipeWireMemory::LinearDmaBuf && datas.len() != 1 {
                state.report_error(format!(
                    "PipeWire linear DMA-BUF has {} planes; packed RGB capture requires one",
                    datas.len()
                ));
                frame_error_mainloop.quit();
                return;
            }
            let Some(data) = datas.first_mut() else {
                return;
            };
            match pipewire_frame(
                data,
                state.format,
                state.memory,
                &mut state.dma_buf_mappings,
                &state.buffer_pool,
            ) {
                Ok(Some(frame)) => state.queue_frame(frame),
                Err(error) => {
                    state.report_error(error);
                    frame_error_mainloop.quit();
                }
                Ok(None) => {}
            }
        })
        .remove_buffer(|_, state, buffer| remove_dma_buf_mapping(state, buffer))
        .register()
        .map_err(|error| format!("PipeWire video listener setup failed: {error}"))?;

    let size = portal.stream.size().unwrap_or((1280, 720));
    let width = u32::try_from(size.0.max(2)).unwrap_or(1280);
    let height = u32::try_from(size.1.max(2)).unwrap_or(720);
    let maximum_width = width.max(8192);
    let maximum_height = height.max(4320);
    let formats = pipewire_format_params(width, height, maximum_width, maximum_height);
    let format_values = formats
        .into_iter()
        .map(|format| {
            serialize_pipewire_object(format, "PipeWire video format serialization failed")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut params = format_values
        .iter()
        .map(|values| Pod::from_bytes(values).expect("serialized PipeWire video format is valid"))
        .collect::<Vec<_>>();
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(portal.stream.pipe_wire_node_id()),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| format!("PipeWire video stream connection failed: {error}"))?;

    logging::debug(
        "stream",
        format!(
            "PipeWire video stream connection requested: node_id={} preferred_size={}x{}",
            portal.stream.pipe_wire_node_id(),
            width,
            height,
        ),
    );
    mainloop.run();
    logging::debug("stream", "PipeWire video main loop stopped");
    let _ = ready_tx.try_send(Err(
        "PipeWire video capture stopped before producing its first frame".to_owned(),
    ));
    Ok(())
}

fn pipewire_format_params(
    width: u32,
    height: u32,
    maximum_width: u32,
    maximum_height: u32,
) -> Vec<spa::pod::Object> {
    let shared_memory = pipewire_format_param(width, height, maximum_width, maximum_height);
    let mut dma_buf = pipewire_format_param(width, height, maximum_width, maximum_height);
    let mut modifier = spa::pod::Property::new(
        spa::param::format::FormatProperties::VideoModifier.as_raw(),
        spa::pod::Value::Choice(spa::pod::ChoiceValue::Long(spa::utils::Choice(
            spa::utils::ChoiceFlags::empty(),
            spa::utils::ChoiceEnum::Enum {
                default: DRM_FORMAT_MOD_LINEAR as i64,
                alternatives: vec![DRM_FORMAT_MOD_LINEAR as i64],
            },
        ))),
    );
    modifier.flags = spa::pod::PropertyFlags::MANDATORY | spa::pod::PropertyFlags::DONT_FIXATE;
    dma_buf.properties.insert(3, modifier);

    // Concord converts frames on the CPU. Prefer mapped shared memory when the
    // producer offers it, while retaining linear DMA-BUF for DMA-only producers.
    vec![shared_memory, dma_buf]
}

fn pipewire_format_param(
    width: u32,
    height: u32,
    maximum_width: u32,
    maximum_height: u32,
) -> spa::pod::Object {
    spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRx,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle { width, height },
            spa::utils::Rectangle {
                width: 2,
                height: 2,
            },
            spa::utils::Rectangle {
                width: maximum_width,
                height: maximum_height,
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: STREAM_CAPTURE_FPS,
                denom: 1,
            },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 360, denom: 1 }
        ),
    )
}

fn serialize_pipewire_object(
    object: spa::pod::Object,
    error_context: &str,
) -> Result<Vec<u8>, String> {
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|serialized| serialized.0.into_inner())
    .map_err(|error| format!("{error_context}: {error}"))
}

fn pipewire_format_has_modifier(param: &Pod) -> bool {
    param
        .as_object()
        .ok()
        .and_then(|object| {
            object.find_prop(spa::utils::Id(
                spa::param::format::FormatProperties::VideoModifier.as_raw(),
            ))
        })
        .is_some()
}

fn pipewire_buffer_param(
    format: spa::param::video::VideoInfoRaw,
    memory: PipeWireMemory,
) -> Result<spa::pod::Object, String> {
    let data_type_mask = match memory {
        PipeWireMemory::SharedMemory => {
            pipewire_data_type_mask(spa::buffer::DataType::MemFd)
                | pipewire_data_type_mask(spa::buffer::DataType::MemPtr)
        }
        PipeWireMemory::LinearDmaBuf => pipewire_data_type_mask(spa::buffer::DataType::DmaBuf),
        PipeWireMemory::Unknown => {
            return Err("PipeWire requested buffers before negotiating a video format".to_owned());
        }
    };
    let mut param = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamBuffers,
        spa::param::ParamType::Buffers,
        spa::pod::Property::new(
            spa::sys::SPA_PARAM_BUFFERS_buffers,
            spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Range {
                    default: 8,
                    min: 2,
                    max: 16,
                },
            ))),
        ),
        spa::pod::Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, spa::pod::Value::Int(1),),
        spa::pod::Property::new(
            spa::sys::SPA_PARAM_BUFFERS_dataType,
            spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Flags {
                    default: data_type_mask,
                    flags: vec![data_type_mask],
                },
            ))),
        ),
    );
    if memory == PipeWireMemory::SharedMemory {
        let stride = format
            .size()
            .width
            .checked_mul(4)
            .and_then(|stride| i32::try_from(stride).ok())
            .ok_or_else(|| "PipeWire video row stride is too large".to_owned())?;
        let size = stride
            .checked_mul(
                i32::try_from(format.size().height)
                    .map_err(|_| "PipeWire video buffer height is too large".to_owned())?,
            )
            .ok_or_else(|| "PipeWire video buffer is too large".to_owned())?;
        param.properties.push(spa::pod::Property::new(
            spa::sys::SPA_PARAM_BUFFERS_size,
            spa::pod::Value::Int(size),
        ));
        param.properties.push(spa::pod::Property::new(
            spa::sys::SPA_PARAM_BUFFERS_stride,
            spa::pod::Value::Int(stride),
        ));
    }
    Ok(param)
}

fn pipewire_data_type_mask(data_type: spa::buffer::DataType) -> i32 {
    1_i32
        .checked_shl(data_type.as_raw())
        .expect("PipeWire data type fits in its negotiated bit mask")
}

fn remove_dma_buf_mapping(state: &mut PipeWireState, buffer: *mut pw::sys::pw_buffer) {
    if buffer.is_null() {
        return;
    }
    let spa_buffer = unsafe { (*buffer).buffer };
    if spa_buffer.is_null() {
        return;
    }
    let data_count = unsafe { (*spa_buffer).n_datas as usize };
    let datas = unsafe { (*spa_buffer).datas };
    if datas.is_null() {
        return;
    }
    for index in 0..data_count {
        let fd = unsafe { (*datas.add(index)).fd as RawFd };
        if fd >= 0 {
            state.dma_buf_mappings.remove(&fd);
        }
    }
}

fn pipewire_frame(
    data: &mut spa::buffer::Data,
    format: spa::param::video::VideoInfoRaw,
    memory: PipeWireMemory,
    dma_buf_mappings: &mut HashMap<RawFd, DmaBufMapping>,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<Option<CaptureFrame>, String> {
    let width = format.size().width;
    let height = format.size().height;
    let video_format = format.format();
    if width == 0 || height == 0 || video_format == spa::param::video::VideoFormat::Unknown {
        return Ok(None);
    }

    let chunk = data.chunk();
    let offset = isize::try_from(chunk.offset())
        .map_err(|_| "PipeWire video frame offset is too large".to_owned())?;
    let stride = if chunk.stride() == 0 {
        isize::try_from(
            width
                .checked_mul(4)
                .ok_or_else(|| "PipeWire video row stride overflowed".to_owned())?,
        )
        .map_err(|_| "PipeWire video row stride is too large".to_owned())?
    } else {
        isize::try_from(chunk.stride())
            .map_err(|_| "PipeWire video row stride is too large".to_owned())?
    };
    let row_length = usize::try_from(
        width
            .checked_mul(4)
            .ok_or_else(|| "PipeWire video row length overflowed".to_owned())?,
    )
    .map_err(|_| "PipeWire video row length is too large".to_owned())?;
    if stride.unsigned_abs() < row_length {
        return Err("PipeWire video frame has an invalid row stride".to_owned());
    }
    let output_length = row_length
        .checked_mul(height as usize)
        .ok_or_else(|| "PipeWire video output buffer is too large".to_owned())?;
    let required_length = pipewire_frame_required_length(offset, stride, row_length, height)?;

    let frame = match (memory, data.type_()) {
        (PipeWireMemory::SharedMemory, spa::buffer::DataType::MemPtr)
        | (PipeWireMemory::SharedMemory, spa::buffer::DataType::MemFd) => {
            let bytes = data
                .data()
                .ok_or_else(|| "PipeWire shared-memory video buffer is not mapped".to_owned())?;
            convert_pipewire_frame(
                bytes,
                required_length,
                offset,
                stride,
                row_length,
                output_length,
                width,
                height,
                video_format,
                buffer_pool,
            )?
        }
        (PipeWireMemory::LinearDmaBuf, spa::buffer::DataType::DmaBuf) => {
            let fd = data.fd();
            if fd < 0 {
                return Err(
                    "PipeWire DMA-BUF video buffer has an invalid file descriptor".to_owned(),
                );
            }
            if let std::collections::hash_map::Entry::Vacant(entry) = dma_buf_mappings.entry(fd) {
                entry.insert(DmaBufMapping::new(fd)?);
            }
            let mapping = dma_buf_mappings
                .get(&fd)
                .expect("newly inserted DMA-BUF mapping is available");
            if required_length > mapping.length {
                return Err(format!(
                    "PipeWire DMA-BUF is shorter than the negotiated frame layout: required={required_length} available={}",
                    mapping.length
                ));
            }
            let sync = DmaBufReadGuard::begin(fd)?;
            let conversion = convert_pipewire_frame(
                mapping.bytes(),
                required_length,
                offset,
                stride,
                row_length,
                output_length,
                width,
                height,
                video_format,
                buffer_pool,
            );
            let sync_result = sync.finish();
            match (conversion, sync_result) {
                (Ok(frame), Ok(())) => frame,
                (Err(error), _) => return Err(error),
                (Ok(_), Err(error)) => return Err(error),
            }
        }
        (PipeWireMemory::Unknown, _) => return Ok(None),
        (expected, actual) => {
            return Err(format!(
                "PipeWire video buffer memory type does not match the negotiated format: expected={expected:?} actual={actual:?}"
            ));
        }
    };

    Ok(Some(frame))
}

#[allow(clippy::too_many_arguments)]
fn convert_pipewire_frame(
    bytes: &[u8],
    required_length: usize,
    offset: isize,
    stride: isize,
    row_length: usize,
    output_length: usize,
    width: u32,
    height: u32,
    video_format: spa::param::video::VideoFormat,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<CaptureFrame, String> {
    if required_length > bytes.len() {
        return Err(format!(
            "PipeWire video frame is shorter than expected: required={required_length} available={}",
            bytes.len()
        ));
    }
    let mut rgba = buffer_pool.take(output_length);

    for row in 0..height as usize {
        let source_offset = offset
            .checked_add(
                stride
                    .checked_mul(row as isize)
                    .ok_or_else(|| "PipeWire video row offset overflowed".to_owned())?,
            )
            .ok_or_else(|| "PipeWire video row offset overflowed".to_owned())?;
        let source_offset = match usize::try_from(source_offset) {
            Ok(offset) => offset,
            Err(_) => {
                return Err("PipeWire video frame has a negative row offset".to_owned());
            }
        };
        let source_end = source_offset
            .checked_add(row_length)
            .ok_or_else(|| "PipeWire video row end overflowed".to_owned())?;
        if source_end > bytes.len() {
            return Err("PipeWire video frame is shorter than expected".to_owned());
        }
        let source = &bytes[source_offset..source_end];
        let destination = &mut rgba[row * row_length..(row + 1) * row_length];
        match video_format {
            spa::param::video::VideoFormat::RGBA => destination.copy_from_slice(source),
            spa::param::video::VideoFormat::RGBx => {
                destination.copy_from_slice(source);
                for alpha in destination.iter_mut().skip(3).step_by(4) {
                    *alpha = 255;
                }
            }
            spa::param::video::VideoFormat::BGRA | spa::param::video::VideoFormat::BGRx => {
                for (source, destination) in
                    source.chunks_exact(4).zip(destination.chunks_exact_mut(4))
                {
                    destination.copy_from_slice(&[
                        source[2],
                        source[1],
                        source[0],
                        if video_format == spa::param::video::VideoFormat::BGRA {
                            source[3]
                        } else {
                            255
                        },
                    ]);
                }
            }
            _ => {
                return Err(format!(
                    "PipeWire negotiated an unsupported video format: {video_format:?}"
                ));
            }
        }
    }

    Ok(CaptureFrame::new(width, height, rgba, buffer_pool.clone()))
}

fn pipewire_frame_required_length(
    offset: isize,
    stride: isize,
    row_length: usize,
    height: u32,
) -> Result<usize, String> {
    let last_row = isize::try_from(height.saturating_sub(1))
        .map_err(|_| "PipeWire video frame height is too large".to_owned())?;
    let last_offset = offset
        .checked_add(
            stride
                .checked_mul(last_row)
                .ok_or_else(|| "PipeWire video frame layout overflowed".to_owned())?,
        )
        .ok_or_else(|| "PipeWire video frame layout overflowed".to_owned())?;
    let first_byte = offset.min(last_offset);
    if first_byte < 0 {
        return Err("PipeWire video frame has a negative row offset".to_owned());
    }
    let final_row = usize::try_from(offset.max(last_offset))
        .map_err(|_| "PipeWire video frame layout is too large".to_owned())?;
    final_row
        .checked_add(row_length)
        .ok_or_else(|| "PipeWire video frame layout is too large".to_owned())
}

fn dma_buf_length(fd: RawFd) -> Result<usize, String> {
    let length = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
    if length < 0 {
        return Err(format!(
            "PipeWire DMA-BUF size lookup failed: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::lseek(fd, 0, libc::SEEK_SET) } < 0 {
        return Err(format!(
            "PipeWire DMA-BUF offset reset failed: {}",
            io::Error::last_os_error()
        ));
    }
    let length =
        usize::try_from(length).map_err(|_| "PipeWire DMA-BUF size is too large".to_owned())?;
    if length == 0 {
        return Err("PipeWire DMA-BUF has zero length".to_owned());
    }
    if length > isize::MAX as usize {
        return Err("PipeWire DMA-BUF is too large to map safely".to_owned());
    }
    Ok(length)
}

fn wait_for_dma_buf(fd: RawFd) -> Result<(), String> {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let result = unsafe { libc::poll(&mut poll_fd, 1, DMA_BUF_READY_TIMEOUT_MS) };
        if result > 0 {
            if poll_fd.revents & libc::POLLNVAL != 0 {
                return Err("PipeWire DMA-BUF became invalid before CPU conversion".to_owned());
            }
            if poll_fd.revents & libc::POLLERR != 0 {
                return Err("PipeWire DMA-BUF reported an error before CPU conversion".to_owned());
            }
            if poll_fd.revents & libc::POLLIN == 0 {
                return Err(format!(
                    "PipeWire DMA-BUF readiness wait returned unexpected flags: {}",
                    poll_fd.revents
                ));
            }
            return Ok(());
        }
        if result == 0 {
            return Err("PipeWire DMA-BUF did not become ready for CPU conversion".to_owned());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(format!("PipeWire DMA-BUF readiness wait failed: {error}"));
    }
}

fn dma_buf_sync(fd: RawFd, flags: u64) -> io::Result<()> {
    let sync = DmaBufSync { flags };
    loop {
        if unsafe { libc::ioctl(fd, DMA_BUF_IOCTL_SYNC, &sync) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(code) if code == libc::EINTR || code == libc::EAGAIN)
        {
            continue;
        }
        return Err(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_portal_wait_observes_capture_cancellation() {
        let stop = AtomicBool::new(true);

        tokio::time::timeout(
            Duration::from_millis(100),
            wait_for_capture_cancellation(&stop),
        )
        .await
        .expect("capture cancellation should wake the portal wait");
    }

    #[test]
    fn first_valid_pipewire_frame_completes_startup_once() {
        let (frames_tx, frames_rx) = mpsc::sync_channel(2);
        let (errors_tx, _errors_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let buffer_pool = CaptureFrameBufferPool::default();
        let mut state = PipeWireState {
            format: Default::default(),
            memory: PipeWireMemory::Unknown,
            dma_buf_mappings: HashMap::new(),
            frames_tx,
            errors_tx,
            buffer_pool: buffer_pool.clone(),
            ready_tx: Some(ready_tx),
        };

        state.queue_frame(CaptureFrame::new(
            1,
            1,
            vec![1, 2, 3, 255],
            buffer_pool.clone(),
        ));
        assert_eq!(
            ready_rx
                .recv()
                .expect("first frame should report readiness"),
            Ok(())
        );
        assert_eq!(
            frames_rx
                .recv()
                .expect("first frame should remain queued")
                .rgba,
            vec![1, 2, 3, 255]
        );

        state.queue_frame(CaptureFrame::new(1, 1, vec![4, 5, 6, 255], buffer_pool));
        assert!(matches!(
            ready_rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn pipewire_formats_prefer_shared_memory_with_linear_dma_buf_fallback() {
        let formats = pipewire_format_params(1920, 1080, 8192, 4320);

        assert_eq!(formats.len(), 2);
        assert!(formats[0].properties.iter().all(|property| {
            property.key != spa::param::format::FormatProperties::VideoModifier.as_raw()
        }));
        let modifier = formats[1]
            .properties
            .iter()
            .find(|property| {
                property.key == spa::param::format::FormatProperties::VideoModifier.as_raw()
            })
            .expect("second format should advertise a DMA-BUF modifier");
        assert!(modifier.flags.contains(spa::pod::PropertyFlags::MANDATORY));
        assert!(
            modifier
                .flags
                .contains(spa::pod::PropertyFlags::DONT_FIXATE)
        );
        assert_eq!(
            modifier.value,
            spa::pod::Value::Choice(spa::pod::ChoiceValue::Long(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Enum {
                    default: DRM_FORMAT_MOD_LINEAR as i64,
                    alternatives: vec![DRM_FORMAT_MOD_LINEAR as i64],
                },
            )))
        );
        for (format, expected) in formats.into_iter().zip([false, true]) {
            let values = serialize_pipewire_object(format, "test format serialization failed")
                .expect("test format should serialize");
            let param = Pod::from_bytes(&values).expect("serialized test format should be valid");
            assert_eq!(pipewire_format_has_modifier(param), expected);
        }
    }

    #[test]
    fn pipewire_buffer_types_follow_the_negotiated_memory_path() {
        let mut format = spa::param::video::VideoInfoRaw::new();
        format.set_size(spa::utils::Rectangle {
            width: 1920,
            height: 1080,
        });

        for (memory, expected_mask) in [
            (
                PipeWireMemory::SharedMemory,
                pipewire_data_type_mask(spa::buffer::DataType::MemFd)
                    | pipewire_data_type_mask(spa::buffer::DataType::MemPtr),
            ),
            (
                PipeWireMemory::LinearDmaBuf,
                pipewire_data_type_mask(spa::buffer::DataType::DmaBuf),
            ),
        ] {
            let param = pipewire_buffer_param(format, memory)
                .expect("negotiated memory should produce a buffer parameter");
            let data_type = param
                .properties
                .iter()
                .find(|property| property.key == spa::sys::SPA_PARAM_BUFFERS_dataType)
                .expect("buffer parameter should declare a memory type");
            assert_eq!(
                data_type.value,
                spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                    spa::utils::ChoiceFlags::empty(),
                    spa::utils::ChoiceEnum::Flags {
                        default: expected_mask,
                        flags: vec![expected_mask],
                    },
                )))
            );
        }
    }

    #[test]
    fn pipewire_frame_layout_accounts_for_positive_and_negative_stride() {
        assert_eq!(
            pipewire_frame_required_length(0, 16, 8, 3)
                .expect("positive stride layout should be valid"),
            40
        );
        assert_eq!(
            pipewire_frame_required_length(32, -16, 8, 3)
                .expect("negative stride layout should be valid"),
            40
        );
        assert!(pipewire_frame_required_length(0, -16, 8, 3).is_err());
    }
}
