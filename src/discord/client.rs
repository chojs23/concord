use std::sync::Arc;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tokio::{sync::broadcast, task::JoinHandle};
use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    id::{
        Id,
        marker::{ChannelMarker, MessageMarker},
    },
};

use crate::{AppError, Result};

use super::{events::AppEvent, gateway::run_gateway, rest::DiscordRest};

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    events_tx: broadcast::Sender<AppEvent>,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self> {
        let http = Arc::new(http_client_for_token(&token)?);
        let rest = DiscordRest::new(http);
        let (events_tx, _) = broadcast::channel(512);

        Ok(Self {
            token,
            rest,
            events_tx,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events_tx.subscribe()
    }

    pub fn publish_event(&self, event: AppEvent) {
        let _ = self.events_tx.send(event);
    }

    pub fn start_gateway(&self) -> JoinHandle<()> {
        let token = self.token.clone();
        let events_tx = self.events_tx.clone();

        tokio::spawn(async move {
            run_gateway(token, events_tx).await;
        })
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<Message> {
        self.rest.send_message(channel_id, content).await
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<Message>> {
        self.rest
            .load_message_history(channel_id, before, limit)
            .await
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &str,
    ) -> Result<()> {
        self.rest
            .add_unicode_reaction(channel_id, message_id, emoji)
            .await
    }
}

fn http_client_for_token(token: &str) -> Result<HttpClient> {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(token)
        .map_err(|source| AppError::InvalidDiscordTokenHeader { source })?;
    headers.insert(AUTHORIZATION, value);

    Ok(HttpClient::builder().default_headers(headers).build())
}

#[cfg(test)]
mod tests {
    use super::http_client_for_token;

    #[tokio::test]
    async fn builds_http_client_with_raw_user_token() {
        http_client_for_token("raw-user-token").expect("raw user token must be accepted");
    }

    #[tokio::test]
    async fn rejects_tokens_that_are_invalid_http_header_values() {
        http_client_for_token("invalid\nuser-token")
            .expect_err("newlines are not valid authorization header values");
    }
}
