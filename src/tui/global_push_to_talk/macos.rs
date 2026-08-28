use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const TAP_DISABLED_BY_TIMEOUT: u32 = u32::MAX - 1;
const TAP_DISABLED_BY_USER_INPUT: u32 = u32::MAX;

const SESSION_EVENT_TAP: u32 = 1;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
const LISTEN_ONLY_EVENT_TAP: u32 = 1;
const KEYBOARD_EVENT_KEYCODE: u32 = 9;

const SHIFT_FLAG: u64 = 1 << 17;
const CONTROL_FLAG: u64 = 1 << 18;
const ALT_FLAG: u64 = 1 << 19;
const COMMAND_FLAG: u64 = 1 << 20;
const SHORTCUT_MODIFIER_FLAGS: u64 = SHIFT_FLAG | CONTROL_FLAG | ALT_FLAG | COMMAND_FLAG;

const RUN_LOOP_POLL_SECONDS: f64 = 0.05;

type CFMachPortRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFStringRef = *const c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;

type CGEventTapCallback = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: u8);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;

    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRelease(value: *const c_void);
    fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRemoveSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: u8) -> i32;
}

pub(super) struct PushToTalkListener {
    events: Receiver<bool>,
    stop_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PushToTalkListener {
    pub(super) fn start(hotkey: HotKey) -> Result<Self, String> {
        let key_code = key_to_macos_key_code(hotkey.key).ok_or_else(|| {
            format!(
                "Push-to-talk key is not supported by the macOS input monitor: {}",
                hotkey.key
            )
        })?;
        let matcher = ShortcutMatcher::new(key_code, modifier_flags(hotkey.mods));
        let (events_tx, events) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let worker_stop_requested = Arc::clone(&stop_requested);
        let worker = thread::Builder::new()
            .name("push-to-talk-input".to_owned())
            .spawn(move || {
                run_event_tap(matcher, events_tx, worker_stop_requested, startup_tx);
            })
            .map_err(|error| format!("Could not start the macOS push-to-talk monitor: {error}"))?;

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
                Err("The macOS push-to-talk monitor stopped during startup".to_owned())
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
            .map_err(|_| "The macOS push-to-talk monitor thread panicked".to_owned())
    }
}

impl Drop for PushToTalkListener {
    fn drop(&mut self) {
        let _ = self.stop_worker();
    }
}

struct EventTapContext {
    event_tap: CFMachPortRef,
    matcher: ShortcutMatcher,
    events: mpsc::Sender<bool>,
}

fn run_event_tap(
    matcher: ShortcutMatcher,
    events: mpsc::Sender<bool>,
    stop_requested: Arc<AtomicBool>,
    startup: mpsc::SyncSender<Result<(), String>>,
) {
    let event_mask = event_mask(KEY_DOWN) | event_mask(KEY_UP);
    let mut context = Box::new(EventTapContext {
        event_tap: std::ptr::null_mut(),
        matcher,
        events,
    });
    let event_tap = unsafe {
        CGEventTapCreate(
            SESSION_EVENT_TAP,
            HEAD_INSERT_EVENT_TAP,
            LISTEN_ONLY_EVENT_TAP,
            event_mask,
            event_tap_callback,
            (&raw mut *context).cast(),
        )
    };
    if event_tap.is_null() {
        let _ = startup.send(Err(
            "Could not monitor push-to-talk keys. Allow Concord or its terminal in macOS \
             System Settings > Privacy & Security > Input Monitoring."
                .to_owned(),
        ));
        return;
    }
    context.event_tap = event_tap;

    let run_loop_source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), event_tap, 0) };
    if run_loop_source.is_null() {
        unsafe {
            CFMachPortInvalidate(event_tap);
            CFRelease(event_tap.cast_const());
        }
        let _ = startup.send(Err(
            "Could not create the macOS push-to-talk run-loop source".to_owned(),
        ));
        return;
    }

    unsafe {
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(run_loop, run_loop_source, kCFRunLoopDefaultMode);
        CGEventTapEnable(event_tap, 1);
        if startup.send(Ok(())).is_err() {
            cleanup_event_tap(run_loop, run_loop_source, event_tap);
            return;
        }

        while !stop_requested.load(Ordering::Acquire) {
            let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, RUN_LOOP_POLL_SECONDS, 1);
        }

        cleanup_event_tap(run_loop, run_loop_source, event_tap);
    }
}

