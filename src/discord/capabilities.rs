//! Discord account and guild capabilities.
//!
//! Nitro and server boosts are entitlements, not channel permissions. They
//! change limits after an action is allowed, while the permission policy
//! decides whether the action may happen at all. Keeping those concepts apart
//! lets every message surface share the same limits without weakening the
//! channel permission boundary.

pub(crate) const BASE_MESSAGE_CHARACTER_LIMIT: usize = 2_000;
pub(crate) const NITRO_MESSAGE_CHARACTER_LIMIT: usize = 4_000;

/// Free-tier default, and the fallback when a tier is unknown. Discord raised
/// this from 8 MiB to 10 MiB and no tier is ever below it.
pub const BASE_ATTACHMENT_LIMIT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MessageSendLimits {
    pub(crate) max_content_chars: usize,
    pub(crate) max_attachment_bytes: u64,
}

impl Default for MessageSendLimits {
    fn default() -> Self {
        Self {
            max_content_chars: BASE_MESSAGE_CHARACTER_LIMIT,
            max_attachment_bytes: BASE_ATTACHMENT_LIMIT_BYTES,
        }
    }
}

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

    pub(crate) fn message_character_limit(self) -> usize {
        match self {
            Self::Nitro => NITRO_MESSAGE_CHARACTER_LIMIT,
            Self::None | Self::NitroClassic | Self::NitroBasic => BASE_MESSAGE_CHARACTER_LIMIT,
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

/// Some servers receive upload-limit experiments independently of their boost
/// tier. Discord exposes these as guild feature names, so resolve them in one
/// place and keep unknown features harmless.
pub(crate) fn guild_attachment_limit_bytes(
    boost_tier: GuildBoostTier,
    features: Option<&[String]>,
) -> u64 {
    let feature_limit = features
        .into_iter()
        .flatten()
        .filter_map(|feature| match feature.as_str() {
            "MAX_FILE_SIZE_50_MB" => Some(50 * 1024 * 1024),
            "MAX_FILE_SIZE_100_MB" => Some(100 * 1024 * 1024),
            "MAX_FILE_SIZE_250_MB" => Some(250 * 1024 * 1024),
            _ => None,
        })
        .max()
        .unwrap_or(BASE_ATTACHMENT_LIMIT_BYTES);
    boost_tier.attachment_limit_bytes().max(feature_limit)
}

pub(crate) fn effective_message_send_limits(
    user: PremiumTier,
    guild_attachment_limit: Option<u64>,
) -> MessageSendLimits {
    MessageSendLimits {
        max_content_chars: user.message_character_limit(),
        max_attachment_bytes: user
            .attachment_limit_bytes()
            .max(guild_attachment_limit.unwrap_or(BASE_ATTACHMENT_LIMIT_BYTES)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_type_maps_to_account_capabilities() {
        let cases = [
            (
                0,
                PremiumTier::None,
                10 * 1024 * 1024,
                BASE_MESSAGE_CHARACTER_LIMIT,
                false,
            ),
            (
                1,
                PremiumTier::NitroClassic,
                50 * 1024 * 1024,
                BASE_MESSAGE_CHARACTER_LIMIT,
                true,
            ),
            (
                2,
                PremiumTier::Nitro,
                500 * 1024 * 1024,
                NITRO_MESSAGE_CHARACTER_LIMIT,
                true,
            ),
            (
                3,
                PremiumTier::NitroBasic,
                50 * 1024 * 1024,
                BASE_MESSAGE_CHARACTER_LIMIT,
                true,
            ),
            (
                99,
                PremiumTier::None,
                10 * 1024 * 1024,
                BASE_MESSAGE_CHARACTER_LIMIT,
                false,
            ),
        ];
        for (raw, tier, upload_limit, message_limit, has_nitro) in cases {
            let parsed = PremiumTier::from_premium_type(raw);
            assert_eq!(parsed, tier, "premium_type {raw}");
            assert_eq!(
                parsed.attachment_limit_bytes(),
                upload_limit,
                "upload limit for {raw}"
            );
            assert_eq!(
                parsed.message_character_limit(),
                message_limit,
                "message limit for {raw}"
            );
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
}
