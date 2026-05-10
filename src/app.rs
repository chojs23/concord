use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Instant,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Duration, timeout};

use crate::{
    DiscordClient, Result,
    discord::{
        AppCommand, AppEvent, AttachmentUpdate, MessageInfo, ReactionUsersInfo,
        validate_token_header,
    },
    error::AppError,
    logging, token_store, tui, version_check,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;
const THREAD_PREVIEW_LIMIT: u16 = 1;
const MAX_ATTACHMENT_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTACHMENT_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
const ATTACHMENT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_ATTACHMENT_PREVIEWS: usize = 4;

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let resolved_token = resolve_token().await?;
        let token = resolved_token.token;
        let token_warnings = resolved_token.warnings;
        let client = DiscordClient::new(token)?;
        let effects = client.take_effects();
        let snapshots = client.subscribe_snapshots();
        let (commands_tx, commands_rx) = mpsc::channel(64);
        let gateway_task = client.start_gateway();
        let command_task = start_command_loop(client.clone(), commands_rx);

        // Warm up the REST connection pool in the background. Without this
        // the first user-triggered REST call (typically opening a forum
        // channel) pays the full TCP+TLS+HTTP/2 handshake before it can even
        // start the request, adding ~1s of perceived latency. Firing a cheap
        // GET here lets the pool finish the handshake while the user is
        // still navigating the UI.
        let prime_client = client.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            match prime_client.prime_rest_pool().await {
                Ok(()) => logging::error(
                    "app",
                    format!(
                        "TIMING op=prime_rest_pool duration={:.0}ms",
                        started.elapsed().as_secs_f64() * 1_000.0,
                    ),
                ),
                Err(error) => logging::error("app", format!("rest pool warmup failed: {error}")),
            }
        });

        let version_client = client.clone();
        tokio::spawn(async move {
            match version_check::check_latest_version().await {
                Ok(Some(latest_version)) => {
                    version_client
                        .publish_event(AppEvent::UpdateAvailable { latest_version })
                        .await;
                }
                Ok(None) => {}
                Err(error) => {
                    logging::debug("version", format!("latest version check failed: {error}"))
                }
            }
        });

        let result = async {
            for warning in token_warnings {
                logging::error("app", &warning);
                client
                    .publish_event(AppEvent::GatewayError { message: warning })
                    .await;
            }

            tui::run(effects, snapshots, commands_tx, client.clone()).await
        }
        .await;

        command_task.abort();
        shutdown_gateway(gateway_task).await;
        result
    }
}