unsafe fn cleanup_event_tap(
    run_loop: CFRunLoopRef,
    run_loop_source: CFRunLoopSourceRef,
    event_tap: CFMachPortRef,
) {
    unsafe {
        CFRunLoopRemoveSource(run_loop, run_loop_source, kCFRunLoopDefaultMode);
        CFMachPortInvalidate(event_tap);
        CFRelease(run_loop_source.cast_const());
        CFRelease(event_tap.cast_const());
    }
}

unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    if user_info.is_null() {
        return event;
    }

    let context = unsafe { &mut *user_info.cast::<EventTapContext>() };
    if matches!(
        event_type,
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT
    ) {
        unsafe {
            CGEventTapEnable(context.event_tap, 1);
        }
        return event;
    }

    let key_code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
    let Ok(key_code) = u16::try_from(key_code) else {
        return event;
    };
    let flags = unsafe { CGEventGetFlags(event) };
    if let Some(pressed) = context.matcher.transition(event_type, key_code, flags) {
        let _ = context.events.send(pressed);
    }

    // A listen-only tap observes input. Returning the original event also makes
    // the pass-through intent explicit if the tap options change later.
    event
}

#[derive(Clone, Copy)]
struct ShortcutMatcher {
    key_code: u16,
    modifier_flags: u64,
    pressed: bool,
}

impl ShortcutMatcher {
    const fn new(key_code: u16, modifier_flags: u64) -> Self {
        Self {
            key_code,
            modifier_flags,
            pressed: false,
        }
    }

    fn transition(&mut self, event_type: u32, key_code: u16, flags: u64) -> Option<bool> {
        if key_code != self.key_code {
            return None;
        }

        match event_type {
            KEY_DOWN if !self.pressed && flags & SHORTCUT_MODIFIER_FLAGS == self.modifier_flags => {
                self.pressed = true;
                Some(true)
            }
            KEY_UP if self.pressed => {
                self.pressed = false;
                Some(false)
            }
            _ => None,
        }
    }
}

const fn event_mask(event_type: u32) -> u64 {
    1 << event_type
}

fn modifier_flags(modifiers: Modifiers) -> u64 {
    let mut flags = 0;
    if modifiers.contains(Modifiers::SHIFT) {
        flags |= SHIFT_FLAG;
    }
    if modifiers.contains(Modifiers::CONTROL) {
        flags |= CONTROL_FLAG;
    }
    if modifiers.contains(Modifiers::ALT) {
        flags |= ALT_FLAG;
    }
    if modifiers.intersects(Modifiers::SUPER | Modifiers::META) {
        flags |= COMMAND_FLAG;
    }
    flags
}

