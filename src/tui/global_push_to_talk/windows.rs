use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT,
            LLKHF_EXTENDED, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
            WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

const SHIFT_MODIFIER: u8 = 1 << 0;
const CONTROL_MODIFIER: u8 = 1 << 1;
const ALT_MODIFIER: u8 = 1 << 2;
const SUPER_MODIFIER: u8 = 1 << 3;

thread_local! {
    static HOOK_CONTEXT: RefCell<Option<HookContext>> = const { RefCell::new(None) };
}

pub(super) struct PushToTalkListener {
    events: Receiver<bool>,
    worker_thread_id: u32,
    worker_running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PushToTalkListener {
    pub(super) fn start(hotkey: HotKey) -> Result<Self, String> {
        let virtual_key = key_to_virtual_key(hotkey.key).ok_or_else(|| {
            format!(
                "Push-to-talk key is not supported by the Windows input monitor: {}",
                hotkey.key
            )
        })?;
        let matcher = ShortcutMatcher::new(virtual_key, modifier_flags(hotkey.mods));
        let (events_tx, events) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker_running = Arc::new(AtomicBool::new(true));
        let worker_running_on_thread = Arc::clone(&worker_running);
        let worker = thread::Builder::new()
            .name("push-to-talk-input".to_owned())
            .spawn(move || {
                run_keyboard_hook(matcher, events_tx, worker_running_on_thread, startup_tx);
            })
            .map_err(|error| {
                format!("Could not start the Windows push-to-talk monitor: {error}")
            })?;

        match startup_rx.recv() {
            Ok(Ok(worker_thread_id)) => Ok(Self {
                events,
                worker_thread_id,
                worker_running,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("The Windows push-to-talk monitor stopped during startup".to_owned())
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
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };

        if self.worker_running.load(Ordering::Acquire) {
            let posted = unsafe { PostThreadMessageW(self.worker_thread_id, WM_QUIT, 0, 0) };
            if posted == 0 {
                self.worker = Some(worker);
                return Err("Could not stop the Windows push-to-talk monitor".to_owned());
            }
        }

        worker
            .join()
            .map_err(|_| "The Windows push-to-talk monitor thread panicked".to_owned())
    }
}

impl Drop for PushToTalkListener {
    fn drop(&mut self) {
        let _ = self.stop_worker();
    }
}

struct HookContext {
    matcher: ShortcutMatcher,
    modifiers: ModifierState,
    events: mpsc::Sender<bool>,
}

fn run_keyboard_hook(
    matcher: ShortcutMatcher,
    events: mpsc::Sender<bool>,
    worker_running: Arc<AtomicBool>,
    startup: mpsc::SyncSender<Result<u32, String>>,
) {
    let worker_thread_id = unsafe { GetCurrentThreadId() };
    let mut message: MSG = unsafe { std::mem::zeroed() };

    // Creating the queue before reporting startup makes `PostThreadMessageW`
    // reliable even if the PTT configuration changes immediately.
    unsafe {
        let _ = PeekMessageW(&raw mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }

    HOOK_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(HookContext {
            matcher,
            modifiers: ModifierState::current(),
            events,
        });
    });

    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module.cast(), 0) };
    if hook.is_null() {
        HOOK_CONTEXT.with(|slot| {
            slot.borrow_mut().take();
        });
        worker_running.store(false, Ordering::Release);
        let _ = startup.send(Err(
            "Could not install the Windows push-to-talk keyboard monitor".to_owned(),
        ));
        return;
    }

    if startup.send(Ok(worker_thread_id)).is_err() {
        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
        HOOK_CONTEXT.with(|slot| {
            slot.borrow_mut().take();
        });
        worker_running.store(false, Ordering::Release);
        return;
    }

    unsafe {
        while GetMessageW(&raw mut message, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
    HOOK_CONTEXT.with(|slot| {
        slot.borrow_mut().take();
    });
    worker_running.store(false, Ordering::Release);
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event_type = wparam as u32;
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        HOOK_CONTEXT.with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return;
            };
            let Some(context) = slot.as_mut() else {
                return;
            };

            context.modifiers.update(event_type, event);
            if let Some(pressed) =
                context
                    .matcher
                    .transition(event_type, event.vkCode, context.modifiers.flags())
            {
                let _ = context.events.send(pressed);
            }
        });
    }

    // Returning the next hook result is what keeps the physical key available
    // to the focused application.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[derive(Clone, Copy)]
struct ShortcutMatcher {
    virtual_key: u32,
    modifiers: u8,
    pressed: bool,
}

impl ShortcutMatcher {
    const fn new(virtual_key: u32, modifiers: u8) -> Self {
        Self {
            virtual_key,
            modifiers,
            pressed: false,
        }
    }

