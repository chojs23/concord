use std::{
    collections::VecDeque,
    io::{Write, stdout},
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::Once;

use tokio::sync::mpsc;

use crate::{
    discord::{AppEvent, SequencedAppEvent},
    logging,
};

use super::{
    media::{
        AvatarImageCache, EmojiImageCache, ImagePreviewCache, ImagePreviewDecodeResult,
        spawn_image_preview_decode,
    },
    requests::{ForumPostRequests, HistoryRequests, PinnedMessageRequests, ThreadPreviewRequests},
    state::{DashboardState, DesktopNotification},
};

pub(super) const MAX_DRAINED_EFFECT_EVENTS: usize = 1024;
static NOTIFICATION_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

pub(super) struct EffectContext<'a> {
    pub(super) state: &'a mut DashboardState,
    pub(super) image_previews: &'a mut ImagePreviewCache,
    pub(super) avatar_images: &'a mut AvatarImageCache,
    pub(super) emoji_images: &'a mut EmojiImageCache,
    pub(super) history_requests: &'a mut HistoryRequests,
    pub(super) forum_post_requests: &'a mut ForumPostRequests,
    pub(super) pinned_message_requests: &'a mut PinnedMessageRequests,
    pub(super) thread_preview_requests: &'a mut ThreadPreviewRequests,
    pub(super) preview_decode_tx: &'a mpsc::UnboundedSender<ImagePreviewDecodeResult>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EffectProcessingOutcome {
    pub(super) processed_event: bool,
    pub(super) force_redraw: bool,
}

impl EffectProcessingOutcome {
    fn processed(event: &AppEvent) -> Self {
        Self {
            processed_event: true,
            force_redraw: effect_forces_redraw(event),
        }
    }

    pub(super) fn combine(&mut self, other: Self) {
        self.processed_event |= other.processed_event;
        self.force_redraw |= other.force_redraw;
    }
}

pub(super) fn effect_forces_redraw(event: &AppEvent) -> bool {
    // Attachment preview events are the shared media-completion path for
    // inline previews, avatars, emoji images, and profile-popup avatars. They
    // must redraw even when the visible dashboard signature is unchanged.
    matches!(
        event,
        AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::GatewayClosed
    )
}

pub(super) fn process_effect_event(
    event: AppEvent,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    let outcome = EffectProcessingOutcome::processed(&event);
    if let Some(notification) = ctx.state.desktop_notification_for_event(&event) {
        dispatch_desktop_notification(notification);
    }
    for job in ctx.image_previews.record_event(&event) {
        spawn_image_preview_decode(job, ctx.preview_decode_tx.clone());
    }
    ctx.avatar_images.record_event(&event);
    ctx.emoji_images.record_event(&event);
    ctx.history_requests.record_event(&event);
    ctx.forum_post_requests.record_event(&event);
    ctx.pinned_message_requests.record_event(&event);
    ctx.thread_preview_requests.record_event(&event);
    if matches!(event, AppEvent::GatewayClosed) {
        handle_gateway_closed(ctx.state);
    } else {
        ctx.state.push_effect(event);
    }
    outcome
}

fn dispatch_desktop_notification(notification: DesktopNotification) {
    tokio::spawn(async move {
        let title = notification.title;
        let body = notification.body;
        let result =
            tokio::task::spawn_blocking(move || deliver_desktop_notification(&title, &body)).await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log_notification_failure_once(
                    "notification",
                    format!("desktop notification and fallbacks failed: {error}"),
                );
                ring_terminal_bell();
            }
            Err(error) => {
                log_notification_failure_once(
                    "notification",
                    format!("desktop notification task failed: {error}"),
                );
                ring_terminal_bell();
            }
        }
    });
}

fn log_notification_failure_once(target: &str, message: String) {
    if !NOTIFICATION_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
        logging::error(target, message);
    }
}

fn deliver_desktop_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        deliver_macos_notification(title, body)
    }
    #[cfg(not(target_os = "macos"))]
    {
        deliver_notify_rust_notification(title, body)
    }
}

