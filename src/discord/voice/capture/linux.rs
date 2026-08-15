use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    os::fd::{OwnedFd, RawFd},
    rc::Rc,
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
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS,
    conversion::{
        CaptureColorInfo, CaptureColorMatrix, CaptureColorPrimaries, CaptureColorRange,
        CapturePixelFormat, CapturePlane, CaptureTransferFunction, convert_capture_frame,
    },
    send_capture_result,
};
use crate::{
    discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

#[path = "linux/dmabuf.rs"]
mod dmabuf;
use dmabuf::{
    DRM_FORMAT_MOD_LINEAR, DmaBufMapping, DmaBufPlane, DmaBufReadGuard, EglDmaBufImporter,
    finish_dma_buf_reads,
};

const FRAME_QUEUE_CAPACITY: usize = 2;
const START_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_FRAME_BUDGET: Duration =
    Duration::from_nanos(1_000_000_000 / STREAM_CAPTURE_FPS as u64);
const CANCEL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

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
    dma_buf_importer: Option<EglDmaBufImporter>,
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
    DmaBuf,
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
    let stop_mainloop = mainloop.clone();
    let _stop_listener = stop_rx.attach(mainloop.loop_(), move |_| stop_mainloop.quit());

    let size = portal.stream.size().unwrap_or((1280, 720));
    let width = u32::try_from(size.0.max(2)).unwrap_or(1280);
    let height = u32::try_from(size.1.max(2)).unwrap_or(720);
    let maximum_width = width.max(8192);
    let maximum_height = height.max(4320);
    let node_id = portal.stream.pipe_wire_node_id();
    let mut include_dma_buf = true;

    loop {
        match run_pipewire_stream_attempt(
            core.clone(),
            &mainloop,
            node_id,
            width,
            height,
            maximum_width,
            maximum_height,
            include_dma_buf,
            frames_tx.clone(),
            errors_tx.clone(),
            buffer_pool.clone(),
            ready_tx.clone(),
        ) {
            Ok(PipeWireAttemptResult::SharedMemoryFallback) if include_dma_buf => {
                include_dma_buf = false;
                logging::debug(
                    "stream",
                    "PipeWire DMA-BUF capture failed; restarting with shared memory only",
                );
            }
            Ok(PipeWireAttemptResult::SharedMemoryFallback) => {
                return Err("PipeWire shared-memory fallback requested twice".to_owned());
            }
            Ok(PipeWireAttemptResult::Stopped) => break,
            Err(error) if include_dma_buf => {
                include_dma_buf = false;
                logging::debug(
                    "stream",
                    format!(
                        "PipeWire capture attempt failed before streaming; retrying with shared memory only: {error}"
                    ),
                );
            }
            Err(error) => return Err(error),
        }
    }

    logging::debug("stream", "PipeWire video main loop stopped");
    let _ = ready_tx.try_send(Err(
        "PipeWire video capture stopped before producing its first frame".to_owned(),
    ));
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PipeWireAttemptResult {
    #[default]
    Stopped,
    SharedMemoryFallback,
}

#[allow(clippy::too_many_arguments)]
fn run_pipewire_stream_attempt(
    core: pw::core::CoreRc,
    mainloop: &pw::main_loop::MainLoopRc,
    node_id: u32,
    width: u32,
    height: u32,
    maximum_width: u32,
    maximum_height: u32,
    include_dma_buf: bool,
    frames_tx: SyncSender<CaptureFrame>,
    errors_tx: Sender<String>,
    buffer_pool: CaptureFrameBufferPool,
    ready_tx: SyncSender<Result<(), String>>,
) -> Result<PipeWireAttemptResult, String> {
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

    let outcome = Rc::new(Cell::new(PipeWireAttemptResult::Stopped));
    let state_error_mainloop = mainloop.clone();
    let format_error_mainloop = mainloop.clone();
    let frame_error_mainloop = mainloop.clone();
    let state_outcome = outcome.clone();
    let format_outcome = outcome.clone();
    let frame_outcome = outcome.clone();
    let dma_buf_importer = if include_dma_buf {
        match EglDmaBufImporter::new() {
            Ok(importer) => Some(importer),
            Err(error) => {
                logging::debug(
                    "stream",
                    format!(
                        "EGL DMA-BUF import is unavailable; keeping linear DMA-BUF and shared-memory capture: {error}"
                    ),
                );
                None
            }
        }
    } else {
        None
    };
    let non_linear_formats = dma_buf_importer
        .as_ref()
        .map(non_linear_dma_buf_formats)
        .unwrap_or_default();
    let state = PipeWireState {
        format: Default::default(),
        memory: PipeWireMemory::Unknown,
        dma_buf_mappings: HashMap::new(),
        dma_buf_importer,
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
                if include_dma_buf {
                    state_outcome.set(PipeWireAttemptResult::SharedMemoryFallback);
                    logging::debug(
                        "stream",
                        format!(
                            "PipeWire stream failed while DMA-BUF was enabled; requesting shared-memory fallback: {error}"
                        ),
                    );
                } else {
                    state.report_error(format!("PipeWire video stream failed: {error}"));
                }
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
                        let capture_format = match capture_pixel_format(state.format.format()) {
                            Ok(format) => format,
                            Err(error) => {
                                state.report_error(error);
                                format_error_mainloop.quit();
                                return;
                            }
                        };
                        if state.format.modifier() != DRM_FORMAT_MOD_LINEAR
                            && !state.dma_buf_importer.as_ref().is_some_and(|importer| {
                                importer.supports(capture_format, state.format.modifier())
                            })
                        {
                            format_outcome.set(PipeWireAttemptResult::SharedMemoryFallback);
                            logging::debug(
                                "stream",
                                format!(
                                    "PipeWire negotiated unsupported non-linear DMA-BUF modifier {}; requesting shared-memory fallback",
                                    state.format.modifier()
                                ),
                            );
                            format_error_mainloop.quit();
                            return;
                        }
                        PipeWireMemory::DmaBuf
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
            if datas.is_empty() {
                return;
            }
            let started_at = Instant::now();
            let frame = pipewire_frame(
                datas,
                state.format,
                state.memory,
                &mut state.dma_buf_mappings,
                state.dma_buf_importer.as_mut(),
                &state.buffer_pool,
            );
            // PipeWire only gets this buffer back when the callback returns, so
            // an overrun here starves the compositor instead of just dropping a
            // frame. Without this line the only symptom is a readiness timeout
            // and buffer warnings in the compositor's own log.
            let elapsed = started_at.elapsed();
            if elapsed > PROCESS_FRAME_BUDGET {
                logging::debug(
                    "stream",
                    format!(
                        "PipeWire frame processing overran its budget: memory={:?} format={:?} modifier={} size={}x{} elapsed_ms={}",
                        state.memory,
                        state.format.format(),
                        state.format.modifier(),
                        state.format.size().width,
                        state.format.size().height,
                        elapsed.as_millis(),
                    ),
                );
            }
            match frame {
                Ok(Some(frame)) => state.queue_frame(frame),
                Err(error) => {
                    if include_dma_buf && state.memory == PipeWireMemory::DmaBuf {
                        frame_outcome.set(PipeWireAttemptResult::SharedMemoryFallback);
                        logging::debug(
                            "stream",
                            format!(
                                "PipeWire DMA-BUF frame processing failed; requesting shared-memory fallback: {error}"
                            ),
                        );
                    } else {
                        state.report_error(error);
                    }
                    frame_error_mainloop.quit();
                }
                Ok(None) => {}
            }
        })
        .remove_buffer(|_, state, buffer| remove_dma_buf_mapping(state, buffer))
        .register()
        .map_err(|error| format!("PipeWire video listener setup failed: {error}"))?;

    let formats = pipewire_format_params(
        width,
        height,
        maximum_width,
        maximum_height,
        include_dma_buf,
        &non_linear_formats,
    );
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
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| format!("PipeWire video stream connection failed: {error}"))?;

    logging::debug(
        "stream",
        format!(
            "PipeWire video stream connection requested: node_id={} preferred_size={}x{}",
            node_id, width, height,
        ),
    );
    mainloop.run();
    Ok(outcome.get())
}

