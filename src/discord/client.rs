use std::sync::{Arc, Mutex};

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker},
    },
};

use crate::{AppError, Result};

use super::{
    MessageInfo, ReactionEmoji, ReactionUserInfo,
    events::AppEvent,
    gateway::{GatewayCommand, run_gateway},
    rest::DiscordRest,
};

#[derive(Clone, Debug)]
pub struct DiscordClient {
    token: String,
    rest: DiscordRest,
    events_tx: broadcast::Sender<AppEvent>,
    gateway_commands_tx: mpsc::UnboundedSender<GatewayCommand>,
    gateway_commands_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<GatewayCommand>>>>,
}

impl DiscordClient {
    pub fn new(token: String) -> Result<Self> {
        let http = Arc::new(http_client_for_token(&token)?);
        let rest = DiscordRest::new(http, token.clone());
        let (events_tx, _) = broadcast::channel(512);
        let (gateway_commands_tx, gateway_commands_rx) = mpsc::unbounded_channel();

        Ok(Self {
            token,
            rest,
            events_tx,
            gateway_commands_tx,
            gateway_commands_rx: Arc::new(Mutex::new(Some(gateway_commands_rx))),
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
        let gateway_commands = self
            .gateway_commands_rx
            .lock()
            .expect("gateway command receiver mutex is not poisoned")
            .take()
            .expect("gateway can only be started once");

        tokio::spawn(async move {
            run_gateway(token, events_tx, gateway_commands).await;
        })
    }

    pub fn request_guild_members(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::RequestGuildMembers { guild_id })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn subscribe_direct_message(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::SubscribeDirectMessage { channel_id })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub fn subscribe_guild_channel(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> std::result::Result<(), String> {
        self.gateway_commands_tx
            .send(GatewayCommand::SubscribeGuildChannel {
                guild_id,
                channel_id,
            })
            .map_err(|_| "gateway command channel closed".to_owned())
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<Message> {
        self.rest.send_message(channel_id, content, reply_to).await
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
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.rest.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.rest
            .remove_current_user_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_reaction_users(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<Vec<ReactionUserInfo>> {
        self.rest
            .load_reaction_users(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        Ok(self
            .rest
            .load_pinned_messages(channel_id)
            .await?
            .into_iter()
            .map(MessageInfo::from_message)
            .collect())
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        self.rest
            .set_message_pinned(channel_id, message_id, pinned)
            .await
    }

    pub async fn vote_poll(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) -> Result<()> {
        self.rest
            .vote_poll(channel_id, message_id, answer_ids)
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
