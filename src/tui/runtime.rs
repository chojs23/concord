use std::collections::VecDeque;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
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
    commands as command_helpers, effects as effect_helpers, events, input,
    media::{
        AvatarImageCache, EmojiImageCache, ImagePreviewCache, visible_avatar_targets,
        visible_emoji_image_targets, visible_image_preview_targets,
    },
    redraw::{
        image_surfaces_visible, should_redraw_after_visible_signature_change,
        visible_dashboard_signature,
    },
    state::DashboardState,
    ui,
};

type ClipboardPasteResult = std::result::Result<
    std::result::Result<ClipboardPasteData, ClipboardError>,
    tokio::task::JoinError,
>;

pub(super) async fn run_dashboard(
    terminal: &mut ratatui::DefaultTerminal,
    effects: &mut mpsc::Receiver<SequencedAppEvent>,
    snapshots: &mut watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
) -> Result<()> {
    let options = match config::load_options() {
        Ok(options) => options,
        Err(error) => {
            logging::error("config", format!("failed to load config: {error}"));
            config::AppOptions::default()
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
        options.notifications,
        options.voice,
        keymap_options,
        options.ui_state,
    );
    drop(snapshots.borrow_and_update());
    let initial_snapshot = client.current_discord_snapshot();
    let mut current_snapshot_revision = initial_snapshot.revision.global;
    let mut current_snapshot_area_revision = initial_snapshot.revision;
    state.restore_discord_snapshot(initial_snapshot.to_state());
    let mut image_previews = ImagePreviewCache::new();
    let mut avatar_images = AvatarImageCache::new();
    let mut emoji_images = EmojiImageCache::new();
    let mut terminal_events = EventStream::new();
    let mut mouse_clicks = input::MouseClickTracker::default();
    let (preview_decode_tx, mut preview_decode_rx) = mpsc::unbounded_channel();
    let (clipboard_paste_tx, mut clipboard_paste_rx) = mpsc::unbounded_channel();
    let (clipboard_paste_indicator_tx, mut clipboard_paste_indicator_rx) =
        mpsc::unbounded_channel();
    let mut last_reported_active_guild: Option<Id<GuildMarker>> = None;
    let mut last_reported_message_channel: Option<Id<ChannelMarker>> = None;
    let mut image_targets = Vec::new();
    let mut avatar_targets = Vec::new();
    let mut emoji_targets = Vec::new();
    let mut deferred_effects = VecDeque::new();
    let mut clipboard = ClipboardService::default();
    let mut last_frame_area = Rect::default();
    let mut dirty = true;
    // Snapshot/effect-driven redraws are coalesced into the next pending
    // deadline so bursts of background Discord events (presence, typing,
    // off-screen messages) do not each trigger a fresh OSC 1337 emission for
    // every visible image. Key/mouse/image-decode arms still mark `dirty`
    // immediately to keep input responsiveness intact.
    const BACKGROUND_REDRAW_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(80);
    let mut pending_redraw_deadline: Option<tokio::time::Instant> = None;
    let mut clipboard_paste_in_flight = false;

    while !state.should_quit() {
        if dirty {
            terminal.draw(|frame| {
                last_frame_area = frame.area();
                ui::sync_view_heights(frame.area(), &mut state);
                let mut preview_layout = ui::image_preview_layout(frame.area(), &state);
                preview_layout.font_size = image_previews.font_size();
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
                avatar_targets = visible_avatar_targets(&state, preview_layout);
                emoji_targets = visible_emoji_image_targets(&state);
                let image_previews = image_previews.render_state(&image_targets);
                let rendered_emojis = emoji_images.render_state(&emoji_targets);
                let popup_avatar_url = state
                    .show_avatars()
                    .then(|| state.user_profile_popup_avatar_url())
                    .flatten();
                let (rendered_avatars, popup_avatar) = avatar_images.render_state_with_popup(
                    &avatar_targets,
                    popup_avatar_url,
                    state.circular_avatars(),
                );
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

            for command in state.drain_pending_commands() {
                if commands.send(command).await.is_err() {
                    command_helpers::record_command_channel_closed(&mut state);
                    dirty = true;
                    break;
                }
            }
            for command in image_previews.next_requests(&image_targets) {
                if commands.send(command).await.is_err() {
                    command_helpers::record_command_channel_closed(&mut state);
                    dirty = true;
                    break;
                }
                dirty = true;
            }
            for command in avatar_images.next_requests(&avatar_targets) {
                if commands.send(command).await.is_err() {
                    command_helpers::record_command_channel_closed(&mut state);
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
                && commands.send(command).await.is_err()
            {
                command_helpers::record_command_channel_closed(&mut state);
                dirty = true;
            }
            for command in emoji_images.next_requests(&emoji_targets) {
                if commands.send(command).await.is_err() {
                    command_helpers::record_command_channel_closed(&mut state);
                    dirty = true;
                    break;
                }
                dirty = true;
            }
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
                            && state.is_composing()
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
                        if let Some(command) = outcome.command
                            && commands.send(command).await.is_err()
                        {
                            command_helpers::record_command_channel_closed(&mut state);
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
            Some(result) = preview_decode_rx.recv() => {
                image_previews.store_decoded(result);
                if pending_redraw_deadline.is_none() {
                    pending_redraw_deadline =
                        Some(tokio::time::Instant::now() + BACKGROUND_REDRAW_DEBOUNCE);
                }
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
                let should_redraw_for_snapshot = match snapshot_changed {
                    Ok(()) => {
                        let before_signature = visible_dashboard_signature(&state);
                        drop(snapshots.borrow_and_update());
                        let snapshot = client.current_discord_snapshot();
                        let previous_snapshot_area_revision = current_snapshot_area_revision;
                        current_snapshot_area_revision = snapshot.revision;
                        current_snapshot_revision = snapshot.revision.global;
                        state.restore_discord_snapshot_areas(
                            &snapshot,
                            previous_snapshot_area_revision,
                        );
                        let mut ctx = effect_helpers::EffectContext {
                            state: &mut state,
                            client: &client,
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            preview_decode_tx: &preview_decode_tx,
                        };
                        let deferred_outcome = effect_helpers::process_deferred_effects(
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        );
                        let after_signature = visible_dashboard_signature(&state);
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
                        let before_signature = visible_dashboard_signature(&state);
                        let mut effect_outcome = effect_helpers::EffectProcessingOutcome::default();
                        let mut ctx = effect_helpers::EffectContext {
                            state: &mut state,
                            client: &client,
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            preview_decode_tx: &preview_decode_tx,
                        };
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
                    if commands
                        .send(command)
                        .await
                        .is_err()
                    {
                        command_helpers::record_command_channel_closed(&mut state);
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

        client.set_mention_member_search_target(
            state.selected_guild_id(),
            state.composer_mention_query(),
            std::time::Instant::now(),
        );
        if let Some((guild_id, query)) =
            client.next_due_mention_member_search(std::time::Instant::now())
            && commands
                .send(AppCommand::SearchGuildMembers { guild_id, query })
                .await
                .is_err()
        {
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }

        if let Some(channel_id) = client.next_message_history_request(
            state.selected_message_history_channel_id(),
            state.selected_message_history_needs_reload(),
        ) && commands
            .send(AppCommand::LoadMessageHistory {
                channel_id,
                before: None,
            })
            .await
            .is_err()
        {
            client.mark_message_history_request_failed(channel_id);
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }

        let active_guild = state.selected_guild_id();
        if active_guild != last_reported_active_guild {
            last_reported_active_guild = active_guild;
            if commands
                .send(AppCommand::SetSelectedGuild {
                    guild_id: active_guild,
                })
                .await
                .is_err()
            {
                command_helpers::record_command_channel_closed(&mut state);
                dirty = true;
            }
        }

        let active_message_channel = state.selected_message_history_channel_id();
        if active_message_channel != last_reported_message_channel {
            last_reported_message_channel = active_message_channel;
            if commands
                .send(AppCommand::SetSelectedMessageChannel {
                    channel_id: active_message_channel,
                })
                .await
                .is_err()
            {
                command_helpers::record_command_channel_closed(&mut state);
                dirty = true;
            }
        }

        if let Some(channel_id) =
            client.next_pinned_message_request(state.pinned_message_view_channel_id())
            && commands
                .send(AppCommand::LoadPinnedMessages { channel_id })
                .await
                .is_err()
        {
            client.mark_pinned_message_request_failed(channel_id);
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }

        if let Some((guild_id, channel_id, archive_state, offset)) =
            client.next_forum_post_request(state.selected_forum_channel_with_load_more())
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
            client.mark_forum_post_request_failed(channel_id, archive_state, offset);
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }

        if let Some(guild_id) = client.next_member_request(state.selected_guild_id()) {
            if commands
                .send(AppCommand::LoadGuildMembers { guild_id })
                .await
                .is_err()
            {
                client.remove_member_request(guild_id);
                command_helpers::record_command_channel_closed(&mut state);
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
                command_helpers::record_command_channel_closed(&mut state);
                dirty = true;
            }
        }

        let initial_unknown_requests = client.next_initial_unknown_member_requests(
            state.initial_unknown_member_requests(),
            std::time::Instant::now(),
        );
        if state.enqueue_guild_member_by_id_requests(initial_unknown_requests) {
            dirty = true;
        }

        for (channel_id, latest_message_id) in
            client.next_thread_preview_requests(state.missing_thread_preview_load_requests())
        {
            if commands
                .send(AppCommand::LoadThreadPreview {
                    channel_id,
                    message_id: latest_message_id,
                })
                .await
                .is_err()
            {
                client.remove_thread_preview_request((channel_id, latest_message_id));
                command_helpers::record_command_channel_closed(&mut state);
                dirty = true;
            }
        }

        let member_list_subscription_target =
            state
                .member_list_subscription_target()
                .map(|(guild_id, channel_id)| {
                    (
                        guild_id,
                        channel_id,
                        state.member_subscription_top_bucket(),
                        state.member_subscription_ranges(),
                    )
                });
        client.set_member_list_subscription_target(
            member_list_subscription_target,
            std::time::Instant::now(),
        );
        if let Some((guild_id, channel_id, ranges)) =
            client.next_due_member_list_subscription(std::time::Instant::now())
            && commands
                .send(AppCommand::UpdateMemberListSubscription {
                    guild_id,
                    channel_id,
                    ranges,
                })
                .await
                .is_err()
        {
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }
    }

    Ok(())
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
    if !state.is_composing() {
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
