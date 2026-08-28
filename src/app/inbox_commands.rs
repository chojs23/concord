use crate::discord::ids::{Id, marker::MessageMarker};
use crate::{DiscordClient, discord::AppEvent, logging};

const INBOX_PAGE_LIMIT: u16 = 25;

pub(super) async fn load_mentions(
    client: DiscordClient,
    request_id: u64,
    before: Option<Id<MessageMarker>>,
) {
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

pub(super) async fn delete_mention(client: DiscordClient, message_id: Id<MessageMarker>) {
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

fn log_inbox_error(context: &str, error: &crate::AppError) {
    logging::error(
        "inbox",
        format!("{context} failed: {error}; detail={}", error.log_detail()),
    );
}
