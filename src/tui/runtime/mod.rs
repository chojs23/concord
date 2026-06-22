use std::collections::VecDeque;

use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::{
    Result, config,
    discord::{AppCommand, AppEvent, DiscordClient, SequencedAppEvent, SnapshotRevision},
    logging,
};

use super::{
    clipboard::{ClipboardError, ClipboardPasteData, ClipboardService},
    commands as command_helpers, input,
    state::DashboardState,
};

pub(super) mod effects;
pub(super) mod events;
mod media_runtime;
pub(super) mod notification_audio;
mod redraw_gate;
mod scheduler;

use effects as effect_helpers;
use media_runtime::{
    DashboardMediaRuntime, LocalUploadPreviewResult, clear_image_surfaces_frame,
    drain_pending_commands_after_draw, draw_dashboard_frame, schedule_media_loads_after_draw,
    store_local_upload_preview_result,
};
use scheduler::DashboardCommandScheduler;

type ClipboardPasteResult = std::result::Result<
    std::result::Result<ClipboardPasteData, ClipboardError>,
    tokio::task::JoinError,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DashboardExit {
    Quit,
    SignOut,
}

pub(super) async fn run_dashboard(
    terminal: &mut ratatui::DefaultTerminal,
    effects: &mut mpsc::Receiver<SequencedAppEvent>,
    snapshots: &mut watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
) -> Result<DashboardExit> {
    let options = match config::load_options() {
        Ok(options) => options,
        Err(error) => {
            logging::error("config", format!("failed to load config: {error}"));
            config::AppOptions::default()
        }
    };
    let ui_state_options = match config::load_ui_state_options() {
        Ok(options) => options,
        Err(error) => {
            logging::error("config", format!("failed to load UI state: {error}"));
            config::UiStateOptions::default()
        }
    };
    let keymap_options = match config::load_keymap_options() {
        Ok(options) => options,
        Err(error) => {
            logging::error("config", format!("failed to load keymap config: {error}"));
            config::KeymapOptions::default()
        }
    };
    let mut state = DashboardState::new_with_options(
        options.display,
        options.composer,
        options.credentials,
        options.notifications,
        options.voice,
        keymap_options,
        ui_state_options,
    );
    drop(snapshots.borrow_and_update());
    let initial_snapshot = client.current_discord_snapshot();
    let mut current_snapshot_revision = initial_snapshot.revision.global;
    let mut current_snapshot_area_revision = initial_snapshot.revision;
    state.restore_discord_snapshot(initial_snapshot.to_state());
    let mut media_runtime = DashboardMediaRuntime::new(options.display.image_protocol);
    let mut terminal_events = EventStream::new();
    let mut mouse_clicks = input::MouseClickTracker::default();
    let (media_decode_tx, mut media_decode_rx) = mpsc::unbounded_channel();
    let (local_upload_preview_tx, mut local_upload_preview_rx) =
        mpsc::unbounded_channel::<LocalUploadPreviewResult>();
    let (clipboard_paste_tx, mut clipboard_paste_rx) = mpsc::unbounded_channel();
    let (clipboard_paste_indicator_tx, mut clipboard_paste_indicator_rx) =
        mpsc::unbounded_channel();
    let mut command_scheduler = DashboardCommandScheduler::default();
    let mut deferred_effects = VecDeque::new();
    let mut clipboard = ClipboardService::default();
    let mut last_frame_area = Rect::default();
    let mut dirty = true;
    // Background Discord events (presence, typing, off-screen messages) are
    // coalesced into the next pending deadline so a burst does not schedule a
    // draw per event. Key and mouse arms still mark `dirty` immediately to keep
    // input responsive. Flicker is no longer a reason to suppress redraws: the
    // image emission tracker re-emits a surface only when it actually changes.
    const BACKGROUND_REDRAW_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(80);
    let mut pending_redraw_deadline: Option<tokio::time::Instant> = None;
    let mut clipboard_paste_in_flight = false;
    // Fingerprint of the last drawn frame's background-visible state. Background
    // events only schedule a redraw when this moves (see `redraw_gate`).
    let mut last_view_signature = redraw_gate::view_signature(&state);
    // Tracks where images sit on screen. When it changes between draws an image
    // has moved or been covered/uncovered, and terminal graphics are a pixel
    // layer the cell diff cannot erase on its own, so we paint one image-free
    // frame first to overwrite the stale pixels before redrawing (see
    // `image_layout_signature`). Plain typing/idle never changes it, so they
    // draw a single frame and never blink.
    let mut last_image_layout = u64::MAX;

    while !state.should_quit() {
        if dirty {
            let size = terminal.size()?;
            let image_layout = redraw_gate::image_layout_signature(
                &state,
                Rect::new(0, 0, size.width, size.height),
            );
            if image_layout != last_image_layout {
                terminal.draw(|frame| {
                    last_frame_area = clear_image_surfaces_frame(frame, &mut state);
                })?;
                last_image_layout = image_layout;
            }
            terminal.draw(|frame| {
                last_frame_area = draw_dashboard_frame(frame, &mut state, &mut media_runtime);
            })?;
            dirty = false;
            last_view_signature = redraw_gate::view_signature(&state);

            dirty |= drain_pending_commands_after_draw(&mut state, &commands).await;
            dirty |= schedule_media_loads_after_draw(
                &mut state,
                &mut media_runtime,
                &commands,
                &local_upload_preview_tx,
            )
            .await;
        }

        let pending_read_ack_deadline = client.next_read_ack_deadline();
        let pending_toast_deadline = state.next_toast_deadline();
        let pending_mention_member_search_deadline = client.mention_member_search_deadline();
        let pending_member_list_subscription_deadline = client.member_list_subscription_deadline();

        tokio::select! {
            maybe_event = terminal_events.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        let outcome = events::handle_terminal_event(
                            &mut state,
                            event,
                            &mut last_frame_area,
                            &mut mouse_clicks,
                        )?;
                        if state.take_open_composer_in_editor_request() {
                            if let Err(error) = open_composer_in_editor(terminal, &mut state) {
                                logging::error("tui", format!("editor failed: {error}"));
                            }
                        }
                        if state.take_paste_clipboard_request()
                            && state.accepts_clipboard_paste()
                            && !clipboard_paste_in_flight
                        {
                            clipboard_paste_in_flight = true;
                            let clipboard_paste_tx = clipboard_paste_tx.clone();
                            let clipboard_paste_indicator_tx = clipboard_paste_indicator_tx.clone();
                            tokio::spawn(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    ClipboardService::read_paste_data_with_progress(|| {
                                        let _ = clipboard_paste_indicator_tx.send(());
                                    })
                                })
                                .await;
                                let _ = clipboard_paste_tx.send(result);
                            });
                        }
                        if let Some(content) = state.take_copy_message_content_request() {
                            let now = std::time::Instant::now();
                            match clipboard.copy_text(&content) {
                                Ok(_) => state.show_success_toast("Message copied", now),
                                Err(error) => {
                                    logging::error("tui", format!("copy message failed: {error}"));
                                    state.show_error_toast("Failed to copy message", now);
                                }
                            }
                            dirty = true;
                        }
                        if let Some(command) = outcome.command {
                            match command {
                                AppCommand::PlayMedia { target, request_id } => {
                                    let request_id = request_id.unwrap_or_else(|| {
                                        state.next_media_playback_request_id()
                                    });
                                    state.show_media_playback_preparing_toast(
                                        request_id,
                                        target.url.clone(),
                                    );
                                    state.enqueue_pending_command(AppCommand::PlayMedia {
                                        target,
                                        request_id: Some(request_id),
                                    });
                                    dirty = true;
                                }
                                command => {
                                    let _ = command_helpers::send_or_record_closed(
                                        &mut state, &commands, command,
                                    )
                                    .await;
                                }
                            }
                        }
                        if outcome.dirty {
                            dirty = true;
                        }
                    }
                    Some(Err(error)) => return Err(error.into()),
                    None => {
                        state.quit();
                        dirty = true;
                    }
                }
            }
            Some(result) = media_decode_rx.recv() => {
                media_runtime.store_media_decode(result);
                schedule_background_redraw(&mut pending_redraw_deadline, BACKGROUND_REDRAW_DEBOUNCE);
            }
            Some(result) = local_upload_preview_rx.recv() => {
                store_local_upload_preview_result(
                    &mut state,
                    result.owner,
                    result.attachment_index,
                    result.generation,
                    result.filename,
                    result.result,
                );
                schedule_background_redraw(&mut pending_redraw_deadline, BACKGROUND_REDRAW_DEBOUNCE);
            }
            Some(result) = clipboard_paste_rx.recv() => {
                let was_pending = clipboard_paste_in_flight;
                clipboard_paste_in_flight = false;
                let indicator_was_visible = state.clipboard_paste_pending();
                state.finish_clipboard_paste();
                if was_pending {
                    apply_clipboard_paste_result(&mut state, result);
                    dirty = true;
                } else if indicator_was_visible {
                    dirty = true;
                }
            }
            Some(()) = clipboard_paste_indicator_rx.recv() => {
                if clipboard_paste_in_flight && state.begin_clipboard_paste() {
                    dirty = true;
                }
            }
            snapshot_changed = snapshots.changed() => {
                match snapshot_changed {
                    Ok(()) => {
                        drop(snapshots.borrow_and_update());
                        let snapshot = client.current_discord_snapshot();
                        let previous_snapshot_area_revision = current_snapshot_area_revision;
                        current_snapshot_area_revision = snapshot.revision;
                        current_snapshot_revision = snapshot.revision.global;
                        state.restore_discord_snapshot_areas(
                            &snapshot,
                            previous_snapshot_area_revision,
                        );
                        let mut ctx = media_runtime.effect_context(
                            &mut state,
                            &client,
                            &media_decode_tx,
                        );
                        let deferred_outcome = effect_helpers::process_deferred_effects(
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        );
                        // Only redraw (coalesced) when the snapshot actually moved
                        // something on screen, or for media completions the view
                        // signature cannot see.
                        if deferred_outcome.force_redraw
                            || redraw_gate::view_signature(&state) != last_view_signature
                        {
                            schedule_background_redraw(
                                &mut pending_redraw_deadline,
                                BACKGROUND_REDRAW_DEBOUNCE,
                            );
                        }
                    }
                    Err(_) => {
                        logging::error("tui", "snapshot stream closed");
                        state.quit();
                        dirty = true;
                    }
                }
            }
            maybe_effect = effects.recv() => {
                match maybe_effect {
                    Some(effect) => {
                        let mut effect_outcome = effect_helpers::EffectProcessingOutcome::default();
                        let mut ctx = media_runtime.effect_context(
                            &mut state,
                            &client,
                            &media_decode_tx,
                        );
                        effect_outcome.combine(effect_helpers::process_sequenced_effect(
                            effect,
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        ));
                        for _ in 0..effect_helpers::MAX_DRAINED_EFFECT_EVENTS {
                            match effects.try_recv() {
                                Ok(effect) => effect_outcome.combine(effect_helpers::process_sequenced_effect(
                                        effect,
                                        current_snapshot_revision,
                                        &mut deferred_effects,
                                        &mut ctx,
                                    )),
                                Err(mpsc::error::TryRecvError::Empty) => break,
                                Err(mpsc::error::TryRecvError::Disconnected) => {
                                    effect_outcome.combine(effect_helpers::process_effect_event(
                                        AppEvent::GatewayClosed,
                                        &mut ctx,
                                    ));
                                    break;
                                }
                            }
                        }
                        // Redraw (coalesced) only when a processed event changed
                        // the visible signature, or forces a redraw for media
                        // completions the signature cannot see.
                        if effect_outcome.processed_event
                            && (effect_outcome.force_redraw
                                || redraw_gate::view_signature(&state) != last_view_signature)
                        {
                            schedule_background_redraw(
                                &mut pending_redraw_deadline,
                                BACKGROUND_REDRAW_DEBOUNCE,
                            );
                        }
                    }
                    None => {
                        effect_helpers::handle_gateway_closed(&mut state);
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
                dirty = true;
            }
            _ = async {
                match pending_read_ack_deadline {
                    Some(deadline) => tokio::time::sleep_until(
                        tokio::time::Instant::from_std(deadline),
                    )
                    .await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                for command in client.due_read_ack_commands(std::time::Instant::now()) {
                    if command_helpers::send_or_record_closed(&mut state, &commands, command)
                        .await
                        .is_channel_closed()
                    {
                        break;
                    }
                }
                dirty = true;
            }
            _ = async {
                match pending_mention_member_search_deadline {
                    Some(deadline) => tokio::time::sleep_until(
                        tokio::time::Instant::from_std(deadline),
                    )
                    .await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = async {
                match pending_member_list_subscription_deadline {
                    Some(deadline) => tokio::time::sleep_until(
                        tokio::time::Instant::from_std(deadline),
                    )
                    .await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = async {
                match pending_toast_deadline {
                    Some(deadline) => tokio::time::sleep_until(
                        tokio::time::Instant::from_std(deadline),
                    )
                    .await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                if state.clear_expired_toast(std::time::Instant::now()) {
                    dirty = true;
                }
            }
        }

        dirty |= command_scheduler
            .schedule_state_driven_commands(&mut state, &client, &commands)
            .await;
    }

    if state.should_sign_out() {
        Ok(DashboardExit::SignOut)
    } else {
        Ok(DashboardExit::Quit)
    }
}

fn schedule_background_redraw(
    pending_redraw_deadline: &mut Option<tokio::time::Instant>,
    debounce: std::time::Duration,
) {
    if pending_redraw_deadline.is_none() {
        *pending_redraw_deadline = Some(tokio::time::Instant::now() + debounce);
    }
}

fn apply_clipboard_paste_result(state: &mut DashboardState, result: ClipboardPasteResult) {
    match result {
        Ok(Ok(data)) => {
            let _ = apply_clipboard_paste_data(state, data);
        }
        Ok(Err(error)) => {
            logging::debug("clipboard", format!("clipboard paste unavailable: {error}"));
        }
        Err(error) => {
            logging::debug("clipboard", format!("clipboard paste task failed: {error}"));
        }
    }
}

fn apply_clipboard_paste_data(state: &mut DashboardState, data: ClipboardPasteData) -> bool {
    if state.accepts_user_profile_avatar_paste() {
        if let Some(mut attachments) = data.file_attachments {
            if attachments.is_empty() {
                return false;
            }
            let first = attachments.remove(0);
            return state.set_user_profile_avatar_from_attachment(first);
        }
        if let Some(text) = data.text.as_deref()
            && input::handle_pasted_user_profile_avatar(state, text)
        {
            return true;
        }
        if let Some(attachment) = data.image_attachment {
            return state.set_user_profile_avatar_from_attachment(attachment);
        }
        return false;
    }

    if !state.is_composing() {
        if state.is_forum_post_composer_active() {
            if state.is_forum_post_composer_editing() {
                if state.forum_post_composer_accepts_attachment_paste() {
                    if let Some(attachments) = data.file_attachments {
                        state.add_pending_forum_post_attachments(attachments);
                        return true;
                    }
                    if let Some(text) = data.text.as_deref()
                        && input::handle_pasted_file_attachments(state, text)
                    {
                        return true;
                    }
                    if let Some(attachment) = data.image_attachment {
                        state.add_pending_forum_post_attachments(vec![attachment]);
                        return true;
                    }
                }
                return data
                    .text
                    .as_deref()
                    .is_some_and(|text| input::handle_paste(state, text));
            }
            return false;
        }
        return false;
    }
    if state.composer_accepts_attachments() {
        if let Some(attachments) = data.file_attachments {
            state.add_pending_composer_attachments(attachments);
            return true;
        }
        if let Some(text) = data.text.as_deref()
            && input::handle_pasted_file_attachments(state, text)
        {
            return true;
        }
        if let Some(attachment) = data.image_attachment {
            state.add_pending_composer_attachments(vec![attachment]);
            return true;
        }
    }
    data.text
        .as_deref()
        .is_some_and(|text| input::handle_paste(state, text))
}

fn open_composer_in_editor(
    terminal: &mut ratatui::DefaultTerminal,
    state: &mut DashboardState,
) -> crate::Result<()> {
    use crossterm::event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    };
    use crossterm::execute;
    use std::{env, io::stdout};

    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());

    let mut temp = tempfile::Builder::new()
        .prefix("concord-message-")
        .suffix(".txt")
        .tempfile()?;
    std::io::Write::write_all(&mut temp, state.composer_input().as_bytes())?;
    let path = temp.path().to_path_buf();

    let _ = execute!(
        stdout(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        DisableBracketedPaste,
    );
    ratatui::restore();

    let status = tokio::task::block_in_place(|| {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$1\""))
            .arg("--")
            .arg(&path)
            .status()
    });

    *terminal = ratatui::init();
    let _ = execute!(
        stdout(),
        PushKeyboardEnhancementFlags(super::terminal::keyboard_enhancement_flags()),
        EnableMouseCapture,
        EnableBracketedPaste,
    );

    if let Ok(status) = status
        && status.success()
        && let Ok(content) = std::fs::read_to_string(&path)
    {
        state.replace_composer_input_from_editor(content);
    }
    Ok(())
}
