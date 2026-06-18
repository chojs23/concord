use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use crate::{
    DiscordClient,
    discord::{AppCommand, AppEvent, MessageHistoryAfterMode, MessageHistoryLoadTarget},
    logging,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;
const THREAD_PREVIEW_LIMIT: u16 = 1;

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::LoadMessageHistory { channel_id, before } => {
            if let Some(before) = before
                && !client.begin_older_message_history_request(channel_id, before)
            {
                return;
            }
            let endpoint =
                format_message_history_endpoint(channel_id, before, MESSAGE_HISTORY_LIMIT);
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
                    publish_message_history_load_failed(
                        &client,
                        channel_id,
                        before
                            .map(|before| MessageHistoryLoadTarget::Older { before })
                            .unwrap_or(MessageHistoryLoadTarget::Latest),
                        message,
                    )
                    .await;
                }
            }
        }
        AppCommand::RefreshMessageHistory { channel_id } => {
            let endpoint = format_message_history_endpoint(channel_id, None, MESSAGE_HISTORY_LIMIT);
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
                    publish_message_history_load_failed(
                        &client,
                        channel_id,
                        MessageHistoryLoadTarget::Latest,
                        message,
                    )
                    .await;
                }
            }
        }
        AppCommand::LoadMessageHistoryAfter {
            channel_id,
            after,
            mode,
        } => {
            if !client.begin_message_history_after_request(channel_id, after, mode) {
                return;
            }
            let (operation, failure_prefix) = match mode {
                MessageHistoryAfterMode::GapFill => {
                    ("load_message_history_after", "load message history failed")
                }
                MessageHistoryAfterMode::CatchUp => (
                    "catch_up_message_history_after",
                    "catch up message history failed",
                ),
            };
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
                            mode,
                        })
                        .await;
                }
                Err(error) => {
                    let message = format!("{failure_prefix}: {error}");
                    let detail = error.log_detail();
                    logging::error(
                        "history",
                        format!(
                            "op={operation} channel_id={} after={} limit={} endpoint=\"{endpoint}\" {message}; detail={detail}",
                            channel_id.get(),
                            after.get(),
                            MESSAGE_HISTORY_LIMIT,
                        ),
                    );
                    publish_message_history_load_failed(
                        &client,
                        channel_id,
                        MessageHistoryLoadTarget::Newer { after },
                        message,
                    )
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
                .load_message_history_around(channel_id, message_id, MESSAGE_HISTORY_LIMIT)
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
                    publish_message_history_load_failed(
                        &client,
                        channel_id,
                        MessageHistoryLoadTarget::Around { message_id },
                        message,
                    )
                    .await;
                }
            }
        }
        AppCommand::LoadThreadPreview {
            channel_id,
            message_id,
        } => match client
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
        },
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
        AppCommand::SearchMessages { query } => match client.search_messages(query.clone()).await {
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
                    .publish_event(AppEvent::MessageSearchLoadFailed { query, message })
                    .await;
            }
        },
        _ => unreachable!("non-history command routed to history handler"),
    }
}

async fn publish_message_history_load_failed(
    client: &DiscordClient,
    channel_id: Id<ChannelMarker>,
    target: MessageHistoryLoadTarget,
    message: String,
) {
    client
        .publish_event(AppEvent::MessageHistoryLoadFailed {
            channel_id,
            target,
            message,
        })
        .await;
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
