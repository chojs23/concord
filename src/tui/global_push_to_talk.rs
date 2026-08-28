use std::time::Duration;

use global_hotkey::hotkey::HotKey;
#[cfg(any(test, target_os = "linux"))]
use global_hotkey::hotkey::{Code, Modifiers};
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
#[cfg(target_os = "linux")]
use tokio::task::JoinHandle;

use crate::{config::VoiceOptions, discord::DiscordClient, logging};

const PUSH_TO_TALK_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11 as platform;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PushToTalkConfiguration {
    enabled: bool,
    shortcut: String,
}

pub(super) struct GlobalPushToTalkRuntime {
    client: DiscordClient,
    configuration: Option<PushToTalkConfiguration>,
    backend: Option<PushToTalkBackend>,
    pressed: bool,
}

enum PushToTalkBackend {
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Native {
        manager: GlobalHotKeyManager,
        hotkey: HotKey,
    },
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    Passive(platform::PushToTalkListener),
    #[cfg(target_os = "linux")]
    Portal {
        messages: tokio::sync::mpsc::UnboundedReceiver<PortalMessage>,
        shutdown: Option<tokio::sync::oneshot::Sender<()>>,
        task: JoinHandle<()>,
        fallback_hotkey: HotKey,
    },
}

impl GlobalPushToTalkRuntime {
    pub(super) fn new(client: DiscordClient) -> Self {
        Self {
            client,
            configuration: None,
            backend: None,
            pressed: false,
        }
    }

    pub(super) const fn poll_interval() -> Duration {
        PUSH_TO_TALK_POLL_INTERVAL
    }

    pub(super) fn needs_polling(&self) -> bool {
        self.backend.is_some()
    }

    pub(super) async fn sync(&mut self, options: &VoiceOptions) -> Option<String> {
        let configuration = PushToTalkConfiguration {
            enabled: options.push_to_talk,
            shortcut: options.push_to_talk_shortcut.trim().to_owned(),
        };
        if self.configuration.as_ref() == Some(&configuration) {
            return None;
        }

        self.release();
        self.stop_backend().await;
        self.client.set_push_to_talk_enabled(configuration.enabled);
        self.configuration = Some(configuration.clone());

        if !configuration.enabled {
            return None;
        }

        let hotkey = match configuration.shortcut.parse::<HotKey>() {
            Ok(hotkey) => hotkey,
            Err(error) => {
                return Some(format!("Push-to-talk shortcut is invalid: {error}"));
            }
        };

        #[cfg(target_os = "linux")]
        if is_wayland_session() {
            self.backend = Some(start_portal_backend(hotkey));
            logging::debug(
                "voice",
                format!(
                    "registering push-to-talk shortcut through the desktop portal: {}",
                    configuration.shortcut
                ),
            );
            return None;
        }

        match register_native_hotkey(hotkey) {
            Ok(backend) => {
                logging::debug(
                    "voice",
                    format!(
                        "started global push-to-talk listener: {}",
                        configuration.shortcut
                    ),
                );
                self.backend = Some(backend);
                None
            }
            Err(error) => Some(error),
        }
    }

    pub(super) fn poll(&mut self) -> Option<String> {
        let mut pressed = None;
        #[cfg(target_os = "linux")]
        let mut backend_failed = None;
        match self.backend.as_mut() {
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            Some(PushToTalkBackend::Native { hotkey, .. }) => {
                while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                    if event.id() == hotkey.id() {
                        pressed = Some(event.state() == HotKeyState::Pressed);
                    }
                }
            }
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            Some(PushToTalkBackend::Passive(listener)) => {
                pressed = listener.latest_state();
            }
            #[cfg(target_os = "linux")]
            Some(PushToTalkBackend::Portal {
                messages,
                fallback_hotkey,
                ..
            }) => {
                while let Ok(message) = messages.try_recv() {
                    match message {
                        PortalMessage::Ready(trigger) => {
                            logging::debug(
                                "voice",
                                format!("registered portal push-to-talk shortcut: {trigger}"),
                            );
                        }
                        PortalMessage::Pressed(value) => pressed = Some(value),
                        PortalMessage::Failed(error) => {
                            backend_failed = Some((*fallback_hotkey, error));
                        }
                    }
                }
            }
            None => {}
        }

        if let Some(pressed) = pressed {
            self.set_pressed(pressed);
        }

        #[cfg(target_os = "linux")]
        if let Some((fallback_hotkey, portal_error)) = backend_failed {
            self.release();
            if let Some(PushToTalkBackend::Portal { task, .. }) = self.backend.take() {
                task.abort();
            }
            if std::env::var_os("DISPLAY").is_some() {
                match register_native_hotkey(fallback_hotkey) {
                    Ok(backend) => {
                        logging::debug(
                            "voice",
                            format!(
                                "desktop portal push-to-talk failed, using X11 instead: \
                                 {portal_error}"
                            ),
                        );
                        self.backend = Some(backend);
                        return None;
                    }
                    Err(x11_error) => {
                        return Some(format!(
                            "Global push-to-talk is unavailable. Portal: {portal_error}. \
                             X11: {x11_error}"
                        ));
                    }
                }
            }
            return Some(format!(
                "Global push-to-talk is unavailable: {portal_error}"
            ));
        }

