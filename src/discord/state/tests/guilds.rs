use super::*;

#[test]
fn stores_and_clears_custom_guild_emojis() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![CustomEmojiInfo {
            id: Id::new(50),
            name: "party".to_owned(),
            animated: true,
            available: true,
        }],
        owner_id: None,
    });

    assert_eq!(state.custom_emojis_for_guild(guild_id).len(), 1);
    assert_eq!(state.custom_emojis_for_guild(guild_id)[0].name, "party");

    state.apply_event(&AppEvent::GuildDelete { guild_id });

    assert!(state.custom_emojis_for_guild(guild_id).is_empty());
}

#[test]
fn guild_emojis_update_replaces_cached_custom_emojis() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![CustomEmojiInfo {
            id: Id::new(50),
            name: "party".to_owned(),
            animated: false,
            available: true,
        }],
        owner_id: None,
    });
    state.apply_event(&AppEvent::GuildEmojisUpdate {
        guild_id,
        emojis: vec![CustomEmojiInfo {
            id: Id::new(60),
            name: "wave".to_owned(),
            animated: true,
            available: true,
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

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![CustomEmojiInfo {
            id: Id::new(50),
            name: "party".to_owned(),
            animated: false,
            available: true,
        }],
        owner_id: None,
    });
    state.apply_event(&AppEvent::GuildUpdate {
        guild_id,
        name: "guild renamed".to_owned(),
        roles: None,
        emojis: Some(vec![CustomEmojiInfo {
            id: Id::new(70),
            name: "dance".to_owned(),
            animated: true,
            available: true,
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

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![CustomEmojiInfo {
            id: Id::new(50),
            name: "party".to_owned(),
            animated: false,
            available: true,
        }],
        owner_id: None,
    });
    state.apply_event(&AppEvent::GuildUpdate {
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
