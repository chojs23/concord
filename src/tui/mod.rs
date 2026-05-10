mod format;
mod fuzzy;
mod input;
mod login;
mod media;
mod message_format;
mod redraw;
mod requests;
mod selection;
mod state;
mod ui;

#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::Once;
use std::{
    collections::{HashSet, VecDeque},
    io::{Write, stdout},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as TerminalEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::{
    Result, config,
    discord::{AppCommand, AppEvent, DiscordClient, SequencedAppEvent, SnapshotRevision},
    logging,
};

use media::{
    AvatarImageCache, EmojiImageCache, ImagePreviewCache, ImagePreviewDecodeResult,
    spawn_image_preview_decode, visible_avatar_targets, visible_emoji_image_targets,
    visible_image_preview_targets,
};
#[cfg(test)]
use redraw::should_suppress_image_redraw_for_signature_change;
use redraw::{
    RedrawDiagnostics, image_surfaces_visible, record_visible_signature_change,
    should_redraw_after_visible_signature_change, visible_dashboard_signature,
};
use requests::{
    ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberRequests,
    PinnedMessageRequests, ThreadPreviewRequests,
};
use state::{DashboardState, DesktopNotification};

const MAX_DRAINED_EFFECT_EVENTS: usize = 1024;
static NOTIFICATION_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

struct EffectContext<'a> {
    state: &'a mut DashboardState,
    image_previews: &'a mut ImagePreviewCache,
    avatar_images: &'a mut AvatarImageCache,
    emoji_images: &'a mut EmojiImageCache,
    history_requests: &'a mut HistoryRequests,
    forum_post_requests: &'a mut ForumPostRequests,
    pinned_message_requests: &'a mut PinnedMessageRequests,
    thread_preview_requests: &'a mut ThreadPreviewRequests,
    preview_decode_tx: &'a mpsc::UnboundedSender<ImagePreviewDecodeResult>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct EffectProcessingOutcome {
    processed_event: bool,
    force_redraw: bool,
}

impl EffectProcessingOutcome {
    fn processed(event: &AppEvent) -> Self {
        Self {
            processed_event: true,
            force_redraw: effect_forces_redraw(event),
        }
    }

    fn combine(&mut self, other: Self) {
        self.processed_event |= other.processed_event;
        self.force_redraw |= other.force_redraw;
    }
}

pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    login::prompt_login(notice).await
}

pub async fn run(
    mut effects: mpsc::Receiver<SequencedAppEvent>,
    mut snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let _restore_guard = match TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };

    run_dashboard(
        &mut terminal,
        &mut effects,
        &mut snapshots,
        commands,
        client,
    )
    .await
}

pub(super) struct TerminalRestoreGuard {
    keyboard_enhancement_enabled: bool,
    mouse_capture_enabled: bool,
    bracketed_paste_enabled: bool,
}

