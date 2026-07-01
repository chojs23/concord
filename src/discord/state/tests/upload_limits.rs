use super::*;

const MIB: u64 = 1024 * 1024;

fn state_for(
    premium_tier: PremiumTier,
    boost_tier: GuildBoostTier,
) -> (DiscordState, Id<ChannelMarker>) {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::CurrentUserCapabilities { premium_tier });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        boost_tier,
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
fn attachment_limit_is_the_more_generous_of_nitro_and_guild_boost() {
    let cases = [
        (PremiumTier::None, GuildBoostTier::None, 10 * MIB),
        (PremiumTier::Nitro, GuildBoostTier::None, 500 * MIB),
        (PremiumTier::None, GuildBoostTier::Tier3, 100 * MIB),
        (PremiumTier::NitroBasic, GuildBoostTier::Tier3, 100 * MIB),
        (PremiumTier::NitroBasic, GuildBoostTier::None, 50 * MIB),
    ];

    for (premium_tier, boost_tier, expected) in cases {
        let (state, channel_id) = state_for(premium_tier, boost_tier);
        assert_eq!(
            state.attachment_size_limit(channel_id),
            expected,
            "premium={premium_tier:?} boost={boost_tier:?}"
        );
    }
}

#[test]
fn attachment_limit_outside_a_guild_uses_only_the_user_tier() {
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

    assert_eq!(state.attachment_size_limit(channel_id), 50 * MIB);
}

#[test]
fn attachment_limit_defaults_to_base_before_ready_reports_a_tier() {
    let state = DiscordState::default();
    assert_eq!(
        state.attachment_size_limit(Id::new(1)),
        BASE_ATTACHMENT_LIMIT_BYTES
    );
}
