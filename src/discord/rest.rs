use std::time::Duration;

use crate::discord::fingerprint::discord_rest_client;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};
use reqwest::{
    StatusCode,
    header::AUTHORIZATION,
    multipart::{Form, Part},
};
use serde_json::{Value, json};

use crate::{
    AppError, Result,
    discord::{
        ChannelInfo, ForumPostArchiveState, FriendStatus, MAX_UPLOAD_ATTACHMENT_COUNT,
        MAX_UPLOAD_FILE_BYTES, MAX_UPLOAD_TOTAL_BYTES, MessageAttachmentUpload, MessageInfo,
        MutualGuildInfo, ReactionEmoji, ReactionUserInfo, UserProfileInfo,
        gateway::{parse_channel_info, parse_message_info},
    },
};

const REACTION_USERS_PAGE_LIMIT: u16 = 100;
const FORUM_POST_SEARCH_PAGE_LIMIT: u16 = 25;
// Discord returns 202 ACCEPTED while it warms the per-forum search index.
// Wait briefly then retry; with two attempts after the original we cover the
// common cold-start window without making the user wait on a stuck index.
const FORUM_POST_SEARCH_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(250), Duration::from_millis(500)];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostPage {
    pub posts: Vec<ChannelInfo>,
    pub preview_messages: Vec<MessageInfo>,
    pub has_more: bool,
    pub next_offset: usize,
}

#[derive(Clone, Debug)]
pub struct DiscordRest {
    raw_http: reqwest::Client,
    token: String,
}

impl DiscordRest {
    pub fn new(token: String) -> Self {
        Self {
            raw_http: discord_rest_client(),
            token,
        }
    }

