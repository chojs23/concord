//! Discord premium (Nitro) and guild boost capabilities. Tiers are modelled as
//! enums with the gated rules (upload limits, nitro-only emoji) as methods, so
//! new capabilities have one place to live.

/// Free-tier default, and the fallback when a tier is unknown. Discord raised
/// this from 8 MiB to 10 MiB and no tier is ever below it.
pub const BASE_ATTACHMENT_LIMIT_BYTES: u64 = 10 * 1024 * 1024;

/// The current user's Nitro tier, from `premium_type`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PremiumTier {
    #[default]
    None,
    NitroClassic,
    Nitro,
    NitroBasic,
}

impl PremiumTier {
    /// Unknown values fall back to `None` so a new tier cannot accidentally
    /// unlock features we have not reasoned about.
    pub fn from_premium_type(premium_type: u64) -> Self {
        match premium_type {
            1 => Self::NitroClassic,
            2 => Self::Nitro,
            3 => Self::NitroBasic,
            _ => Self::None,
        }
    }

    pub fn has_nitro(self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn attachment_limit_bytes(self) -> u64 {
        match self {
            Self::Nitro => 500 * 1024 * 1024,
            Self::NitroClassic | Self::NitroBasic => 50 * 1024 * 1024,
            Self::None => BASE_ATTACHMENT_LIMIT_BYTES,
        }
    }
}

/// A guild's boost level, from `premium_tier`. Raises the attachment limit for
/// everyone posting in the guild, independent of their own Nitro tier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GuildBoostTier {
    #[default]
    None,
    Tier1,
    Tier2,
    Tier3,
}

impl GuildBoostTier {
    pub fn from_premium_tier(premium_tier: u64) -> Self {
        match premium_tier {
            1 => Self::Tier1,
            2 => Self::Tier2,
            3 => Self::Tier3,
            _ => Self::None,
        }
    }

    pub fn level(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Tier1 => 1,
            Self::Tier2 => 2,
            Self::Tier3 => 3,
        }
    }

    /// Only tiers 2 and 3 raise the limit. Tier 1 and unboosted keep the base.
    pub fn attachment_limit_bytes(self) -> u64 {
        match self {
            Self::Tier3 => 100 * 1024 * 1024,
            Self::Tier2 => 50 * 1024 * 1024,
            Self::None | Self::Tier1 => BASE_ATTACHMENT_LIMIT_BYTES,
        }
    }
}

/// The more generous of the user's Nitro tier and the guild's boost tier, since
/// Discord grants whichever is higher. A `None` guild (a DM) uses only the base.
pub fn effective_attachment_limit_bytes(user: PremiumTier, guild: Option<GuildBoostTier>) -> u64 {
    let guild_limit = guild.map_or(
        BASE_ATTACHMENT_LIMIT_BYTES,
        GuildBoostTier::attachment_limit_bytes,
    );
    user.attachment_limit_bytes().max(guild_limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_type_maps_to_tier_and_upload_limit() {
        let cases = [
            (0, PremiumTier::None, 10 * 1024 * 1024, false),
            (1, PremiumTier::NitroClassic, 50 * 1024 * 1024, true),
            (2, PremiumTier::Nitro, 500 * 1024 * 1024, true),
            (3, PremiumTier::NitroBasic, 50 * 1024 * 1024, true),
            (99, PremiumTier::None, 10 * 1024 * 1024, false),
        ];
        for (raw, tier, limit, has_nitro) in cases {
            let parsed = PremiumTier::from_premium_type(raw);
            assert_eq!(parsed, tier, "premium_type {raw}");
            assert_eq!(parsed.attachment_limit_bytes(), limit, "limit for {raw}");
            assert_eq!(parsed.has_nitro(), has_nitro, "has_nitro for {raw}");
        }
    }

    #[test]
    fn guild_boost_tier_maps_to_upload_limit() {
        assert_eq!(
            GuildBoostTier::from_premium_tier(0).attachment_limit_bytes(),
            10 * 1024 * 1024
        );
        assert_eq!(
            GuildBoostTier::from_premium_tier(1).attachment_limit_bytes(),
            10 * 1024 * 1024
        );
        assert_eq!(
            GuildBoostTier::from_premium_tier(2).attachment_limit_bytes(),
            50 * 1024 * 1024
        );
        assert_eq!(
            GuildBoostTier::from_premium_tier(3).attachment_limit_bytes(),
            100 * 1024 * 1024
        );
    }

    #[test]
    fn effective_limit_takes_the_more_generous_tier() {
        assert_eq!(
            effective_attachment_limit_bytes(PremiumTier::None, Some(GuildBoostTier::Tier3)),
            100 * 1024 * 1024
        );
        assert_eq!(
            effective_attachment_limit_bytes(PremiumTier::Nitro, Some(GuildBoostTier::None)),
            500 * 1024 * 1024
        );
        assert_eq!(
            effective_attachment_limit_bytes(PremiumTier::NitroBasic, None),
            50 * 1024 * 1024
        );
    }
}