        None
    }

    pub(super) async fn shutdown(&mut self) {
        self.release();
        self.stop_backend().await;
    }

    fn set_pressed(&mut self, pressed: bool) {
        if self.pressed == pressed {
            return;
        }
        self.pressed = pressed;
        let shortcut = self
            .configuration
            .as_ref()
            .map(|configuration| configuration.shortcut.as_str())
            .unwrap_or("unknown");
        logging::debug(
            "voice",
            format!(
                "push-to-talk {}: shortcut={shortcut}",
                if pressed { "pressed" } else { "released" }
            ),
        );
        self.client.set_push_to_talk_pressed(pressed);
        if self
            .client
            .requested_voice_connection()
            .is_some_and(|voice| {
                voice.allow_microphone_transmit && !voice.self_mute && !voice.self_deaf
            })
        {
            super::runtime::effects::dispatch_push_to_talk_sound(pressed);
        }
    }

    fn release(&mut self) {
        if self.pressed {
            self.pressed = false;
            self.client.set_push_to_talk_pressed(false);
        }
    }

    async fn stop_backend(&mut self) {
        let Some(backend) = self.backend.take() else {
            return;
        };
        match backend {
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            PushToTalkBackend::Native { manager, hotkey } => {
                if let Err(error) = manager.unregister(hotkey) {
                    logging::debug(
                        "voice",
                        format!("failed to unregister push-to-talk shortcut: {error}"),
                    );
                }
            }
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            PushToTalkBackend::Passive(listener) => {
                if let Err(error) = listener.stop() {
                    logging::debug("voice", error);
                }
            }
            #[cfg(target_os = "linux")]
            PushToTalkBackend::Portal {
                mut shutdown,
                mut task,
                ..
            } => {
                if let Some(shutdown) = shutdown.take() {
                    let _ = shutdown.send(());
                }
                if tokio::time::timeout(Duration::from_millis(500), &mut task)
                    .await
                    .is_err()
                {
                    task.abort();
                }
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn register_native_hotkey(hotkey: HotKey) -> std::result::Result<PushToTalkBackend, String> {
    let manager = GlobalHotKeyManager::new()
        .map_err(|error| format!("Could not initialize global push-to-talk: {error}"))?;
    manager
        .register(hotkey)
        .map_err(|error| format!("Could not register global push-to-talk: {error}"))?;
    Ok(PushToTalkBackend::Native { manager, hotkey })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn register_native_hotkey(hotkey: HotKey) -> std::result::Result<PushToTalkBackend, String> {
    platform::PushToTalkListener::start(hotkey).map(PushToTalkBackend::Passive)
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum PortalMessage {
    Ready(String),
    Pressed(bool),
    Failed(String),
}

#[cfg(target_os = "linux")]
fn start_portal_backend(hotkey: HotKey) -> PushToTalkBackend {
    let (messages_tx, messages) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        if let Err(error) = run_portal_shortcut(hotkey, shutdown_rx, messages_tx.clone()).await {
            let _ = messages_tx.send(PortalMessage::Failed(error));
        }
    });
    PushToTalkBackend::Portal {
        messages,
        shutdown: Some(shutdown_tx),
        task,
        fallback_hotkey: hotkey,
    }
}

#[cfg(target_os = "linux")]
async fn run_portal_shortcut(
    hotkey: HotKey,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
    messages: tokio::sync::mpsc::UnboundedSender<PortalMessage>,
) -> std::result::Result<(), String> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures::StreamExt;

    const SHORTCUT_ID: &str = "concord-push-to-talk";

    let portal = GlobalShortcuts::new()
        .await
        .map_err(|error| error.to_string())?;
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|error| error.to_string())?;

    let result = async {
        let mut activated = portal
            .receive_activated()
            .await
            .map_err(|error| error.to_string())?;
        let mut deactivated = portal
            .receive_deactivated()
            .await
            .map_err(|error| error.to_string())?;
        let preferred_trigger = hotkey_to_xdg_trigger(hotkey);
        let shortcut = NewShortcut::new(SHORTCUT_ID, "Concord push to talk")
            .preferred_trigger(preferred_trigger.as_deref());
        let request = portal
            .bind_shortcuts(&session, &[shortcut], None, Default::default())
            .await
            .map_err(|error| error.to_string())?;
        let response = request.response().map_err(|error| error.to_string())?;
        let Some(shortcut) = response
            .shortcuts()
            .iter()
            .find(|shortcut| shortcut.id() == SHORTCUT_ID)
        else {
            return Err("the desktop portal did not bind the shortcut".to_owned());
        };
        let _ = messages.send(PortalMessage::Ready(
            shortcut.trigger_description().to_owned(),
        ));

        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                event = activated.next() => {
                    let Some(event) = event else {
                        return Err("the desktop portal activation stream closed".to_owned());
                    };
                    if event.shortcut_id() == SHORTCUT_ID {
                        let _ = messages.send(PortalMessage::Pressed(true));
                    }
                }
                event = deactivated.next() => {
                    let Some(event) = event else {
                        return Err("the desktop portal deactivation stream closed".to_owned());
                    };
                    if event.shortcut_id() == SHORTCUT_ID {
                        let _ = messages.send(PortalMessage::Pressed(false));
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    let _ = session.close().await;
    result
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|session_type| session_type.eq_ignore_ascii_case("wayland"))
}

#[cfg(any(test, target_os = "linux"))]
fn hotkey_to_xdg_trigger(hotkey: HotKey) -> Option<String> {
    let mut parts = Vec::new();
    if hotkey.mods.contains(Modifiers::CONTROL) {
        parts.push("CTRL".to_owned());
    }
    if hotkey.mods.contains(Modifiers::SHIFT) {
        parts.push("SHIFT".to_owned());
    }
    if hotkey.mods.contains(Modifiers::ALT) {
        parts.push("ALT".to_owned());
    }
    if hotkey.mods.contains(Modifiers::SUPER) {
        parts.push("LOGO".to_owned());
    }
    parts.push(xdg_key_name(hotkey.key)?.to_owned());
    Some(parts.join("+"))
}

#[cfg(any(test, target_os = "linux"))]
fn xdg_key_name(code: Code) -> Option<&'static str> {
    Some(match code {
        Code::KeyA => "a",
        Code::KeyB => "b",
        Code::KeyC => "c",
        Code::KeyD => "d",
        Code::KeyE => "e",
        Code::KeyF => "f",
        Code::KeyG => "g",
        Code::KeyH => "h",
        Code::KeyI => "i",
        Code::KeyJ => "j",
        Code::KeyK => "k",
        Code::KeyL => "l",
        Code::KeyM => "m",
        Code::KeyN => "n",
        Code::KeyO => "o",
        Code::KeyP => "p",
        Code::KeyQ => "q",
        Code::KeyR => "r",
        Code::KeyS => "s",
        Code::KeyT => "t",
        Code::KeyU => "u",
        Code::KeyV => "v",
        Code::KeyW => "w",
        Code::KeyX => "x",
        Code::KeyY => "y",
        Code::KeyZ => "z",
        Code::Digit0 => "0",
        Code::Digit1 => "1",
        Code::Digit2 => "2",
        Code::Digit3 => "3",
        Code::Digit4 => "4",
        Code::Digit5 => "5",
        Code::Digit6 => "6",
        Code::Digit7 => "7",
        Code::Digit8 => "8",
        Code::Digit9 => "9",
        Code::F1 => "F1",
        Code::F2 => "F2",
        Code::F3 => "F3",
        Code::F4 => "F4",
        Code::F5 => "F5",
        Code::F6 => "F6",
        Code::F7 => "F7",
        Code::F8 => "F8",
        Code::F9 => "F9",
        Code::F10 => "F10",
        Code::F11 => "F11",
        Code::F12 => "F12",
        Code::F13 => "F13",
        Code::F14 => "F14",
        Code::F15 => "F15",
        Code::F16 => "F16",
        Code::F17 => "F17",
        Code::F18 => "F18",
        Code::F19 => "F19",
        Code::F20 => "F20",
        Code::F21 => "F21",
        Code::F22 => "F22",
        Code::F23 => "F23",
        Code::F24 => "F24",
        Code::Enter => "Return",
        Code::Space => "space",
        Code::Escape => "Escape",
        Code::Tab => "Tab",
        Code::Backspace => "BackSpace",
        Code::ArrowUp => "Up",
        Code::ArrowDown => "Down",
        Code::ArrowLeft => "Left",
        Code::ArrowRight => "Right",
        Code::Home => "Home",
        Code::End => "End",
        Code::PageUp => "Page_Up",
        Code::PageDown => "Page_Down",
        Code::Insert => "Insert",
        Code::Delete => "Delete",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::keybindings::push_to_talk_shortcut_from_key;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn xdg_trigger_uses_portal_shortcut_syntax() {
        let hotkey: HotKey = "control+shift+F8".parse().expect("test shortcut is valid");

        assert_eq!(
            hotkey_to_xdg_trigger(hotkey).as_deref(),
            Some("CTRL+SHIFT+F8")
        );
    }

    #[test]
    fn captured_shortcut_is_accepted_by_the_global_backend() {
        let shortcut = push_to_talk_shortcut_from_key(KeyEvent::new(
            KeyCode::F(9),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))
        .expect("captured shortcut should be portable");

        let hotkey: HotKey = shortcut.parse().expect("captured shortcut should parse");

        assert_eq!(
            hotkey_to_xdg_trigger(hotkey).as_deref(),
            Some("CTRL+SHIFT+F9")
        );
    }
}
