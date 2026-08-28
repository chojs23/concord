use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xinput::{ConnectionExt as _, Device, EventMask, XIEventMask},
        xproto::{ConnectionExt as _, KeyButMask, Keycode, Window},
    },
    rust_connection::RustConnection,
};
use xkeysym::RawKeysym;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) struct PushToTalkListener {
    events: Receiver<bool>,
    stop_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PushToTalkListener {
    pub(super) fn start(hotkey: HotKey) -> Result<Self, String> {
        let keysym = key_to_x11_keysym(hotkey.key).ok_or_else(|| {
            format!(
                "Push-to-talk key is not supported by the X11 input monitor: {}",
                hotkey.key
            )
        })?;
        let modifiers = modifier_mask(hotkey.mods);
        let (events_tx, events) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let worker_stop_requested = Arc::clone(&stop_requested);
        let worker = thread::Builder::new()
            .name("push-to-talk-input".to_owned())
            .spawn(move || {
                run_x11_listener(
                    keysym,
                    modifiers,
                    events_tx,
                    worker_stop_requested,
                    startup_tx,
                );
            })
            .map_err(|error| format!("Could not start the X11 push-to-talk monitor: {error}"))?;

        match startup_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                events,
                stop_requested,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("The X11 push-to-talk monitor stopped during startup".to_owned())
            }
        }
    }

    pub(super) fn latest_state(&self) -> Option<bool> {
        self.events.try_iter().last()
    }

    pub(super) fn stop(mut self) -> Result<(), String> {
        self.stop_worker()
    }

    fn stop_worker(&mut self) -> Result<(), String> {
        self.stop_requested.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|_| "The X11 push-to-talk monitor thread panicked".to_owned())
    }
}

impl Drop for PushToTalkListener {
    fn drop(&mut self) {
        let _ = self.stop_worker();
    }
}

fn run_x11_listener(
    keysym: RawKeysym,
    modifiers: KeyButMask,
    events: mpsc::Sender<bool>,
    stop_requested: Arc<AtomicBool>,
    startup: mpsc::SyncSender<Result<(), String>>,
) {
    let result = prepare_x11_listener(keysym, modifiers);
    let (connection, root, mut matcher) = match result {
        Ok(listener) => listener,
        Err(error) => {
            let _ = startup.send(Err(error));
            return;
        }
    };
    if startup.send(Ok(())).is_err() {
        return;
    }

    while !stop_requested.load(Ordering::Acquire) {
        loop {
            let event = match connection.poll_for_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => return,
            };
            match event {
                Event::XinputRawKeyPress(event) => {
                    if event.detail == u32::from(matcher.keycode) {
                        let Ok(cookie) = connection.query_pointer(root) else {
                            return;
                        };
                        let Ok(reply) = cookie.reply() else {
                            return;
                        };
                        if let Some(pressed) = matcher.transition(
                            true,
                            event.detail,
                            reply.mask & shortcut_modifiers(),
                        ) {
                            let _ = events.send(pressed);
                        }
                    }
                }
                Event::XinputRawKeyRelease(event) => {
                    if let Some(pressed) =
                        matcher.transition(false, event.detail, KeyButMask::default())
                    {
                        let _ = events.send(pressed);
                    }
                }
                _ => {}
            }
        }
        thread::sleep(EVENT_POLL_INTERVAL);
    }
}

fn prepare_x11_listener(
    keysym: RawKeysym,
    modifiers: KeyButMask,
) -> Result<(RustConnection, Window, ShortcutMatcher), String> {
    let (connection, screen) = RustConnection::connect(None)
        .map_err(|error| format!("Could not open the X11 display: {error}"))?;
    connection
        .xinput_xi_query_version(2, 0)
        .map_err(|error| format!("Could not query XInput 2: {error}"))?
        .reply()
        .map_err(|error| format!("XInput 2 is unavailable: {error}"))?;

    let root = connection.setup().roots[screen].root;
    let keycode = keysym_to_keycode(&connection, keysym)?
        .ok_or_else(|| "Could not map the push-to-talk key on this X11 keyboard".to_owned())?;
    let masks = [EventMask {
        deviceid: Device::ALL_MASTER.into(),
        mask: vec![XIEventMask::RAW_KEY_PRESS | XIEventMask::RAW_KEY_RELEASE],
    }];
    connection
        .xinput_xi_select_events(root, &masks)
        .map_err(|error| format!("Could not select XInput push-to-talk events: {error}"))?
        .check()
        .map_err(|error| format!("Could not monitor XInput push-to-talk events: {error}"))?;
    connection
        .flush()
        .map_err(|error| format!("Could not start the X11 push-to-talk monitor: {error}"))?;

    Ok((connection, root, ShortcutMatcher::new(keycode, modifiers)))
}

