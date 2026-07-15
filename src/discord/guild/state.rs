use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{
    CustomEmojiInfo, GuildBoostTier, GuildFolder, GuildOnboardingInfo, GuildVerificationLevel,
    MemberOnboardingStatus, capabilities::effective_attachment_limit_bytes,
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
    /// Server-wide message verification level from the guild payload.
    pub verification_level: Option<GuildVerificationLevel>,
    /// Whether moderation permissions require two-factor authentication.
    pub mfa_level: Option<u64>,
    /// Discord guild feature names. Unknown values are preserved so feature
    /// based safety checks do not require a parser update for every new flag.
    pub features: Option<Vec<String>>,
    /// Full onboarding configuration when Discord has supplied it.
    pub onboarding: Option<GuildOnboardingInfo>,
}

impl GuildState {
    pub(crate) fn has_feature(&self, feature: &str) -> bool {
        self.features
            .as_ref()
            .is_some_and(|features| features.iter().any(|value| value == feature))
    }

    /// Community capability only means onboarding can be enabled. The
    /// onboarding object's explicit state is the authority for this check.
    pub(crate) fn onboarding_may_require_completion(&self) -> bool {
        self.onboarding
            .as_ref()
            .and_then(|onboarding| onboarding.enabled)
            == Some(true)
    }
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

    pub fn guild_has_feature(&self, guild_id: Id<GuildMarker>, feature: &str) -> bool {
        self.guild(guild_id)
            .is_some_and(|guild| guild.has_feature(feature))
    }

    pub fn current_user_onboarding_status(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Option<MemberOnboardingStatus> {
        let current_user_id = self.current_user_id()?;
        self.guild_details
            .members
            .get(&guild_id)?
            .get(&current_user_id)?
            .onboarding_status()
    }

    /// Whether the current user is actively between starting and completing
    /// onboarding in a guild where cached state says it may be required.
    pub fn current_user_is_onboarding(&self, guild_id: Id<GuildMarker>) -> Option<bool> {
        if !self.guild(guild_id)?.onboarding_may_require_completion() {
            return Some(false);
        }
        self.current_user_onboarding_status(guild_id)
            .map(|status| status == MemberOnboardingStatus::InProgress)
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