// This physical-key table is adapted from `global-hotkey` 0.8.0, which is
// licensed under Apache-2.0 OR MIT and maps the same `Code` values.
fn key_to_macos_key_code(code: Code) -> Option<u16> {
    Some(match code {
        Code::KeyA => 0x00,
        Code::KeyS => 0x01,
        Code::KeyD => 0x02,
        Code::KeyF => 0x03,
        Code::KeyH => 0x04,
        Code::KeyG => 0x05,
        Code::KeyZ => 0x06,
        Code::KeyX => 0x07,
        Code::KeyC => 0x08,
        Code::KeyV => 0x09,
        Code::KeyB => 0x0b,
        Code::KeyQ => 0x0c,
        Code::KeyW => 0x0d,
        Code::KeyE => 0x0e,
        Code::KeyR => 0x0f,
        Code::KeyY => 0x10,
        Code::KeyT => 0x11,
        Code::Digit1 => 0x12,
        Code::Digit2 => 0x13,
        Code::Digit3 => 0x14,
        Code::Digit4 => 0x15,
        Code::Digit6 => 0x16,
        Code::Digit5 => 0x17,
        Code::Equal => 0x18,
        Code::Digit9 => 0x19,
        Code::Digit7 => 0x1a,
        Code::Minus => 0x1b,
        Code::Digit8 => 0x1c,
        Code::Digit0 => 0x1d,
        Code::BracketRight => 0x1e,
        Code::KeyO => 0x1f,
        Code::KeyU => 0x20,
        Code::BracketLeft => 0x21,
        Code::KeyI => 0x22,
        Code::KeyP => 0x23,
        Code::Enter => 0x24,
        Code::KeyL => 0x25,
        Code::KeyJ => 0x26,
        Code::Quote => 0x27,
        Code::KeyK => 0x28,
        Code::Semicolon => 0x29,
        Code::Backslash => 0x2a,
        Code::Comma => 0x2b,
        Code::Slash => 0x2c,
        Code::KeyN => 0x2d,
        Code::KeyM => 0x2e,
        Code::Period => 0x2f,
        Code::Tab => 0x30,
        Code::Space => 0x31,
        Code::Backquote => 0x32,
        Code::Backspace => 0x33,
        Code::Escape => 0x35,
        Code::CapsLock => 0x39,
        Code::F17 => 0x40,
        Code::NumpadDecimal => 0x41,
        Code::NumpadMultiply => 0x43,
        Code::NumpadAdd => 0x45,
        Code::PrintScreen => 0x46,
        Code::NumLock => 0x47,
        Code::NumpadDivide => 0x4b,
        Code::NumpadEnter => 0x4c,
        Code::NumpadSubtract => 0x4e,
        Code::F18 => 0x4f,
        Code::F19 => 0x50,
        Code::NumpadEqual => 0x51,
        Code::Numpad0 => 0x52,
        Code::Numpad1 => 0x53,
        Code::Numpad2 => 0x54,
        Code::Numpad3 => 0x55,
        Code::Numpad4 => 0x56,
        Code::Numpad5 => 0x57,
        Code::Numpad6 => 0x58,
        Code::Numpad7 => 0x59,
        Code::F20 => 0x5a,
        Code::Numpad8 => 0x5b,
        Code::Numpad9 => 0x5c,
        Code::F5 => 0x60,
        Code::F6 => 0x61,
        Code::F7 => 0x62,
        Code::F3 => 0x63,
        Code::F8 => 0x64,
        Code::F9 => 0x65,
        Code::F11 => 0x67,
        Code::F13 => 0x69,
        Code::F16 => 0x6a,
        Code::F14 => 0x6b,
        Code::F10 => 0x6d,
        Code::F12 => 0x6f,
        Code::F15 => 0x71,
        Code::Insert => 0x72,
        Code::Home => 0x73,
        Code::PageUp => 0x74,
        Code::Delete => 0x75,
        Code::F4 => 0x76,
        Code::End => 0x77,
        Code::F2 => 0x78,
        Code::PageDown => 0x79,
        Code::F1 => 0x7a,
        Code::ArrowLeft => 0x7b,
        Code::ArrowRight => 0x7c,
        Code::ArrowDown => 0x7d,
        Code::ArrowUp => 0x7e,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_key_tracks_press_and_release_without_modifiers() {
        let mut matcher = ShortcutMatcher::new(
            key_to_macos_key_code(Code::Digit1).expect("digit has a macOS key code"),
            0,
        );

        assert_eq!(matcher.transition(KEY_DOWN, 0x12, 0), Some(true));
        assert_eq!(matcher.transition(KEY_DOWN, 0x12, 0), None);
        assert_eq!(matcher.transition(KEY_UP, 0x12, 0), Some(false));
        assert_eq!(matcher.transition(KEY_DOWN, 0x12, SHIFT_FLAG), None);
    }
}