#[derive(Clone, Copy)]
struct ShortcutMatcher {
    keycode: Keycode,
    modifiers: KeyButMask,
    pressed: bool,
}

impl ShortcutMatcher {
    const fn new(keycode: Keycode, modifiers: KeyButMask) -> Self {
        Self {
            keycode,
            modifiers,
            pressed: false,
        }
    }

    fn transition(&mut self, pressed: bool, keycode: u32, modifiers: KeyButMask) -> Option<bool> {
        if keycode != u32::from(self.keycode) {
            return None;
        }

        if pressed && !self.pressed && modifiers == self.modifiers {
            self.pressed = true;
            Some(true)
        } else if !pressed && self.pressed {
            self.pressed = false;
            Some(false)
        } else {
            None
        }
    }
}

fn modifier_mask(modifiers: Modifiers) -> KeyButMask {
    let mut mask = KeyButMask::default();
    if modifiers.contains(Modifiers::SHIFT) {
        mask |= KeyButMask::SHIFT;
    }
    if modifiers.contains(Modifiers::CONTROL) {
        mask |= KeyButMask::CONTROL;
    }
    if modifiers.contains(Modifiers::ALT) {
        mask |= KeyButMask::MOD1;
    }
    if modifiers.intersects(Modifiers::SUPER | Modifiers::META) {
        mask |= KeyButMask::MOD4;
    }
    mask
}

fn shortcut_modifiers() -> KeyButMask {
    KeyButMask::CONTROL | KeyButMask::SHIFT | KeyButMask::MOD4 | KeyButMask::MOD1
}

fn keysym_to_keycode(
    connection: &RustConnection,
    keysym: RawKeysym,
) -> Result<Option<Keycode>, String> {
    let setup = connection.setup();
    let minimum = setup.min_keycode;
    let count = setup.max_keycode - minimum + 1;
    let mapping = connection
        .get_keyboard_mapping(minimum, count)
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    let keysyms_per_keycode = usize::from(mapping.keysyms_per_keycode);

    for (offset, keysyms) in mapping.keysyms.chunks(keysyms_per_keycode).enumerate() {
        if keysyms.contains(&keysym) {
            let offset = u8::try_from(offset)
                .map_err(|_| "X11 keyboard mapping contains too many keycodes".to_owned())?;
            return Ok(minimum.checked_add(offset));
        }
    }
    Ok(None)
}