fn pipewire_format_params(
    width: u32,
    height: u32,
    maximum_width: u32,
    maximum_height: u32,
    include_dma_buf: bool,
    non_linear_formats: &[(spa::param::video::VideoFormat, Vec<u64>)],
) -> Vec<spa::pod::Object> {
    let shared_memory = pipewire_format_param(width, height, maximum_width, maximum_height);
    if !include_dma_buf {
        return vec![shared_memory];
    }
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

    // Concord converts frames on the CPU. Prefer mapped shared memory, retain
    // linear DMA-BUF, then add only the non-linear pairs verified by EGL.
    let mut formats = vec![shared_memory, dma_buf];
    formats.extend(non_linear_formats.iter().filter_map(|(format, modifiers)| {
        // The parameter above already offers linear for every format, so
        // repeating it per format here would only duplicate that offer.
        let modifiers = modifiers
            .iter()
            .copied()
            .filter(|modifier| *modifier != DRM_FORMAT_MOD_LINEAR)
            .collect::<Vec<_>>();
        (!modifiers.is_empty()).then(|| {
            pipewire_non_linear_format_param(
                width,
                height,
                maximum_width,
                maximum_height,
                *format,
                &modifiers,
            )
        })
    }));
    formats
}

fn non_linear_dma_buf_formats(
    importer: &EglDmaBufImporter,
) -> Vec<(spa::param::video::VideoFormat, Vec<u64>)> {
    [
        (
            spa::param::video::VideoFormat::RGBA,
            CapturePixelFormat::Rgba,
        ),
        (
            spa::param::video::VideoFormat::RGBx,
            CapturePixelFormat::Rgbx,
        ),
        (
            spa::param::video::VideoFormat::BGRA,
            CapturePixelFormat::Bgra,
        ),
        (
            spa::param::video::VideoFormat::BGRx,
            CapturePixelFormat::Bgrx,
        ),
        (
            spa::param::video::VideoFormat::xRGB_210LE,
            CapturePixelFormat::Xrgb210Le,
        ),
        (
            spa::param::video::VideoFormat::xBGR_210LE,
            CapturePixelFormat::Xbgr210Le,
        ),
        (
            spa::param::video::VideoFormat::RGBx_102LE,
            CapturePixelFormat::Rgbx102Le,
        ),
        (
            spa::param::video::VideoFormat::BGRx_102LE,
            CapturePixelFormat::Bgrx102Le,
        ),
    ]
    .into_iter()
    .filter_map(|(video_format, capture_format)| {
        let modifiers = importer.modifiers(capture_format);
        (!modifiers.is_empty()).then(|| (video_format, modifiers.to_vec()))
    })
    .collect()
}

