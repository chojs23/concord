use super::*;

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
    state.apply_event(&AppEvent::GuildUpdate {
        boost_tier: None,
        boost_count: None,
        guild_id,
        name: "guild renamed".to_owned(),
        roles: None,
        emojis: Some(vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(70), "dance")
        }]),
        owner_id: None,
    });

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
    state.apply_event(&AppEvent::GuildUpdate {
        boost_tier: None,
        boost_count: None,
        guild_id,
        name: "guild renamed".to_owned(),
        roles: None,
        emojis: None,
        owner_id: None,
    });

    let emojis = state.custom_emojis_for_guild(guild_id);
    assert_eq!(emojis.len(), 1);
    assert_eq!(emojis[0].name, "party");
}
