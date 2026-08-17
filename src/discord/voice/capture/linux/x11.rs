use std::{
    collections::{HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use x11rb::{
    connection::Connection,
    image::{BitsPerPixel, Image as X11Image, ImageOrder, PixelLayout},
    properties::WmClass,
    protocol::{
        randr::{ConnectionExt as _, MonitorInfo},
        xfixes::{ConnectionExt as _, GetCursorImageReply},
        xproto::{
            Atom, AtomEnum, ConnectionExt as _, Drawable, MapState, Visualid, Visualtype, Window,
            WindowClass,
        },
    },
    rust_connection::RustConnection,
};

use super::super::{
    CaptureFrame, CaptureFrameBufferPool, CaptureOutput, STREAM_CAPTURE_FPS, send_capture_result,
};
use crate::{
    discord::voice::{StreamCaptureTarget, StreamCaptureTargetKind},
    logging,
};

const FRAME_QUEUE_CAPACITY: usize = 2;
const START_TIMEOUT: Duration = Duration::from_secs(5);
const START_POLL_INTERVAL: Duration = Duration::from_millis(20);
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / STREAM_CAPTURE_FPS as u64);
const MAX_WINDOW_TREE_DEPTH: usize = 8;
const MAX_WINDOW_TREE_NODES: usize = 8_192;
const MAX_PROPERTY_LENGTH: u32 = 4_096;

pub(super) struct CaptureSession {
    stop_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
struct CaptureArea {
    drawable: Drawable,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    root_x: i32,
    root_y: i32,
}

#[derive(Clone, Copy)]
enum CaptureTargetSpec {
    Display(CaptureArea),
    Window(Window),
}

struct Atoms {
    net_client_list: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    wm_state: Atom,
}

pub(super) fn list_targets() -> Result<Vec<StreamCaptureTarget>, String> {
    let (connection, _) = connect()?;
    let atoms = Atoms::load(&connection)?;
    let mut targets = list_displays(&connection);
    targets.extend(list_windows(&connection, &atoms));
    Ok(targets)
}

pub(super) fn start_capture(
    target: &StreamCaptureTarget,
    external_stop: &AtomicBool,
) -> Result<(CaptureSession, CaptureOutput), String> {
    if !matches!(
        target.kind,
        StreamCaptureTargetKind::Display | StreamCaptureTargetKind::Window
    ) {
        return Err("native X11 capture requires a display or window target".to_owned());
    }

    let target = target.clone();
    let (frames_tx, frames_rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
    let (errors_tx, errors_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop_requested);
    let worker = thread::Builder::new()
        .name("stream-x11-video".to_owned())
        .spawn(move || {
            let result = run_capture(
                target,
                frames_tx,
                errors_tx.clone(),
                worker_stop,
                &startup_tx,
            );
            if let Err(error) = result {
                let _ = startup_tx.try_send(Err(error.clone()));
                let _ = errors_tx.send(error);
            }
        })
        .map_err(|error| format!("X11 video worker spawn failed: {error}"))?;

    match wait_for_start(&startup_rx, external_stop) {
        Ok(()) => Ok((
            CaptureSession {
                stop_requested,
                worker: Some(worker),
            },
            CaptureOutput {
                frames: frames_rx,
                errors: errors_rx,
            },
        )),
        Err(error) => {
            stop_requested.store(true, Ordering::Release);
            let _ = worker.join();
            Err(error)
        }
    }
}

impl CaptureSession {
    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.stop_requested.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|error| format!("X11 video worker panicked: {error:?}"))
    }
}

impl Atoms {
    fn load(connection: &RustConnection) -> Result<Self, String> {
        Ok(Self {
            net_client_list: intern_atom(connection, b"_NET_CLIENT_LIST")?,
            net_wm_name: intern_atom(connection, b"_NET_WM_NAME")?,
            utf8_string: intern_atom(connection, b"UTF8_STRING")?,
            wm_state: intern_atom(connection, b"WM_STATE")?,
        })
    }
}

fn connect() -> Result<(RustConnection, usize), String> {
    RustConnection::connect(None).map_err(|error| format!("could not open X11 display: {error}"))
}

fn intern_atom(connection: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    connection
        .intern_atom(false, name)
        .map_err(|error| format!("could not request X11 atom: {error}"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| format!("could not resolve X11 atom: {error}"))
}

fn list_displays(connection: &RustConnection) -> Vec<StreamCaptureTarget> {
    let randr_1_5 = supports_randr_1_5(connection);
    let mut targets = Vec::new();

    for (screen_index, screen) in connection.setup().roots.iter().enumerate() {
        let monitors = randr_1_5
            .then(|| active_monitors(connection, screen.root))
            .transpose()
            .ok()
            .flatten()
            .unwrap_or_default();
        if monitors.is_empty() {
            targets.push(StreamCaptureTarget {
                kind: StreamCaptureTargetKind::Display,
                id: display_target_id(screen_index, 0),
                title: format!(
                    "Screen: Display {} ({}x{})",
                    screen_index + 1,
                    screen.width_in_pixels,
                    screen.height_in_pixels
                ),
            });
            continue;
        }

        for (monitor_index, monitor) in monitors.into_iter().enumerate() {
            let name = atom_name(connection, monitor.name)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Display {}", monitor_index + 1));
            targets.push(StreamCaptureTarget {
                kind: StreamCaptureTargetKind::Display,
                id: display_target_id(screen_index, monitor.name),
                title: format!("Screen: {name} ({}x{})", monitor.width, monitor.height),
            });
        }
    }

    targets
}

fn supports_randr_1_5(connection: &RustConnection) -> bool {
    connection
        .randr_query_version(1, 5)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| {
            reply.major_version > 1 || (reply.major_version == 1 && reply.minor_version >= 5)
        })
}

fn active_monitors(connection: &RustConnection, root: Window) -> Result<Vec<MonitorInfo>, String> {
    connection
        .randr_get_monitors(root, true)
        .map_err(|error| format!("could not request X11 monitor list: {error}"))?
        .reply()
        .map(|reply| reply.monitors)
        .map_err(|error| format!("could not read X11 monitor list: {error}"))
}

fn display_target_id(screen_index: usize, monitor: Atom) -> u64 {
    ((screen_index as u64) << 32) | u64::from(monitor)
}

fn display_target_parts(id: u64) -> (usize, Atom) {
    ((id >> 32) as usize, id as Atom)
}

fn atom_name(connection: &RustConnection, atom: Atom) -> Option<String> {
    let reply = connection.get_atom_name(atom).ok()?.reply().ok()?;
    clean_text(&String::from_utf8_lossy(&reply.name))
}

fn list_windows(connection: &RustConnection, atoms: &Atoms) -> Vec<StreamCaptureTarget> {
    let mut windows = HashSet::new();
    for screen in &connection.setup().roots {
        let ewmh_windows = ewmh_client_windows(connection, screen.root, atoms.net_client_list);
        if ewmh_windows.is_empty() {
            // Small window managers may not publish the EWMH client list. ICCCM's
            // WM_STATE property still finds clients below reparenting frames.
            windows.extend(icccm_client_windows(
                connection,
                screen.root,
                atoms.wm_state,
            ));
        } else {
            windows.extend(ewmh_windows);
        }
    }

    windows
        .into_iter()
        .filter_map(|window| window_target(connection, atoms, window))
        .collect()
}

fn ewmh_client_windows(
    connection: &RustConnection,
    root: Window,
    client_list: Atom,
) -> Vec<Window> {
    let Some(reply) = connection
        .get_property(
            false,
            root,
            client_list,
            AtomEnum::WINDOW,
            0,
            MAX_PROPERTY_LENGTH,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return Vec::new();
    };
    if reply.format != 32 {
        return Vec::new();
    }

    reply
        .value
        .chunks_exact(4)
        .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect()
}

fn icccm_client_windows(connection: &RustConnection, root: Window, wm_state: Atom) -> Vec<Window> {
    let root_children = connection
        .query_tree(root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.children)
        .unwrap_or_default();
    let mut queue: VecDeque<_> = root_children
        .iter()
        .copied()
        .map(|window| (window, 0))
        .collect();
    let mut visited = HashSet::new();
    let mut clients = Vec::new();

    while let Some((window, depth)) = queue.pop_front() {
        if visited.len() >= MAX_WINDOW_TREE_NODES || !visited.insert(window) {
            continue;
        }
        if window_has_property(connection, window, wm_state) {
            clients.push(window);
            continue;
        }
        if depth >= MAX_WINDOW_TREE_DEPTH {
            continue;
        }
        if let Some(children) = connection
            .query_tree(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.children)
        {
            queue.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }

    if clients.is_empty() {
        root_children
    } else {
        clients
    }
}

fn window_has_property(connection: &RustConnection, window: Window, property: Atom) -> bool {
    connection
        .get_property(false, window, property, AtomEnum::ANY, 0, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.type_ != u32::from(AtomEnum::NONE))
}

fn window_target(
    connection: &RustConnection,
    atoms: &Atoms,
    window: Window,
) -> Option<StreamCaptureTarget> {
    let attributes = connection
        .get_window_attributes(window)
        .ok()?
        .reply()
        .ok()?;
    if attributes.class != WindowClass::INPUT_OUTPUT
        || attributes.map_state != MapState::VIEWABLE
        || attributes.override_redirect
    {
        return None;
    }
    let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
    if geometry.width < 2 || geometry.height < 2 {
        return None;
    }

    let title = window_title(connection, atoms, window)?;
    let app_name = WmClass::get(connection, window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .flatten()
        .and_then(|class| clean_text(&String::from_utf8_lossy(class.class())));
    let label = match app_name {
        Some(app_name) if !title.starts_with(&app_name) => format!("{app_name}: {title}"),
        _ => title,
    };

    Some(StreamCaptureTarget {
        kind: StreamCaptureTargetKind::Window,
        id: u64::from(window),
        title: format!("Window: {label}"),
    })
}

fn window_title(connection: &RustConnection, atoms: &Atoms, window: Window) -> Option<String> {
    property_text(connection, window, atoms.net_wm_name, atoms.utf8_string).or_else(|| {
        property_text(
            connection,
            window,
            AtomEnum::WM_NAME.into(),
            AtomEnum::ANY.into(),
        )
    })
}

fn property_text(
    connection: &RustConnection,
    window: Window,
    property: Atom,
    property_type: Atom,
) -> Option<String> {
    let reply = connection
        .get_property(
            false,
            window,
            property,
            property_type,
            0,
            MAX_PROPERTY_LENGTH,
        )
        .ok()?
        .reply()
        .ok()?;
    if reply.format != 8 {
        return None;
    }
    clean_text(&String::from_utf8_lossy(&reply.value))
}

fn clean_text(text: &str) -> Option<String> {
    let text = text
        .trim_matches('\0')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

fn wait_for_start(
    startup: &Receiver<Result<(), String>>,
    external_stop: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if external_stop.load(Ordering::Acquire) {
            return Err("X11 screen capture was cancelled".to_owned());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err("X11 screen capture did not start in time".to_owned());
        }
        let wait = (deadline - now).min(START_POLL_INTERVAL);
        match startup.recv_timeout(wait) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("X11 screen capture stopped during startup".to_owned());
            }
        }
    }
}

fn run_capture(
    target: StreamCaptureTarget,
    frames: SyncSender<CaptureFrame>,
    errors: Sender<String>,
    stop_requested: Arc<AtomicBool>,
    startup: &SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let (connection, _) = connect()?;
    let target = resolve_capture_target(&connection, &target)?;
    let buffer_pool = CaptureFrameBufferPool::default();
    let mut cursor_supported = supports_xfixes_cursor(&connection);
    let mut startup_pending = true;

    while !stop_requested.load(Ordering::Acquire) {
        let frame_started = Instant::now();
        let area = match target {
            CaptureTargetSpec::Display(area) => area,
            CaptureTargetSpec::Window(window) => resolve_window_area(&connection, window)?,
        };
        let mut frame = capture_area(&connection, area, &buffer_pool)?;
        if cursor_supported {
            match connection
                .xfixes_get_cursor_image()
                .map_err(|error| error.to_string())
                .and_then(|cookie| cookie.reply().map_err(|error| error.to_string()))
            {
                Ok(cursor) => composite_cursor(&mut frame.rgba, area, &cursor),
                Err(error) => {
                    cursor_supported = false;
                    logging::debug(
                        "stream",
                        format!("X11 cursor capture was disabled after an error: {error}"),
                    );
                }
            }
        }
        send_capture_result(&frames, &errors, Ok(frame));
        if startup_pending {
            startup_pending = false;
            if startup.send(Ok(())).is_err() {
                return Ok(());
            }
        }

        if let Some(remaining) = FRAME_INTERVAL.checked_sub(frame_started.elapsed()) {
            thread::sleep(remaining);
        }
    }

    Ok(())
}

fn resolve_capture_target(
    connection: &RustConnection,
    target: &StreamCaptureTarget,
) -> Result<CaptureTargetSpec, String> {
    match target.kind {
        StreamCaptureTargetKind::Display => {
            resolve_display_area(connection, target.id).map(CaptureTargetSpec::Display)
        }
        StreamCaptureTargetKind::Window => {
            let window = u32::try_from(target.id)
                .map_err(|_| "X11 window target has an invalid identifier".to_owned())?;
            resolve_window_area(connection, window)?;
            Ok(CaptureTargetSpec::Window(window))
        }
        StreamCaptureTargetKind::Portal => {
            Err("native X11 capture cannot use a portal target".to_owned())
        }
    }
}

fn resolve_display_area(connection: &RustConnection, id: u64) -> Result<CaptureArea, String> {
    let (screen_index, monitor_atom) = display_target_parts(id);
    let screen = connection
        .setup()
        .roots
        .get(screen_index)
        .ok_or_else(|| "selected X11 screen is no longer available".to_owned())?;
    if monitor_atom == 0 {
        return Ok(CaptureArea {
            drawable: screen.root,
            x: 0,
            y: 0,
            width: screen.width_in_pixels,
            height: screen.height_in_pixels,
            root_x: 0,
            root_y: 0,
        });
    }

    let monitor = active_monitors(connection, screen.root)?
        .into_iter()
        .find(|monitor| monitor.name == monitor_atom)
        .ok_or_else(|| "selected X11 monitor is no longer available".to_owned())?;
    Ok(CaptureArea {
        drawable: screen.root,
        x: monitor.x,
        y: monitor.y,
        width: monitor.width,
        height: monitor.height,
        root_x: i32::from(monitor.x),
        root_y: i32::from(monitor.y),
    })
}

fn resolve_window_area(connection: &RustConnection, window: Window) -> Result<CaptureArea, String> {
    let attributes = connection
        .get_window_attributes(window)
        .map_err(|error| format!("could not request X11 window state: {error}"))?
        .reply()
        .map_err(|error| format!("selected X11 window is unavailable: {error}"))?;
    if attributes.map_state != MapState::VIEWABLE {
        return Err("selected X11 window is not visible".to_owned());
    }
    let geometry = connection
        .get_geometry(window)
        .map_err(|error| format!("could not request X11 window geometry: {error}"))?
        .reply()
        .map_err(|error| format!("could not read X11 window geometry: {error}"))?;
    if geometry.width < 2 || geometry.height < 2 {
        return Err("selected X11 window is too small to capture".to_owned());
    }
    let position = connection
        .translate_coordinates(window, geometry.root, 0, 0)
        .map_err(|error| format!("could not request X11 window position: {error}"))?
        .reply()
        .map_err(|error| format!("could not read X11 window position: {error}"))?;
    if !position.same_screen {
        return Err("selected X11 window moved to another screen".to_owned());
    }

    Ok(CaptureArea {
        drawable: window,
        x: 0,
        y: 0,
        width: geometry.width,
        height: geometry.height,
        root_x: i32::from(position.dst_x),
        root_y: i32::from(position.dst_y),
    })
}

fn capture_area(
    connection: &RustConnection,
    area: CaptureArea,
    buffer_pool: &CaptureFrameBufferPool,
) -> Result<CaptureFrame, String> {
    let (image, visual) = X11Image::get(
        connection,
        area.drawable,
        area.x,
        area.y,
        area.width,
        area.height,
    )
    .map_err(|error| format!("could not read X11 pixels: {error}"))?;
    let visual = visual_type(connection, visual)?;
    let pixel_layout = PixelLayout::from_visual_type(visual).map_err(|error| {
        format!(
            "X11 visual {:#x} is not a supported RGB visual: {error}",
            visual.visual_id
        )
    })?;
    let rgba_length = usize::from(area.width)
        .checked_mul(usize::from(area.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "X11 capture dimensions are too large".to_owned())?;
    let mut rgba = buffer_pool.take(rgba_length);

    convert_image_to_rgba(&image, &visual, pixel_layout, &mut rgba);

    Ok(CaptureFrame::new(
        u32::from(area.width),
        u32::from(area.height),
        rgba,
        buffer_pool.clone(),
    ))
}

fn visual_type(connection: &RustConnection, visual_id: Visualid) -> Result<Visualtype, String> {
    connection
        .setup()
        .roots
        .iter()
        .flat_map(|screen| &screen.allowed_depths)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == visual_id)
        .cloned()
        .ok_or_else(|| format!("X11 visual {visual_id:#x} is not available"))
}

fn convert_image_to_rgba(
    image: &X11Image<'_>,
    visual: &Visualtype,
    pixel_layout: PixelLayout,
    rgba: &mut [u8],
) {
    let standard_rgb_masks = visual.red_mask == 0x00ff_0000
        && visual.green_mask == 0x0000_ff00
        && visual.blue_mask == 0x0000_00ff;
    if image.bits_per_pixel() == BitsPerPixel::B32 && standard_rgb_masks {
        // The common Xorg format can be reordered directly. Other visuals use
        // x11rb's decoder so uncommon bit layouts remain correct.
        for (source, destination) in image.data().chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            match image.byte_order() {
                ImageOrder::LsbFirst => {
                    destination[0] = source[2];
                    destination[1] = source[1];
                    destination[2] = source[0];
                }
                ImageOrder::MsbFirst => {
                    destination[0] = source[1];
                    destination[1] = source[2];
                    destination[2] = source[3];
                }
            }
            destination[3] = 255;
        }
        return;
    }

    for y in 0..image.height() {
        for x in 0..image.width() {
            let (red, green, blue) = pixel_layout.decode(image.get_pixel(x, y));
            let offset = (usize::from(y) * usize::from(image.width()) + usize::from(x)) * 4;
            rgba[offset] = (red >> 8) as u8;
            rgba[offset + 1] = (green >> 8) as u8;
            rgba[offset + 2] = (blue >> 8) as u8;
            rgba[offset + 3] = 255;
        }
    }
}

