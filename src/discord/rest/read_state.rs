use serde_json::{Map, Value, json};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::{AppError, Result};

use super::DiscordRest;

/// `read-states/ack-bulk` accepts at most 100 read states per request.
const ACK_BULK_MAX_TARGETS: usize = 100;

impl DiscordRest {
    /// `token: null` is the legacy anti-spam echo field. Modern clients
    /// always send null.
    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        flags: Option<u64>,
        last_viewed: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/channels/{}/messages/{}/ack",
                    channel_id.get(),
                    message_id.get()
                ))
                .json(&ack_channel_request_body(flags, last_viewed)),
            "ack channel",
        )
        .await
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        let mut failures = Vec::new();
        for (index, body) in ack_bulk_request_bodies(targets).into_iter().enumerate() {
            if let Err(error) = self
                .send_unit(
                    self.raw_http
                        .post("https://discord.com/api/v9/read-states/ack-bulk")
                        .json(&body),
                    "ack channels",
                )
                .await
            {
                failures.push(format!("chunk {}: {}", index + 1, error.log_detail()));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::DiscordRequest(format!(
                "{} read-state acknowledgement chunk(s) failed: {}",
                failures.len(),
                failures.join(", ")
            )))
        }
    }
}

fn ack_channel_request_body(flags: Option<u64>, last_viewed: u64) -> Value {
    let mut body = Map::new();
    body.insert("token".to_owned(), Value::Null);
    body.insert("last_viewed".to_owned(), Value::from(last_viewed));
    if let Some(flags) = flags {
        body.insert("flags".to_owned(), Value::from(flags));
    }
    Value::Object(body)
}

fn ack_bulk_request_bodies(targets: &[(Id<ChannelMarker>, Id<MessageMarker>)]) -> Vec<Value> {
    targets
        .chunks(ACK_BULK_MAX_TARGETS)
        .map(|chunk| {
            let read_states = chunk
                .iter()
                .map(|(channel_id, message_id)| {
                    json!({
                        "read_state_type": 0,
                        "channel_id": channel_id.to_string(),
                        "message_id": message_id.to_string(),
                    })
                })
                .collect::<Vec<_>>();
            json!({ "read_states": read_states })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ACK_BULK_MAX_TARGETS, ack_bulk_request_bodies, ack_channel_request_body};
    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, MessageMarker},
    };
    use serde_json::json;

    #[test]
    fn single_ack_payload_only_includes_known_flags() {
        assert_eq!(
            ack_channel_request_body(Some(3), 4_223),
            json!({
                "token": null,
                "flags": 3,
                "last_viewed": 4_223,
            })
        );
        assert_eq!(
            ack_channel_request_body(None, 4_223),
            json!({
                "token": null,
                "last_viewed": 4_223,
            })
        );
    }

    #[test]
    fn bulk_ack_payloads_chunk_all_targets() {
        let targets = (1..=ACK_BULK_MAX_TARGETS * 2 + 5)
            .map(|value| {
                (
                    Id::<ChannelMarker>::new(value as u64),
                    Id::<MessageMarker>::new((value + 1_000) as u64),
                )
            })
            .collect::<Vec<_>>();

        let bodies = ack_bulk_request_bodies(&targets);

        assert_eq!(bodies.len(), 3);
        assert_eq!(
            bodies[0]["read_states"]
                .as_array()
                .expect("read states should be an array")
                .len(),
            ACK_BULK_MAX_TARGETS
        );
        assert_eq!(
            bodies[2]["read_states"]
                .as_array()
                .expect("read states should be an array")
                .len(),
            5
        );
        assert_eq!(bodies[0]["read_states"][0]["read_state_type"], 0);
        assert_eq!(bodies[0]["read_states"][0]["channel_id"], "1");
        assert_eq!(bodies[0]["read_states"][0]["message_id"], "1001");
    }
}
