use chrono::{NaiveDate, TimeZone, Utc};
use reqwest::StatusCode;
use serde_json::Value;

use crate::{
    AppError, Result,
    discord::{MessageInfo, MessageSearchPage, MessageSearchQuery, gateway::parse_message_info},
};

use super::{DiscordRest, clone_array, extra_fields};

const MESSAGE_SEARCH_PAGE_LIMIT: u16 = 25;
const MESSAGE_SEARCH_MAX_OFFSET: usize = 9_975;
const DISCORD_EPOCH_MILLIS: i64 = 1_420_070_400_000;

impl DiscordRest {
    pub async fn search_messages(&self, query: MessageSearchQuery) -> Result<MessageSearchPage> {
        if query.is_empty() {
            return Ok(MessageSearchPage {
                query,
                messages: Vec::new(),
                total_results: Some(0),
                has_more: false,
            });
        }

        let endpoint = match (query.guild_id, query.channel_id) {
            (Some(guild_id), _) => format!(
                "https://discord.com/api/v9/guilds/{}/messages/search",
                guild_id.get()
            ),
            (None, Some(channel_id)) => format!(
                "https://discord.com/api/v9/channels/{}/messages/search",
                channel_id.get()
            ),
            (None, None) => {
                return Err(AppError::DiscordRequest(
                    "message search requires a server or channel".to_owned(),
                ));
            }
        };
        let params = message_search_query_params(&query);
        let response = self
            .authenticated(self.raw_http.get(endpoint))
            .query(&params)
            .send()
            .await
            .map_err(|_| AppError::DiscordRequest("message search request failed".to_owned()))?;

        let status = response.status();
        let raw: Value = response.json().await.map_err(|error| {
            AppError::DiscordRequest(format!("message search decode failed: {error}"))
        })?;
        if status == StatusCode::ACCEPTED {
            return Err(AppError::DiscordRequest(message_search_indexing_message(
                &raw,
            )));
        }
        if !status.is_success() {
            return Err(AppError::DiscordRequest(format!(
                "message search failed: HTTP {status}"
            )));
        }

        let response = parse_message_search_response(&raw)?;
        let next_offset = query
            .offset
            .saturating_add(MESSAGE_SEARCH_PAGE_LIMIT as usize);
        let has_more = response
            .total_results
            .is_some_and(|total| next_offset < total)
            && next_offset <= MESSAGE_SEARCH_MAX_OFFSET;
        Ok(MessageSearchPage {
            query,
            messages: response.messages,
            total_results: response.total_results,
            has_more,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MessageSearchResponse {
    pub(super) total_results: Option<usize>,
    pub(super) messages: Vec<MessageInfo>,
    pub(super) message_groups: Vec<Value>,
    pub(super) extra_fields: std::collections::BTreeMap<String, Value>,
}

pub(super) fn parse_message_search_response(raw: &Value) -> Result<MessageSearchResponse> {
    Ok(MessageSearchResponse {
        total_results: raw
            .get("total_results")
            .and_then(Value::as_u64)
            .map(|value| usize::try_from(value).unwrap_or(usize::MAX)),
        messages: parse_message_search_messages(raw)?,
        message_groups: clone_array(raw.get("messages")),
        extra_fields: extra_fields(raw, &["total_results", "messages"]),
    })
}

pub(super) fn message_search_query_params(
    query: &MessageSearchQuery,
) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("limit", MESSAGE_SEARCH_PAGE_LIMIT.clamp(1, 25).to_string()),
        (
            "offset",
            query.offset.min(MESSAGE_SEARCH_MAX_OFFSET).to_string(),
        ),
        ("sort_by", "timestamp".to_owned()),
        ("sort_order", "desc".to_owned()),
    ];
    if let Some(content) = query.content.as_deref().filter(|value| !value.is_empty()) {
        params.push(("content", content.chars().take(1024).collect()));
    }
    if let Some(channel_id) = query.channel_id
        && query.guild_id.is_some()
    {
        params.push(("channel_id", channel_id.to_string()));
    }
    if let Some(author_id) = query.author_id {
        params.push(("author_id", author_id.to_string()));
    }
    if let Some(user_id) = query.mentions_user_id {
        params.push(("mentions", user_id.to_string()));
    }
    for has in &query.has {
        params.push(("has", has.as_query_value().to_owned()));
    }
    for author_type in &query.author_type {
        params.push(("author_type", author_type.as_query_value().to_owned()));
    }
    if let Some(pinned) = query.pinned {
        params.push(("pinned", pinned.to_string()));
    }
    if let Some(bounds) = query
        .date
        .as_deref()
        .and_then(message_search_date_snowflake_bounds)
    {
        if let Some(min_id) = bounds.min_id {
            params.push(("min_id", min_id.to_string()));
        }
        if let Some(max_id) = bounds.max_id {
            params.push(("max_id", max_id.to_string()));
        }
    }
    params
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MessageSearchDateBounds {
    pub(super) min_id: Option<u64>,
    pub(super) max_id: Option<u64>,
}

pub(super) fn message_search_date_snowflake_bounds(value: &str) -> Option<MessageSearchDateBounds> {
    let mut min_id = None;
    let mut max_id = None;
    for token in value.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        let (operator, date) = token
            .split_once(':')
            .map(|(operator, date)| (operator.trim(), date.trim()))
            .unwrap_or(("equal", token));
        let (lower, upper) = match operator {
            "gte" => (
                Some(message_search_date_start_snowflake(date)?.saturating_sub(1)),
                None,
            ),
            "lte" => (None, Some(message_search_date_next_snowflake(date)?)),
            "equal" => (
                Some(message_search_date_start_snowflake(date)?.saturating_sub(1)),
                Some(message_search_date_next_snowflake(date)?),
            ),
            _ => return None,
        };
        if let Some(lower) = lower {
            min_id = Some(min_id.map_or(lower, |current: u64| current.max(lower)));
        }
        if let Some(upper) = upper {
            max_id = Some(max_id.map_or(upper, |current: u64| current.min(upper)));
        }
    }
    Some(MessageSearchDateBounds { min_id, max_id })
}

fn message_search_date_start_snowflake(value: &str) -> Option<u64> {
    let date = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let start = date.and_hms_opt(0, 0, 0)?;
    let start_millis = Utc.from_utc_datetime(&start).timestamp_millis();
    Some(timestamp_millis_to_snowflake(start_millis))
}

fn message_search_date_next_snowflake(value: &str) -> Option<u64> {
    let date = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let end = date.succ_opt()?.and_hms_opt(0, 0, 0)?;
    let end_millis = Utc.from_utc_datetime(&end).timestamp_millis();
    Some(timestamp_millis_to_snowflake(end_millis))
}

fn timestamp_millis_to_snowflake(timestamp_millis: i64) -> u64 {
    let discord_millis = timestamp_millis.saturating_sub(DISCORD_EPOCH_MILLIS).max(0);
    u64::try_from(discord_millis).unwrap_or_default() << 22
}

fn parse_message_search_messages(raw: &Value) -> Result<Vec<MessageInfo>> {
    let Some(groups) = raw.get("messages").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::new();
    for group in groups {
        let Some(group_messages) = group.as_array() else {
            continue;
        };
        for raw_message in group_messages {
            let message = parse_message_info(raw_message).ok_or_else(|| {
                AppError::DiscordRequest(
                    "search message response was missing required fields".to_owned(),
                )
            })?;
            messages.push(message);
        }
    }
    Ok(messages)
}

fn message_search_indexing_message(raw: &Value) -> String {
    let retry_after = raw
        .get("retry_after")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if retry_after > 0.0 {
        format!("message search index is not ready, retry after {retry_after:.1}s")
    } else {
        "message search index is not ready, try again shortly".to_owned()
    }
}