fn pipewire_non_linear_format_param(
    width: u32,
    height: u32,
    maximum_width: u32,
    maximum_height: u32,
    format: spa::param::video::VideoFormat,
    modifiers: &[u64],
) -> spa::pod::Object {
    let mut param = pipewire_format_param(width, height, maximum_width, maximum_height);
    let format_property = param
        .properties
        .iter_mut()
        .find(|property| property.key == spa::param::format::FormatProperties::VideoFormat.as_raw())
        .expect("PipeWire raw format parameter contains a video format");
    format_property.value = spa::pod::Value::Id(spa::utils::Id(format.as_raw()));

    let modifier_values = modifiers
        .iter()
        .copied()
        .map(|modifier| modifier as i64)
        .collect::<Vec<_>>();
    let mut modifier = spa::pod::Property::new(
        spa::param::format::FormatProperties::VideoModifier.as_raw(),
        spa::pod::Value::Choice(spa::pod::ChoiceValue::Long(spa::utils::Choice(
            spa::utils::ChoiceFlags::empty(),
            spa::utils::ChoiceEnum::Enum {
                default: modifier_values[0],
                alternatives: modifier_values,
            },
        ))),
    );
    modifier.flags = spa::pod::PropertyFlags::MANDATORY | spa::pod::PropertyFlags::DONT_FIXATE;
    param.properties.push(modifier);
    param
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
            spa::param::video::VideoFormat::xRGB_210LE,
            spa::param::video::VideoFormat::xBGR_210LE,
            spa::param::video::VideoFormat::RGBx_102LE,
            spa::param::video::VideoFormat::BGRx_102LE,
            spa::param::video::VideoFormat::NV12,
            spa::param::video::VideoFormat::P010_10LE,
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
    let capture_format = capture_pixel_format(format.format())?;
    let plane_count = i32::try_from(capture_format.plane_count())
        .map_err(|_| "PipeWire video plane count is too large".to_owned())?;
    let data_type_mask = match memory {
        PipeWireMemory::SharedMemory => {
            pipewire_data_type_mask(spa::buffer::DataType::MemFd)
                | pipewire_data_type_mask(spa::buffer::DataType::MemPtr)
        }
        PipeWireMemory::DmaBuf => pipewire_data_type_mask(spa::buffer::DataType::DmaBuf),
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
        spa::pod::Property::new(
            spa::sys::SPA_PARAM_BUFFERS_blocks,
            spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Range {
                    default: plane_count,
                    min: 1,
                    max: plane_count,
                },
            ))),
        ),
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
        let row_lengths = capture_format.plane_row_lengths(format.size().width)?;
        let plane_heights = capture_format.plane_heights(format.size().height);
        let stride_length = row_lengths
            .iter()
            .copied()
            .max()
            .expect("capture formats contain at least one plane");
        let stride = i32::try_from(stride_length)
            .map_err(|_| "PipeWire video row stride is too large".to_owned())?;
        let size = plane_heights
            .into_iter()
            .try_fold(0_usize, |size, plane_height| {
                stride_length
                    .checked_mul(plane_height as usize)
                    .and_then(|plane_size| size.checked_add(plane_size))
                    .ok_or_else(|| "PipeWire video buffer is too large".to_owned())
            })
            .and_then(|size| {
                i32::try_from(size).map_err(|_| "PipeWire video buffer is too large".to_owned())
            })?;
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

