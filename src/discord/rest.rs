use std::collections::BTreeMap;

use crate::discord::fingerprint::discord_rest_client;
use crate::{AppError, Result};

use reqwest::{RequestBuilder, header::AUTHORIZATION};
use serde::de::DeserializeOwned;
use serde_json::Value;

mod application_commands;
mod connection;
mod forum;
mod guilds;
mod messages;
mod notification_settings;
mod polls;
mod presence;
mod profile;
mod reactions;
mod read_state;
mod search;
mod user_settings;

pub use forum::ForumPostPage;
pub(in crate::discord) use messages::MessageEditRequest;

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

    fn authenticated(&self, request: RequestBuilder) -> RequestBuilder {
        request.header(AUTHORIZATION, &self.token)
    }

    async fn send_unit(&self, request: RequestBuilder, label: &str) -> Result<()> {
        let response = self.authenticated(request).send().await.map_err(|error| {
            AppError::DiscordRequest(format!("{label} request failed: {error}"))
        })?;
        if let Err(error) = response.error_for_status_ref() {
            let detail = response.text().await.ok().map(truncate_error_body);
            let message = match detail.filter(|detail| !detail.trim().is_empty()) {
                Some(detail) => format!("{label} failed: {error}: {detail}"),
                None => format!("{label} failed: {error}"),
            };
            return Err(AppError::DiscordRequest(message));
        }
        Ok(())
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        label: &str,
    ) -> Result<T> {
        let response = self.authenticated(request).send().await.map_err(|error| {
            AppError::DiscordRequest(format!("{label} request failed: {error}"))
        })?;
        if let Err(error) = response.error_for_status_ref() {
            let detail = response.text().await.ok().map(truncate_error_body);
            let message = match detail.filter(|detail| !detail.trim().is_empty()) {
                Some(detail) => format!("{label} failed: {error}: {detail}"),
                None => format!("{label} failed: {error}"),
            };
            return Err(AppError::DiscordRequest(message));
        }
        response
            .json()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("{label} decode failed: {error}")))
    }
}

fn truncate_error_body(body: String) -> String {
    const MAX_ERROR_BODY_CHARS: usize = 500;
    let mut chars = body.chars();
    let truncated: String = chars.by_ref().take(MAX_ERROR_BODY_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn extra_fields(value: &Value, known_fields: &[&str]) -> BTreeMap<String, Value> {
    let Some(object) = value.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter(|(field, _)| !known_fields.contains(&field.as_str()))
        .map(|(field, value)| (field.clone(), value.clone()))
        .collect()
}

fn clone_array(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|values| values.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
