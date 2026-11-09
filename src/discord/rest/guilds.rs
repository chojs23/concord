use serde_json::{Value, json};
use wreq::RequestBuilder;

use crate::Result;
use crate::discord::gateway::parse_member_info;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{MemberInfo, MemberModeration};

use super::DiscordRest;

impl DiscordRest {
    pub async fn leave_guild(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/users/@me/guilds/{}",
                guild_id.get()
            )),
            "leave guild",
        )
        .await
    }

    pub async fn moderate_member(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        moderation: &MemberModeration,
    ) -> Result<Option<MemberInfo>> {
        let request = self.member_moderation_request(guild_id, user_id, moderation);
        let member: Value = self.send_json(request, "moderate member").await?;
        Ok(parse_member_info(&member, Some(guild_id)))
    }

    pub(super) fn member_moderation_request(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        moderation: &MemberModeration,
    ) -> RequestBuilder {
        let body = match moderation {
            MemberModeration::SetMute(mute) => json!({ "mute": mute }),
            MemberModeration::SetDeaf(deaf) => json!({ "deaf": deaf }),
            MemberModeration::Move(channel_id) => {
                json!({ "channel_id": channel_id.get().to_string() })
            }
            MemberModeration::Disconnect => json!({ "channel_id": null }),
        };
        self.raw_http
            .patch(format!(
                "https://discord.com/api/v9/guilds/{}/members/{}",
                guild_id.get(),
                user_id.get()
            ))
            .json(&body)
    }
}
