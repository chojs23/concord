mod format;
mod input;
mod login;
mod media;
mod message_format;
mod requests;
mod selection;
mod state;
mod ui;

use std::{collections::HashSet, io::stdout};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crossterm::{
    event::{
        Event as TerminalEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};

use crate::{
    Result,
    discord::{AppCommand, AppEvent},
    logging,
};

use media::{
    AvatarImageCache, EmojiImageCache, ImagePreviewCache, spawn_image_preview_decode,
    visible_avatar_targets, visible_emoji_image_targets, visible_image_preview_targets,
};
use requests::{
    ForumPostRequestTarget, ForumPostRequests, HistoryRequests, MemberRequests,
    PinnedMessageRequests, ThreadPreviewRequests,
};
use state::DashboardState;

pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    login::prompt_login(notice).await
}

pub async fn run(
    mut events: broadcast::Receiver<AppEvent>,
    commands: mpsc::Sender<AppCommand>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let _restore_guard = match TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };

    run_dashboard(&mut terminal, &mut events, commands).await
}

pub(super) struct TerminalRestoreGuard {
    keyboard_enhancement_enabled: bool,
}

impl TerminalRestoreGuard {
    pub(super) fn new() -> Result<Self> {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        Ok(Self {
            keyboard_enhancement_enabled: true,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        ratatui::restore();
    }
}

async fn run_dashboard(
    terminal: &mut ratatui::DefaultTerminal,
    events: &mut broadcast::Receiver<AppEvent>,
    commands: mpsc::Sender<AppCommand>,
) -> Result<()> {
    let mut state = DashboardState::new();
    let mut image_previews = ImagePreviewCache::new();
    let mut avatar_images = AvatarImageCache::new();
    let mut emoji_images = EmojiImageCache::new();
    let mut terminal_events = EventStream::new();
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
    let mut dirty = true;

    while !state.should_quit() {
        if dirty {
            terminal.draw(|frame| {
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

            for command in image_previews.next_requests(&image_targets) {
                if commands.send(command).await.is_err() {
                    logging::error("tui", "command channel closed");
                    state.push_event(AppEvent::GatewayError {
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
                    state.push_event(AppEvent::GatewayError {
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
                state.push_event(AppEvent::GatewayError {
                    message: "command channel closed".to_owned(),
                });
                dirty = true;
            }
            for command in emoji_images.next_requests(&emoji_targets) {
                if commands.send(command).await.is_err() {
                    logging::error("tui", "command channel closed");
                    state.push_event(AppEvent::GatewayError {
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
                            state.push_event(AppEvent::GatewayError {
                                message: "command channel closed".to_owned(),
                            });
                        }
                        if key.kind == KeyEventKind::Press {
                            dirty = true;
                        }
                    }
                    Some(Ok(TerminalEvent::Resize(_, _))) => dirty = true,
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
                dirty = true;
            }
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        for job in image_previews.record_event(&event) {
                            spawn_image_preview_decode(job, preview_decode_tx.clone());
                        }
                        avatar_images.record_event(&event);
                        emoji_images.record_event(&event);
                        history_requests.record_event(&event);
                        forum_post_requests.record_event(&event);
                        pinned_message_requests.record_event(&event);
                        thread_preview_requests.record_event(&event);
                        state.push_event(event);
                        dirty = true;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_lag(skipped);
                        dirty = true;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        state.push_event(AppEvent::GatewayClosed);
                        state.quit();
                        dirty = true;
                    }
                }
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
            state.push_event(AppEvent::GatewayError {
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
            state.push_event(AppEvent::GatewayError {
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
            state.push_event(AppEvent::GatewayError {
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
                state.push_event(AppEvent::GatewayError {
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
                state.push_event(AppEvent::GatewayError {
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
                state.push_event(AppEvent::GatewayError {
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
                state.push_event(AppEvent::GatewayError {
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
                    state.push_event(AppEvent::GatewayError {
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
