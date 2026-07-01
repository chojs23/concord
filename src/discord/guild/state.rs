use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{
    CustomEmojiInfo, GuildBoostTier, GuildFolder, capabilities::effective_attachment_limit_bytes,
};

use crate::discord::state::DiscordState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildState {
    pub id: Id<GuildMarker>,
    pub name: String,
    pub member_count: Option<u64>,
    pub online_count: Option<u32>,
    /// Snowflake of the guild owner. Owners short-circuit permission checks
    /// (they always see every channel). `None` until the GUILD_CREATE /
    /// GUILD_UPDATE payload supplies it.
    pub owner_id: Option<Id<UserMarker>>,
    pub boost_tier: GuildBoostTier,
    pub boost_count: u32,
}

impl DiscordState {
    pub fn guild_folders(&self) -> &[GuildFolder] {
        &self.navigation.guild_folders
    }

    pub fn guild(&self, guild_id: Id<GuildMarker>) -> Option<&GuildState> {
        self.navigation.guilds.get(&guild_id)
    }

    pub fn guilds(&self) -> Vec<&GuildState> {
        self.navigation.guilds.values().collect()
    }

    /// Per-file upload limit for the current user posting in `channel_id`:
    /// the more generous of their Nitro tier and the channel's guild boost.
    pub fn attachment_size_limit(&self, channel_id: Id<ChannelMarker>) -> u64 {
        let user_tier = self.session.current_user_premium_tier.unwrap_or_default();
        let guild_boost = self
            .channel(channel_id)
            .and_then(|channel| channel.guild_id)
            .and_then(|guild_id| self.guild(guild_id))
            .map(|guild| guild.boost_tier);
        effective_attachment_limit_bytes(user_tier, guild_boost)
    }

    pub fn all_custom_emojis(
        &self,
    ) -> impl Iterator<Item = (&Id<GuildMarker>, &Vec<CustomEmojiInfo>)> {
        self.navigation.custom_emojis.iter()
    }
    pub fn custom_emojis(&self) -> impl Iterator<Item = &CustomEmojiInfo> {
        self.navigation.custom_emojis.values().flatten()
    }

    pub fn custom_emojis_for_guild(&self, guild_id: Id<GuildMarker>) -> &[CustomEmojiInfo] {
        self.navigation
            .custom_emojis
            .get(&guild_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(in crate::discord) fn increment_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(count) = self
            .navigation
            .guilds
            .get_mut(&guild_id)
            .and_then(|guild| guild.member_count.as_mut())
        {
            *count = count.saturating_add(1);
        }
    }

    pub(in crate::discord) fn decrement_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(count) = self
            .navigation
            .guilds
            .get_mut(&guild_id)
            .and_then(|guild| guild.member_count.as_mut())
        {
            *count = count.saturating_sub(1);
        }
    }
}