fn supports_xfixes_cursor(connection: &RustConnection) -> bool {
    let supported = connection
        .xfixes_query_version(2, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some();
    if !supported {
        logging::debug(
            "stream",
            "XFixes is unavailable, so native X11 capture will omit the cursor",
        );
    }
    supported
}

fn composite_cursor(rgba: &mut [u8], area: CaptureArea, cursor: &GetCursorImageReply) {
    let cursor_left = i32::from(cursor.x) - i32::from(cursor.xhot) - area.root_x;
    let cursor_top = i32::from(cursor.y) - i32::from(cursor.yhot) - area.root_y;
    let frame_width = i32::from(area.width);
    let frame_height = i32::from(area.height);

    for cursor_y in 0..i32::from(cursor.height) {
        let frame_y = cursor_top + cursor_y;
        if !(0..frame_height).contains(&frame_y) {
            continue;
        }
        for cursor_x in 0..i32::from(cursor.width) {
            let frame_x = cursor_left + cursor_x;
            if !(0..frame_width).contains(&frame_x) {
                continue;
            }
            let cursor_index = usize::try_from(cursor_y)
                .ok()
                .and_then(|y| y.checked_mul(usize::from(cursor.width)))
                .and_then(|row| {
                    usize::try_from(cursor_x)
                        .ok()
                        .and_then(|x| row.checked_add(x))
                });
            let Some(cursor_pixel) = cursor_index.and_then(|index| cursor.cursor_image.get(index))
            else {
                continue;
            };
            let frame_index = (usize::try_from(frame_y).expect("clipped cursor y")
                * usize::from(area.width)
                + usize::try_from(frame_x).expect("clipped cursor x"))
                * 4;
            blend_premultiplied_argb(&mut rgba[frame_index..frame_index + 4], *cursor_pixel);
        }
    }
}

fn blend_premultiplied_argb(destination: &mut [u8], source: u32) {
    let alpha = (source >> 24) as u8;
    if alpha == 0 {
        return;
    }
    let inverse_alpha = u16::from(255 - alpha);
    for (channel, shift) in [(0, 16), (1, 8), (2, 0)] {
        let source_channel = ((source >> shift) & 0xff) as u16;
        let destination_channel = u16::from(destination[channel]);
        destination[channel] = source_channel
            .saturating_add((destination_channel * inverse_alpha + 127) / 255)
            .min(255) as u8;
    }
    destination[3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    use x11rb::{
        COPY_FROM_PARENT,
        protocol::xproto::{CreateWindowAux, PropMode},
        wrapper::ConnectionExt as _,
    };

    #[test]
    fn display_target_id_round_trips_screen_and_monitor() {
        let id = display_target_id(7, 0xfedc_ba98);
        assert_eq!(display_target_parts(id), (7, 0xfedc_ba98));
    }

    #[test]
    fn premultiplied_cursor_blending_preserves_opaque_output() {
        let mut destination = [20, 40, 60, 255];
        blend_premultiplied_argb(&mut destination, 0x8064_3219);
        assert_eq!(destination, [110, 70, 55, 255]);

        blend_premultiplied_argb(&mut destination, 0);
        assert_eq!(destination, [110, 70, 55, 255]);
    }

    #[test]
    #[ignore = "requires an X11 server"]
    fn captures_live_x11_display_and_window_targets() {
        let (connection, screen_index) = connect().expect("X11 test display should be available");
        let screen = &connection.setup().roots[screen_index];
        let window = connection
            .generate_id()
            .expect("test window id should be available");
        connection
            .create_window(
                COPY_FROM_PARENT as u8,
                window,
                screen.root,
                10,
                10,
                96,
                64,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().background_pixel(screen.white_pixel),
            )
            .expect("test window creation should be requested")
            .check()
            .expect("test window should be created");
        connection
            .change_property8(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                b"Concord X11 capture test",
            )
            .expect("test window title should be requested")
            .check()
            .expect("test window title should be set");
        connection
            .map_window(window)
            .expect("test window mapping should be requested")
            .check()
            .expect("test window should be mapped");
        connection.flush().expect("test window should be flushed");
        connection
            .get_window_attributes(window)
            .expect("test window state should be requested")
            .reply()
            .expect("test window should be visible");

        let targets = list_targets().expect("X11 targets should be discovered");
        assert!(
            targets
                .iter()
                .any(|target| target.kind == StreamCaptureTargetKind::Display)
        );
        let window_target = targets
            .into_iter()
            .find(|target| {
                target.kind == StreamCaptureTargetKind::Window && target.id == u64::from(window)
            })
            .expect("test window should be discovered");
        let display_target = StreamCaptureTarget {
            kind: StreamCaptureTargetKind::Display,
            id: display_target_id(screen_index, 0),
            title: "test display".to_owned(),
        };

        assert_target_produces_frame(
            display_target,
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        );
        assert_target_produces_frame(window_target, 96, 64);

        connection
            .destroy_window(window)
            .expect("test window destruction should be requested")
            .check()
            .expect("test window should be destroyed");
    }

    fn assert_target_produces_frame(
        target: StreamCaptureTarget,
        expected_width: u32,
        expected_height: u32,
    ) {
        let external_stop = AtomicBool::new(false);
        let (mut session, output) =
            start_capture(&target, &external_stop).expect("X11 capture should start");
        let frame = output
            .frames
            .recv_timeout(START_TIMEOUT)
            .expect("X11 capture should produce a frame");
        assert_eq!(
            (frame.width, frame.height),
            (expected_width, expected_height)
        );
        session.stop().expect("X11 capture should stop cleanly");
    }
}