fn capture_pixel_format(
    format: spa::param::video::VideoFormat,
) -> Result<CapturePixelFormat, String> {
    match format {
        spa::param::video::VideoFormat::RGBA => Ok(CapturePixelFormat::Rgba),
        spa::param::video::VideoFormat::RGBx => Ok(CapturePixelFormat::Rgbx),
        spa::param::video::VideoFormat::BGRA => Ok(CapturePixelFormat::Bgra),
        spa::param::video::VideoFormat::BGRx => Ok(CapturePixelFormat::Bgrx),
        spa::param::video::VideoFormat::xRGB_210LE => Ok(CapturePixelFormat::Xrgb210Le),
        spa::param::video::VideoFormat::xBGR_210LE => Ok(CapturePixelFormat::Xbgr210Le),
        spa::param::video::VideoFormat::RGBx_102LE => Ok(CapturePixelFormat::Rgbx102Le),
        spa::param::video::VideoFormat::BGRx_102LE => Ok(CapturePixelFormat::Bgrx102Le),
        spa::param::video::VideoFormat::NV12 => Ok(CapturePixelFormat::Nv12),
        spa::param::video::VideoFormat::P010_10LE => Ok(CapturePixelFormat::P010Le),
        _ => Err(format!(
            "PipeWire negotiated an unsupported video format: {format:?}"
        )),
    }
}

