use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde_json::{Value, json};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::{AppError, Result};

use super::{DiscordRest, truncate_error_body};

/// `read-states/ack-bulk` accepts at most 100 read states per request.
const ACK_BULK_MAX_TARGETS: usize = 100;
const ACK_BULK_MAX_RETRIES: u32 = 5;
const ACK_BULK_DEFAULT_RETRY_SECS: f64 = 1.0;

impl DiscordRest {
    /// `token: null` is the legacy anti-spam echo field. Modern clients
    /// always send null.
    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/channels/{}/messages/{}/ack",
                    channel_id.get(),
                    message_id.get()
                ))
                .json(&json!({ "token": Value::Null })),
            "ack channel",
        )
        .await
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        if targets.is_empty() {
            return Ok(());
        }

        for chunk in targets.chunks(ACK_BULK_MAX_TARGETS) {
            let read_states: Vec<_> = chunk
                .iter()
                .map(|(channel_id, message_id)| {
                    json!({
                        "read_state_type": 0,
                        "channel_id": channel_id.get().to_string(),
                        "message_id": message_id.get().to_string(),
                    })
                })
                .collect();

            self.send_ack_bulk_chunk(&read_states).await?;
        }
        Ok(())
    }

    /// Send one ack-bulk chunk, retrying on HTTP 429 after `retry_after`.
    async fn send_ack_bulk_chunk(&self, read_states: &[Value]) -> Result<()> {
        for attempt in 0..=ACK_BULK_MAX_RETRIES {
            let request = self
                .raw_http
                .post("https://discord.com/api/v9/read-states/ack-bulk")
                .json(&json!({ "read_states": read_states }));
            let response = self.authenticated(request).send().await.map_err(|error| {
                AppError::DiscordRequest(format!("ack channels request failed: {error}"))
            })?;

            if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt < ACK_BULK_MAX_RETRIES
            {
                let wait = ack_bulk_retry_after(response).await;
                tokio::time::sleep(Duration::from_secs_f64(wait)).await;
                continue;
            }

            if let Err(error) = response.error_for_status_ref() {
                let detail = response.text().await.ok().map(truncate_error_body);
                let message = match detail.filter(|detail| !detail.trim().is_empty()) {
                    Some(detail) => format!("ack channels failed: {error}: {detail}"),
                    None => format!("ack channels failed: {error}"),
                };
                return Err(AppError::DiscordRequest(message));
            }
            return Ok(());
        }
        unreachable!("ack-bulk loop returns within the retry budget");
    }
}

async fn ack_bulk_retry_after(response: reqwest::Response) -> f64 {
    let header_secs = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<f64>().ok());
    if let Some(secs) = header_secs {
        return secs.max(0.0);
    }
    response
        .json::<Value>()
        .await
        .ok()
        .and_then(|body| body.get("retry_after").and_then(Value::as_f64))
        .map(|secs| secs.max(0.0))
        .unwrap_or(ACK_BULK_DEFAULT_RETRY_SECS)
}