#[cfg(not(target_os = "macos"))]
fn deliver_notify_rust_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn deliver_macos_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    init_macos_notification_identity();
    // macOS can accept a notify-rust notification without presenting it or
    // playing its sound when the terminal app is frontmost. Keep every visual
    // notification path silent and let afplay own exactly one audible alert.
    let visual_result = deliver_macos_visual_notification(title, body);
    play_macos_sound_fallback().map_err(|sound_error| match visual_result {
        Ok(()) => format!("macOS notification sound failed: {sound_error}"),
        Err(visual_error) => {
            format!("macOS visual notification failed: {visual_error}; sound failed: {sound_error}")
        }
    })
}

#[cfg(target_os = "macos")]
fn deliver_macos_visual_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    match notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
    {
        Ok(_) => Ok(()),
        Err(primary_error) => {
            deliver_macos_fallback_notification(title, body).map_err(|fallback_error| {
                format!(
                    "notify-rust failed: {primary_error}; macOS fallback failed: {fallback_error}"
                )
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn init_macos_notification_identity() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let Some(app_name) = std::env::var("TERM_PROGRAM")
            .ok()
            .and_then(|program| macos_terminal_app_name(&program))
        else {
            return;
        };
        let bundle_id = notify_rust::get_bundle_identifier_or_default(app_name);
        if bundle_id != "com.apple.Finder" {
            let _ = notify_rust::set_application(&bundle_id);
        }
    });
}

#[cfg(target_os = "macos")]
fn macos_terminal_app_name(term_program: &str) -> Option<&'static str> {
    match term_program {
        "Apple_Terminal" => Some("Terminal"),
        "iTerm.app" => Some("iTerm"),
        "WezTerm" => Some("WezTerm"),
        "WarpTerminal" => Some("Warp"),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn deliver_macos_fallback_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    run_terminal_notifier(title, body).or_else(|terminal_error| {
        run_osascript_notification(title, body)
            .map_err(|osascript_error| format!("{terminal_error}; {osascript_error}"))
    })
}

#[cfg(target_os = "macos")]
fn run_terminal_notifier(title: &str, body: &str) -> std::result::Result<(), String> {
    command_success(
        Command::new("terminal-notifier")
            .args(["-title", title, "-message", body, "-group", "concord"]),
        "terminal-notifier",
    )
}

#[cfg(target_os = "macos")]
fn run_osascript_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title),
    );
    command_success(Command::new("osascript").args(["-e", &script]), "osascript")
}

#[cfg(target_os = "macos")]
fn play_macos_sound_fallback() -> std::result::Result<(), String> {
    command_success(
        Command::new("afplay").arg("/System/Library/Sounds/Ping.aiff"),
        "afplay",
    )
}

#[cfg(target_os = "macos")]
fn command_success(command: &mut Command, label: &str) -> std::result::Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{label} exited with {status}")),
        Err(error) => Err(format!("{label} failed to start: {error}")),
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn applescript_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' | '\r' => escaped.push(' '),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

fn ring_terminal_bell() {
    let mut output = stdout();
    let _ = output.write_all(b"\x07");
    let _ = output.flush();
}

pub(super) fn process_sequenced_effect(
    event: SequencedAppEvent,
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    if event.revision > current_snapshot_revision {
        deferred_effects.push_back(event);
        return EffectProcessingOutcome::default();
    }
    process_effect_event(event.event, ctx)
}

pub(super) fn process_deferred_effects(
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    let mut outcome = EffectProcessingOutcome::default();
    for _ in 0..deferred_effects.len() {
        let Some(event) = deferred_effects.pop_front() else {
            break;
        };
        outcome.combine(process_sequenced_effect(
            event,
            current_snapshot_revision,
            deferred_effects,
            ctx,
        ));
    }
    outcome
}

pub(super) fn handle_gateway_closed(state: &mut DashboardState) {
    logging::error("tui", "gateway closed");
    state.push_effect(AppEvent::GatewayClosed);
    state.quit();
}