fn capture_color_info(format: spa::param::video::VideoInfoRaw) -> CaptureColorInfo {
    // libspa exposes these ABI-stable C enum values as raw integers.
    let range = match format.color_range() {
        1 => CaptureColorRange::Full,
        2 => CaptureColorRange::Limited,
        _ => CaptureColorRange::Unknown,
    };
    let matrix = match format.color_matrix() {
        3 => CaptureColorMatrix::Bt709,
        4 => CaptureColorMatrix::Bt601,
        6 => CaptureColorMatrix::Bt2020,
        _ => CaptureColorMatrix::Unknown,
    };
    let transfer = match format.transfer_function() {
        5 => CaptureTransferFunction::Bt709,
        7 => CaptureTransferFunction::Srgb,
        13 => CaptureTransferFunction::Bt2020Ten,
        14 => CaptureTransferFunction::Pq,
        15 => CaptureTransferFunction::Hlg,
        _ => CaptureTransferFunction::Unknown,
    };
    let primaries = match format.color_primaries() {
        1 => CaptureColorPrimaries::Bt709,
        7 => CaptureColorPrimaries::Bt2020,
        _ => CaptureColorPrimaries::Unknown,
    };
    CaptureColorInfo {
        range,
        matrix,
        transfer,
        primaries,
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PipeWirePlaneLayout {
    data_index: usize,
    offset: isize,
    stride: isize,
}

fn pipewire_frame(
    datas: &mut [spa::buffer::Data],
    format: spa::param::video::VideoInfoRaw,
    memory: PipeWireMemory,
    dma_buf_mappings: &mut HashMap<RawFd, DmaBufMapping>,
    dma_buf_importer: Option<&mut EglDmaBufImporter>,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<Option<CaptureFrame>, String> {
    let width = format.size().width;
    let height = format.size().height;
    let video_format = format.format();
    if width == 0 || height == 0 || video_format == spa::param::video::VideoFormat::Unknown {
        return Ok(None);
    }
    let capture_format = capture_pixel_format(video_format)?;
    let color = capture_color_info(format);
    let layouts = pipewire_plane_layouts(datas, capture_format, width, height)?;

    let frame = match memory {
        PipeWireMemory::SharedMemory => {
            let mut sources = Vec::with_capacity(datas.len());
            for data in datas {
                if !matches!(
                    data.type_(),
                    spa::buffer::DataType::MemPtr | spa::buffer::DataType::MemFd
                ) {
                    return Err(format!(
                        "PipeWire shared-memory buffer has an unexpected data type: {:?}",
                        data.type_()
                    ));
                }
                sources.push(
                    data.data().ok_or_else(|| {
                        "PipeWire shared-memory video buffer is not mapped".to_owned()
                    })? as &[u8],
                );
            }
            convert_pipewire_planes(
                &sources,
                &layouts,
                width,
                height,
                capture_format,
                color,
                buffer_pool,
            )?
        }
        PipeWireMemory::DmaBuf => {
            let mut fds = Vec::with_capacity(datas.len());
            for data in datas.iter() {
                if data.type_() != spa::buffer::DataType::DmaBuf {
                    return Err(format!(
                        "PipeWire DMA-BUF buffer has an unexpected data type: {:?}",
                        data.type_()
                    ));
                }
                let fd = data.fd();
                if fd < 0 {
                    return Err(
                        "PipeWire DMA-BUF video buffer has an invalid file descriptor".to_owned(),
                    );
                }
                fds.push(fd);
            }
            let dma_layouts = dma_buf_plane_layouts(datas, &layouts)?;

            // Let the GPU do the copy whenever EGL can import the buffer,
            // linear included. Reading a DMA-BUF with the CPU runs at roughly
            // 25MB/s on radeonsi, which is ~640ms for one 1440p frame, so the
            // mapped path below is a last resort for hosts without EGL.
            let importer = dma_buf_importer
                .filter(|importer| importer.supports(capture_format, format.modifier()));
            if let Some(importer) = importer {
                // EGL DMA-BUF import uses the driver's implicit synchronization.
                // TODO: Negotiate SPA_META_SyncTimeline when Concord can require
                // PipeWire 1.2 instead of the current 0.3.33-compatible ABI.
                let planes = dma_layouts
                    .iter()
                    .map(|layout| {
                        let pitch = u32::try_from(layout.stride).map_err(|_| {
                            "EGL DMA-BUF import requires a positive plane stride".to_owned()
                        })?;
                        let offset = u32::try_from(layout.offset)
                            .map_err(|_| "EGL DMA-BUF plane offset is too large".to_owned())?;
                        Ok(DmaBufPlane {
                            fd: fds[layout.data_index],
                            offset,
                            pitch,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let output_length = width
                    .checked_mul(height)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .and_then(|length| usize::try_from(length).ok())
                    .ok_or_else(|| "PipeWire video output buffer is too large".to_owned())?;
                let mut rgba = buffer_pool.take(output_length);
                importer.import(
                    width,
                    height,
                    capture_format,
                    format.modifier(),
                    color,
                    &planes,
                    &mut rgba,
                )?;
                CaptureFrame::new(width, height, rgba, buffer_pool.clone())
            } else if format.modifier() != DRM_FORMAT_MOD_LINEAR {
                // Only a linear buffer has a layout the CPU can read directly.
                return Err(
                    "PipeWire negotiated non-linear DMA-BUF without an EGL importer".to_owned(),
                );
            } else {
                for fd in fds.iter().copied() {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        dma_buf_mappings.entry(fd)
                    {
                        entry.insert(DmaBufMapping::new(fd)?);
                    }
                }

                let mut synchronized = Vec::new();
                let mut unique_fds = HashSet::new();
                for fd in fds.iter().copied() {
                    if unique_fds.insert(fd) {
                        synchronized.push(DmaBufReadGuard::begin(fd)?);
                    }
                }
                for fd in &unique_fds {
                    dma_buf_mappings
                        .get_mut(fd)
                        .expect("validated DMA-BUF mapping is available")
                        .refresh();
                }
                let sources = fds
                    .iter()
                    .map(|fd| {
                        dma_buf_mappings
                            .get(fd)
                            .expect("validated DMA-BUF mapping is available")
                            .bytes()
                    })
                    .collect::<Vec<_>>();
                let conversion = convert_pipewire_planes(
                    &sources,
                    &dma_layouts,
                    width,
                    height,
                    capture_format,
                    color,
                    buffer_pool,
                );
                let synchronization = finish_dma_buf_reads(synchronized);
                match (conversion, synchronization) {
                    (Ok(frame), Ok(())) => frame,
                    (Err(error), _) | (Ok(_), Err(error)) => return Err(error),
                }
            }
        }
        PipeWireMemory::Unknown => return Ok(None),
    };

    Ok(Some(frame))
}

fn dma_buf_plane_layouts(
    datas: &[spa::buffer::Data],
    layouts: &[PipeWirePlaneLayout],
) -> Result<Vec<PipeWirePlaneLayout>, String> {
    layouts
        .iter()
        .map(|layout| {
            let map_offset = isize::try_from(datas[layout.data_index].as_raw().mapoffset)
                .map_err(|_| "PipeWire DMA-BUF map offset is too large".to_owned())?;
            let offset = layout
                .offset
                .checked_add(map_offset)
                .ok_or_else(|| "PipeWire DMA-BUF plane offset overflowed".to_owned())?;
            Ok(PipeWirePlaneLayout { offset, ..*layout })
        })
        .collect()
}

fn pipewire_plane_layouts(
    datas: &[spa::buffer::Data],
    format: CapturePixelFormat,
    width: u32,
    height: u32,
) -> Result<Vec<PipeWirePlaneLayout>, String> {
    let plane_count = format.plane_count();
    if datas.len() != plane_count && !(datas.len() == 1 && plane_count == 2) {
        return Err(format!(
            "PipeWire video format {format:?} requires {plane_count} image planes, received {}",
            datas.len()
        ));
    }
    let row_lengths = format.plane_row_lengths(width)?;
    let plane_heights = format.plane_heights(height);
    let mut layouts = Vec::with_capacity(plane_count);
    for (plane, data) in datas.iter().enumerate() {
        let row_length = row_lengths[plane];
        let chunk = data.chunk();
        let offset = isize::try_from(chunk.offset())
            .map_err(|_| "PipeWire video frame offset is too large".to_owned())?;
        let default_stride = if datas.len() == 1 && plane_count > 1 {
            row_lengths
                .iter()
                .copied()
                .max()
                .expect("capture formats contain at least one plane")
        } else {
            row_length
        };
        let stride = if chunk.stride() == 0 {
            isize::try_from(default_stride)
                .map_err(|_| "PipeWire video row stride is too large".to_owned())?
        } else {
            isize::try_from(chunk.stride())
                .map_err(|_| "PipeWire video row stride is too large".to_owned())?
        };
        pipewire_frame_required_length(offset, stride, row_length, plane_heights[plane])?;
        layouts.push(PipeWirePlaneLayout {
            data_index: plane,
            offset,
            stride,
        });
    }

    if datas.len() == 1 && plane_count == 2 {
        let first = layouts[0];
        if first.stride < 0 {
            return Err(
                "PipeWire contiguous planar video does not support a negative stride".to_owned(),
            );
        }
        let second_offset = first
            .stride
            .checked_mul(
                isize::try_from(plane_heights[0])
                    .map_err(|_| "PipeWire video plane height is too large".to_owned())?,
            )
            .and_then(|size| first.offset.checked_add(size))
            .ok_or_else(|| "PipeWire video plane offset overflowed".to_owned())?;
        pipewire_frame_required_length(
            second_offset,
            first.stride,
            row_lengths[1],
            plane_heights[1],
        )?;
        layouts.push(PipeWirePlaneLayout {
            data_index: 0,
            offset: second_offset,
            stride: first.stride,
        });
    }

    Ok(layouts)
}

#[allow(clippy::too_many_arguments)]
fn convert_pipewire_planes(
    sources: &[&[u8]],
    layouts: &[PipeWirePlaneLayout],
    width: u32,
    height: u32,
    format: CapturePixelFormat,
    color: CaptureColorInfo,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<CaptureFrame, String> {
    let output_length = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| "PipeWire video output buffer is too large".to_owned())?;
    let planes = layouts
        .iter()
        .map(|layout| CapturePlane {
            bytes: sources[layout.data_index],
            offset: layout.offset,
            stride: layout.stride,
        })
        .collect::<Vec<_>>();
    let mut rgba = buffer_pool.take(output_length);
    convert_capture_frame(&planes, width, height, format, color, &mut rgba)?;
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
            dma_buf_importer: None,
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
        let formats = pipewire_format_params(1920, 1080, 8192, 4320, true, &[]);

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

        let shared_memory_only = pipewire_format_params(1920, 1080, 8192, 4320, false, &[]);
        assert_eq!(shared_memory_only.len(), 1);
        assert!(shared_memory_only[0].properties.iter().all(|property| {
            property.key != spa::param::format::FormatProperties::VideoModifier.as_raw()
        }));
    }

    #[test]
    fn pipewire_formats_advertise_only_egl_verified_non_linear_pairs() {
        let modifiers = [
            (
                spa::param::video::VideoFormat::RGBA,
                vec![
                    DRM_FORMAT_MOD_LINEAR,
                    0x0102_0304_0506_0708,
                    0x1112_1314_1516_1718,
                ],
            ),
            // EGL reports linear for every format, but the linear parameter
            // already covers them all, so this pair contributes nothing.
            (
                spa::param::video::VideoFormat::BGRx,
                vec![DRM_FORMAT_MOD_LINEAR],
            ),
        ];
        let formats = pipewire_format_params(1920, 1080, 8192, 4320, true, &modifiers);

        assert_eq!(formats.len(), 3);
        let video_format = formats[2]
            .properties
            .iter()
            .find(|property| {
                property.key == spa::param::format::FormatProperties::VideoFormat.as_raw()
            })
            .expect("non-linear format should fix the video format");
        assert_eq!(
            video_format.value,
            spa::pod::Value::Id(spa::utils::Id(
                spa::param::video::VideoFormat::RGBA.as_raw()
            ))
        );
        let modifier = formats[2]
            .properties
            .iter()
            .find(|property| {
                property.key == spa::param::format::FormatProperties::VideoModifier.as_raw()
            })
            .expect("non-linear format should contain EGL modifiers");
        assert_eq!(
            modifier.value,
            spa::pod::Value::Choice(spa::pod::ChoiceValue::Long(spa::utils::Choice(
                spa::utils::ChoiceFlags::empty(),
                spa::utils::ChoiceEnum::Enum {
                    default: 0x0102_0304_0506_0708,
                    alternatives: vec![0x0102_0304_0506_0708, 0x1112_1314_1516_1718],
                },
            )))
        );
    }

    #[test]
    fn pipewire_buffer_types_follow_the_negotiated_memory_path() {
        let mut format = spa::param::video::VideoInfoRaw::new();
        format.set_format(spa::param::video::VideoFormat::RGBA);
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
                PipeWireMemory::DmaBuf,
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
    fn pipewire_buffer_layout_matches_each_formats_image_planes() {
        let cases = [
            (spa::param::video::VideoFormat::RGBA, 2, 2, 1, 8, 16),
            (spa::param::video::VideoFormat::NV12, 3, 3, 2, 4, 20),
            (spa::param::video::VideoFormat::P010_10LE, 3, 3, 2, 8, 40),
        ];

        for (video_format, width, height, plane_count, stride, size) in cases {
            let mut format = spa::param::video::VideoInfoRaw::new();
            format.set_format(video_format);
            format.set_size(spa::utils::Rectangle { width, height });
            let param = pipewire_buffer_param(format, PipeWireMemory::SharedMemory)
                .expect("supported format should produce a buffer parameter");

            let blocks = param
                .properties
                .iter()
                .find(|property| property.key == spa::sys::SPA_PARAM_BUFFERS_blocks)
                .expect("buffer parameter should declare plane blocks");
            assert_eq!(
                blocks.value,
                spa::pod::Value::Choice(spa::pod::ChoiceValue::Int(spa::utils::Choice(
                    spa::utils::ChoiceFlags::empty(),
                    spa::utils::ChoiceEnum::Range {
                        default: plane_count,
                        min: 1,
                        max: plane_count,
                    },
                ))),
                "format={video_format:?}"
            );
            for (property_key, expected) in [
                (spa::sys::SPA_PARAM_BUFFERS_stride, stride),
                (spa::sys::SPA_PARAM_BUFFERS_size, size),
            ] {
                let property = param
                    .properties
                    .iter()
                    .find(|property| property.key == property_key)
                    .expect("shared-memory buffer parameter should declare its layout");
                assert_eq!(
                    property.value,
                    spa::pod::Value::Int(expected),
                    "format={video_format:?} key={property_key}"
                );
            }
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
