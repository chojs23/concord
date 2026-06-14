use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Duration, Instant as TokioInstant, sleep, timeout};

use crate::{
    DiscordClient, Result, config,
    discord::{
        AppCommand, AppEvent, AttachmentDownloadId, AttachmentUpdate,
        ChannelNotificationOverrideInfo, DownloadAttachmentSource, GuildNotificationSettingsInfo,
        MediaPlaybackRequestId, MessageHistoryLoadTarget, MessageInfo, MuteDuration,
        ReactionUsersInfo, VoiceConnectionStatus, read_profile_avatar_image, validate_token_header,
    },
    error::AppError,
    logging, token_store, tui,
    url_policy::normalize_openable_url,
    version_check,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;
const THREAD_PREVIEW_LIMIT: u16 = 1;
const MENTION_MEMBER_SEARCH_LIMIT: u16 = 10;
const MAX_ATTACHMENT_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const ATTACHMENT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);
const ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const MAX_CONCURRENT_ATTACHMENT_PREVIEWS: usize = 4;
const MAX_CONCURRENT_ATTACHMENT_DOWNLOADS: usize = 2;
const MEDIA_PLAYER_WINDOW_READY_TIMEOUT: Duration = Duration::from_secs(300);
const MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const MEDIA_PLAYER_IPC_WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(100);

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

        // Warm the REST pool before the first user-triggered request pays the
        // TCP, TLS, and HTTP/2 setup cost.
        let prime_client = client.clone();
        tokio::spawn(async move {
            if let Err(error) = prime_client.prime_rest_pool().await {
                logging::error("app", format!("rest pool warmup failed: {error}"));
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
        leave_current_voice_channel_on_shutdown(&client);
        shutdown_gateway(&client, gateway_task).await;
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
        let attachment_download_permits =
            Arc::new(Semaphore::new(MAX_CONCURRENT_ATTACHMENT_DOWNLOADS));
        // Spawn commands independently so slow REST calls do not block the
        // whole UI command queue.
        while let Some(command) = commands.recv().await {
            let client = client.clone();
            let attachment_preview_permits = attachment_preview_permits.clone();
            let attachment_download_permits = attachment_download_permits.clone();
            tokio::spawn(async move {
                match command {
                    AppCommand::LoadMessageHistory { channel_id, before } => {
                        if let Some(before) = before
                            && !client.begin_older_message_history_request(channel_id, before)
                        {
                            return;
                        }
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
                                        target: before
                                            .map(|before| MessageHistoryLoadTarget::Older {
                                                before,
                                            })
                                            .unwrap_or(MessageHistoryLoadTarget::Latest),
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::RefreshMessageHistory { channel_id } => {
                        let endpoint = format_message_history_endpoint(
                            channel_id,
                            None,
                            MESSAGE_HISTORY_LIMIT,
                        );
                        match client
                            .load_message_history(channel_id, None, MESSAGE_HISTORY_LIMIT)
                            .await
                        {
                            Ok(messages) => {
                                client
                                    .publish_event(AppEvent::MessageHistoryRefreshed {
                                        channel_id,
                                        messages,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("refresh message history failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "history",
                                    format!(
                                        "op=refresh_message_history channel_id={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                                        channel_id.get(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoadFailed {
                                        channel_id,
                                        target: MessageHistoryLoadTarget::Latest,
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::LoadMessageHistoryAfter { channel_id, after } => {
                        if !client.begin_newer_message_history_request(channel_id, after) {
                            return;
                        }
                        let endpoint = format_message_history_anchor_endpoint(
                            channel_id,
                            "after",
                            after,
                            MESSAGE_HISTORY_LIMIT,
                        );
                        match client
                            .load_message_history_after(channel_id, after, MESSAGE_HISTORY_LIMIT)
                            .await
                        {
                            Ok(messages) => {
                                let has_more = messages.len() >= usize::from(MESSAGE_HISTORY_LIMIT);
                                client
                                    .publish_event(AppEvent::MessageHistoryAfterLoaded {
                                        channel_id,
                                        after,
                                        messages,
                                        has_more,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("load message history failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "history",
                                    format!(
                                        "op=load_message_history_after channel_id={} after={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                                        channel_id.get(),
                                        after.get(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoadFailed {
                                        channel_id,
                                        target: MessageHistoryLoadTarget::Newer { after },
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::CatchUpMessageHistoryAfter { channel_id, after } => {
                        if !client.begin_catch_up_message_history_request(channel_id, after) {
                            return;
                        }
                        let endpoint = format_message_history_anchor_endpoint(
                            channel_id,
                            "after",
                            after,
                            MESSAGE_HISTORY_LIMIT,
                        );
                        match client
                            .load_message_history_after(channel_id, after, MESSAGE_HISTORY_LIMIT)
                            .await
                        {
                            Ok(messages) => {
                                let has_more = messages.len() >= usize::from(MESSAGE_HISTORY_LIMIT);
                                client
                                    .publish_event(AppEvent::MessageHistoryCatchUpLoaded {
                                        channel_id,
                                        after,
                                        messages,
                                        has_more,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("catch up message history failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "history",
                                    format!(
                                        "op=catch_up_message_history_after channel_id={} after={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                                        channel_id.get(),
                                        after.get(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoadFailed {
                                        channel_id,
                                        target: MessageHistoryLoadTarget::Newer { after },
                                        message,
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::LoadMessageHistoryAround {
                        channel_id,
                        message_id,
                    } => {
                        let endpoint = format_message_history_anchor_endpoint(
                            channel_id,
                            "around",
                            message_id,
                            MESSAGE_HISTORY_LIMIT,
                        );
                        match client
                            .load_message_history_around(
                                channel_id,
                                message_id,
                                MESSAGE_HISTORY_LIMIT,
                            )
                            .await
                        {
                            Ok(messages) => {
                                client
                                    .publish_event(AppEvent::MessageHistoryAroundLoaded {
                                        channel_id,
                                        message_id,
                                        messages,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("load message history failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "history",
                                    format!(
                                        "op=load_message_history_around channel_id={} message_id={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                                        channel_id.get(),
                                        message_id.get(),
                                        MESSAGE_HISTORY_LIMIT,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageHistoryLoadFailed {
                                        channel_id,
                                        target: MessageHistoryLoadTarget::Around { message_id },
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
                        match client
                            .load_message_history(channel_id, None, THREAD_PREVIEW_LIMIT)
                            .await
                        {
                            Ok(messages) => {
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
                                logging::error(
                                    "history",
                                    format!(
                                        "op=load_thread_preview channel_id={} message_id={} {message}; detail={detail}",
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
                    }
                    AppCommand::LoadForumPosts {
                        guild_id,
                        channel_id,
                        archive_state,
                        offset,
                    } => {
                        match client
                            .load_forum_posts(guild_id, channel_id, archive_state, offset)
                            .await
                        {
                            Ok(page) => {
                                client
                                    .publish_event(AppEvent::ForumPostsLoaded {
                                        channel_id,
                                        archive_state,
                                        offset,
                                        next_offset: page.next_offset,
                                        threads: page.threads,
                                        first_messages: page.first_messages,
                                        has_more: page.has_more,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("load forum posts failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "history",
                                    format!(
                                        "op=load_forum_posts guild_id={} channel_id={} archive_state={} offset={} {message}; detail={detail}",
                                        guild_id.get(),
                                        channel_id.get(),
                                        archive_state.as_log_label(),
                                        offset,
                                    ),
                                );
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
                    AppCommand::SearchMessages { query } => {
                        match client.search_messages(query.clone()).await {
                            Ok(page) => {
                                client
                                    .publish_event(AppEvent::MessageSearchLoaded { page })
                                    .await;
                            }
                            Err(error) => {
                                let message = format!("message search failed: {error}");
                                let detail = error.log_detail();
                                logging::error(
                                    "search",
                                    format!(
                                        "op=message_search offset={} {message}; detail={detail}",
                                        query.offset,
                                    ),
                                );
                                client
                                    .publish_event(AppEvent::MessageSearchLoadFailed {
                                        query,
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
                    AppCommand::LoadGuildMembersByIds { guild_id, user_ids } => {
                        if let Err(message) =
                            client.request_guild_members_by_ids(guild_id, user_ids)
                        {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::SearchGuildMembers { guild_id, query } => {
                        if let Err(message) = client.search_guild_members(
                            guild_id,
                            query,
                            MENTION_MEMBER_SEARCH_LIMIT,
                        ) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::SetSelectedGuild { guild_id } => {
                        client
                            .publish_event(AppEvent::SelectedGuildChanged { guild_id })
                            .await;
                    }
                    AppCommand::SetSelectedMessageChannel { channel_id } => {
                        client
                            .publish_event(AppEvent::SelectedMessageChannelChanged { channel_id })
                            .await;
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
                    AppCommand::JoinVoiceChannel {
                        guild_id,
                        channel_id,
                        self_mute,
                        self_deaf,
                        allow_microphone_transmit,
                        microphone_sensitivity,
                        microphone_volume,
                        voice_output_volume,
                    } => {
                        if let Err(message) = client.update_voice_state(
                            guild_id,
                            Some(channel_id),
                            self_mute,
                            self_deaf,
                        ) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::VoiceConnectionStatusChanged {
                                    guild_id,
                                    channel_id: Some(channel_id),
                                    status: VoiceConnectionStatus::Failed,
                                    message: Some(message),
                                })
                                .await;
                        } else {
                            client.update_voice_capture_permission(
                                guild_id,
                                channel_id,
                                allow_microphone_transmit,
                                microphone_sensitivity,
                                microphone_volume,
                                voice_output_volume,
                            );
                            client
                                .publish_event(AppEvent::VoiceConnectionStatusChanged {
                                    guild_id,
                                    channel_id: Some(channel_id),
                                    status: VoiceConnectionStatus::Connecting,
                                    message: Some("Voice join requested".to_owned()),
                                })
                                .await;
                        }
                    }
                    AppCommand::UpdateVoiceState {
                        guild_id,
                        channel_id,
                        self_mute,
                        self_deaf,
                    } => {
                        if let Err(message) = client.update_voice_state(
                            guild_id,
                            Some(channel_id),
                            self_mute,
                            self_deaf,
                        ) {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::GatewayError { message })
                                .await;
                        }
                    }
                    AppCommand::UpdateVoiceCapturePermission {
                        guild_id,
                        channel_id,
                        allow_microphone_transmit,
                        microphone_sensitivity,
                        microphone_volume,
                        voice_output_volume,
                    } => {
                        client.update_voice_capture_permission(
                            guild_id,
                            channel_id,
                            allow_microphone_transmit,
                            microphone_sensitivity,
                            microphone_volume,
                            voice_output_volume,
                        );
                    }
                    AppCommand::LeaveVoiceChannel {
                        guild_id,
                        self_mute,
                        self_deaf,
                    } => {
                        if let Err(message) =
                            client.update_voice_state(guild_id, None, self_mute, self_deaf)
                        {
                            logging::error("app", &message);
                            client
                                .publish_event(AppEvent::VoiceConnectionStatusChanged {
                                    guild_id,
                                    channel_id: None,
                                    status: VoiceConnectionStatus::Failed,
                                    message: Some(message),
                                })
                                .await;
                        } else {
                            client
                                .publish_event(AppEvent::VoiceConnectionStatusChanged {
                                    guild_id,
                                    channel_id: None,
                                    status: VoiceConnectionStatus::Disconnected,
                                    message: Some("Voice leave requested".to_owned()),
                                })
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
                    AppCommand::LoadProfileAvatarPreview { key, upload } => {
                        match read_profile_avatar_image(&upload).await {
                            Ok(image) => {
                                client
                                    .publish_event(AppEvent::AttachmentPreviewLoaded {
                                        url: key,
                                        bytes: image.bytes,
                                    })
                                    .await;
                            }
                            Err(message) => {
                                logging::error("preview", &message);
                                client
                                    .publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                        url: key,
                                        message,
                                    })
                                    .await;
                            }
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
                    AppCommand::SendTtsMessage {
                        channel_id,
                        content,
                    } => match client.send_tts_message(channel_id, &content).await {
                        Ok(message) => client.publish_event(message_create_event(message)).await,
                        Err(error) => {
                            log_app_error("send tts message failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("send tts message failed: {error}"),
                                })
                                .await;
                        }
                    },
                    AppCommand::LoadApplicationCommands { guild_id } => {
                        match client.load_application_commands(guild_id).await {
                            Ok(Some(commands)) => {
                                client
                                    .publish_event(AppEvent::ApplicationCommandsLoaded {
                                        guild_id,
                                        commands,
                                    })
                                    .await;
                            }
                            Ok(None) => {}
                            Err(error) => log_app_error("load application commands failed", &error),
                        }
                    }
                    AppCommand::RunApplicationCommand { invocation } => {
                        if let Err(error) = client.run_application_command(&invocation).await {
                            log_app_error("run application command failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("run application command failed: {error}"),
                                })
                                .await;
                        }
                    }
                    AppCommand::EditMessage {
                        channel_id,
                        message_id,
                        content,
                    } => match client.edit_message(channel_id, message_id, &content).await {
                        Ok(message) => {
                            client.publish_event(message_update_event(message)).await;
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
                    AppCommand::LeaveGuild { guild_id, label } => {
                        match client.leave_guild(guild_id).await {
                            Ok(()) => {
                                client
                                    .publish_event(AppEvent::GuildDelete { guild_id })
                                    .await;
                            }
                            Err(error) => {
                                log_app_error("leave guild failed", &error);
                                client
                                    .publish_event(AppEvent::GatewayError {
                                        message: format!("leave server {label} failed: {error}"),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::OpenUrl { url } => {
                        if let Err(error) = open_url(&url) {
                            logging::error("app", format!("open url failed: {error}"));
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("open url failed: {error}"),
                                })
                                .await;
                        }
                    }
                    AppCommand::PlayMedia { target, request_id } => {
                        let request_id =
                            request_id.unwrap_or_else(|| MediaPlaybackRequestId::new(0));
                        if let Err(error) =
                            play_media(client.clone(), request_id, &target.url, &target.label).await
                        {
                            logging::error("media", format!("play media failed: {error}"));
                            let label = if target.label.is_empty() {
                                "media"
                            } else {
                                target.label.as_str()
                            };
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: format!("play {label} failed: {error}"),
                                })
                                .await;
                        }
                    }
                    AppCommand::DownloadAttachment {
                        id,
                        url,
                        filename,
                        source,
                    } => {
                        let Ok(_permit) = attachment_download_permits.acquire_owned().await else {
                            let message = "attachment download limiter closed".to_owned();
                            logging::error("attachment", &message);
                            client
                                .publish_event(AppEvent::AttachmentDownloadFailed {
                                    id,
                                    filename,
                                    message,
                                    source,
                                })
                                .await;
                            return;
                        };
                        match download_attachment(&client, id, &url, &filename, source).await {
                            Ok(path) => {
                                client
                                    .publish_event(AppEvent::AttachmentDownloadCompleted {
                                        id,
                                        path: path.display().to_string(),
                                        source,
                                    })
                                    .await
                            }
                            Err(message) => {
                                logging::error("attachment", &message);
                                client
                                    .publish_event(AppEvent::AttachmentDownloadFailed {
                                        id,
                                        filename,
                                        message,
                                        source,
                                    })
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
                        let profile_request = client.next_user_profile_request(user_id, guild_id);
                        let note_request = client.next_user_note_request(user_id);
                        if let Some((user_id, guild_id, is_self)) = profile_request {
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
                        if let Some(user_id) = note_request {
                            match client.load_user_note(user_id).await {
                                Ok(note) => {
                                    client
                                        .publish_event(AppEvent::UserNoteLoaded { user_id, note })
                                        .await;
                                }
                                Err(error) => {
                                    client.mark_user_note_request_failed(user_id);
                                    log_app_error("load user note failed", &error);
                                }
                            }
                        }
                    }
                    AppCommand::LoadUserNote { user_id } => {
                        let Some(user_id) = client.next_user_note_request(user_id) else {
                            return;
                        };
                        match client.load_user_note(user_id).await {
                            Ok(note) => {
                                client
                                    .publish_event(AppEvent::UserNoteLoaded { user_id, note })
                                    .await;
                            }
                            Err(error) => {
                                client.mark_user_note_request_failed(user_id);
                                log_app_error("load user note failed", &error);
                            }
                        }
                    }
                    AppCommand::UpdateUserProfile { update } => {
                        let user_id = update.user_id;
                        let guild_id = update.guild_id;
                        if client.current_user_id() != Some(user_id) {
                            client
                                .publish_event(AppEvent::UserProfileUpdateFailed {
                                    user_id,
                                    guild_id,
                                    message: "profile update can only edit the current user"
                                        .to_owned(),
                                })
                                .await;
                            return;
                        }
                        match client.update_user_profile(&update).await {
                            Ok(()) => match client.load_user_profile(user_id, guild_id, true).await
                            {
                                Ok(profile) => {
                                    client
                                        .publish_event(AppEvent::UserProfileLoaded {
                                            guild_id,
                                            profile,
                                        })
                                        .await;
                                }
                                Err(error) => {
                                    log_app_error(
                                        "reload user profile after update failed",
                                        &error,
                                    );
                                    client
                                        .publish_event(AppEvent::UserProfileLoadFailed {
                                            user_id,
                                            guild_id,
                                            message: error.to_string(),
                                        })
                                        .await;
                                }
                            },
                            Err(error) => {
                                log_app_error("update user profile failed", &error);
                                client
                                    .publish_event(AppEvent::UserProfileUpdateFailed {
                                        user_id,
                                        guild_id,
                                        message: error.to_string(),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::UpdateCurrentUserStatus { status } => {
                        match client.update_presence_status(status).await {
                            Ok(activities) => {
                                if let Some(user_id) = client.current_user_id() {
                                    client
                                        .publish_event(AppEvent::UserPresenceUpdate {
                                            user_id,
                                            status,
                                            activities,
                                        })
                                        .await;
                                }
                            }
                            Err(error) => {
                                log_app_error("update presence status failed", &error);
                                client
                                    .publish_event(AppEvent::GatewayError {
                                        message: error.to_string(),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::UpdateCurrentUserActivity { status, activities } => {
                        if let Err(error) =
                            client.update_presence_activity(status, activities.clone())
                        {
                            log_app_error("update presence activity failed", &error);
                            client
                                .publish_event(AppEvent::GatewayError {
                                    message: error.to_string(),
                                })
                                .await;
                        } else if let Some(user_id) = client.current_user_id() {
                            client
                                .publish_event(AppEvent::UserPresenceUpdate {
                                    user_id,
                                    status,
                                    activities,
                                })
                                .await;
                        }
                    }
                    AppCommand::AckChannel {
                        channel_id,
                        message_id,
                    } => {
                        client.clear_read_ack(channel_id);
                        client
                            .publish_optimistic_read_ack(channel_id, message_id)
                            .await;
                        // A failure here only loses cross-client sync because
                        // the backend has already published the local read
                        // state update.
                        if let Err(error) = client.ack_channel(channel_id, message_id).await {
                            log_app_error("ack channel failed", &error);
                        }
                    }
                    AppCommand::ScheduleAckChannel {
                        channel_id,
                        message_id,
                    } => {
                        client
                            .publish_optimistic_read_ack(channel_id, message_id)
                            .await;
                        client.schedule_read_ack(channel_id, message_id, std::time::Instant::now());
                    }
                    AppCommand::SetGuildMuted {
                        guild_id,
                        muted,
                        duration,
                        label: _,
                    } => {
                        let mute_end_time = mute_end_time_from_duration(duration, muted);
                        let selected_time_window =
                            selected_time_window_from_duration(duration, muted);
                        match client
                            .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
                            .await
                        {
                            Ok(()) => {
                                client
                                    .publish_event(AppEvent::UserGuildNotificationSettingsUpdate {
                                        settings: guild_notification_settings_update(
                                            &client,
                                            Some(guild_id),
                                            Some((muted, mute_end_time)),
                                            None,
                                        ),
                                    })
                                    .await;
                            }
                            Err(error) => {
                                log_app_error("set guild mute failed", &error);
                                client
                                    .publish_event(AppEvent::GatewayError {
                                        message: format!("set guild mute failed: {error}"),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::SetChannelMuted {
                        guild_id,
                        channel_id,
                        muted,
                        duration,
                        label: _,
                    } => {
                        let mute_end_time = mute_end_time_from_duration(duration, muted);
                        let selected_time_window =
                            selected_time_window_from_duration(duration, muted);
                        match client
                            .set_channel_muted(
                                guild_id,
                                channel_id,
                                muted,
                                mute_end_time,
                                selected_time_window,
                            )
                            .await
                        {
                            Ok(()) => {
                                client
                                    .publish_event(AppEvent::UserGuildNotificationSettingsUpdate {
                                        settings: guild_notification_settings_update(
                                            &client,
                                            guild_id,
                                            None,
                                            Some((channel_id, muted, mute_end_time)),
                                        ),
                                    })
                                    .await;
                            }
                            Err(error) => {
                                log_app_error("set channel mute failed", &error);
                                client
                                    .publish_event(AppEvent::GatewayError {
                                        message: format!("set channel mute failed: {error}"),
                                    })
                                    .await;
                            }
                        }
                    }
                    AppCommand::AckChannels { targets } => {
                        client.clear_read_acks(targets.iter().map(|(channel_id, _)| *channel_id));
                        client.publish_optimistic_read_acks(&targets).await;
                        // A failure here only loses cross-client sync because
                        // the backend has already published the local read
                        // state updates.
                        if let Err(error) = client.ack_channels(&targets).await {
                            log_app_error("ack channels failed", &error);
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

fn mute_end_time_from_duration(
    duration: Option<MuteDuration>,
    muted: bool,
) -> Option<chrono::DateTime<Utc>> {
    if !muted {
        return None;
    }
    duration
        .and_then(MuteDuration::minutes)
        .filter(|minutes| *minutes > 0)
        .and_then(|minutes| i64::try_from(minutes).ok())
        .map(|minutes| Utc::now() + ChronoDuration::minutes(minutes))
}

fn selected_time_window_from_duration(duration: Option<MuteDuration>, muted: bool) -> Option<i64> {
    muted.then(|| {
        duration
            .unwrap_or(MuteDuration::Permanent)
            .selected_time_window_seconds()
    })
}

fn guild_notification_settings_update(
    client: &DiscordClient,
    guild_id: Option<Id<crate::discord::ids::marker::GuildMarker>>,
    guild_update: Option<(bool, Option<chrono::DateTime<Utc>>)>,
    channel_override: Option<(
        Id<crate::discord::ids::marker::ChannelMarker>,
        bool,
        Option<chrono::DateTime<Utc>>,
    )>,
) -> GuildNotificationSettingsInfo {
    let snapshot = client.current_discord_snapshot();
    let mut settings = snapshot
        .to_state()
        .guild_notification_settings_info(guild_id);
    if let Some((muted, mute_end_time)) = guild_update {
        settings.muted = muted;
        settings.mute_end_time =
            mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    if let Some((channel_id, muted, mute_end_time)) = channel_override {
        if let Some(override_info) = settings
            .channel_overrides
            .iter_mut()
            .find(|override_info| override_info.channel_id == channel_id)
        {
            override_info.muted = muted;
            override_info.mute_end_time =
                mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
        } else {
            settings
                .channel_overrides
                .push(ChannelNotificationOverrideInfo {
                    channel_id,
                    message_notifications: None,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)),
                });
        }
    }
    settings
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

fn format_message_history_anchor_endpoint(
    channel_id: Id<ChannelMarker>,
    anchor_name: &str,
    message_id: Id<MessageMarker>,
    limit: u16,
) -> String {
    format!(
        "GET /channels/{}/messages?limit={limit}&{anchor_name}={}",
        channel_id.get(),
        message_id.get(),
    )
}

fn message_create_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.message_id,
        author_id: message.author_id,
        author: message.author,
        author_avatar_url: message.author_avatar_url,
        author_is_bot: message.author_is_bot,
        author_role_ids: message.author_role_ids,
        message_kind: message.message_kind,
        interaction: message.interaction,
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

async fn download_attachment(
    client: &DiscordClient,
    id: AttachmentDownloadId,
    url: &str,
    filename: &str,
    source: DownloadAttachmentSource,
) -> std::result::Result<PathBuf, String> {
    let mut response = timeout(ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT, reqwest::get(url))
        .await
        .map_err(|_| "download attachment timed out".to_owned())?
        .map_err(|error| format!("download attachment failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download attachment failed: {error}"))?;
    let total_bytes = response.content_length();
    let filename = sanitize_filename(filename);
    let directory = downloads_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create download directory failed: {error}"))?;
    let (mut file, temp_path) = create_download_temp_file(&directory)?;

    client
        .publish_event(AppEvent::AttachmentDownloadStarted {
            id,
            filename: filename.clone(),
            total_bytes,
            source,
        })
        .await;

    let mut downloaded_bytes = 0u64;
    let mut last_reported_bytes = 0u64;
    let mut next_progress_at = TokioInstant::now() + ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL;
    while let Some(chunk) = timeout(ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT, response.chunk())
        .await
        .map_err(|_| "read attachment timed out".to_owned())?
        .map_err(|error| format!("read attachment failed: {error}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("write attachment failed: {error}"))?;
        downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
        let now = TokioInstant::now();
        if now >= next_progress_at {
            client
                .publish_event(AppEvent::AttachmentDownloadProgress {
                    id,
                    downloaded_bytes,
                    total_bytes,
                })
                .await;
            last_reported_bytes = downloaded_bytes;
            next_progress_at = now + ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL;
        }
    }

    file.flush()
        .await
        .map_err(|error| format!("write attachment failed: {error}"))?;
    drop(file.into_std().await);
    if downloaded_bytes != last_reported_bytes {
        client
            .publish_event(AppEvent::AttachmentDownloadProgress {
                id,
                downloaded_bytes,
                total_bytes,
            })
            .await;
    }
    persist_unique_download_file(&directory, &filename, temp_path)
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

fn create_download_temp_file(
    directory: &Path,
) -> std::result::Result<(tokio::fs::File, tempfile::TempPath), String> {
    let temp = tempfile::Builder::new()
        .prefix(".concord-download-")
        .tempfile_in(directory)
        .map_err(|error| format!("create temporary download file failed: {error}"))?;
    let (file, path) = temp.into_parts();
    Ok((tokio::fs::File::from_std(file), path))
}

fn persist_unique_download_file(
    directory: &Path,
    filename: &str,
    mut temp_path: tempfile::TempPath,
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

        match temp_path.persist_noclobber(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                temp_path = error.path;
            }
            Err(error) => return Err(format!("persist attachment failed: {}", error.error)),
        }
    }

    unreachable!("unbounded search returns a path before exhausting usize")
}

fn open_url(url: &str) -> io::Result<()> {
    let url = normalize_openable_url(url)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsupported URL scheme"))?;
    let status = open_url_command(&url).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "open command exited with status {status}"
        )))
    }
}

fn open_url_command(url: &str) -> Command {
    let spec = current_open_url_command_spec(url);
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    command
}

async fn play_media(
    client: DiscordClient,
    request_id: MediaPlaybackRequestId,
    url: &str,
    label: &str,
) -> io::Result<()> {
    let ipc_endpoint = MediaPlayerIpcEndpoint::unique();
    ipc_endpoint.prepare()?;
    let spec = media_player_command_spec_for_url_with_ipc(url, Some(ipc_endpoint.server_arg()))?;
    let mut command = TokioCommand::new(spec.program);
    command
        .args(spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = match command.spawn().map_err(media_player_spawn_error) {
        Ok(child) => child,
        Err(error) => {
            ipc_endpoint.cleanup();
            return Err(error);
        }
    };
    let url = url.to_owned();
    let label = media_playback_label(label).to_owned();
    let _player_monitor_task = tokio::spawn(async move {
        monitor_media_player_window(child, ipc_endpoint, client, request_id, url, label).await;
    });
    Ok(())
}

async fn monitor_media_player_window(
    mut child: tokio::process::Child,
    ipc_endpoint: MediaPlayerIpcEndpoint,
    client: DiscordClient,
    request_id: MediaPlaybackRequestId,
    url: String,
    label: String,
) {
    let ready_timeout = sleep(MEDIA_PLAYER_WINDOW_READY_TIMEOUT);
    tokio::pin!(ready_timeout);
    let ready_result = wait_for_media_player_window_ready(ipc_endpoint.clone());
    tokio::pin!(ready_result);

    let outcome = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(status) => {
                    let message = if status.success() {
                        format!("play {label} failed: media player exited before opening a window")
                    } else {
                        format!("play {label} failed: media player exited with status {status}")
                    };
                    logging::error("media", &message);
                    client.publish_event(AppEvent::GatewayError { message }).await;
                }
                Err(error) => {
                    logging::error("media", format!("media player wait failed: {error}"));
                    client
                        .publish_event(AppEvent::GatewayError {
                            message: format!("play {label} failed: media player wait failed: {error}"),
                        })
                        .await;
                }
            }
            MediaPlayerWindowMonitorOutcome::ChildExited
        }
        result = &mut ready_result => {
            match result {
                Ok(()) => MediaPlayerWindowMonitorOutcome::Ready,
                Err(error) => {
                    MediaPlayerWindowMonitorOutcome::ReadinessFailed(format!(
                        "play {label} failed: media player readiness check failed: {error}"
                    ))
                }
            }
        }
        () = &mut ready_timeout => {
            MediaPlayerWindowMonitorOutcome::ReadinessFailed(
                format!(
                    "play {label} failed: media player did not report a window within {} seconds",
                    MEDIA_PLAYER_WINDOW_READY_TIMEOUT.as_secs()
                ),
            )
        }
    };

    ipc_endpoint.cleanup();

    match outcome {
        MediaPlayerWindowMonitorOutcome::Ready => {
            client
                .publish_event(AppEvent::MediaPlaybackWindowReady { request_id, url })
                .await;
        }
        MediaPlayerWindowMonitorOutcome::ReadinessFailed(message) => {
            logging::error("media", &message);
            client
                .publish_event(AppEvent::GatewayError { message })
                .await;
        }
        MediaPlayerWindowMonitorOutcome::ChildExited => return,
    }

    if let Err(error) = child.wait().await {
        logging::error("media", format!("media player wait failed: {error}"));
    }
}

enum MediaPlayerWindowMonitorOutcome {
    Ready,
    ReadinessFailed(String),
    ChildExited,
}

fn media_playback_label(label: &str) -> &str {
    if label.is_empty() { "media" } else { label }
}

fn media_player_spawn_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        return io::Error::new(
            io::ErrorKind::NotFound,
            "mpv is required for media playback; install mpv and make sure it is on PATH",
        );
    }

    error
}

#[cfg(test)]
fn media_player_command_spec_for_url(url: &str) -> io::Result<MediaPlayerCommandSpec> {
    media_player_command_spec_for_url_with_ipc(url, None)
}

fn media_player_command_spec_for_url_with_ipc(
    url: &str,
    ipc_server: Option<&str>,
) -> io::Result<MediaPlayerCommandSpec> {
    let url = normalize_openable_url(url).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "unsupported media URL scheme")
    })?;
    let mut args = vec!["--no-terminal".to_owned()];
    if let Some(ipc_server) = ipc_server {
        args.push(format!("--input-ipc-server={ipc_server}"));
    }
    args.extend(["--".to_owned(), url]);
    Ok(MediaPlayerCommandSpec {
        program: "mpv",
        args,
    })
}

#[derive(Clone, Debug)]
struct MediaPlayerIpcEndpoint {
    server_arg: String,
    #[cfg(unix)]
    socket_path: PathBuf,
}

impl MediaPlayerIpcEndpoint {
    fn unique() -> Self {
        let id = uuid::Uuid::new_v4();

        #[cfg(unix)]
        {
            let socket_path = std::env::temp_dir().join(format!("concord-mpv-{id}.sock"));
            Self {
                server_arg: socket_path.display().to_string(),
                socket_path,
            }
        }

        #[cfg(windows)]
        {
            Self {
                server_arg: format!(r"\\.\pipe\concord-mpv-{id}"),
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            Self {
                server_arg: std::env::temp_dir()
                    .join(format!("concord-mpv-{id}.sock"))
                    .display()
                    .to_string(),
            }
        }
    }

    fn server_arg(&self) -> &str {
        &self.server_arg
    }

    fn prepare(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            match fs::remove_file(&self.socket_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }

        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    fn cleanup(&self) {
        #[cfg(unix)]
        if let Err(error) = fs::remove_file(&self.socket_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            logging::error("media", format!("media player IPC cleanup failed: {error}"));
        }
    }
}

async fn wait_for_media_player_window_ready(endpoint: MediaPlayerIpcEndpoint) -> io::Result<()> {
    #[cfg(unix)]
    {
        let stream = connect_media_player_unix_ipc(&endpoint).await?;
        wait_for_mpv_window_id(stream).await
    }

    #[cfg(windows)]
    {
        let stream = connect_media_player_windows_ipc(&endpoint).await?;
        wait_for_mpv_window_id(stream).await
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = endpoint;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "media player IPC is not supported on this platform",
        ))
    }
}

#[cfg(unix)]
async fn connect_media_player_unix_ipc(
    endpoint: &MediaPlayerIpcEndpoint,
) -> io::Result<tokio::net::UnixStream> {
    loop {
        match tokio::net::UnixStream::connect(&endpoint.socket_path).await {
            Ok(stream) => return Ok(stream),
            Err(error) if media_player_ipc_connect_error_is_retryable(&error) => {
                sleep(MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
async fn connect_media_player_windows_ipc(
    endpoint: &MediaPlayerIpcEndpoint,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint.server_arg()) {
            Ok(stream) => return Ok(stream),
            Err(error) if media_player_ipc_connect_error_is_retryable(&error) => {
                sleep(MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn media_player_ipc_connect_error_is_retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused | io::ErrorKind::WouldBlock
    )
}

async fn wait_for_mpv_window_id<S>(stream: S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stream);
    let mut request_id = 1_u64;
    let mut line = Vec::new();

    loop {
        let request =
            format!(r#"{{"command":["get_property","window-id"],"request_id":{request_id}}}"#);
        reader.get_mut().write_all(request.as_bytes()).await?;
        reader.get_mut().write_all(b"\n").await?;
        reader.get_mut().flush().await?;

        loop {
            line.clear();
            let bytes_read = reader.read_until(b'\n', &mut line).await?;
            if bytes_read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "media player IPC closed before reporting a window",
                ));
            }
            if let Some(window_ready) = mpv_window_id_response_readiness(&line, request_id) {
                if window_ready {
                    return Ok(());
                }
                break;
            }
        }

        request_id = request_id.saturating_add(1);
        sleep(MEDIA_PLAYER_IPC_WINDOW_POLL_INTERVAL).await;
    }
}

fn mpv_window_id_response_readiness(line: &[u8], request_id: u64) -> Option<bool> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
        return None;
    };
    if value.get("request_id").and_then(serde_json::Value::as_u64) != Some(request_id) {
        return None;
    }
    let success = value.get("error").and_then(serde_json::Value::as_str) == Some("success");

    Some(
        success
            && match value.get("data") {
                Some(serde_json::Value::Number(number)) => {
                    number.as_i64().is_some_and(|id| id != 0)
                        || number.as_u64().is_some_and(|id| id != 0)
                }
                Some(serde_json::Value::String(id)) => !id.is_empty() && id != "0",
                _ => false,
            },
    )
}

#[derive(Debug, Eq, PartialEq)]
struct MediaPlayerCommandSpec {
    program: &'static str,
    args: Vec<String>,
}

struct UrlOpenCommandSpec {
    program: &'static str,
    args: Vec<String>,
}

fn current_open_url_command_spec(url: &str) -> UrlOpenCommandSpec {
    #[cfg(target_os = "macos")]
    {
        UrlOpenCommandSpec {
            program: "open",
            args: vec![url.to_owned()],
        }
    }

    #[cfg(target_os = "windows")]
    {
        windows_open_url_command_spec(url)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        UrlOpenCommandSpec {
            program: "xdg-open",
            args: vec![url.to_owned()],
        }
    }
}

#[cfg(any(test, target_os = "windows"))]
fn windows_open_url_command_spec(url: &str) -> UrlOpenCommandSpec {
    UrlOpenCommandSpec {
        program: "rundll32",
        args: vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
    }
}

struct ResolvedToken {
    token: String,
    warnings: Vec<String>,
}

async fn resolve_token() -> Result<ResolvedToken> {
    let mut warnings = Vec::new();
    let credential_store = match config::load_options() {
        Ok(options) => options.credentials.store,
        Err(error) => {
            warnings.push(format!(
                "config could not be loaded for credential settings: {error}; using auto credential storage"
            ));
            config::CredentialStoreMode::default()
        }
    };

    match load_token_from_store(credential_store).await {
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
    match save_token_to_store(token.clone(), credential_store).await {
        Ok(token_store::TokenSaveLocation::PlaintextFile)
            if credential_store == config::CredentialStoreMode::Auto =>
        {
            warnings.push(
                "system keychain is unavailable; token was saved to the plaintext fallback credential store"
                    .to_owned(),
            );
        }
        Ok(_) => {}
        Err(error) => warnings.push(format!("token was not saved: {error}")),
    }

    Ok(ResolvedToken { token, warnings })
}

async fn load_token_from_store(store: config::CredentialStoreMode) -> Result<Option<String>> {
    tokio::task::spawn_blocking(move || token_store::load_token(store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
}

async fn save_token_to_store(
    token: String,
    store: config::CredentialStoreMode,
) -> Result<token_store::TokenSaveLocation> {
    tokio::task::spawn_blocking(move || token_store::save_token(&token, store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
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

fn leave_current_voice_channel_on_shutdown(client: &DiscordClient) {
    let Some(voice) = client.requested_voice_connection() else {
        return;
    };
    if let Err(message) =
        client.update_voice_state(voice.guild_id, None, voice.self_mute, voice.self_deaf)
    {
        logging::error("app", format!("voice shutdown leave failed: {message}"));
    }
}

async fn shutdown_gateway(client: &DiscordClient, mut gateway_task: tokio::task::JoinHandle<()>) {
    if let Err(message) = client.shutdown_gateway() {
        logging::error("app", format!("gateway shutdown request failed: {message}"));
        gateway_task.abort();
    }

    tokio::select! {
        result = &mut gateway_task => {
            if let Err(error) = result
                && !error.is_cancelled()
            {
                logging::error("app", format!("gateway task ended unexpectedly: {error}"));
            }
        }
        () = sleep(Duration::from_secs(2)) => {
            gateway_task.abort();
            if let Err(error) = gateway_task.await
                && !error.is_cancelled()
            {
                logging::error("app", format!("gateway task ended unexpectedly: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, process};

    use super::{
        login_notice_for_token_warnings, media_player_command_spec_for_url,
        media_player_command_spec_for_url_with_ipc, media_player_spawn_error,
        mpv_window_id_response_readiness, open_url, persist_unique_download_file,
        sanitize_filename, windows_open_url_command_spec,
    };

    fn unix_timestamp_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    }

    #[test]
    fn persist_unique_download_file_uses_next_available_name() {
        let directory = std::env::temp_dir().join(format!(
            "concord-download-test-{}-{}",
            process::id(),
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let existing = directory.join("cat.png");
        fs::write(&existing, b"old").expect("existing file should be written");
        let mut temp = tempfile::Builder::new()
            .tempfile_in(&directory)
            .expect("temporary file should be created");
        temp.write_all(b"new")
            .expect("temporary file should be written");
        let temp_path = temp.into_temp_path();

        let path = persist_unique_download_file(&directory, "cat.png", temp_path)
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

    #[test]
    fn open_url_rejects_non_web_schemes_before_spawning_opener() {
        let error = open_url("file:///etc/passwd").expect_err("file URLs should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn media_player_rejects_non_web_schemes_before_spawning_player() {
        let error = media_player_command_spec_for_url("file:///etc/passwd")
            .expect_err("file URLs should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn media_player_uses_mpv_without_shell_parsing() {
        let spec = media_player_command_spec_for_url("https://example.com/video.mp4?x=1&y=2")
            .expect("https media URLs should be accepted");

        assert_eq!(spec.program, "mpv");
        assert_eq!(
            spec.args,
            vec![
                "--no-terminal".to_owned(),
                "--".to_owned(),
                "https://example.com/video.mp4?x=1&y=2".to_owned(),
            ]
        );
    }

    #[test]
    fn media_player_command_can_enable_json_ipc() {
        let spec = media_player_command_spec_for_url_with_ipc(
            "https://example.com/video.mp4",
            Some("/tmp/concord-mpv.sock"),
        )
        .expect("https media URLs should be accepted");

        assert_eq!(spec.program, "mpv");
        assert_eq!(
            spec.args,
            vec![
                "--no-terminal".to_owned(),
                "--input-ipc-server=/tmp/concord-mpv.sock".to_owned(),
                "--".to_owned(),
                "https://example.com/video.mp4".to_owned(),
            ]
        );
    }

    #[test]
    fn mpv_window_id_response_reports_window_readiness() {
        assert_eq!(
            mpv_window_id_response_readiness(
                br#"{"data":945,"error":"success","request_id":7}"#,
                7,
            ),
            Some(true)
        );
        assert_eq!(
            mpv_window_id_response_readiness(
                br#"{"data":null,"error":"success","request_id":7}"#,
                7,
            ),
            Some(false)
        );
        assert_eq!(
            mpv_window_id_response_readiness(
                br#"{"data":945,"error":"success","request_id":6}"#,
                7,
            ),
            None
        );
    }

    #[test]
    fn media_player_missing_binary_error_mentions_mpv_requirement() {
        let error = media_player_spawn_error(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file or directory",
        ));

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            error.to_string(),
            "mpv is required for media playback; install mpv and make sure it is on PATH"
        );
    }

    #[test]
    fn windows_url_opener_avoids_cmd_shell_parsing() {
        let spec = windows_open_url_command_spec("https://example.com/?a=1&b=2");

        assert_eq!(spec.program, "rundll32");
        assert_eq!(
            spec.args,
            vec![
                "url.dll,FileProtocolHandler".to_owned(),
                "https://example.com/?a=1&b=2".to_owned(),
            ]
        );
    }
}
