use super::*;
use crate::discord::{GuildOnboardingInfo, GuildOnboardingMode};
use serde_json::json;
use std::sync::Arc;

fn onboarding(guild_id: Id<GuildMarker>, enabled: bool) -> GuildOnboardingInfo {
    let raw = json!({
        "guild_id": guild_id.to_string(),
        "enabled": enabled,
        "mode": 0,
        "default_channel_ids": [],
        "prompts": [],
        "future_field": "kept"
    });
    GuildOnboardingInfo {
        guild_id,
        enabled: Some(enabled),
        mode: Some(GuildOnboardingMode::Default),
        default_channel_ids: Vec::new(),
        raw: Arc::new(raw),
    }
}

#[test]
fn guild_onboarding_is_cached_and_updated_without_losing_raw_fields() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        onboarding: Some(onboarding(guild_id, false)),
        ..GuildCreateFixture::new(guild_id)
    }));

    let cached = state
        .guild(guild_id)
        .and_then(|guild| guild.onboarding.as_ref())
        .expect("onboarding should be cached");
    assert_eq!(cached.enabled, Some(false));
    assert_eq!(cached.raw["future_field"], json!("kept"));

    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "renamed".to_owned(),
        ..GuildUpdateFixture::new()
    }));
    assert_eq!(
        state
            .guild(guild_id)
            .and_then(|guild| guild.onboarding.as_ref())
            .and_then(|onboarding| onboarding.enabled),
        Some(false)
    );

    state.apply_event(&AppEvent::GuildOnboardingUpdate {
        guild_id,
        onboarding: onboarding(guild_id, true),
    });
    assert_eq!(
        state
            .guild(guild_id)
            .and_then(|guild| guild.onboarding.as_ref())
            .and_then(|onboarding| onboarding.enabled),
        Some(true)
    );
}

#[test]
fn guild_features_are_cached_and_only_replaced_when_supplied() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        features: vec!["COMMUNITY".to_owned(), "FUTURE_FEATURE".to_owned()],
        ..GuildCreateFixture::new(guild_id)
    }));

    assert!(state.guild_has_feature(guild_id, "COMMUNITY"));
    assert!(state.guild_has_feature(guild_id, "FUTURE_FEATURE"));

    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "renamed".to_owned(),
        ..GuildUpdateFixture::new()
    }));
    assert!(state.guild_has_feature(guild_id, "COMMUNITY"));

    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "renamed again".to_owned(),
        features: Some(vec!["MEMBER_VERIFICATION_GATE_ENABLED".to_owned()]),
        ..GuildUpdateFixture::new()
    }));
    assert!(!state.guild_has_feature(guild_id, "COMMUNITY"));
    assert!(state.guild_has_feature(guild_id, "MEMBER_VERIFICATION_GATE_ENABLED"));
}

#[test]
fn stores_and_clears_custom_guild_emojis() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        emojis: vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(50), "party")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    assert_eq!(state.custom_emojis_for_guild(guild_id).len(), 1);
    assert_eq!(state.custom_emojis_for_guild(guild_id)[0].name, "party");

    state.apply_event(&AppEvent::GuildDelete { guild_id });

    assert!(state.custom_emojis_for_guild(guild_id).is_empty());
}

#[test]
fn guild_emojis_update_replaces_cached_custom_emojis() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        emojis: vec![CustomEmojiInfo::test(Id::new(50), "party")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::GuildEmojisUpdate {
        guild_id,
        emojis: vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(60), "wave")
        }],
    });

    let emojis = state.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(60));
    assert_eq!(emojis[0].name, "wave");
    assert!(emojis[0].animated);
}

#[test]
fn guild_update_replaces_custom_emojis_when_field_is_present() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        emojis: vec![CustomEmojiInfo::test(Id::new(50), "party")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "guild renamed".to_owned(),
        emojis: Some(vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(70), "dance")
        }]),
        ..GuildUpdateFixture::new()
    }));

    let emojis = state.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(70));
    assert_eq!(emojis[0].name, "dance");
}

#[test]
fn guild_update_without_emoji_field_keeps_cached_custom_emojis() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        emojis: vec![CustomEmojiInfo::test(Id::new(50), "party")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "guild renamed".to_owned(),
        ..GuildUpdateFixture::new()
    }));

    let emojis = state.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].name, "party");
}
