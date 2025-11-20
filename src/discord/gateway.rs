use serde_json::Value;
use tokio::sync::broadcast;
use twilight_gateway::{
    EventTypeFlags, Shard, StreamExt, error::ReceiveMessageErrorType,
};
use twilight_model::gateway::{Intents, ShardId};

use super::events::{AppEvent, map_event};

pub async fn run_gateway(
    token: String,
    message_content_enabled: bool,
    tx: broadcast::Sender<AppEvent>,
) {
    let mut intents = Intents::GUILDS
        | Intents::GUILD_MESSAGES
        | Intents::DIRECT_MESSAGES
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_PRESENCES;
    if message_content_enabled {
        intents |= Intents::MESSAGE_CONTENT;
    }

    let mut shard = Shard::new(ShardId::ONE, token, intents);

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        match item {
            Ok(event) => {
                if let Some(event) = map_event(event, message_content_enabled) {
                    let _ = tx.send(event);
                }
            }
            Err(error) => {
                // User-account payloads (e.g. READY) often don't fit twilight's bot-shaped
                // structs. Fall back to a minimal raw-JSON parser for the events we care
                // about and silently drop the rest instead of spamming the footer.
                if let ReceiveMessageErrorType::Deserializing { event } = error.kind() {
                    if let Some(fallback) = parse_user_account_event(event) {
                        let _ = tx.send(fallback);
                    }
                    continue;
                }

                let _ = tx.send(AppEvent::GatewayError {
                    message: error.to_string(),
                });
            }
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}

/// Best-effort fallback for events that twilight cannot deserialize into its
/// bot-typed structs. We only extract the fields the dashboard actually needs.
fn parse_user_account_event(raw: &str) -> Option<AppEvent> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let event_type = value.get("t").and_then(Value::as_str)?;
    let data = value.get("d")?;

    match event_type {
        "READY" => {
            let user = data.get("user")?;
            let name = user
                .get("global_name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .or_else(|| user.get("username").and_then(Value::as_str))
                .unwrap_or("unknown")
                .to_owned();
            Some(AppEvent::Ready { user: name })
        }
        _ => None,
    }
}