    fn transition(&mut self, event_type: u32, virtual_key: u32, modifiers: u8) -> Option<bool> {
        if virtual_key != self.virtual_key {
            return None;
        }

        match event_type {
            WM_KEYDOWN | WM_SYSKEYDOWN if !self.pressed && modifiers == self.modifiers => {
                self.pressed = true;
                Some(true)
            }
            WM_KEYUP | WM_SYSKEYUP if self.pressed => {
                self.pressed = false;
                Some(false)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct ModifierState {
    left_shift: bool,
    right_shift: bool,
    left_control: bool,
    right_control: bool,
    left_alt: bool,
    right_alt: bool,
    left_super: bool,
    right_super: bool,
}

impl ModifierState {
    fn current() -> Self {
        Self {
            left_shift: key_is_down(VK_LSHIFT),
            right_shift: key_is_down(VK_RSHIFT),
            left_control: key_is_down(VK_LCONTROL),
            right_control: key_is_down(VK_RCONTROL),
            left_alt: key_is_down(VK_LMENU),
            right_alt: key_is_down(VK_RMENU),
            left_super: key_is_down(VK_LWIN),
            right_super: key_is_down(VK_RWIN),
        }
    }

    fn update(&mut self, event_type: u32, event: &KBDLLHOOKSTRUCT) {
        let pressed = matches!(event_type, WM_KEYDOWN | WM_SYSKEYDOWN);
        let released = matches!(event_type, WM_KEYUP | WM_SYSKEYUP);
        if !pressed && !released {
            return;
        }

        match event.vkCode as u16 {
            VK_SHIFT => {
                if event.scanCode == 0x36 {
                    self.right_shift = pressed;
                } else {
                    self.left_shift = pressed;
                }
            }
            VK_LSHIFT => self.left_shift = pressed,
            VK_RSHIFT => self.right_shift = pressed,
            VK_CONTROL => {
                if event.flags & LLKHF_EXTENDED != 0 {
                    self.right_control = pressed;
                } else {
                    self.left_control = pressed;
                }
            }
            VK_LCONTROL => self.left_control = pressed,
            VK_RCONTROL => self.right_control = pressed,
            VK_MENU => {
                if event.flags & LLKHF_EXTENDED != 0 {
                    self.right_alt = pressed;
                } else {
                    self.left_alt = pressed;
                }
            }
            VK_LMENU => self.left_alt = pressed,
            VK_RMENU => self.right_alt = pressed,
            VK_LWIN => self.left_super = pressed,
            VK_RWIN => self.right_super = pressed,
            _ => {}
        }
    }

    const fn flags(self) -> u8 {
        (if self.left_shift || self.right_shift {
            SHIFT_MODIFIER
        } else {
            0
        }) | (if self.left_control || self.right_control {
            CONTROL_MODIFIER
        } else {
            0
        }) | (if self.left_alt || self.right_alt {
            ALT_MODIFIER
        } else {
            0
        }) | (if self.left_super || self.right_super {
            SUPER_MODIFIER
        } else {
            0
        })
    }
}

fn key_is_down(virtual_key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(virtual_key)) < 0 }
}

fn modifier_flags(modifiers: Modifiers) -> u8 {
    let mut flags = 0;
    if modifiers.contains(Modifiers::SHIFT) {
        flags |= SHIFT_MODIFIER;
    }
    if modifiers.contains(Modifiers::CONTROL) {
        flags |= CONTROL_MODIFIER;
    }
    if modifiers.contains(Modifiers::ALT) {
        flags |= ALT_MODIFIER;
    }
    if modifiers.intersects(Modifiers::SUPER | Modifiers::META) {
        flags |= SUPER_MODIFIER;
    }
    flags
}

// This virtual-key table is adapted from `global-hotkey` 0.8.0, which is
// licensed under Apache-2.0 OR MIT and maps the same `Code` values.
fn key_to_virtual_key(code: Code) -> Option<u32> {
    Some(u32::from(match code {
        Code::KeyA => VK_A,
        Code::KeyB => VK_B,
        Code::KeyC => VK_C,
        Code::KeyD => VK_D,
        Code::KeyE => VK_E,
        Code::KeyF => VK_F,
        Code::KeyG => VK_G,
        Code::KeyH => VK_H,
        Code::KeyI => VK_I,
        Code::KeyJ => VK_J,
        Code::KeyK => VK_K,
        Code::KeyL => VK_L,
        Code::KeyM => VK_M,
        Code::KeyN => VK_N,
        Code::KeyO => VK_O,
        Code::KeyP => VK_P,
        Code::KeyQ => VK_Q,
        Code::KeyR => VK_R,
        Code::KeyS => VK_S,
        Code::KeyT => VK_T,
        Code::KeyU => VK_U,
        Code::KeyV => VK_V,
        Code::KeyW => VK_W,
        Code::KeyX => VK_X,
        Code::KeyY => VK_Y,
        Code::KeyZ => VK_Z,
        Code::Digit0 => VK_0,
        Code::Digit1 => VK_1,
        Code::Digit2 => VK_2,
        Code::Digit3 => VK_3,
        Code::Digit4 => VK_4,
        Code::Digit5 => VK_5,
        Code::Digit6 => VK_6,
        Code::Digit7 => VK_7,
        Code::Digit8 => VK_8,
        Code::Digit9 => VK_9,
        Code::Equal => VK_OEM_PLUS,
        Code::Comma => VK_OEM_COMMA,
        Code::Minus => VK_OEM_MINUS,
        Code::Period => VK_OEM_PERIOD,
        Code::Semicolon => VK_OEM_1,
        Code::Slash => VK_OEM_2,
        Code::Backquote => VK_OEM_3,
        Code::BracketLeft => VK_OEM_4,
        Code::Backslash => VK_OEM_5,
        Code::BracketRight => VK_OEM_6,
        Code::Quote => VK_OEM_7,
        Code::Backspace => VK_BACK,
        Code::Tab => VK_TAB,
        Code::Space => VK_SPACE,
        Code::Enter | Code::NumpadEnter => VK_RETURN,
        Code::CapsLock => VK_CAPITAL,
        Code::Escape => VK_ESCAPE,
        Code::PageUp => VK_PRIOR,
        Code::PageDown => VK_NEXT,
        Code::End => VK_END,
        Code::Home => VK_HOME,
        Code::ArrowLeft => VK_LEFT,
        Code::ArrowUp => VK_UP,
        Code::ArrowRight => VK_RIGHT,
        Code::ArrowDown => VK_DOWN,
        Code::PrintScreen => VK_SNAPSHOT,
        Code::Insert => VK_INSERT,
        Code::Delete => VK_DELETE,
        Code::F1 => VK_F1,
        Code::F2 => VK_F2,
        Code::F3 => VK_F3,
        Code::F4 => VK_F4,
        Code::F5 => VK_F5,
        Code::F6 => VK_F6,
        Code::F7 => VK_F7,
        Code::F8 => VK_F8,
        Code::F9 => VK_F9,
        Code::F10 => VK_F10,
        Code::F11 => VK_F11,
        Code::F12 => VK_F12,
        Code::F13 => VK_F13,
        Code::F14 => VK_F14,
        Code::F15 => VK_F15,
        Code::F16 => VK_F16,
        Code::F17 => VK_F17,
        Code::F18 => VK_F18,
        Code::F19 => VK_F19,
        Code::F20 => VK_F20,
        Code::F21 => VK_F21,
        Code::F22 => VK_F22,
        Code::F23 => VK_F23,
        Code::F24 => VK_F24,
        Code::NumLock => VK_NUMLOCK,
        Code::Numpad0 => VK_NUMPAD0,
        Code::Numpad1 => VK_NUMPAD1,
        Code::Numpad2 => VK_NUMPAD2,
        Code::Numpad3 => VK_NUMPAD3,
        Code::Numpad4 => VK_NUMPAD4,
        Code::Numpad5 => VK_NUMPAD5,
        Code::Numpad6 => VK_NUMPAD6,
        Code::Numpad7 => VK_NUMPAD7,
        Code::Numpad8 => VK_NUMPAD8,
        Code::Numpad9 => VK_NUMPAD9,
        Code::NumpadAdd => VK_ADD,
        Code::NumpadDecimal => VK_DECIMAL,
        Code::NumpadDivide => VK_DIVIDE,
        Code::NumpadMultiply => VK_MULTIPLY,
        Code::NumpadSubtract => VK_SUBTRACT,
        Code::ScrollLock => VK_SCROLL,
        Code::AudioVolumeDown => VK_VOLUME_DOWN,
        Code::AudioVolumeUp => VK_VOLUME_UP,
        Code::AudioVolumeMute => VK_VOLUME_MUTE,
        Code::MediaPlay => VK_PLAY,
        Code::MediaPause | Code::Pause => VK_PAUSE,
        Code::MediaPlayPause => VK_MEDIA_PLAY_PAUSE,
        Code::MediaStop => VK_MEDIA_STOP,
        Code::MediaTrackNext => VK_MEDIA_NEXT_TRACK,
        Code::MediaTrackPrevious => VK_MEDIA_PREV_TRACK,
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_key_tracks_press_and_release_without_modifiers() {
        let mut matcher = ShortcutMatcher::new(
            key_to_virtual_key(Code::Digit1).expect("digit has a Windows virtual key"),
            0,
        );

        assert_eq!(
            matcher.transition(WM_KEYDOWN, u32::from(VK_1), 0),
            Some(true)
        );
        assert_eq!(matcher.transition(WM_KEYDOWN, u32::from(VK_1), 0), None);
        assert_eq!(
            matcher.transition(WM_KEYUP, u32::from(VK_1), 0),
            Some(false)
        );
        assert_eq!(
            matcher.transition(WM_KEYDOWN, u32::from(VK_1), SHIFT_MODIFIER),
            None
        );
    }
}
