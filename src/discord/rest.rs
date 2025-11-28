use std::sync::Arc;

use twilight_http::Client as HttpClient;
use twilight_model::{
    channel::Message,
    id::{Id, marker::ChannelMarker},
};

use crate::{AppError, Result};

#[derive(Clone, Debug)]
pub struct DiscordRest {
    http: Arc<HttpClient>,
}

impl DiscordRest {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<Message> {
        validate_message_content(content)?;

        let response = self
            .http
            .create_message(channel_id)
            .content(content)
            .await?;

        response.model().await.map_err(Into::into)
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        limit: u16,
    ) -> Result<Vec<Message>> {
        let response = self.http.channel_messages(channel_id).limit(limit).await?;

        response.models().await.map_err(Into::into)
    }
}

pub fn validate_message_content(content: &str) -> Result<()> {
    if content.trim().is_empty() {
        return Err(AppError::EmptyMessageContent);
    }

    let len = content.chars().count();
    if len > 2_000 {
        return Err(AppError::MessageTooLong { len });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{AppError, discord::rest::validate_message_content};

    #[test]
    fn rejects_empty_messages() {
        let error = validate_message_content("   ").expect_err("blank messages must fail");
        assert!(matches!(error, AppError::EmptyMessageContent));
    }

    #[test]
    fn rejects_messages_over_discord_limit() {
        let content = "x".repeat(2_001);
        let error = validate_message_content(&content).expect_err("oversized message must fail");
        assert!(matches!(error, AppError::MessageTooLong { len: 2_001 }));
    }
}
