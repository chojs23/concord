use std::time::Duration;

use crate::discord::fingerprint::discord_rest_client;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{
    StatusCode,
    header::AUTHORIZATION,
    multipart::{Form, Part},
};
use serde_json::{Value, json};

use crate::{
    AppError, Result,
    discord::{
        ApplicationCommandChoiceInfo, ApplicationCommandInfo, ApplicationCommandInteraction,
        ApplicationCommandInteractionOption, ApplicationCommandOptionInfo, ChannelInfo,
        ForumPostArchiveState, FriendStatus, MAX_UPLOAD_ATTACHMENT_COUNT, MAX_UPLOAD_FILE_BYTES,
        MAX_UPLOAD_TOTAL_BYTES, MessageAttachmentUpload, MessageInfo, MutualGuildInfo,
        ReactionEmoji, ReactionUserInfo, UserProfileInfo,
        gateway::{parse_channel_info, parse_message_info},
    },
};

const REACTION_USERS_PAGE_LIMIT: u16 = 100;
const REACTION_USERS_MAX_PAGES: usize = 3;
const FORUM_POST_SEARCH_PAGE_LIMIT: u16 = 25;
// Discord returns 202 ACCEPTED while it warms the per-forum search index.
// Wait briefly then retry. With two attempts after the original we cover the
// common cold-start window without making the user wait on a stuck index.
const FORUM_POST_SEARCH_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(250), Duration::from_millis(500)];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostPage {
    pub threads: Vec<ChannelInfo>,
    pub first_messages: Vec<MessageInfo>,
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

    pub async fn load_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Result<Vec<ApplicationCommandInfo>> {
        let endpoint = match guild_id {
            Some(guild_id) => format!(
                "https://discord.com/api/v9/guilds/{}/application-command-index",
                guild_id.get()
            ),
            None => "https://discord.com/api/v9/users/@me/application-command-index".to_owned(),
        };
        let raw = self
            .raw_http
            .get(endpoint)
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!(
                    "application command index request failed: {error}"
                ))
            })?
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("application command index failed: {error}"))
            })?
            .json::<Value>()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!(
                    "application command index decode failed: {error}"
                ))
            })?;
        Ok(parse_application_command_index(&raw))
    }

    pub async fn run_application_command(
        &self,
        interaction: &ApplicationCommandInteraction,
        session_id: &str,
    ) -> Result<()> {
        let body = application_command_interaction_body(interaction, session_id);
        self.raw_http
            .post("https://discord.com/api/v9/interactions")
            .header(AUTHORIZATION, &self.token)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("application command request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("application command failed: {error}"))
            })?;
        Ok(())
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

    pub async fn leave_guild(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        self.raw_http
            .delete(format!(
                "https://discord.com/api/v9/users/@me/guilds/{}",
                guild_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("leave guild request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("leave guild failed: {error}")))?;
        Ok(())
    }

    /// `token: null` is the legacy anti-spam echo field. Modern clients
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

    pub async fn set_guild_muted(
        &self,
        guild_id: Id<GuildMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.raw_http
            .patch(format!(
                "https://discord.com/api/v9/users/@me/guilds/{}/settings",
                guild_id.get()
            ))
            .header(AUTHORIZATION, &self.token)
            .json(&mute_request_body(
                muted,
                mute_end_time,
                selected_time_window,
            ))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("set guild mute request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("set guild mute failed: {error}")))?;
        Ok(())
    }

    pub async fn set_channel_muted(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        let endpoint = match guild_id {
            Some(guild_id) => format!(
                "https://discord.com/api/v9/users/@me/guilds/{}/settings",
                guild_id.get()
            ),
            None => "https://discord.com/api/v9/users/@me/guilds/@me/settings".to_owned(),
        };
        self.raw_http
            .patch(endpoint)
            .header(AUTHORIZATION, &self.token)
            .json(&json!({
                "channel_overrides": {
                    channel_id.to_string(): mute_request_body(
                        muted,
                        mute_end_time,
                        selected_time_window,
                    ),
                }
            }))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("set channel mute request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                AppError::DiscordRequest(format!("set channel mute failed: {error}"))
            })?;
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
        // fall back to the active or archived endpoints. They cannot supply
        // previews and routinely 403 on user-account tokens. Instead retry
        // briefly when the search index is still warming up.
        let mut last_error = None;
        for delay in std::iter::once(Duration::ZERO).chain(FORUM_POST_SEARCH_RETRY_DELAYS) {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
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
                Ok(page) => return Ok(page),
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

        let threads = parse_forum_threads(&raw, Some(guild_id), channel_id, true);
        let first_messages = parse_forum_first_messages(&raw, &threads);

        Ok(ForumPostPage {
            next_offset: offset.saturating_add(threads.len()),
            threads,
            first_messages,
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
        let mut pages_loaded = 0usize;

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
            pages_loaded = pages_loaded.saturating_add(1);
            let next_after = next_reaction_users_after(
                parsed_page.len(),
                parsed_page.last().map(|user| user.user_id),
                pages_loaded,
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

        Ok(parse_user_profile_response(user_id, &body, None))
    }

    /// Returns the user's saved note, or `None` if Discord responds 404
    /// (no note set). Other errors propagate.
    pub(super) async fn load_user_note(&self, user_id: Id<UserMarker>) -> Result<Option<String>> {
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

fn mute_request_body(
    muted: bool,
    mute_end_time: Option<DateTime<Utc>>,
    selected_time_window: Option<i64>,
) -> Value {
    json!({
        "muted": muted,
        "mute_config": selected_time_window.map(|selected_time_window| json!({
            "end_time": mute_end_time.map(|end_time| {
                end_time.to_rfc3339_opts(SecondsFormat::Millis, true)
            }),
            "selected_time_window": selected_time_window,
        })),
    })
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
/// `/users/{id}/profile` JSON. Friend status is left as `None` here because the
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
    emoji.route_component()
}

fn parse_forum_threads(
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
                    let mut channel = parse_channel_info(thread, guild_id)?;
                    if fill_missing_parent && channel.parent_id.is_none() {
                        channel.parent_id = Some(parent_channel_id);
                    }
                    if channel.parent_id != Some(parent_channel_id) {
                        return None;
                    }
                    Some(channel)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_forum_first_messages(raw: &Value, threads: &[ChannelInfo]) -> Vec<MessageInfo> {
    let mut seen = std::collections::HashSet::new();
    parse_forum_messages_from_field(raw, threads, "first_messages")
        .into_iter()
        .filter(|message| seen.insert(message.message_id))
        .collect()
}

fn parse_forum_messages_from_field(
    raw: &Value,
    threads: &[ChannelInfo],
    field: &str,
) -> Vec<MessageInfo> {
    raw.get(field)
        .and_then(Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(parse_message_info)
                .filter(|message| {
                    threads
                        .iter()
                        .any(|thread| thread.channel_id == message.channel_id)
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
/// activity" view. `active` (last_message_time) carries threads with replies.
/// `recent` (creation_time) carries the freshly-created zero-reply ones. We
/// dedupe by `channel_id`. The order does not matter because the display layer
/// re-sorts by `last_message_id` snowflake. `has_more` only follows the
/// `last_message_time` cursor since subsequent pages use that sort alone.
fn merge_forum_pages(active: ForumPostPage, recent: ForumPostPage) -> ForumPostPage {
    let mut seen_threads = std::collections::HashSet::new();
    let mut threads = Vec::with_capacity(active.threads.len() + recent.threads.len());
    for thread in active.threads.into_iter().chain(recent.threads) {
        if seen_threads.insert(thread.channel_id) {
            threads.push(thread);
        }
    }
    let mut seen_first_messages = std::collections::HashSet::new();
    let mut first_messages =
        Vec::with_capacity(active.first_messages.len() + recent.first_messages.len());
    for message in active
        .first_messages
        .into_iter()
        .chain(recent.first_messages)
    {
        if seen_first_messages.insert(message.message_id) {
            first_messages.push(message);
        }
    }
    ForumPostPage {
        next_offset: active.next_offset,
        threads,
        first_messages,
        has_more: active.has_more,
    }
}

fn next_reaction_users_after(
    page_len: usize,
    last_user_id: Option<Id<UserMarker>>,
    pages_loaded: usize,
) -> Option<Id<UserMarker>> {
    (pages_loaded < REACTION_USERS_MAX_PAGES && page_len == usize::from(REACTION_USERS_PAGE_LIMIT))
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

fn parse_application_command_index(raw: &Value) -> Vec<ApplicationCommandInfo> {
    let applications = parse_application_command_applications(raw);
    raw.get("application_commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| parse_application_command_info(command, &applications))
        .collect()
}

fn parse_application_command_applications(
    raw: &Value,
) -> std::collections::HashMap<String, &Value> {
    raw.get("applications")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|application| Some((application.get("id")?.as_str()?.to_owned(), application)))
        .collect()
}

fn parse_application_command_info(
    raw: &Value,
    applications: &std::collections::HashMap<String, &Value>,
) -> Option<ApplicationCommandInfo> {
    let id = raw
        .get("id")?
        .as_str()?
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let application_id_raw = raw.get("application_id")?.as_str()?;
    let application_id = application_id_raw
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let name = raw.get("name")?.as_str()?.to_owned();
    Some(ApplicationCommandInfo {
        id,
        application_id,
        version: raw.get("version")?.as_str()?.to_owned(),
        name,
        application_name: parse_application_command_application_name(
            raw,
            applications.get(application_id_raw).copied(),
        ),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        options: raw
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_application_command_option_info)
            .collect(),
        raw: raw.clone(),
    })
}

fn parse_application_command_application_name(
    raw: &Value,
    application: Option<&Value>,
) -> Option<String> {
    [
        raw.get("application").and_then(|value| value.get("name")),
        application.and_then(|value| value.get("name")),
        raw.get("bot").and_then(|value| value.get("global_name")),
        raw.get("bot").and_then(|value| value.get("username")),
        application
            .and_then(|value| value.get("bot"))
            .and_then(|value| value.get("global_name")),
        application
            .and_then(|value| value.get("bot"))
            .and_then(|value| value.get("username")),
        raw.get("user").and_then(|value| value.get("global_name")),
        raw.get("user").and_then(|value| value.get("username")),
        raw.get("display_name"),
        raw.get("application_name"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|value| !value.trim().is_empty())
    .map(str::to_owned)
}

fn parse_application_command_option_info(raw: &Value) -> Option<ApplicationCommandOptionInfo> {
    Some(ApplicationCommandOptionInfo {
        kind: raw.get("type")?.as_u64()?,
        name: raw.get("name")?.as_str()?.to_owned(),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        required: raw
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        autocomplete: raw
            .get("autocomplete")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        choices: raw
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|choice| {
                Some(ApplicationCommandChoiceInfo {
                    name: choice.get("name")?.as_str()?.to_owned(),
                    value: choice.get("value")?.clone(),
                })
            })
            .collect(),
        options: raw
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_application_command_option_info)
            .collect(),
    })
}

fn application_command_interaction_body(
    interaction: &ApplicationCommandInteraction,
    session_id: &str,
) -> Value {
    let mut body = json!({
        "type": 2,
        "application_id": interaction.command.application_id.to_string(),
        "guild_id": interaction.guild_id.map(|guild_id| guild_id.to_string()),
        "channel_id": interaction.channel_id.to_string(),
        "session_id": session_id,
        "data": {
            "version": interaction.command.version,
            "id": interaction.command.id.to_string(),
            "name": interaction.command.name,
            "type": 1,
            "options": interaction.options.iter().map(application_command_option_body).collect::<Vec<_>>(),
            "application_command": interaction.command.raw,
            "attachments": [],
        },
        "nonce": interaction_nonce(),
        "analytics_location": "slash_ui",
    });
    if let Some(command_guild_id) = interaction
        .command
        .raw
        .get("guild_id")
        .and_then(Value::as_str)
    {
        body["data"]["guild_id"] = Value::String(command_guild_id.to_owned());
    }
    body
}

fn application_command_option_body(option: &ApplicationCommandInteractionOption) -> Value {
    let mut body = json!({
        "type": option.kind,
        "name": option.name,
    });
    if let Some(value) = &option.value {
        body["value"] = value.clone();
    } else if !option.options.is_empty() {
        body["options"] = Value::Array(
            option
                .options
                .iter()
                .map(application_command_option_body)
                .collect(),
        );
    }
    body
}

fn interaction_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    (millis << 22).to_string()
}

async fn message_multipart_form(
    body: Value,
    attachments: &[MessageAttachmentUpload],
) -> Result<Form> {
    let actual_sizes = attachment_sizes(attachments).await?;
    validate_attachment_sizes(&actual_sizes)?;

    let mut form = Form::new().part(
        "payload_json",
        Part::text(body.to_string())
            .mime_str("application/json")
            .map_err(|error| AppError::DiscordRequest(format!("upload payload failed: {error}")))?,
    );

    for (index, attachment) in attachments.iter().enumerate() {
        let bytes = attachment_bytes(attachment).await?;
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

async fn attachment_sizes(attachments: &[MessageAttachmentUpload]) -> Result<Vec<(String, u64)>> {
    let mut sizes = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let size = if let Some(path) = attachment.path() {
            tokio::fs::metadata(path)
                .await
                .map_err(|error| {
                    AppError::DiscordRequest(format!(
                        "stat attachment {} failed: {error}",
                        attachment.filename
                    ))
                })?
                .len()
        } else {
            attachment.size_bytes
        };
        sizes.push((attachment.filename.clone(), size));
    }
    Ok(sizes)
}

async fn attachment_bytes(attachment: &MessageAttachmentUpload) -> Result<Vec<u8>> {
    if let Some(bytes) = attachment.bytes() {
        return Ok(bytes.to_vec());
    }
    let Some(path) = attachment.path() else {
        return Err(AppError::DiscordRequest(format!(
            "attachment {} has no data",
            attachment.filename
        )));
    };
    tokio::fs::read(path).await.map_err(|error| {
        AppError::DiscordRequest(format!(
            "read attachment {} failed: {error}",
            attachment.filename
        ))
    })
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
mod tests;