    /// Fire a cheap REST call to establish the HTTPS connection up front.
    /// `reqwest::Client` lazily opens a TCP+TLS+HTTP/2 connection on the first
    /// request, which costs ~500ms-1s of round-trips. The first user-facing
    /// fetch (e.g. opening a forum) would otherwise pay that cost on top of
    /// the search index cold-start, doubled because we issue two parallel
    /// search calls. Priming the pool at startup lets the first real request
    /// reuse the warmed connection and start in single-digit milliseconds.
    pub async fn prime_connection_pool(&self) -> Result<()> {
        self.raw_http
            .get("https://discord.com/api/v9/users/@me")
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("connection prime request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("connection prime failed: {error}"))
            })?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<Id<MessageMarker>>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<MessageInfo> {
        validate_message_payload(content, attachments)?;
        let body = message_request_body(content, reply_to, attachments);

        let request = self
            .raw_http
            .post(format!(
                "https://discord.com/api/v9/channels/{}/messages",
                channel_id.get()
            ))
            .header(AUTHORIZATION, &self.token);

        let request = if attachments.is_empty() {
            request.json(&body)
        } else {
            request.multipart(message_multipart_form(body, attachments).await?)
        };

        let raw = request
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

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        validate_message_content(content)?;
        let raw = self
            .raw_http
            .patch(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}",
                channel_id.get(),
                message_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .json(&json!({ "content": content }))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("edit message request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("edit message failed: {error}")))?
            .json::<Value>()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("edit message decode failed: {error}"))
            })?;
        parse_message_info(&raw).ok_or_else(|| {
            AppError::DiscordRequest("edit message response was missing required fields".to_owned())
        })
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.raw_http
            .delete(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}",
                channel_id.get(),
                message_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("delete message request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("delete message failed: {error}")))?;
        Ok(())
    }

    /// `token: null` is the legacy anti-spam echo field; modern clients
    /// always send null.
    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.raw_http
            .post(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/ack",
                channel_id.get(),
                message_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .json(&json!({ "token": Value::Null }))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("ack channel request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("ack channel failed: {error}")))?;
        Ok(())
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        if targets.is_empty() {
            return Ok(());
        }

        let read_states: Vec<_> = targets
            .iter()
            .map(|(channel_id, message_id)| {
                json!({
                    "read_state_type": 0,
                    "channel_id": channel_id.get().to_string(),
                    "message_id": message_id.get().to_string(),
                })
            })
            .collect();

        self.raw_http
            .post("https://discord.com/api/v9/read-states/ack-bulk")
            .header(AUTHORIZATION, &self.token)
            .json(&json!({ "read_states": read_states }))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("ack channels request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("ack channels failed: {error}")))?;
        Ok(())
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

    pub async fn load_forum_posts(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) -> Result<ForumPostPage> {
        // The `last_message_time` index excludes posts where nobody has
        // replied yet (`message_count == 0`), and the `creation_time` index
        // doesn't surface old-but-active threads in its first page. Discord's
        // own client gets the union by querying both, so on the very first
        // page we issue both calls in parallel and merge. Subsequent pages
        // only need `last_message_time` because zero-reply posts are almost
        // always recent and already covered by the first response.
        if offset == 0 {
            let (activity, recent) = tokio::join!(
                self.load_forum_post_search_page(
                    guild_id,
                    channel_id,
                    archive_state,
                    offset,
                    ForumSearchSort::LastMessageTime,
                ),
                self.load_forum_post_search_page(
                    guild_id,
                    channel_id,
                    archive_state,
                    offset,
                    ForumSearchSort::CreationTime,
                ),
            );
            return Ok(merge_forum_pages(activity?, recent?));
        }

        self.load_forum_post_search_page(
            guild_id,
            channel_id,
            archive_state,
            offset,
            ForumSearchSort::LastMessageTime,
        )
        .await
    }

    async fn load_forum_post_search_page(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        sort_by: ForumSearchSort,
    ) -> Result<ForumPostPage> {
        // `/threads/search` is the only Discord endpoint that ships
        // `first_messages` alongside thread metadata, so we never want to
        // fall back to the active/archived endpoints — they can't supply
        // previews and routinely 403 on user-account tokens. Instead retry
        // briefly when the search index is still warming up.
        let mut last_error = None;
        for delay in std::iter::once(Duration::ZERO).chain(FORUM_POST_SEARCH_RETRY_DELAYS) {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let started = std::time::Instant::now();
            match self
                .request_forum_post_search_page(
                    guild_id,
                    channel_id,
                    archive_state,
                    offset,
                    sort_by,
                )
                .await
            {
                Ok(page) => {
                    crate::logging::error(
                        "history",
                        format!(
                            "TIMING op=forum_search archive_state={} sort={} channel_id={} offset={} duration={:.0}ms",
                            archive_state.as_log_label(),
                            sort_by.as_str(),
                            channel_id.get(),
                            offset,
                            started.elapsed().as_secs_f64() * 1_000.0,
                        ),
                    );
                    return Ok(page);
                }
                Err(error) if is_search_index_warming(&error) => {
                    last_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.expect("retry loop runs at least once"))
    }

    async fn request_forum_post_search_page(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        sort_by: ForumSearchSort,
    ) -> Result<ForumPostPage> {
        let response = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/threads/search",
                channel_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .query(&[
                ("archived", archive_state.as_query_value().to_owned()),
                ("sort_by", sort_by.as_str().to_owned()),
                ("sort_order", "desc".to_owned()),
                ("limit", FORUM_POST_SEARCH_PAGE_LIMIT.to_string()),
                ("tag_setting", "match_some".to_owned()),
                ("offset", offset.to_string()),
            ])
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("forum post search request failed: {error}"))
            })?;
        if response.status() == StatusCode::ACCEPTED {
            return Err(AppError::DiscordRequest(
                "forum post search index is not ready".to_owned(),
            ));
        }
        let raw: Value = response
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("forum post search failed: {error}"))
            })?
            .json()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("forum post search decode failed: {error}"))
            })?;

        let posts = parse_forum_thread_page(&raw, Some(guild_id), channel_id, true);
        let preview_messages = parse_forum_preview_messages(&raw, &posts);

        Ok(ForumPostPage {
            next_offset: offset.saturating_add(posts.len()),
            posts,
            preview_messages,
            has_more: raw
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
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
        is_self: bool,
    ) -> Result<UserProfileInfo> {
        let mut url = format!(
            "https://discord.com/api/v9/users/{}/profile?",
            user_id.get(),
        );
        if !is_self {
            url.push_str("with_mutual_guilds=true&with_mutual_friends_count=true&");
        }
        if let Some(guild_id) = guild_id {
            url.push_str(&format!("guild_id={}", guild_id.get()));
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
    let role_ids = body
        .get("guild_member")
        .and_then(|member| member.get("roles"))
        .and_then(Value::as_array)
        .map(|roles| roles.iter().filter_map(parse_profile_role_id).collect())
        .unwrap_or_default();

    UserProfileInfo {
        user_id,
        username,
        global_name,
        guild_nick,
        role_ids,
        avatar_url,
        bio,
        pronouns,
        mutual_guilds,
        mutual_friends_count,
        friend_status: FriendStatus::None,
        note,
    }
}

fn parse_profile_role_id(value: &Value) -> Option<Id<RoleMarker>> {
    value
        .as_str()
        .and_then(|raw| raw.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
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

fn parse_forum_thread_page(
    raw: &Value,
    guild_id: Option<Id<GuildMarker>>,
    parent_channel_id: Id<ChannelMarker>,
    fill_missing_parent: bool,
) -> Vec<ChannelInfo> {
    raw.get("threads")
        .and_then(Value::as_array)
        .map(|threads| {
            threads
                .iter()
                .filter_map(|thread| {
                    let mut info = parse_channel_info(thread, guild_id)?;
                    if fill_missing_parent && info.parent_id.is_none() {
                        info.parent_id = Some(parent_channel_id);
                    }
                    Some(info)
                })
                .filter(|thread| thread.parent_id == Some(parent_channel_id))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_forum_preview_messages(raw: &Value, posts: &[ChannelInfo]) -> Vec<MessageInfo> {
    let mut seen = std::collections::HashSet::new();
    ["first_messages", "messages", "most_recent_messages"]
        .into_iter()
        .flat_map(|field| parse_forum_messages_from_field(raw, posts, field))
        .filter(|message| seen.insert(message.message_id))
        .collect()
}

fn parse_forum_messages_from_field(
    raw: &Value,
    posts: &[ChannelInfo],
    field: &str,
) -> Vec<MessageInfo> {
    raw.get(field)
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(parse_message_info)
                .filter(|message| {
                    posts
                        .iter()
                        .any(|post| post.channel_id == message.channel_id)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_search_index_warming(error: &AppError) -> bool {
    match error {
        AppError::DiscordRequest(message) => {
            message.contains("forum post search index is not ready")
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForumSearchSort {
    LastMessageTime,
    CreationTime,
}

impl ForumSearchSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::LastMessageTime => "last_message_time",
            Self::CreationTime => "creation_time",
        }
    }
}

/// Combine the two first-page responses Discord uses to build the "Recent
/// activity" view. `active` (last_message_time) carries threads with replies;
/// `recent` (creation_time) carries the freshly-created zero-reply ones. We
/// dedupe by `channel_id` — the order doesn't matter because the display
/// layer re-sorts by `last_message_id` snowflake. `has_more` only follows the
/// `last_message_time` cursor since subsequent pages use that sort alone.
fn merge_forum_pages(active: ForumPostPage, recent: ForumPostPage) -> ForumPostPage {
    let mut seen_posts = std::collections::HashSet::new();
    let mut posts = Vec::with_capacity(active.posts.len() + recent.posts.len());
    for post in active.posts.into_iter().chain(recent.posts) {
        if seen_posts.insert(post.channel_id) {
            posts.push(post);
        }
    }
    let mut seen_previews = std::collections::HashSet::new();
    let mut preview_messages =
        Vec::with_capacity(active.preview_messages.len() + recent.preview_messages.len());
    for message in active
        .preview_messages
        .into_iter()
        .chain(recent.preview_messages)
    {
        if seen_previews.insert(message.message_id) {
            preview_messages.push(message);
        }
    }
    ForumPostPage {
        next_offset: active.next_offset,
        posts,
        preview_messages,
        has_more: active.has_more,
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

fn message_request_body(
    content: &str,
    reply_to: Option<Id<MessageMarker>>,
    attachments: &[MessageAttachmentUpload],
) -> Value {
    let mut body = json!({ "content": content });
    if let Some(message_id) = reply_to {
        body["message_reference"] = json!({ "message_id": message_id.to_string() });
    }
    if !attachments.is_empty() {
        body["attachments"] = Value::Array(
            attachments
                .iter()
                .enumerate()
                .map(|(index, attachment)| {
                    json!({
                        "id": index,
                        "filename": attachment.filename,
                    })
                })
                .collect(),
        );
    }
    body
}

async fn message_multipart_form(
    body: Value,
    attachments: &[MessageAttachmentUpload],
) -> Result<Form> {
    let actual_sizes = attachment_file_sizes(attachments).await?;
    validate_attachment_sizes(&actual_sizes)?;

    let mut form = Form::new().part(
        "payload_json",
        Part::text(body.to_string())
            .mime_str("application/json")
            .map_err(|error| AppError::DiscordRequest(format!("upload payload failed: {error}")))?,
    );

    for (index, attachment) in attachments.iter().enumerate() {
        let bytes = tokio::fs::read(&attachment.path).await.map_err(|error| {
            AppError::DiscordRequest(format!(
                "read attachment {} failed: {error}",
                attachment.filename
            ))
        })?;
        validate_attachment_sizes(&[(attachment.filename.clone(), bytes.len() as u64)])?;
        let content_type = upload_content_type(&attachment.filename);
        let part = Part::bytes(bytes)
            .file_name(attachment.filename.clone())
            .mime_str(&content_type)
            .map_err(|error| {
                AppError::DiscordRequest(format!(
                    "attachment {} content type failed: {error}",
                    attachment.filename
                ))
            })?;
        form = form.part(format!("files[{index}]"), part);
    }
    Ok(form)
}

async fn attachment_file_sizes(
    attachments: &[MessageAttachmentUpload],
) -> Result<Vec<(String, u64)>> {
    let mut sizes = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let metadata = tokio::fs::metadata(&attachment.path)
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!(
                    "stat attachment {} failed: {error}",
                    attachment.filename
                ))
            })?;
        sizes.push((attachment.filename.clone(), metadata.len()));
    }
    Ok(sizes)
}

fn upload_content_type(filename: &str) -> String {
    mime_guess::from_path(filename)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

pub fn validate_message_payload(
    content: &str,
    attachments: &[MessageAttachmentUpload],
) -> Result<()> {
    if content.trim().is_empty() && attachments.is_empty() {
        return Err(AppError::EmptyMessageContent);
    }

    let len = content.chars().count();
    if len > 2_000 {
        return Err(AppError::MessageTooLong { len });
    }

    let sizes = attachments
        .iter()
        .map(|attachment| (attachment.filename.clone(), attachment.size_bytes))
        .collect::<Vec<_>>();
    validate_attachment_sizes(&sizes)
}

fn validate_attachment_sizes(attachments: &[(String, u64)]) -> Result<()> {
    if attachments.len() > MAX_UPLOAD_ATTACHMENT_COUNT {
        return Err(AppError::TooManyAttachments {
            count: attachments.len(),
        });
    }

    let mut total_size = 0_u64;
    for (filename, size) in attachments {
        if *size > MAX_UPLOAD_FILE_BYTES {
            return Err(AppError::AttachmentTooLarge {
                filename: filename.clone(),
                size: *size,
            });
        }
        total_size = total_size.saturating_add(*size);
    }
    if total_size > MAX_UPLOAD_TOTAL_BYTES {
        return Err(AppError::AttachmentsTooLarge { size: total_size });
    }

    Ok(())
}

pub fn validate_message_content(content: &str) -> Result<()> {
    validate_message_payload(content, &[])
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, EmojiMarker, GuildMarker},
    };

    use crate::{
        AppError,
        discord::{
            ChannelInfo, MAX_UPLOAD_FILE_BYTES, MessageAttachmentUpload, ReactionEmoji,
            rest::{
                ForumPostPage, ForumSearchSort, is_search_index_warming, merge_forum_pages,
                message_multipart_form, message_request_body, next_reaction_users_after,
                parse_forum_preview_messages, parse_forum_thread_page, parse_user_profile_response,
                poll_vote_request_body, reaction_route_component, upload_content_type,
                validate_message_content, validate_message_payload,
            },
        },
    };

    #[test]
    fn rejects_invalid_message_content() {
        let error = validate_message_content("   ").expect_err("blank messages must fail");
        assert!(matches!(error, AppError::EmptyMessageContent));

        let content = "x".repeat(2_001);
        let error = validate_message_content(&content).expect_err("oversized message must fail");
        assert!(matches!(error, AppError::MessageTooLong { len: 2_001 }));
    }

    #[test]
    fn validates_attachment_only_message_payload() {
        let attachments = vec![MessageAttachmentUpload {
            path: "/tmp/cat.png".into(),
            filename: "cat.png".to_owned(),
            size_bytes: 2_048,
            requires_cleanup: false,
        }];

        validate_message_payload("   ", &attachments).expect("file-only messages should be valid");

        let body = message_request_body("", Some(Id::new(44)), &attachments);
        assert_eq!(body["content"], "");
        assert_eq!(body["message_reference"]["message_id"], "44");
        assert_eq!(body["attachments"][0]["id"], 0);
        assert_eq!(body["attachments"][0]["filename"], "cat.png");
    }

    #[test]
    fn rejects_attachment_upload_limits() {
        let too_large_file = vec![MessageAttachmentUpload {
            path: "/tmp/large.bin".into(),
            filename: "large.bin".to_owned(),
            size_bytes: MAX_UPLOAD_FILE_BYTES + 1,
            requires_cleanup: false,
        }];
        let error = validate_message_payload("", &too_large_file)
            .expect_err("oversized attachment must fail");
        assert!(matches!(error, AppError::AttachmentTooLarge { .. }));

        let too_large_total = vec![
            MessageAttachmentUpload {
                path: "/tmp/a.bin".into(),
                filename: "a.bin".to_owned(),
                size_bytes: MAX_UPLOAD_FILE_BYTES - 1,
                requires_cleanup: false,
            },
            MessageAttachmentUpload {
                path: "/tmp/b.bin".into(),
                filename: "b.bin".to_owned(),
                size_bytes: MAX_UPLOAD_FILE_BYTES - 1,
                requires_cleanup: false,
            },
            MessageAttachmentUpload {
                path: "/tmp/c.bin".into(),
                filename: "c.bin".to_owned(),
                size_bytes: MAX_UPLOAD_FILE_BYTES - 1,
                requires_cleanup: false,
            },
        ];
        let error = validate_message_payload("", &too_large_total)
            .expect_err("oversized attachment total must fail");
        assert!(matches!(error, AppError::AttachmentsTooLarge { .. }));
    }

    #[tokio::test]
    async fn multipart_form_rechecks_current_file_size() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("concord-rest-{unique}"));
        std::fs::create_dir_all(&directory).expect("temp upload directory can be created");
        let path = directory.join("changed.bin");
        std::fs::write(&path, [0_u8]).expect("small temp file can be written");
        let attachment = MessageAttachmentUpload {
            path: path.clone(),
            filename: "changed.bin".to_owned(),
            size_bytes: 1,
            requires_cleanup: false,
        };
        std::fs::write(&path, vec![0_u8; (MAX_UPLOAD_FILE_BYTES + 1) as usize])
            .expect("oversized temp file can be written");

        let result = message_multipart_form(
            message_request_body("", None, std::slice::from_ref(&attachment)),
            &[attachment],
        )
        .await;
        let Err(error) = result else {
            panic!("multipart form must re-check actual file size");
        };

        assert!(matches!(error, AppError::AttachmentTooLarge { .. }));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn upload_content_type_uses_common_media_types() {
        assert_eq!(upload_content_type("clip.MP4"), "video/mp4");
        assert_eq!(upload_content_type("song.mp3"), "audio/mpeg");
        assert_eq!(
            upload_content_type("sheet.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            upload_content_type("unknown.concord"),
            "application/octet-stream"
        );
    }

    #[test]
    fn reaction_route_component_formats_unicode_and_custom_reactions() {
        let custom = ReactionEmoji::Custom {
            id: Id::<EmojiMarker>::new(42),
            name: Some("party".to_owned()),
            animated: true,
        };
        let cases = [
            (ReactionEmoji::Unicode("🎉".to_owned()), "%F0%9F%8E%89"),
            (custom, "party%3A42"),
        ];

        for (reaction, expected) in cases {
            assert_eq!(reaction_route_component(&reaction), expected);
        }
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
    fn forum_thread_page_filters_or_fills_parent_and_supplies_guild() {
        let guild_id = Id::<GuildMarker>::new(1);
        let forum_id = Id::<ChannelMarker>::new(20);
        let raw = serde_json::json!({
            "threads": [
                {
                    "id": "30",
                    "parent_id": "20",
                    "type": 11,
                    "name": "welcome",
                    "thread_metadata": { "archived": false, "locked": false }
                },
                {
                    "id": "31",
                    "parent_id": "21",
                    "type": 11,
                    "name": "other-forum-post"
                }
            ],
            "has_more": false
        });

        let posts = parse_forum_thread_page(&raw, Some(guild_id), forum_id, false);

        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].guild_id, Some(guild_id));
        assert_eq!(posts[0].channel_id, Id::new(30));
        assert_eq!(posts[0].parent_id, Some(forum_id));
        assert_eq!(posts[0].name, "welcome");

        let raw = serde_json::json!({
            "threads": [
                {
                    "id": "30",
                    "type": 11,
                    "name": "welcome",
                    "thread_metadata": { "archived": false, "locked": false }
                }
            ],
            "has_more": false
        });

        let posts = parse_forum_thread_page(&raw, Some(guild_id), forum_id, true);

        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].parent_id, Some(forum_id));
    }

    #[test]
    fn forum_first_messages_are_filtered_to_loaded_posts() {
        let guild_id = Id::<GuildMarker>::new(1);
        let forum_id = Id::<ChannelMarker>::new(20);
        let posts = vec![forum_post(forum_id, 30, "welcome")];
        let raw = serde_json::json!({
            "first_messages": [
                {
                    "id": "300",
                    "channel_id": "30",
                    "guild_id": "1",
                    "author": { "id": "10", "username": "neo" },
                    "type": 0,
                    "pinned": false,
                    "content": "hello from the first post",
                    "mentions": [],
                    "attachments": [],
                    "embeds": []
                },
                {
                    "id": "301",
                    "channel_id": "31",
                    "guild_id": "1",
                    "author": { "id": "11", "username": "other" },
                    "type": 0,
                    "pinned": false,
                    "content": "other forum",
                    "mentions": [],
                    "attachments": [],
                    "embeds": []
                }
            ]
        });

        let messages = parse_forum_preview_messages(&raw, &posts);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].guild_id, Some(guild_id));
        assert_eq!(messages[0].channel_id, Id::new(30));
        assert_eq!(messages[0].author, "neo");
        assert_eq!(
            messages[0].content.as_deref(),
            Some("hello from the first post")
        );
    }

    #[test]
    fn forum_preview_messages_accept_search_message_fields() {
        let guild_id = Id::<GuildMarker>::new(1);
        let forum_id = Id::<ChannelMarker>::new(20);
        let posts = vec![forum_post(forum_id, 30, "welcome")];
        let raw = serde_json::json!({
            "messages": [
                {
                    "id": "300",
                    "channel_id": "30",
                    "guild_id": "1",
                    "author": { "id": "10", "username": "neo" },
                    "type": 0,
                    "pinned": false,
                    "content": "archived search preview",
                    "mentions": [],
                    "attachments": [],
                    "embeds": []
                }
            ],
            "most_recent_messages": [
                {
                    "id": "300",
                    "channel_id": "30",
                    "guild_id": "1",
                    "author": { "id": "10", "username": "neo" },
                    "type": 0,
                    "pinned": false,
                    "content": "duplicate preview",
                    "mentions": [],
                    "attachments": [],
                    "embeds": []
                }
            ]
        });

        let messages = parse_forum_preview_messages(&raw, &posts);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].guild_id, Some(guild_id));
        assert_eq!(messages[0].channel_id, Id::new(30));
        assert_eq!(
            messages[0].content.as_deref(),
            Some("archived search preview")
        );
    }

    #[test]
    fn forum_search_sort_serializes_to_discord_query_value() {
        assert_eq!(
            ForumSearchSort::LastMessageTime.as_str(),
            "last_message_time"
        );
        assert_eq!(ForumSearchSort::CreationTime.as_str(), "creation_time");
    }

    #[test]
    fn merge_forum_pages_dedupes_posts_and_keeps_last_message_time_has_more() {
        let forum_id = Id::<ChannelMarker>::new(20);
        let active = ForumPostPage {
            next_offset: 25,
            posts: vec![
                forum_post(forum_id, 100, "active-only"),
                forum_post(forum_id, 200, "shared"),
            ],
            preview_messages: Vec::new(),
            has_more: true,
        };
        let recent = ForumPostPage {
            next_offset: 25,
            posts: vec![
                forum_post(forum_id, 200, "shared-from-creation"),
                forum_post(forum_id, 300, "creation-only"),
            ],
            preview_messages: Vec::new(),
            // `has_more` from the creation_time side should be ignored —
            // pagination beyond the first page only follows last_message_time.
            has_more: false,
        };

        let merged = merge_forum_pages(active, recent);

        let names: Vec<_> = merged.posts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["active-only", "shared", "creation-only"]);
        assert!(merged.has_more, "must follow last_message_time has_more");
        assert_eq!(merged.next_offset, 25);
    }

    #[test]
    fn search_index_warming_error_is_detected() {
        let warming = AppError::DiscordRequest("forum post search index is not ready".to_owned());
        let other = AppError::DiscordRequest("forum post search failed: 500".to_owned());

        assert!(is_search_index_warming(&warming));
        assert!(!is_search_index_warming(&other));
        assert!(!is_search_index_warming(&AppError::EmptyMessageContent));
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

    #[test]
    fn user_profile_parser_keeps_guild_member_roles() {
        let profile = parse_user_profile_response(
            Id::new(10),
            &serde_json::json!({
                "user": { "id": "10", "username": "test-user" },
                "guild_member": { "roles": ["90", "91"] }
            }),
            None,
        );

        assert_eq!(profile.role_ids, vec![Id::new(90), Id::new(91)]);
    }

    fn forum_post(parent_id: Id<ChannelMarker>, post_id: u64, name: &str) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(post_id),
            parent_id: Some(parent_id),
            position: None,
            last_message_id: None,
            name: name.to_owned(),
            kind: "public_thread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }
}
