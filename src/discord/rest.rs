use std::sync::Arc;

use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::{
    channel::Message,
    id::{Id, marker::ChannelMarker, marker::MessageMarker},
};

use crate::{AppError, Result, discord::ReactionEmoji};

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
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<Message> {
        validate_message_content(content)?;

        let mut request = self.http.create_message(channel_id).content(content);
        if let Some(message_id) = reply_to {
            request = request.reply(message_id);
        }

        let response = request.await?;

        response.model().await.map_err(Into::into)
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<Message>> {
        let request = self.http.channel_messages(channel_id);
        let response = match before {
            Some(message_id) => request.before(message_id).limit(limit).await?,
            None => request.limit(limit).await?,
        };

        response.models().await.map_err(Into::into)
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        let reaction = request_reaction_type(emoji);
        self.http
            .create_reaction(channel_id, message_id, &reaction)
            .await?;
        Ok(())
    }
}

pub fn request_reaction_type(emoji: &ReactionEmoji) -> RequestReactionType<'_> {
    match emoji {
        ReactionEmoji::Unicode(name) => RequestReactionType::Unicode { name },
        ReactionEmoji::Custom { id, name, .. } => RequestReactionType::Custom {
            id: *id,
            name: name.as_deref(),
        },
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
    use twilight_http::request::channel::reaction::RequestReactionType;
    use twilight_model::id::Id;

    use crate::{
        AppError,
        discord::{
            ReactionEmoji,
            rest::{request_reaction_type, validate_message_content},
        },
    };

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

    #[test]
    fn unicode_reaction_uses_twilight_unicode_request_type() {
        let reaction = ReactionEmoji::Unicode("🎉".to_owned());

        assert_eq!(
            request_reaction_type(&reaction),
            RequestReactionType::Unicode { name: "🎉" }
        );
    }

    #[test]
    fn custom_reaction_uses_twilight_custom_request_type_without_animated() {
        let reaction = ReactionEmoji::Custom {
            id: Id::new(42),
            name: Some("party".to_owned()),
            animated: true,
        };

        assert_eq!(
            request_reaction_type(&reaction),
            RequestReactionType::Custom {
                id: Id::new(42),
                name: Some("party"),
            }
        );
    }
}