impl TerminalRestoreGuard {
    pub(super) fn new() -> Result<Self> {
        // Kitty progressive enhancement isn't supported on every terminal
        // (e.g. legacy Windows console). Fall back silently when unavailable
        // so the app still runs with basic key handling.
        let keyboard_enhancement_enabled = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
        let mouse_capture_enabled = execute!(stdout(), EnableMouseCapture).is_ok();
        let bracketed_paste_enabled = execute!(stdout(), EnableBracketedPaste).is_ok();
        Ok(Self {
            keyboard_enhancement_enabled,
            mouse_capture_enabled,
            bracketed_paste_enabled,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        if self.mouse_capture_enabled {
            let _ = execute!(stdout(), DisableMouseCapture);
        }
        if self.bracketed_paste_enabled {
            let _ = execute!(stdout(), DisableBracketedPaste);
        }
        ratatui::restore();
    }
}

fn effect_forces_redraw(event: &AppEvent) -> bool {
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

fn process_effect_event(event: AppEvent, ctx: &mut EffectContext<'_>) -> EffectProcessingOutcome {
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
        logging::debug(target, message);
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
fn applescript_string(value: &str) -> String {
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

fn process_sequenced_effect(
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

fn process_deferred_effects(
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

fn handle_gateway_closed(state: &mut DashboardState) {
    logging::error("tui", "gateway closed");
    state.push_effect(AppEvent::GatewayClosed);
    state.quit();
}

async fn run_dashboard(
    terminal: &mut ratatui::DefaultTerminal,
    effects: &mut mpsc::Receiver<SequencedAppEvent>,
    snapshots: &mut watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
) -> Result<()> {
    let display_options = match config::load_display_options() {
        Ok(options) => options,
        Err(error) => {
            logging::error("config", format!("failed to load config: {error}"));
            config::DisplayOptions::default()
        }
    };
    let mut state = DashboardState::new_with_display_options(display_options);
    drop(snapshots.borrow_and_update());
    let initial_snapshot = client.current_discord_snapshot();
    let mut current_snapshot_revision = initial_snapshot.revision;
    state.restore_discord_snapshot(initial_snapshot.state);
    let mut image_previews = ImagePreviewCache::new();
    let mut avatar_images = AvatarImageCache::new();
    let mut emoji_images = EmojiImageCache::new();
    let mut terminal_events = EventStream::new();
    let mut mouse_clicks = input::MouseClickTracker::default();
    let (preview_decode_tx, mut preview_decode_rx) = mpsc::unbounded_channel();
    let mut history_requests = HistoryRequests::default();
    let mut forum_post_requests = ForumPostRequests::default();
    let mut pinned_message_requests = PinnedMessageRequests::default();
    let mut member_requests = MemberRequests::default();
    let mut thread_preview_requests = ThreadPreviewRequests::default();
    let mut last_member_subscription: Option<(Id<GuildMarker>, Id<ChannelMarker>, u32)> = None;
    let mut requested_author_profiles: HashSet<(Id<UserMarker>, Id<GuildMarker>)> = HashSet::new();
    let mut image_targets = Vec::new();
    let mut avatar_targets = Vec::new();
    let mut emoji_targets = Vec::new();
    let mut deferred_effects = VecDeque::new();
    let mut last_frame_area = Rect::default();
    let mut dirty = true;
    // Diagnostic: count redraws and snapshot-change wakeups per second so we
    // can confirm whether the Discord backend snapshot stream is what's
    // saturating ConPTY with OSC 1337 traffic. Removed once the lag fix lands.
    let mut frames_drawn: u32 = 0;
    let mut snapshot_changes: u32 = 0;
    let mut total_draw_ms: u64 = 0;
    let mut max_draw_ms: u64 = 0;
    let mut redraw_diagnostics = RedrawDiagnostics::default();
    let mut redraw_window_start = std::time::Instant::now();
    // Snapshot/effect-driven redraws are coalesced into the next pending
    // deadline so bursts of background Discord events (presence, typing,
    // off-screen messages) do not each trigger a fresh OSC 1337 emission for
    // every visible image. Key/mouse/image-decode arms still mark `dirty`
    // immediately to keep input responsiveness intact.
    const BACKGROUND_REDRAW_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(80);
    let mut pending_redraw_deadline: Option<tokio::time::Instant> = None;

    while !state.should_quit() {
        if redraw_window_start.elapsed() >= std::time::Duration::from_secs(1) {
            logging::debug(
                "tui",
                format!(
                    "redraws/sec={frames_drawn} snapshot_changes/sec={snapshot_changes} \
                     total_draw_ms={total_draw_ms} max_draw_ms={max_draw_ms} \
                     dirty_key={} dirty_mouse={} dirty_resize={} dirty_terminal_closed={} \
                     preview_decodes={} snapshot_events={} effect_events={} redraw_timer={} \
                     media_requests={} request_failures={} visible_image_previews_max={} \
                     snapshot_msg={} snapshot_member={} snapshot_channel={} snapshot_guild={} \
                     snapshot_popup={}",
                    redraw_diagnostics.key_presses,
                    redraw_diagnostics.mouse_events,
                    redraw_diagnostics.resizes,
                    redraw_diagnostics.terminal_closed,
                    redraw_diagnostics.preview_decodes,
                    redraw_diagnostics.snapshot_events,
                    redraw_diagnostics.effect_events,
                    redraw_diagnostics.redraw_timer_fires,
                    redraw_diagnostics.media_requests,
                    redraw_diagnostics.request_failures,
                    redraw_diagnostics.visible_image_previews_max,
                    redraw_diagnostics.snapshot_message_changes,
                    redraw_diagnostics.snapshot_member_changes,
                    redraw_diagnostics.snapshot_channel_changes,
                    redraw_diagnostics.snapshot_guild_changes,
                    redraw_diagnostics.snapshot_popup_changes,
                ),
            );
            frames_drawn = 0;
            snapshot_changes = 0;
            total_draw_ms = 0;
            max_draw_ms = 0;
            redraw_diagnostics = RedrawDiagnostics::default();
            redraw_window_start = std::time::Instant::now();
        }

        if dirty {
            let draw_start = std::time::Instant::now();
            terminal.draw(|frame| {
                last_frame_area = frame.area();
                ui::sync_view_heights(frame.area(), &mut state);
                let mut preview_layout = ui::image_preview_layout(frame.area(), &state);
                if !state.show_images() {
                    preview_layout.preview_width = 0;
                    preview_layout.max_preview_height = 0;
                    preview_layout.viewer_preview_width = 0;
                    preview_layout.viewer_max_preview_height = 0;
                }
                state.clamp_message_viewport_for_image_previews(
                    preview_layout.content_width,
                    preview_layout.preview_width,
                    preview_layout.max_preview_height,
                );
                image_targets = visible_image_preview_targets(&state, preview_layout);
                redraw_diagnostics.visible_image_previews_max = redraw_diagnostics
                    .visible_image_previews_max
                    .max(image_targets.len());
                avatar_targets = visible_avatar_targets(&state, preview_layout);
                emoji_targets = visible_emoji_image_targets(&state);
                let image_previews = image_previews.render_state(&image_targets);
                let rendered_avatars = avatar_images.render_state(&avatar_targets);
                let rendered_emojis = emoji_images.render_state(&emoji_targets);
                let popup_avatar = state
                    .show_avatars()
                    .then(|| {
                        state
                            .user_profile_popup_avatar_url()
                            .and_then(|url| avatar_images.popup_avatar_image(url))
                    })
                    .flatten();
                ui::render(
                    frame,
                    &state,
                    image_previews,
                    rendered_avatars,
                    rendered_emojis,
                    popup_avatar,
                );
            })?;
            dirty = false;
            let draw_ms = draw_start.elapsed().as_millis() as u64;
            frames_drawn = frames_drawn.saturating_add(1);
            total_draw_ms = total_draw_ms.saturating_add(draw_ms);
            max_draw_ms = max_draw_ms.max(draw_ms);

            for command in state.drain_pending_commands() {
                if commands.send(command).await.is_err() {
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                    break;
                }
            }
            for command in image_previews.next_requests(&image_targets) {
                redraw_diagnostics.media_requests =
                    redraw_diagnostics.media_requests.saturating_add(1);
                if commands.send(command).await.is_err() {
                    redraw_diagnostics.request_failures =
                        redraw_diagnostics.request_failures.saturating_add(1);
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                    break;
                }
                dirty = true;
            }
            for command in avatar_images.next_requests(&avatar_targets) {
                redraw_diagnostics.media_requests =
                    redraw_diagnostics.media_requests.saturating_add(1);
                if commands.send(command).await.is_err() {
                    redraw_diagnostics.request_failures =
                        redraw_diagnostics.request_failures.saturating_add(1);
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                    break;
                }
                dirty = true;
            }
            // Profile popup avatar isn't part of the message-pane targets, so
            // schedule its fetch separately. It uses a larger avatar CDN size
            // than message-pane avatars, so it may have its own cache entry.
            if state.show_avatars()
                && let Some(url) = state.user_profile_popup_avatar_url().map(str::to_owned)
                && let Some(command) = avatar_images.next_request_for_url(&url)
            {
                redraw_diagnostics.media_requests =
                    redraw_diagnostics.media_requests.saturating_add(1);
                if commands.send(command).await.is_err() {
                    redraw_diagnostics.request_failures =
                        redraw_diagnostics.request_failures.saturating_add(1);
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                }
            }
            for command in emoji_images.next_requests(&emoji_targets) {
                redraw_diagnostics.media_requests =
                    redraw_diagnostics.media_requests.saturating_add(1);
                if commands.send(command).await.is_err() {
                    redraw_diagnostics.request_failures =
                        redraw_diagnostics.request_failures.saturating_add(1);
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                    break;
                }
                dirty = true;
            }
        }

        tokio::select! {
            maybe_event = terminal_events.next() => {
                match maybe_event {
                    Some(Ok(TerminalEvent::Key(key))) => {
                        if let Some(command) = input::handle_key(&mut state, key)
                            && commands.send(command).await.is_err()
                        {
                            logging::error("tui", "command channel closed");
                            state.push_effect(AppEvent::GatewayError {
                                message: "command channel closed".to_owned(),
                            });
                        }
                        if key.kind == KeyEventKind::Press {
                            save_display_options_if_needed(&mut state);
                            redraw_diagnostics.key_presses =
                                redraw_diagnostics.key_presses.saturating_add(1);
                            dirty = true;
                        }
                    }
                    Some(Ok(TerminalEvent::Mouse(mouse))) => {
                        let outcome = input::handle_mouse_event(
                            &mut state,
                            mouse,
                            last_frame_area,
                            &mut mouse_clicks,
                        );
                        if let Some(command) = outcome.command
                            && commands.send(command).await.is_err()
                        {
                            logging::error("tui", "command channel closed");
                            state.push_effect(AppEvent::GatewayError {
                                message: "command channel closed".to_owned(),
                            });
                        }
                        if outcome.handled {
                            redraw_diagnostics.mouse_events =
                                redraw_diagnostics.mouse_events.saturating_add(1);
                            dirty = true;
                        }
                    }
                    Some(Ok(TerminalEvent::Resize(width, height))) => {
                        last_frame_area = Rect::new(0, 0, width, height);
                        redraw_diagnostics.resizes = redraw_diagnostics.resizes.saturating_add(1);
                        dirty = true;
                    }
                    Some(Ok(TerminalEvent::Paste(text))) => {
                        if input::handle_paste(&mut state, &text) {
                            dirty = true;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.into()),
                    None => {
                        state.quit();
                        redraw_diagnostics.terminal_closed =
                            redraw_diagnostics.terminal_closed.saturating_add(1);
                        dirty = true;
                    }
                }
            }
            Some(result) = preview_decode_rx.recv() => {
                redraw_diagnostics.preview_decodes =
                    redraw_diagnostics.preview_decodes.saturating_add(1);
                image_previews.store_decoded(result);
                if pending_redraw_deadline.is_none() {
                    pending_redraw_deadline =
                        Some(tokio::time::Instant::now() + BACKGROUND_REDRAW_DEBOUNCE);
                }
            }
            snapshot_changed = snapshots.changed() => {
                redraw_diagnostics.snapshot_events =
                    redraw_diagnostics.snapshot_events.saturating_add(1);
                snapshot_changes = snapshot_changes.saturating_add(1);
                let should_redraw_for_snapshot = match snapshot_changed {
                    Ok(()) => {
                        let before_signature = visible_dashboard_signature(&state);
                        drop(snapshots.borrow_and_update());
                        let snapshot = client.current_discord_snapshot();
                        current_snapshot_revision = snapshot.revision;
                        state.restore_discord_snapshot(snapshot.state);
                        let mut ctx = EffectContext {
                            state: &mut state,
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            history_requests: &mut history_requests,
                            forum_post_requests: &mut forum_post_requests,
                            pinned_message_requests: &mut pinned_message_requests,
                            thread_preview_requests: &mut thread_preview_requests,
                            preview_decode_tx: &preview_decode_tx,
                        };
                        let deferred_outcome = process_deferred_effects(
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        );
                        let after_signature = visible_dashboard_signature(&state);
                        let signature_changed = before_signature != after_signature;
                        if signature_changed {
                            record_visible_signature_change(
                                &mut redraw_diagnostics,
                                &before_signature,
                                &after_signature,
                            );
                        }
                        let images_visible = image_surfaces_visible(
                            &state,
                            !image_targets.is_empty(),
                            !avatar_targets.is_empty(),
                            !emoji_targets.is_empty(),
                        );
                        should_redraw_after_visible_signature_change(
                            &before_signature,
                            &after_signature,
                            images_visible,
                            deferred_outcome.force_redraw,
                        )
                    }
                    Err(_) => {
                        logging::error("tui", "snapshot stream closed");
                        state.quit();
                        true
                    }
                };
                if should_redraw_for_snapshot && pending_redraw_deadline.is_none() {
                    pending_redraw_deadline =
                        Some(tokio::time::Instant::now() + BACKGROUND_REDRAW_DEBOUNCE);
                }
            }
            maybe_effect = effects.recv() => {
                match maybe_effect {
                    Some(effect) => {
                        redraw_diagnostics.effect_events =
                            redraw_diagnostics.effect_events.saturating_add(1);
                        let before_signature = visible_dashboard_signature(&state);
                        let mut effect_outcome = EffectProcessingOutcome::default();
                        let mut ctx = EffectContext {
                            state: &mut state,
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            history_requests: &mut history_requests,
                            forum_post_requests: &mut forum_post_requests,
                            pinned_message_requests: &mut pinned_message_requests,
                            thread_preview_requests: &mut thread_preview_requests,
                            preview_decode_tx: &preview_decode_tx,
                        };
                        effect_outcome.combine(process_sequenced_effect(
                            effect,
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        ));
                        for _ in 0..MAX_DRAINED_EFFECT_EVENTS {
                            match effects.try_recv() {
                                Ok(effect) => effect_outcome.combine(process_sequenced_effect(
                                        effect,
                                        current_snapshot_revision,
                                        &mut deferred_effects,
                                        &mut ctx,
                                    )),
                                Err(mpsc::error::TryRecvError::Empty) => break,
                                Err(mpsc::error::TryRecvError::Disconnected) => {
                                    effect_outcome.combine(process_effect_event(
                                        AppEvent::GatewayClosed,
                                        &mut ctx,
                                    ));
                                    break;
                                }
                            }
                        }
                        let after_signature = visible_dashboard_signature(&state);
                        let images_visible = image_surfaces_visible(
                            &state,
                            !image_targets.is_empty(),
                            !avatar_targets.is_empty(),
                            !emoji_targets.is_empty(),
                        );
                        let should_redraw_for_effects = effect_outcome.processed_event
                            && should_redraw_after_visible_signature_change(
                                &before_signature,
                                &after_signature,
                                images_visible,
                                effect_outcome.force_redraw,
                            );
                        if should_redraw_for_effects && pending_redraw_deadline.is_none() {
                            pending_redraw_deadline = Some(
                                tokio::time::Instant::now() + BACKGROUND_REDRAW_DEBOUNCE,
                            );
                        }
                    }
                    None => {
                        handle_gateway_closed(&mut state);
                        dirty = true;
                    }
                }
            }
            _ = async {
                match pending_redraw_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                pending_redraw_deadline = None;
                redraw_diagnostics.redraw_timer_fires =
                    redraw_diagnostics.redraw_timer_fires.saturating_add(1);
                dirty = true;
            }
        }

        if let Some(channel_id) = history_requests.next(state.selected_message_history_channel_id())
            && commands
                .send(AppCommand::LoadMessageHistory {
                    channel_id,
                    before: None,
                })
                .await
                .is_err()
        {
            history_requests.mark_failed(channel_id);
            logging::error("tui", "command channel closed");
            state.push_effect(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
            dirty = true;
        }

        if let Some(channel_id) =
            pinned_message_requests.next(state.selected_message_history_channel_id())
            && commands
                .send(AppCommand::LoadPinnedMessages { channel_id })
                .await
                .is_err()
        {
            pinned_message_requests.mark_failed(channel_id);
            logging::error("tui", "command channel closed");
            state.push_effect(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
            dirty = true;
        }

        let forum_post_target = state.selected_forum_channel_with_load_more().map(
            |(guild_id, channel_id, should_load_more)| ForumPostRequestTarget {
                guild_id,
                channel_id,
                should_load_more,
            },
        );
        if let Some((guild_id, channel_id, archive_state, offset)) =
            forum_post_requests.next(forum_post_target)
            && commands
                .send(AppCommand::LoadForumPosts {
                    guild_id,
                    channel_id,
                    archive_state,
                    offset,
                })
                .await
                .is_err()
        {
            forum_post_requests.mark_failed(channel_id, archive_state, offset);
            logging::error("tui", "command channel closed");
            state.push_effect(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
            dirty = true;
        }

        if let Some(guild_id) = member_requests.next(state.selected_guild_id()) {
            if commands
                .send(AppCommand::LoadGuildMembers { guild_id })
                .await
                .is_err()
            {
                member_requests.remove(guild_id);
                logging::error("tui", "command channel closed");
                state.push_effect(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }

            // The op-8 RequestGuildMembers above is unreliable for user
            // tokens in larger guilds. Send an op-37 subscription against any
            // text channel as well so Discord starts streaming
            // `GUILD_MEMBER_LIST_UPDATE` events into the sidebar even before
            // the user opens a channel.
            if let Some(channel_id) = state.guild_member_list_channel(guild_id)
                && commands
                    .send(AppCommand::SubscribeGuildChannel {
                        guild_id,
                        channel_id,
                    })
                    .await
                    .is_err()
            {
                logging::error("tui", "command channel closed");
                state.push_effect(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }
        }

        for (user_id, guild_id) in state.missing_message_author_profile_requests() {
            if !requested_author_profiles.insert((user_id, guild_id)) {
                continue;
            }
            if commands
                .send(AppCommand::LoadUserProfile {
                    user_id,
                    guild_id: Some(guild_id),
                })
                .await
                .is_err()
            {
                logging::error("tui", "command channel closed");
                state.push_effect(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }
        }

        for (channel_id, latest_message_id) in
            thread_preview_requests.next(state.missing_thread_preview_load_requests())
        {
            if commands
                .send(AppCommand::LoadThreadPreview {
                    channel_id,
                    message_id: latest_message_id,
                })
                .await
                .is_err()
            {
                thread_preview_requests.remove((channel_id, latest_message_id));
                logging::error("tui", "command channel closed");
                state.push_effect(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }
        }

        // Resubscribe the member-list ranges whenever the user scrolls into a
        // new 100-member bucket so Discord keeps shipping fresh member rows
        // and presence events for what's actually visible.
        if let Some((guild_id, channel_id)) = state.member_list_subscription_target() {
            let bucket = state.member_subscription_top_bucket();
            let needs_update = match last_member_subscription {
                Some((prev_guild, prev_channel, prev_bucket)) => {
                    prev_guild != guild_id || prev_channel != channel_id || prev_bucket != bucket
                }
                None => bucket > 0,
            };
            if needs_update {
                let ranges = state.member_subscription_ranges();
                if commands
                    .send(AppCommand::UpdateMemberListSubscription {
                        guild_id,
                        channel_id,
                        ranges,
                    })
                    .await
                    .is_err()
                {
                    logging::error("tui", "command channel closed");
                    state.push_effect(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                } else {
                    last_member_subscription = Some((guild_id, channel_id, bucket));
                }
            }
        }
    }

    Ok(())
}

fn save_display_options_if_needed(state: &mut DashboardState) {
    let Some(options) = state.take_display_options_save_request() else {
        return;
    };

    match config::save_display_options(&options) {
        Ok(()) => state.push_effect(AppEvent::StatusMessage {
            message: "Options saved.".to_owned(),
        }),
        Err(error) => state.push_effect(AppEvent::GatewayError {
            message: format!("save options failed: {error}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker},
    };
    use crate::discord::{AppEvent, ChannelInfo, MessageKind, ReadStateInfo, SequencedAppEvent};

    use super::{
        AvatarImageCache, EffectContext, EmojiImageCache, ForumPostRequests, HistoryRequests,
        ImagePreviewCache, PinnedMessageRequests, ThreadPreviewRequests, applescript_string,
        effect_forces_redraw, process_deferred_effects, process_sequenced_effect,
        should_redraw_after_visible_signature_change,
        should_suppress_image_redraw_for_signature_change, visible_dashboard_signature,
    };
    use crate::tui::state::{DashboardState, FocusPane};

    #[test]
    fn effect_waits_until_snapshot_revision_catches_up() {
        let mut state = DashboardState::new();
        let mut image_previews = ImagePreviewCache::new();
        let mut avatar_images = AvatarImageCache::new();
        let mut emoji_images = EmojiImageCache::new();
        let mut history_requests = HistoryRequests::default();
        let mut forum_post_requests = ForumPostRequests::default();
        let mut pinned_message_requests = PinnedMessageRequests::default();
        let mut thread_preview_requests = ThreadPreviewRequests::default();
        let (preview_decode_tx, _preview_decode_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut deferred_effects = VecDeque::new();

        {
            let mut ctx = EffectContext {
                state: &mut state,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                history_requests: &mut history_requests,
                forum_post_requests: &mut forum_post_requests,
                pinned_message_requests: &mut pinned_message_requests,
                thread_preview_requests: &mut thread_preview_requests,
                preview_decode_tx: &preview_decode_tx,
            };
            process_sequenced_effect(
                SequencedAppEvent {
                    revision: 2,
                    event: AppEvent::Ready {
                        user: "tester".to_owned(),
                        user_id: None,
                    },
                },
                1,
                &mut deferred_effects,
                &mut ctx,
            );
        }

        assert_eq!(deferred_effects.len(), 1);
        assert_eq!(state.current_user(), None);

        {
            let mut ctx = EffectContext {
                state: &mut state,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                history_requests: &mut history_requests,
                forum_post_requests: &mut forum_post_requests,
                pinned_message_requests: &mut pinned_message_requests,
                thread_preview_requests: &mut thread_preview_requests,
                preview_decode_tx: &preview_decode_tx,
            };
            process_deferred_effects(2, &mut deferred_effects, &mut ctx);
        }

        assert!(deferred_effects.is_empty());
        assert_eq!(state.current_user(), Some("tester"));
    }

    #[test]
    fn applescript_string_escapes_quotes_backslashes_and_newlines() {
        assert_eq!(
            applescript_string("hello \"neo\"\\world\nagain"),
            "\"hello \\\"neo\\\"\\\\world again\""
        );
    }

    #[test]
    fn visible_signature_changes_when_new_messages_notice_count_changes() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_eq!(before.new_messages_count, 0);
        assert_eq!(after.new_messages_count, 1);
        assert_ne!(before, after);
    }

    #[test]
    fn visible_signature_changes_when_update_notice_arrives() {
        let mut state = DashboardState::new();
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::UpdateAvailable {
            latest_version: "9.9.9".to_owned(),
        });
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
    }

    #[test]
    fn new_message_count_only_change_is_suppressed_while_images_are_visible() {
        let mut state = state_with_messages(5);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);
        let mut after = before.clone();
        after.new_messages_count = 1;

        assert_eq!(before.new_messages_count, 0);
        assert_eq!(after.new_messages_count, 1);
        assert!(should_suppress_image_redraw_for_signature_change(
            &before, &after, true,
        ));
        assert!(!should_suppress_image_redraw_for_signature_change(
            &before, &after, false,
        ));
        assert!(!should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, true,
        ));
    }

    #[test]
    fn visible_channel_activity_redraws_while_images_are_visible() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert_eq!(before.visible_messages, after.visible_messages);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
    }

    #[test]
    fn visible_sidebar_unread_state_redraws_while_images_are_visible() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.push_event(AppEvent::ReadStateInit {
            entries: vec![read_state(2, Some(10), 0)],
        });
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::ReadStateInit {
            entries: vec![read_state(2, Some(10), 1)],
        });
        let after = visible_dashboard_signature(&state);

        assert_eq!(before.visible_messages, after.visible_messages);
        assert_ne!(before.visible_channels, after.visible_channels);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));

        let mut state = state_with_active_dm_and_guild();
        state.focus_pane(FocusPane::Messages);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 1);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert_eq!(before.visible_messages, after.visible_messages);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn background_message_activity_redraws_while_channels_are_focused() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        state.focus_pane(FocusPane::Channels);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn visible_message_changes_redraw_even_while_images_are_visible() {
        let mut state = state_with_messages(2);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(8);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 3);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before.visible_messages, after.visible_messages);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn media_effects_force_redraw_without_signature_change() {
        let state = state_with_messages(1);
        let signature = visible_dashboard_signature(&state);
        let loaded = AppEvent::AttachmentPreviewLoaded {
            url: "https://cdn.discordapp.com/avatars/1/hash.png?size=32".to_owned(),
            bytes: Vec::new(),
        };
        let failed = AppEvent::AttachmentPreviewLoadFailed {
            url: "https://cdn.discordapp.com/emoji/1.png".to_owned(),
            message: "failed".to_owned(),
        };

        assert!(effect_forces_redraw(&loaded));
        assert!(effect_forces_redraw(&failed));
        assert!(should_redraw_after_visible_signature_change(
            &signature, &signature, true, true,
        ));
        assert!(!should_redraw_after_visible_signature_change(
            &signature, &signature, true, false,
        ));
    }

    fn state_with_messages(count: u64) -> DashboardState {
        let guild_id: Id<GuildMarker> = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=count {
            push_message(&mut state, id);
        }
        state
    }

    fn state_with_active_dm_and_guild() -> DashboardState {
        let guild_id: Id<GuildMarker> = Id::new(1);
        let guild_channel_id: Id<ChannelMarker> = Id::new(2);
        let dm_channel_id: Id<ChannelMarker> = Id::new(3);
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: guild_channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id: dm_channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "dm".to_owned(),
            kind: "DM".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.push_event(AppEvent::ActivateChannel {
            channel_id: dm_channel_id,
        });
        state
    }

    fn push_message(state: &mut DashboardState, message_id: u64) {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {message_id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }

    fn read_state(
        channel_id: u64,
        last_acked_message_id: Option<u64>,
        mention_count: u32,
    ) -> ReadStateInfo {
        ReadStateInfo {
            channel_id: Id::new(channel_id),
            last_acked_message_id: last_acked_message_id.map(Id::<MessageMarker>::new),
            mention_count,
        }
    }
}
