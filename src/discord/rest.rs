use std::sync::Arc;

use reqwest::header::AUTHORIZATION;
use serde_json::json;
use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::{
    channel::Message,
    channel::message::ReactionType,
    id::{
        Id,
        marker::{ChannelMarker, MessageMarker, UserMarker},
    },
};

use crate::{
    AppError, Result,
    discord::{ReactionEmoji, ReactionUserInfo, events::reaction_user_info},
};

const REACTION_USERS_PAGE_LIMIT: u16 = 100;

#[derive(Clone, Debug)]
pub struct DiscordRest {
    http: Arc<HttpClient>,
    raw_http: reqwest::Client,
    token: String,
}

impl DiscordRest {
    pub fn new(http: Arc<HttpClient>, token: String) -> Self {
        Self {
            http,
            raw_http: reqwest::Client::new(),
            token,
        }
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

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        let reaction = request_reaction_type(emoji);
        self.http
            .delete_current_user_reaction(channel_id, message_id, &reaction)
            .await?;
        Ok(())
    }

    pub async fn load_reaction_users(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<Vec<ReactionUserInfo>> {
        let reaction = request_reaction_type(emoji);
        let mut users = Vec::new();
        let mut after = None;

        loop {
            let mut request = self
                .http
                .reactions(channel_id, message_id, &reaction)
                .kind(ReactionType::Normal)
                .limit(REACTION_USERS_PAGE_LIMIT);
            if let Some(user_id) = after {
                request = request.after(user_id);
            }

            let page = request.await?.models().await?;
            let next_after = next_reaction_users_after(page.len(), page.last().map(|user| user.id));
            users.extend(page.into_iter().map(|user| reaction_user_info(&user)));

            let Some(user_id) = next_after else {
                break;
            };
            after = Some(user_id);
        }

        Ok(users)
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<Message>> {
        Ok(self
            .http
            .pins(channel_id)
            .limit(50)
            .await?
            .model()
            .await?
            .items
            .into_iter()
            .map(|pin| pin.message)
            .collect())
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        if pinned {
            self.http.create_pin(channel_id, message_id).await?;
        } else {
            self.http.delete_pin(channel_id, message_id).await?;
        }
        Ok(())
    }

    pub async fn vote_poll(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) -> Result<()> {
        let url = format!(
            "https://discord.com/api/v9/channels/{}/polls/{}/answers/@me",
            channel_id.get(),
            message_id.get()
        );
        let answer_ids = answer_ids
            .iter()
            .map(|answer_id| answer_id.to_string())
            .collect::<Vec<_>>();
        self.raw_http
            .put(url)
            .header(AUTHORIZATION, &self.token)
            .json(&json!({ "answer_ids": answer_ids }))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("poll vote request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("poll vote failed: {error}")))?;
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

fn next_reaction_users_after(
    page_len: usize,
    last_user_id: Option<Id<UserMarker>>,
) -> Option<Id<UserMarker>> {
    (page_len == usize::from(REACTION_USERS_PAGE_LIMIT))
        .then_some(last_user_id)
        .flatten()
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
            rest::{next_reaction_users_after, request_reaction_type, validate_message_content},
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

    #[test]
    fn reaction_user_pagination_continues_only_after_full_pages() {
        let last_user_id = Id::new(123);

        assert_eq!(
            next_reaction_users_after(100, Some(last_user_id)),
            Some(last_user_id)
        );
        assert_eq!(next_reaction_users_after(99, Some(last_user_id)), None);
        assert_eq!(next_reaction_users_after(100, None), None);
    }
}
