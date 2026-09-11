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
fn guild_partial_updates_preserve_and_replace_optional_metadata() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        onboarding: Some(onboarding(guild_id, false)),
        features: vec!["COMMUNITY".to_owned(), "FUTURE_FEATURE".to_owned()],
        ..GuildCreateFixture::new(guild_id)
    }));

    let cached = state
        .guild(guild_id)
        .and_then(|guild| guild.onboarding.as_ref())
        .expect("onboarding should be cached");
    assert_eq!(cached.enabled, Some(false));
    assert_eq!(cached.raw["future_field"], json!("kept"));
    assert!(state.guild_has_feature(guild_id, "COMMUNITY"));
    assert!(state.guild_has_feature(guild_id, "FUTURE_FEATURE"));

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
    assert!(state.guild_has_feature(guild_id, "COMMUNITY"));

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
fn guild_update_clears_icon_from_value_to_explicit_null() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        guild_id,
        icon_hash: Some("icon_hash".to_owned()),
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        icon_hash: Some(String::new()),
        ..GuildUpdateFixture::new()
    }));
    assert!(
        state
            .guild(guild_id)
            .is_some_and(|guild| guild.icon_hash.is_none())
    );
}

#[test]
fn guild_outage_preserves_cache_and_membership_removal_clears_it() {
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

    state.apply_event(&AppEvent::GuildUnavailable { guild_id });

    assert!(state.guild(guild_id).is_some());
    assert_eq!(state.custom_emojis_for_guild(guild_id).len(), 1);

    state.apply_event(&AppEvent::GuildDelete { guild_id });

    assert!(state.guild(guild_id).is_none());
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
fn guild_update_replaces_custom_emojis_only_when_the_field_is_present() {
    let guild_id = Id::new(1);
    let cached = || {
        let mut state = DiscordState::default();
        state.apply_event(&guild_create_event(GuildCreateFixture {
            guild_id,
            emojis: vec![CustomEmojiInfo::test(Id::new(50), "party")],
            ..GuildCreateFixture::new(guild_id)
        }));
        state
    };

    let mut replaced = cached();
    replaced.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "guild renamed".to_owned(),
        emojis: Some(vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(70), "dance")
        }]),
        ..GuildUpdateFixture::new()
    }));
    let emojis = replaced.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].id, Id::new(70));
    assert_eq!(emojis[0].name, "dance");

    // A rename that omits `emojis` must not be read as "the guild has none".
    let mut renamed_only = cached();
    renamed_only.apply_event(&guild_update_event(GuildUpdateFixture {
        guild_id,
        name: "guild renamed".to_owned(),
        ..GuildUpdateFixture::new()
    }));
    let emojis = renamed_only.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].name, "party");
}

#[test]
fn fresh_ready_reconciles_guild_channel_and_private_channel_snapshots() {
    let stale_guild = Id::new(1);
    let current_guild = Id::new(2);
    let stale_guild_channel = Id::new(10);
    let stale_current_channel = Id::new(20);
    let ready_channel = Id::new(21);
    let stale_dm = Id::new(30);
    let ready_dm = Id::new(31);
    let supplemental_dm = Id::new(32);
    let stale_member = Id::new(40);
    let ready_member = Id::new(41);
    let guild_channel = |guild_id, channel_id, name: &str| ChannelInfo {
        guild_id: Some(guild_id),
        name: name.to_owned(),
        ..channel_info(channel_id, "GuildText", Vec::new())
    };
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_channel(
            stale_guild,
            stale_guild_channel,
            "stale guild",
        )],
        ..GuildCreateFixture::new(stale_guild)
    }));
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_channel(
            current_guild,
            stale_current_channel,
            "stale channel",
        )],
        members: vec![member_info(stale_member, "stale member")],
        ..GuildCreateFixture::new(current_guild)
    }));
    for (channel_id, name) in [(stale_dm, "stale dm"), (ready_dm, "ready dm")] {
        state.apply_event(&AppEvent::ChannelUpsert(dm_channel(channel_id, name)));
    }
    state.apply_event(&AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(stale_guild_channel, None, 0),
            read_state_info(stale_current_channel, None, 0),
            read_state_info(stale_dm, None, 0),
        ],
    });

    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_channel(current_guild, ready_channel, "ready channel")],
        members: vec![member_info(ready_member, "ready member")],
        ..GuildCreateFixture::new(current_guild)
    }));
    state.apply_event(&AppEvent::ReadySnapshotComplete {
        snapshot: ReadySnapshotInfo {
            guild_ids: Some(vec![current_guild]),
            guild_channel_ids: BTreeMap::from([(current_guild, vec![ready_channel])]),
            private_channel_ids: Some(vec![ready_dm]),
        },
    });

    assert!(state.guild(stale_guild).is_none());
    assert!(state.channel(stale_guild_channel).is_none());
    assert!(state.channel(stale_current_channel).is_none());
    assert!(state.channel(ready_channel).is_some());
    assert_eq!(
        state
            .searchable_members_for_guild(current_guild)
            .into_iter()
            .map(|member| member.user_id)
            .collect::<Vec<_>>(),
        vec![ready_member]
    );
    assert!(
        state
            .members_for_guild(current_guild)
            .into_iter()
            .any(|member| member.user_id == stale_member),
        "stale entities may remain available to cached message rows"
    );
    assert!(
        !state
            .notifications
            .read_states
            .contains_key(&stale_guild_channel)
    );
    assert!(
        !state
            .notifications
            .read_states
            .contains_key(&stale_current_channel)
    );
    assert!(
        state.channel(stale_dm).is_some(),
        "DM reconciliation must wait for READY_SUPPLEMENTAL"
    );

    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(
        supplemental_dm,
        "supplemental dm",
    )));
    state.apply_event(&AppEvent::ReadySupplementalComplete {
        private_channel_ids: vec![supplemental_dm],
    });

    assert!(state.channel(stale_dm).is_none());
    assert!(state.channel(ready_dm).is_some());
    assert!(state.channel(supplemental_dm).is_some());
    assert!(!state.notifications.read_states.contains_key(&stale_dm));
}
