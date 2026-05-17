use std::{
    collections::{HashSet, VecDeque},
    io::stdout,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crossterm::{clipboard::CopyToClipboard, event::EventStream};
use futures::StreamExt;
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::{
    Result, config,
    discord::{AppCommand, AppEvent, DiscordClient, SequencedAppEvent, SnapshotRevision},
    logging,
};

use super::{
    commands as command_helpers, effects as effect_helpers, events, input,
    media::{
        AvatarImageCache, EmojiImageCache, ImagePreviewCache, visible_avatar_targets,
        visible_emoji_image_targets, visible_image_preview_targets,
    },
    redraw::{
        image_surfaces_visible, should_redraw_after_visible_signature_change,
        visible_dashboard_signature,
    },
    requests::{
        ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberRequests,
        PinnedMessageRequests, ThreadPreviewRequests,
    },
    state::DashboardState,
    ui,
};

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
    let mut state = DashboardState::new_with_options(
        options.display,
        options.notifications,
        options.voice,
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
    let mut history_requests = HistoryRequests::default();
    let mut forum_post_requests = ForumPostRequests::default();
    let mut pinned_message_requests = PinnedMessageRequests::default();
    let mut member_requests = MemberRequests::default();
    let mut thread_preview_requests = ThreadPreviewRequests::default();
    let mut last_member_subscription: Option<(Id<GuildMarker>, Id<ChannelMarker>, u32)> = None;
    let mut last_reported_active_guild: Option<Id<GuildMarker>> = None;
    let mut last_reported_message_channel: Option<Id<ChannelMarker>> = None;
    let mut requested_author_profiles: HashSet<(Id<UserMarker>, Option<Id<GuildMarker>>)> =
        HashSet::new();
    let mut image_targets = Vec::new();
    let mut avatar_targets = Vec::new();
    let mut emoji_targets = Vec::new();
    let mut deferred_effects = VecDeque::new();
    let mut last_frame_area = Rect::default();
    let mut dirty = true;
    // Snapshot/effect-driven redraws are coalesced into the next pending
    // deadline so bursts of background Discord events (presence, typing,
    // off-screen messages) do not each trigger a fresh OSC 1337 emission for
    // every visible image. Key/mouse/image-decode arms still mark `dirty`
    // immediately to keep input responsiveness intact.
    const BACKGROUND_REDRAW_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(80);
    let mut pending_redraw_deadline: Option<tokio::time::Instant> = None;

    while !state.should_quit() {
        if dirty {
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
                avatar_targets = visible_avatar_targets(&state, preview_layout);
                emoji_targets = visible_emoji_image_targets(&state);
                let image_previews = image_previews.render_state(&image_targets);
                let rendered_emojis = emoji_images.render_state(&emoji_targets);
                let popup_avatar_url = state
                    .show_avatars()
                    .then(|| state.user_profile_popup_avatar_url())
                    .flatten();
                let (rendered_avatars, popup_avatar) =
                    avatar_images.render_state_with_popup(&avatar_targets, popup_avatar_url);
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

        let pending_read_ack_deadline = state.next_read_ack_deadline();
        let pending_toast_deadline = state.next_toast_deadline();

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
                        if let Some(content) = state.take_copy_message_content_request() {
                            let now = std::time::Instant::now();
                            match copy_to_clipboard(&content) {
                                Ok(()) => state.show_success_toast("Message copied", now),
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
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            history_requests: &mut history_requests,
                            forum_post_requests: &mut forum_post_requests,
                            pinned_message_requests: &mut pinned_message_requests,
                            thread_preview_requests: &mut thread_preview_requests,
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
                            image_previews: &mut image_previews,
                            avatar_images: &mut avatar_images,
                            emoji_images: &mut emoji_images,
                            history_requests: &mut history_requests,
                            forum_post_requests: &mut forum_post_requests,
                            pinned_message_requests: &mut pinned_message_requests,
                            thread_preview_requests: &mut thread_preview_requests,
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
                state.flush_due_read_acks(std::time::Instant::now());
                dirty = true;
            }
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

        if let Some(channel_id) = history_requests.next(
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
            history_requests.mark_failed(channel_id);
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
            pinned_message_requests.next(state.selected_message_history_channel_id())
            && commands
                .send(AppCommand::LoadPinnedMessages { channel_id })
                .await
                .is_err()
        {
            pinned_message_requests.mark_failed(channel_id);
            command_helpers::record_command_channel_closed(&mut state);
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
            command_helpers::record_command_channel_closed(&mut state);
            dirty = true;
        }

        if let Some(guild_id) = member_requests.next(state.selected_guild_id()) {
            if commands
                .send(AppCommand::LoadGuildMembers { guild_id })
                .await
                .is_err()
            {
                member_requests.remove(guild_id);
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

        let profile_requests = state
            .missing_message_author_profile_requests()
            .into_iter()
            .chain(state.missing_visible_member_profile_requests());
        for (user_id, guild_id) in profile_requests {
            if !requested_author_profiles.insert((user_id, guild_id)) {
                continue;
            }
            if commands
                .send(AppCommand::LoadUserProfile { user_id, guild_id })
                .await
                .is_err()
            {
                command_helpers::record_command_channel_closed(&mut state);
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
                command_helpers::record_command_channel_closed(&mut state);
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
                    command_helpers::record_command_channel_closed(&mut state);
                    dirty = true;
                } else {
                    last_member_subscription = Some((guild_id, channel_id, bucket));
                }
            }
        }
    }

    Ok(())
}

fn copy_to_clipboard(content: &str) -> std::io::Result<()> {
    crossterm::execute!(stdout(), CopyToClipboard::to_clipboard_from(content))
}

fn open_composer_in_editor(
    terminal: &mut ratatui::DefaultTerminal,
    state: &mut DashboardState,
) -> crate::Result<()> {
    use crossterm::event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
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
