use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use super::DiscordRest;

impl DiscordRest {
    pub async fn set_guild_muted(
        &self,
        guild_id: Id<GuildMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/users/@me/guilds/{}/settings",
                    guild_id.get()
                ))
                .json(&mute_request_body(
                    muted,
                    mute_end_time,
                    selected_time_window,
                )),
            "set guild mute",
        )
        .await
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
        self.send_unit(
            self.raw_http.patch(endpoint).json(&json!({
                "channel_overrides": {
                    channel_id.to_string(): mute_request_body(
                        muted,
                        mute_end_time,
                        selected_time_window,
                    ),
                }
            })),
            "set channel mute",
        )
        .await
    }

    /// Set the notification level for a thread. Uses the same thread-member
    /// settings endpoint as mute. Flags: 2 = All messages, 4 = Only @mentions
    /// (Discord default), 8 = Nothing.
    pub async fn set_thread_notification_level(
        &self,
        thread_id: Id<ChannelMarker>,
        flags: u64,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/channels/{}/thread-members/@me/settings",
                    thread_id.get()
                ))
                .json(&json!({ "flags": flags })),
            "set thread notification level",
        )
        .await
    }

    /// Mute a forum post / thread. Threads are not accepted in the guild
    /// settings `channel_overrides`, so the mute lives on the current user's
    /// thread member settings instead (same body shape, different endpoint).
    pub async fn set_thread_muted(
        &self,
        thread_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/channels/{}/thread-members/@me/settings",
                    thread_id.get()
                ))
                .json(&mute_request_body(
                    muted,
                    mute_end_time,
                    selected_time_window,
                )),
            "set post mute",
        )
        .await
    }
}

pub(super) fn mute_request_body(
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
