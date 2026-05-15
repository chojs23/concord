use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{CustomEmojiInfo, GuildFolder};

use super::DiscordState;

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

    pub fn custom_emojis_for_guild(&self, guild_id: Id<GuildMarker>) -> &[CustomEmojiInfo] {
        self.navigation
            .custom_emojis
            .get(&guild_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn increment_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(count) = self
            .navigation
            .guilds
            .get_mut(&guild_id)
            .and_then(|guild| guild.member_count.as_mut())
        {
            *count = count.saturating_add(1);
        }
    }

    pub(super) fn decrement_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
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