fn start_command_loop(
    client: DiscordClient,
    mut commands: mpsc::Receiver<AppCommand>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let attachment_preview_permits =
            Arc::new(Semaphore::new(MAX_CONCURRENT_ATTACHMENT_PREVIEWS));
        // Each command spawns its own task so they run concurrently. The
        // previous serial loop blocked the queue: a slow LoadGuildMembers
        // could delay the LoadForumPosts behind it for seconds. The HTTP
        // client multiplexes over a shared connection pool, so parallel
        // spawning costs almost nothing here.
        while let Some(command) = commands.recv().await {
            let client = client.clone();
            let attachment_preview_permits = attachment_preview_permits.clone();
            tokio::spawn(async move {
                match command {
                    AppCommand::LoadMessageHistory { channel_id, before } => {
                        let started = Instant::now();
                        let endpoint = format_message_history_endpoint(
                            channel_id,
                            before,
                            MESSAGE_HISTORY_LIMIT,
                        );
                        match client
                            .load_message_history(channel_id, before, MESSAGE_HISTORY_LIMIT)
                            .await
                        {
                            Ok(messages) => {
                                logging::timing(
                                    "history",
                                    format!(
                                        "op=load_message_history channel_id={} before={} limit={} messages={}",
                                        channel_id.get(),
                                        before.map(|id| id.get()).unwrap_or_default(),
                                        MESSAGE_HISTORY_LIMIT,
                                        messages.len()
                                    ),
                                    started.elapsed(),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoaded {
                                        channel_id,
                                        before,
                                        messages,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("load message history failed: {error}");
                                let detail = error.log_detail();
                                logging::timing(
                                    "history",
                                    format!(
                                        "op=load_message_history channel_id={} before={} limit={} messages=0",
                                        channel_id.get(),
                                        before.map(|id| id.get()).unwrap_or_default(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                    started.elapsed(),
                                );
                                logging::error(
                                    "history",
                                    format!(
                                        "op=load_message_history channel_id={} before={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                                        channel_id.get(),
                                        before.map(|id| id.get()).unwrap_or_default(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoadFailed {
                                        channel_id,
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::LoadThreadPreview {
                        channel_id,
                        message_id,
                    } => {
                        let started = Instant::now();
                        match client
                            .load_message_history(channel_id, None, THREAD_PREVIEW_LIMIT)
                            .await
                        {
                            Ok(messages) => {
                                logging::timing(
                                    "history",
                                    format!(
                                        "op=load_thread_preview channel_id={} message_id={} limit={} messages={}",
                                        channel_id.get(),
                                        message_id.get(),
                                        THREAD_PREVIEW_LIMIT,
                                        messages.len(),
                                    ),
                                    started.elapsed(),
                                );
                                if let Some(message) = messages
                                    .into_iter()
                                    .next()
                                    .filter(|message| message.message_id == message_id)
                                {
                                    client
                                        .publish_event(AppEvent::ThreadPreviewLoaded {
                                            channel_id,
                                            message,
                                        })
                                        .await;
                                } else {
                                    logging::error(
                                        "history",
                                        format!(
                                            "load thread preview missing requested message: channel_id={} message_id={}",
                                            channel_id.get(),
                                            message_id.get(),
                                        ),
                                    );
                                    client
                                        .publish_event(AppEvent::ThreadPreviewLoadFailed {
                                            channel_id,
                                            message_id,
                                        })
                                        .await;
                                }
                            }
                            Err(error) => {
                                let message = format!("load thread preview failed: {error}");
                                let detail = error.log_detail();
                                logging::timing(
                                    "history",
                                    format!(
                                        "op=load_thread_preview channel_id={} message_id={} messages=0 {message}; detail={detail}",
                                        channel_id.get(),
                                        message_id.get(),
                                    ),
                                    started.elapsed(),
                                );
                                logging::error("history", &message);
                                client
                                    .publish_event(AppEvent::ThreadPreviewLoadFailed {
                                        channel_id,
                                        message_id,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::LoadForumPosts {
                        guild_id,
                        channel_id,
                        archive_state,
                        offset,
                    } => {
                        let started = Instant::now();
                        match client
                            .load_forum_posts(guild_id, channel_id, archive_state, offset)
                            .await
                        {
                            Ok(page) => {
                                let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
                                // Surface forum-load timing at error level so it
                                // always lands in the log file, even without
                                // CONCORD_DEBUG. The first forum opened in a
                                // session is reproducibly the slow one and we
                                // need elapsed numbers per call to tell whether
                                // the bottleneck is TLS, the search index, or
                                // something else.
                                logging::error(
                                    "history",
                                    format!(
                                        "TIMING op=load_forum_posts channel_id={} archive_state={} offset={} posts={} has_more={} duration={:.0}ms",
                                        channel_id.get(),
                                        archive_state.as_log_label(),
                                        offset,
                                        page.posts.len(),
                                        page.has_more,
                                        elapsed_ms,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::ForumPostsLoaded {
                                        channel_id,
                                        archive_state,
                                        offset,
                                        next_offset: page.next_offset,
                                        posts: page.posts,
                                        preview_messages: page.preview_messages,
                                        has_more: page.has_more,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("load forum posts failed: {error}");
                                let detail = error.log_detail();
                                logging::timing(
                                    "history",
                                    format!(
                                        "op=load_forum_posts guild_id={} channel_id={} archive_state={} offset={} posts=0 {message}; detail={detail}",
                                        guild_id.get(),
                                        channel_id.get(),
                                        archive_state.as_log_label(),
                                        offset,
                                    ),
                                    started.elapsed(),
                                );
                                logging::error("history", &message);
                                client
                                    .publish_event(AppEvent::ForumPostsLoadFailed {
                                        channel_id,
                                        archive_state,
                                        offset,
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::LoadGuildMembers { guild_id } => {
                        if let Err(message) = client.request_guild_members(guild_id) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::SubscribeDirectMessage { channel_id } => {
                        if let Err(message) = client.subscribe_direct_message(channel_id) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::SubscribeGuildChannel {
                        guild_id,
                        channel_id,
                    } => {
                        if let Err(message) = client.subscribe_guild_channel(guild_id, channel_id) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::UpdateMemberListSubscription {
                        guild_id,
                        channel_id,
                        ranges,
                    } => {
                        if let Err(message) =
                            client.update_member_list_subscription(guild_id, channel_id, ranges)
                        {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::LoadAttachmentPreview { url } => {
                        let Ok(_permit) = attachment_preview_permits.acquire_owned().await else {
                            let message = "attachment preview limiter closed".to_owned();
                            logging::error("preview", &message);
                            client
                                .publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                    url,
                                    message,
                                })
                                .await;
                            return;
                        };
                        match timeout(ATTACHMENT_PREVIEW_TIMEOUT, fetch_attachment_preview(&url))
                            .await
                        {
                            Err(_) => {
                                let message = "download image preview timed out".to_owned();
                                logging::error("preview", &message);
                                client
                                    .publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                        url,
                                        message,
                                    })
                                    .await;
                            }
                            Ok(bytes) => match bytes {
                                Ok(bytes) => {
                                    client
                                        .publish_event(AppEvent::AttachmentPreviewLoaded {
                                            url,
                                            bytes,
                                        })
                                        .await
                                }
                                Err(message) => {
                                    logging::error("preview", &message);
                                    client
                                        .publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                            url,
                                            message,
                                        })
                                        .await;
                                }
                            },
                        }
                    }
                    AppCommand::SendMessage {
                        channel_id,
                        content,
                        reply_to,
                        attachments,
                    } => match client
                        .send_message(channel_id, &content, reply_to, &attachments)
                        .await
                    {
                        Ok(message) => client.publish_event(message_create_event(message)).await,
                        Err(error) => {
                            log_app_error("send message failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("send message failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::EditMessage {
                        channel_id,
                        message_id,
                        content,
                    } => match client.edit_message(channel_id, message_id, &content).await {
                        Ok(message) => {
                            client.publish_event(message_update_event(message)).await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: "edited message".to_owned(),
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("edit message failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("edit message failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::DeleteMessage {
                        channel_id,
                        message_id,
                    } => match client.delete_message(channel_id, message_id).await {
                        Ok(()) => {
                            client
                                .publish_event(AppEvent::MessageDelete {
                                    guild_id: None,
                                    channel_id,
                                    message_id,
                                })
                                .await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: "deleted message".to_owned(),
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("delete message failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("delete message failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::OpenUrl { url } => {
                        if let Err(error) = open_url(&url) {
                            logging::error("app", format!("open attachment failed: {error}"));
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("open attachment failed: {error}"),
                                })
                                .await;
                        }
                    }
                    AppCommand::DownloadAttachment { url, filename } => {
                        match timeout(
                            ATTACHMENT_PREVIEW_TIMEOUT,
                            download_attachment(&url, &filename),
                        )
                        .await
                        {
                            Err(_) => {
                                let message = "download attachment timed out".to_owned();
                                logging::error("attachment", &message);
                                client
                                    .publish_event(AppEvent::GatewayError { message })
                                    .await;
                            }
                            Ok(Ok(path)) => {
                                client
                                    .publish_event(AppEvent::StatusMessage {
                                        message: format!(
                                            "downloaded attachment to {}",
                                            path.display()
                                        ),
                                    })
                                    .await
                            }
                            Ok(Err(message)) => {
                                logging::error("attachment", &message);
                                client
                                    .publish_event(AppEvent::GatewayError { message })
                                    .await;
                            }
                        }
                    }
                    AppCommand::AddReaction {
                        channel_id,
                        message_id,
                        emoji,
                    } => match client.add_reaction(channel_id, message_id, &emoji).await {
                        Ok(()) => {
                            client
                                .publish_event(AppEvent::CurrentUserReactionAdd {
                                    channel_id,
                                    message_id,
                                    emoji: emoji.clone(),
                                })
                                .await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: format!("added {} reaction", emoji.status_label()),
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("add reaction failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("add reaction failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::RemoveReaction {
                        channel_id,
                        message_id,
                        emoji,
                    } => match client
                        .remove_current_user_reaction(channel_id, message_id, &emoji)
                        .await
                    {
                        Ok(()) => {
                            client
                                .publish_event(AppEvent::CurrentUserReactionRemove {
                                    channel_id,
                                    message_id,
                                    emoji: emoji.clone(),
                                })
                                .await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: format!("removed {} reaction", emoji.status_label()),
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("remove reaction failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("remove reaction failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::LoadReactionUsers {
                        channel_id,
                        message_id,
                        reactions,
                    } => {
                        let mut loaded_reactions = Vec::with_capacity(reactions.len());
                        let mut failed = false;
                        for emoji in reactions {
                            match client
                                .load_reaction_users(channel_id, message_id, &emoji)
                                .await
                            {
                                Ok(users) => {
                                    loaded_reactions.push(ReactionUsersInfo { emoji, users })
                                }
                                Err(error) => {
                                    log_app_error("load reaction users failed", &error);
                                    client
                                        .publish_event(AppEvent::GatewayError {
                                            message: format!("load reaction users failed: {error}"),
                                        })
                                        .await;
                                    failed = true;
                                    break;
                                }
                            }
                        }
                        if !failed {
                            client
                                .publish_event(AppEvent::ReactionUsersLoaded {
                                    channel_id,
                                    message_id,
                                    reactions: loaded_reactions,
                                })
                                .await;
                        }
                    }
                    AppCommand::LoadPinnedMessages { channel_id } => {
                        match client.load_pinned_messages(channel_id).await {
                            Ok(messages) => {
                                client
                                    .publish_event(AppEvent::PinnedMessagesLoaded {
                                        channel_id,
                                        messages,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                log_app_error("load pinned messages failed", &error);
                                client
                                    .publish_event(AppEvent::PinnedMessagesLoadFailed {
                                        channel_id,
                                        message: format!("load pinned messages failed: {error}"),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::SetMessagePinned {
                        channel_id,
                        message_id,
                        pinned,
                    } => match client
                        .set_message_pinned(channel_id, message_id, pinned)
                        .await
                    {
                        Ok(()) => {
                            client
                                .publish_event(AppEvent::MessagePinnedUpdate {
                                    channel_id,
                                    message_id,
                                    pinned,
                                })
                                .await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: if pinned {
                                        "pinned message".to_owned()
                                    } else {
                                        "unpinned message".to_owned()
                                    },
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("set pin failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("set pin failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::VotePoll {
                        channel_id,
                        message_id,
                        answer_ids,
                    } => match client.vote_poll(channel_id, message_id, &answer_ids).await {
                        Ok(()) => {
                            client
                                .publish_event(AppEvent::CurrentUserPollVoteUpdate {
                                    channel_id,
                                    message_id,
                                    answer_ids,
                                })
                                .await;
                            client
                                .publish_event(AppEvent::StatusMessage {
                                    message: "updated poll vote".to_owned(),
                                })
                                .await;
                        }
                        Err(error) => {
                            log_app_error("poll vote failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("poll vote failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::LoadUserProfile { user_id, guild_id } => {
                        let is_self = client.current_discord_snapshot().state.current_user_id()
                            == Some(user_id);
                        match client.load_user_profile(user_id, guild_id, is_self).await {
                            Ok(profile) => {
                                client
                                    .publish_event(AppEvent::UserProfileLoaded {
                                        guild_id,
                                        profile,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                log_app_error("load user profile failed", &error);
                                client
                                    .publish_event(AppEvent::UserProfileLoadFailed {
                                        user_id,
                                        guild_id,
                                        message: error.to_string(),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::AckChannel {
                        channel_id,
                        message_id,
                    } => {
                        // Fire-and-forget: the TUI already cleared its local
                        // unread state, a failure here only loses the cross-
                        // client sync.
                        if let Err(error) = client.ack_channel(channel_id, message_id).await {
                            log_app_error("ack channel failed", &error);
                        }
                    }
                }
            });
        }
    })
}

fn log_app_error(context: &str, error: &AppError) {
    logging::error(
        "app",
        format!("{context}: {}; detail={}", error, error.log_detail()),
    );
}

/// Builds the Discord REST endpoint string for a message-history request so
/// debug logs name exactly what was attempted, e.g.
/// `GET /channels/123/messages?limit=50&before=789`.
fn format_message_history_endpoint(
    channel_id: Id<ChannelMarker>,
    before: Option<Id<MessageMarker>>,
    limit: u16,
) -> String {
    match before {
        Some(message_id) => format!(
            "GET /channels/{}/messages?limit={limit}&before={}",
            channel_id.get(),
            message_id.get(),
        ),
        None => format!("GET /channels/{}/messages?limit={limit}", channel_id.get(),),
    }
}

fn message_create_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.message_id,
        author_id: message.author_id,
        author: message.author,
        author_avatar_url: message.author_avatar_url,
        author_role_ids: message.author_role_ids,
        message_kind: message.message_kind,
        reference: message.reference,
        reply: message.reply,
        poll: message.poll,
        content: message.content,
        sticker_names: message.sticker_names,
        mentions: message.mentions,
        attachments: message.attachments,
        embeds: message.embeds,
        forwarded_snapshots: message.forwarded_snapshots,
    }
}

fn message_update_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageUpdate {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.message_id,
        poll: message.poll,
        content: message.content,
        sticker_names: Some(message.sticker_names),
        mentions: Some(message.mentions),
        attachments: AttachmentUpdate::Replace(message.attachments),
        embeds: Some(message.embeds),
        edited_timestamp: message.edited_timestamp,
    }
}

async fn fetch_attachment_preview(url: &str) -> std::result::Result<Vec<u8>, String> {
    fetch_limited_bytes(
        url,
        MAX_ATTACHMENT_PREVIEW_BYTES,
        "image preview",
        "download image preview failed",
        "read image preview failed",
    )
    .await
}

async fn download_attachment(url: &str, filename: &str) -> std::result::Result<PathBuf, String> {
    let bytes = fetch_limited_bytes(
        url,
        MAX_ATTACHMENT_DOWNLOAD_BYTES,
        "attachment",
        "download attachment failed",
        "read attachment failed",
    )
    .await?;

    let directory = downloads_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create download directory failed: {error}"))?;
    write_unique_download_file(&directory, &sanitize_filename(filename), &bytes)
}

async fn fetch_limited_bytes(
    url: &str,
    max_bytes: usize,
    size_label: &str,
    download_error: &str,
    read_error: &str,
) -> std::result::Result<Vec<u8>, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|error| format!("{download_error}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("{download_error}: {error}"))?;

    if let Some(length) = response.content_length()
        && length > max_bytes as u64
    {
        return Err(format!(
            "{size_label} is too large: {length} bytes (max {max_bytes})"
        ));
    }

    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{read_error}: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "{size_label} is too large: {} bytes (max {max_bytes})",
                bytes.len().saturating_add(chunk.len())
            ));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

fn downloads_directory() -> std::result::Result<PathBuf, String> {
    crate::paths::download_dir()
        .ok_or_else(|| "could not resolve user download directory".to_owned())
}

fn sanitize_filename(filename: &str) -> String {
    let sanitized: String = filename
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches([' ', '.']);
    if sanitized.is_empty() {
        "attachment".to_owned()
    } else {
        sanitized.to_owned()
    }
}

fn write_unique_download_file(
    directory: &Path,
    filename: &str,
    bytes: &[u8],
) -> std::result::Result<PathBuf, String> {
    let original = Path::new(filename);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = original.extension().and_then(|value| value.to_str());

    for index in 0.. {
        let candidate = if index == 0 {
            directory.join(filename)
        } else {
            match extension {
                Some(extension) => directory.join(format!("{stem} ({index}).{extension}")),
                None => directory.join(format!("{stem} ({index})")),
            }
        };

        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(bytes)
                    .map_err(|error| format!("write attachment failed: {error}"))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("write attachment failed: {error}")),
        }
    }

    unreachable!("unbounded search returns a path before exhausting usize")
}

fn open_url(url: &str) -> io::Result<()> {
    let status = open_url_command(url).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "open command exited with status {status}"
        )))
    }
}

fn open_url_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
}

struct ResolvedToken {
    token: String,
    warnings: Vec<String>,
}

async fn resolve_token() -> Result<ResolvedToken> {
    let mut warnings = Vec::new();

    match token_store::load_token() {
        Ok(Some(token)) => {
            if let Err(error) = validate_token_header(&token) {
                warnings.push(format!(
                    "saved Discord token is invalid: {error}; enter a new token"
                ));
            } else {
                return Ok(ResolvedToken { token, warnings });
            }
        }
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "credential store unavailable: {error}; enter a token to continue for this session"
        )),
    }

    let login_notice = login_notice_for_token_warnings(&warnings);

    let token = tui::prompt_login(login_notice).await?;
    validate_token_header(&token)?;
    if let Err(error) = token_store::save_token(&token) {
        warnings.push(format!("token was not saved: {error}"));
    }

    Ok(ResolvedToken { token, warnings })
}

fn login_notice_for_token_warnings(warnings: &[String]) -> Option<String> {
    if warnings
        .iter()
        .any(|warning| warning.starts_with("saved Discord token"))
    {
        Some("Saved Discord token is invalid; enter a new token.".to_owned())
    } else if warnings.is_empty() {
        None
    } else {
        Some("Credential storage is unavailable; token may not be saved.".to_owned())
    }
}

async fn shutdown_gateway(gateway_task: tokio::task::JoinHandle<()>) {
    gateway_task.abort();

    if let Err(error) = gateway_task.await
        && !error.is_cancelled()
    {
        logging::error("app", format!("gateway task ended unexpectedly: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::{login_notice_for_token_warnings, sanitize_filename, write_unique_download_file};

    fn unix_timestamp_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    }

    #[test]
    fn write_unique_download_file_uses_next_available_name() {
        let directory = std::env::temp_dir().join(format!(
            "concord-download-test-{}-{}",
            process::id(),
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let existing = directory.join("cat.png");
        fs::write(&existing, b"old").expect("existing file should be written");

        let path = write_unique_download_file(&directory, "cat.png", b"new")
            .expect("download file should be written");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("cat (1).png")
        );
        assert_eq!(
            fs::read(&existing).expect("existing file should remain"),
            b"old"
        );
        assert_eq!(fs::read(&path).expect("new file should be written"), b"new");

        fs::remove_dir_all(&directory).expect("test directory should be removed");
    }

    #[test]
    fn login_notice_for_token_warnings_reports_user_action() {
        let cases = [
            (
                "saved Discord token is invalid: bad; enter a new token",
                "Saved Discord token is invalid; enter a new token.",
            ),
            (
                "credential store unavailable: permission denied",
                "Credential storage is unavailable; token may not be saved.",
            ),
        ];

        for (warning, expected) in cases {
            let warnings = vec![warning.to_owned()];
            assert_eq!(
                login_notice_for_token_warnings(&warnings).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn sanitize_filename_replaces_path_separators() {
        assert_eq!(sanitize_filename("../cat\\dog.png"), "_cat_dog.png");
    }
}
