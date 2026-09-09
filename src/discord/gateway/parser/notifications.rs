use serde_json::Value;

use crate::discord::events::AppEvent;

use super::shared::parse_id;

pub(super) fn parse_recent_mention_delete(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::InboxRecentMentionDeleted {
        message_id: parse_id(data.get("message_id")?)?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::discord::ids::{Id, marker::MessageMarker};

    #[test]
    fn recent_mention_delete_keeps_the_message_id() {
        assert!(matches!(
            parse_recent_mention_delete(&json!({ "message_id": "200" })),
            Some(AppEvent::InboxRecentMentionDeleted { message_id })
                if message_id == Id::<MessageMarker>::new(200)
        ));
    }
}
