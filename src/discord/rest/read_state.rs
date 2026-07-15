use serde_json::{Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

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

    async fn send_ack_bulk_chunk(&self, read_states: &[Value]) -> Result<()> {
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/read-states/ack-bulk")
                .json(&json!({ "read_states": read_states })),
            "ack channels",
        )
        .await
    }
}
