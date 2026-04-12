use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use reqwest::header::AUTHORIZATION;
use serde_json::{Value, json};

use crate::{
    AppError, Result,
    discord::{
        FriendStatus, MessageInfo, MutualGuildInfo, ReactionEmoji, ReactionUserInfo,
        UserProfileInfo, gateway::parse_message_info,
    },
};

const REACTION_USERS_PAGE_LIMIT: u16 = 100;

#[derive(Clone, Debug)]
pub struct DiscordRest {
    raw_http: reqwest::Client,
    token: String,
}

impl DiscordRest {
    pub fn new(token: String) -> Self {
        Self {
            raw_http: reqwest::Client::new(),
            token,
        }
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<Id<MessageMarker>>,
    ) -> Result<MessageInfo> {
        validate_message_content(content)?;
        let mut body = json!({ "content": content });
        if let Some(message_id) = reply_to {
            body["message_reference"] = json!({ "message_id": message_id.to_string() });
        }

        let raw = self
            .raw_http
            .post(format!(
                "https://discord.com/api/v9/channels/{}/messages",
                channel_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("send message request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("send message failed: {error}")))?
            .json::<Value>()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("send message decode failed: {error}"))
            })?;
        parse_message_info(&raw).ok_or_else(|| {
            AppError::DiscordRequest("send message response was missing required fields".to_owned())
        })
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        let mut request = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/messages",
                channel_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .query(&[("limit", limit.to_string())]);
        if let Some(message_id) = before {
            request = request.query(&[("before", message_id.to_string())]);
        }
        let raw_messages: Vec<Value> = request
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("message history request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("message history failed: {error}")))?
            .json()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("message history decode failed: {error}"))
            })?;

        raw_messages
            .iter()
            .map(|raw| {
                parse_message_info(raw).ok_or_else(|| {
                    AppError::DiscordRequest(
                        "history message response was missing required fields".to_owned(),
                    )
                })
            })
            .collect()
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.raw_http
            .put(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}/@me",
                channel_id.get(),
                message_id.get(),
                reaction_route_component(emoji)
            ))
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("add reaction request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("add reaction failed: {error}")))?;
        Ok(())
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.raw_http
            .delete(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}/@me",
                channel_id.get(),
                message_id.get(),
                reaction_route_component(emoji)
            ))
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("remove reaction request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("remove reaction failed: {error}"))
            })?;
        Ok(())
    }

    pub async fn load_reaction_users(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<Vec<ReactionUserInfo>> {
        let mut users = Vec::new();
        let mut after: Option<Id<UserMarker>> = None;

        loop {
            let mut request = self
                .raw_http
                .get(format!(
                    "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}",
                    channel_id.get(),
                    message_id.get(),
                    reaction_route_component(emoji)
                ))
                .header(AUTHORIZATION, &self.token)
                .query(&[
                    ("limit", REACTION_USERS_PAGE_LIMIT.to_string()),
                    ("type", "0".to_owned()),
                ]);
            if let Some(user_id) = after {
                request = request.query(&[("after", user_id.to_string())]);
            }

            let page: Vec<Value> = request
                .send()
                .await
                .map_err(|error| {
                    AppError::DiscordRequest(format!("reaction users request failed: {error}"))
                })?
                .error_for_status()
                .map_err(|error| {
                    AppError::DiscordRequest(format!("reaction users failed: {error}"))
                })?
                .json()
                .await
                .map_err(|error| {
                    AppError::DiscordRequest(format!("reaction users decode failed: {error}"))
                })?;
            let parsed_page: Vec<ReactionUserInfo> = page
                .iter()
                .filter_map(reaction_user_info_from_raw)
                .collect();
            let next_after = next_reaction_users_after(
                parsed_page.len(),
                parsed_page.last().map(|user| user.user_id),
            );
            users.extend(parsed_page);

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
    ) -> Result<Vec<MessageInfo>> {
        let raw: Value = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/messages/pins",
                channel_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .query(&[("limit", "50")])
            .send()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("pins request failed: {error}")))?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("pins failed: {error}")))?
            .json()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("pins decode failed: {error}")))?;
        let messages: Vec<&Value> = match &raw {
            Value::Array(items) => items.iter().collect(),
            Value::Object(object) => object
                .get("items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("message"))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        messages
            .into_iter()
            .map(|raw| {
                parse_message_info(raw).ok_or_else(|| {
                    AppError::DiscordRequest("pin message was missing required fields".to_owned())
                })
            })
            .collect()
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        let request = if pinned {
            self.raw_http.put(format!(
                "https://discord.com/api/v9/channels/{}/pins/{}",
                channel_id.get(),
                message_id.get()
            ))
        } else {
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/pins/{}",
                channel_id.get(),
                message_id.get()
            ))
        };
        request
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("pin request failed: {error}")))?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("pin update failed: {error}")))?;
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
        let response = response
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("user note failed: {error}")))?;
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

fn reaction_user_info_from_raw(value: &Value) -> Option<ReactionUserInfo> {
    let user_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .and_then(Id::<UserMarker>::new_checked)?;
    let display_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| value.get("username").and_then(Value::as_str))?
        .to_owned();

    Some(ReactionUserInfo {
        user_id,
        display_name,
    })
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

fn reaction_route_component(emoji: &ReactionEmoji) -> String {
    match emoji {
        ReactionEmoji::Unicode(name) => percent_encode_path_segment(name),
        ReactionEmoji::Custom { id, name, .. } => {
            percent_encode_path_segment(&format!("{}:{id}", name.as_deref().unwrap_or_default()))
        }
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
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
    use crate::discord::ids::{Id, marker::EmojiMarker};

    use crate::{
        AppError,
        discord::{
            ReactionEmoji,
            rest::{
                next_reaction_users_after, poll_vote_request_body, reaction_route_component,
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
    fn unicode_reaction_route_component_is_percent_encoded() {
        let reaction = ReactionEmoji::Unicode("🎉".to_owned());

        assert_eq!(reaction_route_component(&reaction), "%F0%9F%8E%89");
    }

    #[test]
    fn custom_reaction_route_component_uses_name_and_id() {
        let reaction = ReactionEmoji::Custom {
            id: Id::<EmojiMarker>::new(42),
            name: Some("party".to_owned()),
            animated: true,
        };

        assert_eq!(reaction_route_component(&reaction), "party%3A42");
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
