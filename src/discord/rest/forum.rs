use std::time::Duration;

use reqwest::StatusCode;
use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{
    AppError, Result,
    discord::{
        ChannelInfo, ForumPostArchiveState, MessageInfo,
        gateway::{parse_channel_info, parse_message_info},
    },
};

use super::{DiscordRest, clone_array, extra_fields};

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

impl DiscordRest {
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
            .authenticated(self.raw_http.get(format!(
                "https://discord.com/api/v9/channels/{}/threads/search",
                channel_id.get()
            )))
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

        let response = parse_forum_thread_search_response(&raw, Some(guild_id), channel_id, true);

        Ok(ForumPostPage {
            next_offset: offset.saturating_add(response.threads.len()),
            threads: response.threads,
            first_messages: response.first_messages,
            has_more: response.has_more,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ForumThreadSearchResponse {
    pub(super) threads: Vec<ChannelInfo>,
    pub(super) first_messages: Vec<MessageInfo>,
    pub(super) has_more: bool,
    pub(super) raw_threads: Vec<Value>,
    pub(super) raw_first_messages: Vec<Value>,
    pub(super) extra_fields: std::collections::BTreeMap<String, Value>,
}

pub(super) fn parse_forum_thread_search_response(
    raw: &Value,
    guild_id: Option<Id<GuildMarker>>,
    parent_channel_id: Id<ChannelMarker>,
    fill_missing_parent: bool,
) -> ForumThreadSearchResponse {
    let threads = parse_forum_threads(raw, guild_id, parent_channel_id, fill_missing_parent);
    let first_messages = parse_forum_first_messages(raw, &threads);
    ForumThreadSearchResponse {
        threads,
        first_messages,
        has_more: raw
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        raw_threads: clone_array(raw.get("threads")),
        raw_first_messages: clone_array(raw.get("first_messages")),
        extra_fields: extra_fields(raw, &["threads", "first_messages", "has_more"]),
    }
}

pub(super) fn parse_forum_threads(
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

pub(super) fn parse_forum_first_messages(raw: &Value, threads: &[ChannelInfo]) -> Vec<MessageInfo> {
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

pub(super) fn is_search_index_warming(error: &AppError) -> bool {
    match error {
        AppError::DiscordRequest(message) => {
            message.contains("forum post search index is not ready")
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ForumSearchSort {
    LastMessageTime,
    CreationTime,
}

impl ForumSearchSort {
    pub(super) fn as_str(self) -> &'static str {
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
pub(super) fn merge_forum_pages(active: ForumPostPage, recent: ForumPostPage) -> ForumPostPage {
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
