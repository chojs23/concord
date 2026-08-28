use super::*;
use crate::discord::{BASE_MESSAGE_CHARACTER_LIMIT, NITRO_MESSAGE_CHARACTER_LIMIT};

const MIB: u64 = 1024 * 1024;

fn state_for(
    premium_tier: PremiumTier,
    boost_tier: GuildBoostTier,
    features: &[&str],
) -> (DiscordState, Id<ChannelMarker>) {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::CurrentUserCapabilities { premium_tier });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        boost_tier,
        features: features
            .iter()
            .map(|feature| (*feature).to_owned())
            .collect(),
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    (state, channel_id)
}

#[test]
fn message_limits_follow_account_and_guild_capabilities() {
    let cases = [
        (
            PremiumTier::None,
            GuildBoostTier::None,
            &[] as &[&str],
            BASE_MESSAGE_CHARACTER_LIMIT,
            10 * MIB,
        ),
        (
            PremiumTier::Nitro,
            GuildBoostTier::None,
            &[],
            NITRO_MESSAGE_CHARACTER_LIMIT,
            500 * MIB,
        ),
        (
            PremiumTier::None,
            GuildBoostTier::Tier3,
            &[],
            BASE_MESSAGE_CHARACTER_LIMIT,
            100 * MIB,
        ),
        (
            PremiumTier::NitroBasic,
            GuildBoostTier::Tier3,
            &[],
            BASE_MESSAGE_CHARACTER_LIMIT,
            100 * MIB,
        ),
        (
            PremiumTier::NitroBasic,
            GuildBoostTier::None,
            &[],
            BASE_MESSAGE_CHARACTER_LIMIT,
            50 * MIB,
        ),
        (
            PremiumTier::None,
            GuildBoostTier::Tier1,
            &["MAX_FILE_SIZE_250_MB"],
            BASE_MESSAGE_CHARACTER_LIMIT,
            250 * MIB,
        ),
    ];

    for (premium_tier, boost_tier, features, message_limit, upload_limit) in cases {
        let (state, channel_id) = state_for(premium_tier, boost_tier, features);
        let limits = state.message_send_limits(channel_id);
        assert_eq!(
            (limits.max_content_chars, limits.max_attachment_bytes),
            (message_limit, upload_limit),
            "premium={premium_tier:?} boost={boost_tier:?} features={features:?}"
        );
        assert_eq!(
            state.attachment_size_limit(channel_id),
            upload_limit,
            "public upload limit for premium={premium_tier:?} boost={boost_tier:?} features={features:?}"
        );
    }

    // Direct messages have no guild benefit, so only the account tier applies.
    let channel_id = Id::new(9);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::NitroBasic,
    });
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        name: "dm".to_owned(),
        ..channel_info(channel_id, "dm", Vec::new())
    }));

    let limits = state.message_send_limits(channel_id);
    assert_eq!(
        (limits.max_content_chars, limits.max_attachment_bytes),
        (BASE_MESSAGE_CHARACTER_LIMIT, 50 * MIB)
    );

    // Missing account and channel data must keep the conservative base limits.
    let state = DiscordState::default();
    let limits = state.message_send_limits(Id::new(1));
    assert_eq!(
        (limits.max_content_chars, limits.max_attachment_bytes),
        (BASE_MESSAGE_CHARACTER_LIMIT, BASE_ATTACHMENT_LIMIT_BYTES)
    );
}
