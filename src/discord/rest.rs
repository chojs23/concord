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

pub use forum::ForumPostPage;

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
        self.authenticated(request)
            .send()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("{label} request failed: {error}")))?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("{label} failed: {error}")))?;
        Ok(())
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        label: &str,
    ) -> Result<T> {
        self.authenticated(request)
            .send()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("{label} request failed: {error}")))?
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("{label} failed: {error}")))?
            .json()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("{label} decode failed: {error}")))
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
