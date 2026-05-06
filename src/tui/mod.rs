mod format;
mod input;
mod login;
mod media;
mod message_format;
mod requests;
mod selection;
mod state;
mod ui;

use std::{
    collections::{HashSet, VecDeque},
    io::stdout,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as TerminalEvent, EventStream, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use ratatui::layout::Rect;
use tokio::sync::{mpsc, watch};

use crate::{
    Result,
    discord::{AppCommand, AppEvent, DiscordClient, SequencedAppEvent, SnapshotRevision},
    logging,
};

use media::{
    AvatarImageCache, EmojiImageCache, ImagePreviewCache, ImagePreviewDecodeResult,
    spawn_image_preview_decode, visible_avatar_targets, visible_emoji_image_targets,
    visible_image_preview_targets,
};
use requests::{
    ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberRequests,
    PinnedMessageRequests, ThreadPreviewRequests,
};
use state::DashboardState;

const MAX_DRAINED_EFFECT_EVENTS: usize = 1024;

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
        Ok(Self {
            keyboard_enhancement_enabled,
            mouse_capture_enabled,
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
        ratatui::restore();
    }
}

fn process_effect_event(event: AppEvent, ctx: &mut EffectContext<'_>) {
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
}

fn process_sequenced_effect(
    event: SequencedAppEvent,
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) {
    if event.revision > current_snapshot_revision {
        deferred_effects.push_back(event);
        return;
    }
    process_effect_event(event.event, ctx);
}

fn process_deferred_effects(
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) {
    for _ in 0..deferred_effects.len() {
        let Some(event) = deferred_effects.pop_front() else {
            break;
        };
        process_sequenced_effect(event, current_snapshot_revision, deferred_effects, ctx);
    }
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
    let mut state = DashboardState::new();
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
    let mut redraw_window_start = std::time::Instant::now();
    // Snapshot/effect-driven redraws are coalesced into the next pending
    // deadline so bursts of background Discord events (presence, typing,
    // off-screen messages) do not each trigger a fresh OSC 1337 emission for
    // every visible image. Key/mouse/image-decode arms still mark `dirty`
    // immediately to keep input responsiveness intact.
    const BACKGROUND_REDRAW_DEBOUNCE: std::time::Duration =
        std::time::Duration::from_millis(80);
    let mut pending_redraw_deadline: Option<tokio::time::Instant> = None;

    while !state.should_quit() {
        if redraw_window_start.elapsed() >= std::time::Duration::from_secs(1) {
            logging::debug(
                "tui",
                format!(
                    "redraws/sec={frames_drawn} snapshot_changes/sec={snapshot_changes} \
                     total_draw_ms={total_draw_ms} max_draw_ms={max_draw_ms}"
                ),
            );
            frames_drawn = 0;
            snapshot_changes = 0;
            total_draw_ms = 0;
            max_draw_ms = 0;
            redraw_window_start = std::time::Instant::now();
        }

        if dirty {
            let draw_start = std::time::Instant::now();
            terminal.draw(|frame| {
                last_frame_area = frame.area();
                ui::sync_view_heights(frame.area(), &mut state);
                let preview_layout = ui::image_preview_layout(frame.area(), &state);
                state.clamp_message_viewport_for_image_previews(
                    preview_layout.content_width,
                    preview_layout.preview_width,
                    preview_layout.max_preview_height,
                );
                image_targets = visible_image_preview_targets(&state, preview_layout);
                avatar_targets = visible_avatar_targets(&state, preview_layout);
                emoji_targets = visible_emoji_image_targets(&state);
                let image_previews = image_previews.render_state(&image_targets);
                let rendered_avatars = avatar_images.render_state(&avatar_targets);
                let rendered_emojis = emoji_images.render_state(&emoji_targets);
                let popup_avatar = state
                    .user_profile_popup_avatar_url()
                    .and_then(|url| avatar_images.popup_avatar_image(url));
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

            for command in image_previews.next_requests(&image_targets) {
                if commands.send(command).await.is_err() {
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
                if commands.send(command).await.is_err() {
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
            // schedule its fetch separately. The cache is URL-keyed and will
            // dedupe with anything already requested for messages.
            if let Some(url) = state.user_profile_popup_avatar_url().map(str::to_owned)
                && let Some(command) = avatar_images.next_request_for_url(&url)
                && commands.send(command).await.is_err()
            {
                logging::error("tui", "command channel closed");
                state.push_effect(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }
            for command in emoji_images.next_requests(&emoji_targets) {
                if commands.send(command).await.is_err() {
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
                            dirty = true;
                        }
                    }
                    Some(Ok(TerminalEvent::Resize(width, height))) => {
                        last_frame_area = Rect::new(0, 0, width, height);
                        dirty = true;
                    }
                    Some(Ok(_)) => {}
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
                snapshot_changes = snapshot_changes.saturating_add(1);
                match snapshot_changed {
                    Ok(()) => {
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
                        process_deferred_effects(
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        );
                    }
                    Err(_) => {
                        logging::error("tui", "snapshot stream closed");
                        state.quit();
                    }
                }
                if pending_redraw_deadline.is_none() {
                    pending_redraw_deadline =
                        Some(tokio::time::Instant::now() + BACKGROUND_REDRAW_DEBOUNCE);
                }
            }
            maybe_effect = effects.recv() => {
                match maybe_effect {
                    Some(effect) => {
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
                            effect,
                            current_snapshot_revision,
                            &mut deferred_effects,
                            &mut ctx,
                        );
                        for _ in 0..MAX_DRAINED_EFFECT_EVENTS {
                            match effects.try_recv() {
                                Ok(effect) => process_sequenced_effect(
                                    effect,
                                    current_snapshot_revision,
                                    &mut deferred_effects,
                                    &mut ctx,
                                ),
                                Err(mpsc::error::TryRecvError::Empty) => break,
                                Err(mpsc::error::TryRecvError::Disconnected) => {
                                    handle_gateway_closed(&mut state);
                                    break;
                                }
                            }
                        }
                        if pending_redraw_deadline.is_none() {
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
        if let Some((guild_id, channel_id, offset)) = forum_post_requests.next(forum_post_target)
            && commands
                .send(AppCommand::LoadForumPosts {
                    guild_id,
                    channel_id,
                    offset,
                })
                .await
                .is_err()
        {
            forum_post_requests.mark_failed(channel_id, offset);
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::discord::{AppEvent, SequencedAppEvent};

    use super::{
        AvatarImageCache, EffectContext, EmojiImageCache, ForumPostRequests, HistoryRequests,
        ImagePreviewCache, PinnedMessageRequests, ThreadPreviewRequests, process_deferred_effects,
        process_sequenced_effect,
    };
    use crate::tui::state::DashboardState;

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
}
