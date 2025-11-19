use tokio::sync::broadcast;
use twilight_gateway::{EventTypeFlags, Shard, StreamExt};
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
                let _ = tx.send(AppEvent::GatewayError {
                    message: error.to_string(),
                });
            }
        }
    }

    let _ = tx.send(AppEvent::GatewayClosed);
}
