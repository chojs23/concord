use std::sync::Arc;

use tokio::{sync::broadcast, task::JoinHandle};
use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    id::{Id, marker::ChannelMarker},
};

use crate::Result;

use super::{events::AppEvent, gateway::run_gateway, rest::DiscordRest};

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    events_tx: broadcast::Sender<AppEvent>,
}

impl DiscordClient {
    pub fn new(token: String) -> Self {
        let http = Arc::new(HttpClient::new(token.clone()));
        let rest = DiscordRest::new(http);
        let (events_tx, _) = broadcast::channel(512);

        Self {
            token,
            rest,
            events_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events_tx.subscribe()
    }

    pub fn publish_event(&self, event: AppEvent) {
        let _ = self.events_tx.send(event);
    }

    pub fn start_gateway(&self, message_content_enabled: bool) -> JoinHandle<()> {
        let token = self.token.clone();
        let events_tx = self.events_tx.clone();

        tokio::spawn(async move {
            run_gateway(token, message_content_enabled, events_tx).await;
        })
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<Message> {
        self.rest.send_message(channel_id, content).await
    }
}
