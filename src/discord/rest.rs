use std::sync::Arc;

use reqwest::header::AUTHORIZATION;
use serde_json::{Value, json};
use twilight_http::Client as HttpClient;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::{
    channel::Message,
    channel::message::ReactionType,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};

use crate::{
    AppError, Result,
    discord::{
        FriendStatus, MutualGuildInfo, ReactionEmoji, ReactionUserInfo, UserProfileInfo,
        events::reaction_user_info,
    },
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

    pub async fn load_user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Result<UserProfileInfo> {
        let mut url = format!(
            "https://discord.com/api/v9/users/{}/profile?with_mutual_guilds=true&with_mutual_friends_count=true",
            user_id.get()
        );
        if let Some(guild_id) = guild_id {
            url.push_str(&format!("&guild_id={}", guild_id.get()));
        }
        let response = self
            .raw_http
            .get(url)
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("user profile request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("user profile failed: {error}")))?;
        let body: Value = response.json().await.map_err(|error| {
            AppError::DiscordRequest(format!("user profile decode failed: {error}"))
        })?;

        let note = self.load_user_note(user_id).await.unwrap_or(None);

        Ok(parse_user_profile_response(user_id, &body, note))
    }

    /// Returns the user's saved note, or `None` if Discord responds 404
    /// (no note set). Other errors propagate.
    async fn load_user_note(&self, user_id: Id<UserMarker>) -> Result<Option<String>> {
        let url = format!(
            "https://discord.com/api/v9/users/@me/notes/{}",
            user_id.get()
        );
        let response = self
            .raw_http
            .get(url)
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("user note request failed: {error}"))
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status().map_err(|error| {
            AppError::DiscordRequest(format!("user note failed: {error}"))
        })?;
        let body: Value = response.json().await.map_err(|error| {
            AppError::DiscordRequest(format!("user note decode failed: {error}"))
        })?;
        Ok(body
            .get("note")
            .and_then(Value::as_str)
            .filter(|note| !note.is_empty())
            .map(str::to_owned))
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
        self.raw_http
            .put(url)
            .header(AUTHORIZATION, &self.token)
            .json(&poll_vote_request_body(answer_ids))
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

fn poll_vote_request_body(answer_ids: &[u8]) -> Value {
    json!({ "answer_ids": answer_ids })
}

/// Builds the dashboard's `UserProfileInfo` from Discord's
/// `/users/{id}/profile` JSON. Friend status is left as `None` here — the
/// caller fills it in from cached relationship data.
fn parse_user_profile_response(
    user_id: Id<UserMarker>,
    body: &Value,
    note: Option<String>,
) -> UserProfileInfo {
    let user = body.get("user");
    let username = user
        .and_then(|user| user.get("username"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let global_name = user
        .and_then(|user| user.get("global_name"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let avatar_url = user.and_then(profile_avatar_url);
    let user_profile = body.get("user_profile");
    let bio = user_profile
        .and_then(|profile| profile.get("bio"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let pronouns = user_profile
        .and_then(|profile| profile.get("pronouns"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mutual_guilds = body
        .get("mutual_guilds")
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|entry| {
                    let guild_id = entry
                        .get("id")
                        .and_then(Value::as_str)
                        .and_then(|raw| raw.parse::<u64>().ok())
                        .and_then(Id::<GuildMarker>::new_checked)?;
                    let nick = entry
                        .get("nick")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned);
                    Some(MutualGuildInfo { guild_id, nick })
                })
                .collect()
        })
        .unwrap_or_default();
    let mutual_friends_count = body
        .get("mutual_friends_count")
        .and_then(Value::as_u64)
        .map(|value| u32::try_from(value).unwrap_or(u32::MAX))
        .unwrap_or(0);
    let guild_nick = body
        .get("guild_member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    UserProfileInfo {
        user_id,
        username,
        global_name,
        guild_nick,
        avatar_url,
        bio,
        pronouns,
        mutual_guilds,
        mutual_friends_count,
        friend_status: FriendStatus::None,
        note,
    }
}

fn profile_avatar_url(user: &Value) -> Option<String> {
    let user_id = user
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())?;
    let hash = user
        .get("avatar")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let extension = if hash.starts_with("a_") { "gif" } else { "png" };
    Some(format!(
        "https://cdn.discordapp.com/avatars/{user_id}/{hash}.{extension}"
    ))
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
            rest::{
                next_reaction_users_after, poll_vote_request_body, request_reaction_type,
                validate_message_content,
            },
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

    #[test]
    fn poll_vote_request_body_uses_numeric_answer_ids() {
        assert_eq!(
            poll_vote_request_body(&[1, 2]),
            serde_json::json!({ "answer_ids": [1, 2] })
        );
        assert_eq!(
            poll_vote_request_body(&[]),
            serde_json::json!({ "answer_ids": [] })
        );
    }
}
