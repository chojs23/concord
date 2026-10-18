use crate::Result;
use crate::discord::ids::{Id, marker::ChannelMarker};

use super::DiscordRest;

impl DiscordRest {
    pub async fn leave_group_dm(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}",
                channel_id.get()
            )),
            "leave group DM",
        )
        .await
    }

    pub async fn close_dm(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}",
                channel_id.get()
            )),
            "close DM",
        )
        .await
    }
}
