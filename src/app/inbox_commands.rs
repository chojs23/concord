use crate::{
    DiscordClient,
    discord::{AppCommand, AppEvent},
    logging,
};

const INBOX_PAGE_LIMIT: u16 = 25;

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::LoadInboxMentions { request_id, before } => {
            match client.load_recent_mentions(before, INBOX_PAGE_LIMIT).await {
                Ok(messages) => {
                    let has_more = messages.len() >= usize::from(INBOX_PAGE_LIMIT);
                    client
                        .publish_event(AppEvent::InboxMentionsLoaded {
                            request_id,
                            before,
                            messages,
                            has_more,
                        })
                        .await;
                }
                Err(error) => {
                    log_inbox_error("load recent mentions", &error);
                    client
                        .publish_event(AppEvent::InboxMentionsLoadFailed { request_id, before })
                        .await;
                }
            }
        }
        AppCommand::DeleteInboxMention { message_id } => {
            match client.delete_recent_mention(message_id).await {
                Ok(()) => {
                    client
                        .publish_event(AppEvent::InboxRecentMentionDeleted { message_id })
                        .await;
                }
                Err(error) => {
                    let message = format!("delete recent mention failed: {error}");
                    log_inbox_error("delete recent mention", &error);
                    client
                        .publish_event(AppEvent::InboxRecentMentionDeleteFailed {
                            message_id,
                            message,
                        })
                        .await;
                }
            }
        }
        _ => unreachable!("non-inbox command routed to inbox handler"),
    }
}

fn log_inbox_error(context: &str, error: &crate::AppError) {
    logging::error(
        "inbox",
        format!("{context} failed: {error}; detail={}", error.log_detail()),
    );
}
