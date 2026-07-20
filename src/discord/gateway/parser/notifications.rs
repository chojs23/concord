use serde_json::Value;

use crate::discord::{events::AppEvent, ids::Id};

pub(super) fn parse_recent_mention_delete(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::InboxRecentMentionDeleted {
        message_id: parse_id(data.get("message_id")?)?,
    })
}

fn parse_id<T>(value: &Value) -> Option<Id<T>> {
    value
        .as_str()
        .and_then(|raw| raw.parse().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::discord::ids::marker::MessageMarker;

    #[test]
    fn recent_mention_delete_keeps_the_message_id() {
        assert!(matches!(
            parse_recent_mention_delete(&json!({ "message_id": "200" })),
            Some(AppEvent::InboxRecentMentionDeleted { message_id })
                if message_id == Id::<MessageMarker>::new(200)
        ));
    }
}