// This keysym table is adapted from `global-hotkey` 0.8.0, which is licensed
// under Apache-2.0 OR MIT and maps the same `Code` values.
fn key_to_x11_keysym(key: Code) -> Option<RawKeysym> {
    Some(match key {
        Code::KeyA => xkeysym::key::A,
        Code::KeyB => xkeysym::key::B,
        Code::KeyC => xkeysym::key::C,
        Code::KeyD => xkeysym::key::D,
        Code::KeyE => xkeysym::key::E,
        Code::KeyF => xkeysym::key::F,
        Code::KeyG => xkeysym::key::G,
        Code::KeyH => xkeysym::key::H,
        Code::KeyI => xkeysym::key::I,
        Code::KeyJ => xkeysym::key::J,
        Code::KeyK => xkeysym::key::K,
        Code::KeyL => xkeysym::key::L,
        Code::KeyM => xkeysym::key::M,
        Code::KeyN => xkeysym::key::N,
        Code::KeyO => xkeysym::key::O,
        Code::KeyP => xkeysym::key::P,
        Code::KeyQ => xkeysym::key::Q,
        Code::KeyR => xkeysym::key::R,
        Code::KeyS => xkeysym::key::S,
        Code::KeyT => xkeysym::key::T,
        Code::KeyU => xkeysym::key::U,
        Code::KeyV => xkeysym::key::V,
        Code::KeyW => xkeysym::key::W,
        Code::KeyX => xkeysym::key::X,
        Code::KeyY => xkeysym::key::Y,
        Code::KeyZ => xkeysym::key::Z,
        Code::Backslash => xkeysym::key::backslash,
        Code::BracketLeft => xkeysym::key::bracketleft,
        Code::BracketRight => xkeysym::key::bracketright,
        Code::Backquote => xkeysym::key::quoteleft,
        Code::Comma => xkeysym::key::comma,
        Code::Digit0 => xkeysym::key::_0,
        Code::Digit1 => xkeysym::key::_1,
        Code::Digit2 => xkeysym::key::_2,
        Code::Digit3 => xkeysym::key::_3,
        Code::Digit4 => xkeysym::key::_4,
        Code::Digit5 => xkeysym::key::_5,
        Code::Digit6 => xkeysym::key::_6,
        Code::Digit7 => xkeysym::key::_7,
        Code::Digit8 => xkeysym::key::_8,
        Code::Digit9 => xkeysym::key::_9,
        Code::Equal => xkeysym::key::equal,
        Code::Minus => xkeysym::key::minus,
        Code::Period => xkeysym::key::period,
        Code::Quote => xkeysym::key::leftsinglequotemark,
        Code::Semicolon => xkeysym::key::semicolon,
        Code::Slash => xkeysym::key::slash,
        Code::Backspace => xkeysym::key::BackSpace,
        Code::CapsLock => xkeysym::key::Caps_Lock,
        Code::Enter => xkeysym::key::Return,
        Code::Space => xkeysym::key::space,
        Code::Tab => xkeysym::key::Tab,
        Code::Delete => xkeysym::key::Delete,
        Code::End => xkeysym::key::End,
        Code::Home => xkeysym::key::Home,
        Code::Insert => xkeysym::key::Insert,
        Code::PageDown => xkeysym::key::Page_Down,
        Code::PageUp => xkeysym::key::Page_Up,
        Code::ArrowDown => xkeysym::key::Down,
        Code::ArrowLeft => xkeysym::key::Left,
        Code::ArrowRight => xkeysym::key::Right,
        Code::ArrowUp => xkeysym::key::Up,
        Code::Numpad0 => xkeysym::key::KP_0,
        Code::Numpad1 => xkeysym::key::KP_1,
        Code::Numpad2 => xkeysym::key::KP_2,
        Code::Numpad3 => xkeysym::key::KP_3,
        Code::Numpad4 => xkeysym::key::KP_4,
        Code::Numpad5 => xkeysym::key::KP_5,
        Code::Numpad6 => xkeysym::key::KP_6,
        Code::Numpad7 => xkeysym::key::KP_7,
        Code::Numpad8 => xkeysym::key::KP_8,
        Code::Numpad9 => xkeysym::key::KP_9,
        Code::NumpadAdd => xkeysym::key::KP_Add,
        Code::NumpadDecimal => xkeysym::key::KP_Decimal,
        Code::NumpadDivide => xkeysym::key::KP_Divide,
        Code::NumpadMultiply => xkeysym::key::KP_Multiply,
        Code::NumpadSubtract => xkeysym::key::KP_Subtract,
        Code::Escape => xkeysym::key::Escape,
        Code::PrintScreen => xkeysym::key::Print,
        Code::ScrollLock => xkeysym::key::Scroll_Lock,
        Code::NumLock => xkeysym::key::Num_Lock,
        Code::F1 => xkeysym::key::F1,
        Code::F2 => xkeysym::key::F2,
        Code::F3 => xkeysym::key::F3,
        Code::F4 => xkeysym::key::F4,
        Code::F5 => xkeysym::key::F5,
        Code::F6 => xkeysym::key::F6,
        Code::F7 => xkeysym::key::F7,
        Code::F8 => xkeysym::key::F8,
        Code::F9 => xkeysym::key::F9,
        Code::F10 => xkeysym::key::F10,
        Code::F11 => xkeysym::key::F11,
        Code::F12 => xkeysym::key::F12,
        Code::F13 => xkeysym::key::F13,
        Code::F14 => xkeysym::key::F14,
        Code::F15 => xkeysym::key::F15,
        Code::F16 => xkeysym::key::F16,
        Code::F17 => xkeysym::key::F17,
        Code::F18 => xkeysym::key::F18,
        Code::F19 => xkeysym::key::F19,
        Code::F20 => xkeysym::key::F20,
        Code::F21 => xkeysym::key::F21,
        Code::F22 => xkeysym::key::F22,
        Code::F23 => xkeysym::key::F23,
        Code::F24 => xkeysym::key::F24,
        Code::AudioVolumeDown => xkeysym::key::XF86_AudioLowerVolume,
        Code::AudioVolumeMute => xkeysym::key::XF86_AudioMute,
        Code::AudioVolumeUp => xkeysym::key::XF86_AudioRaiseVolume,
        Code::MediaPlay => xkeysym::key::XF86_AudioPlay,
        Code::MediaPause => xkeysym::key::XF86_AudioPause,
        Code::MediaStop => xkeysym::key::XF86_AudioStop,
        Code::MediaTrackNext => xkeysym::key::XF86_AudioNext,
        Code::MediaTrackPrevious => xkeysym::key::XF86_AudioPrev,
        Code::Pause => xkeysym::key::Pause,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_key_tracks_press_and_release_without_modifiers() {
        let mut matcher = ShortcutMatcher::new(10, KeyButMask::default());

        assert_eq!(
            matcher.transition(true, 10, KeyButMask::default()),
            Some(true)
        );
        assert_eq!(matcher.transition(true, 10, KeyButMask::default()), None);
        assert_eq!(
            matcher.transition(false, 10, KeyButMask::default()),
            Some(false)
        );
        assert_eq!(matcher.transition(true, 10, KeyButMask::SHIFT), None);
    }
}
