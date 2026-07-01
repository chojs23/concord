use super::*;

#[test]
fn user_profile_cache_is_scoped_by_guild() {
    let user_id = Id::new(10);
    let guild_a = Id::new(1);
    let guild_b = Id::new(2);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::UserProfileLoaded {
        guild_id: Some(guild_a),
        profile: profile_info(user_id.get(), Some("guild a nick")),
    });
    state.apply_event(&AppEvent::UserProfileLoaded {
        guild_id: Some(guild_b),
        profile: profile_info(user_id.get(), Some("guild b nick")),
    });

    assert_eq!(
        state
            .user_profile(user_id, Some(guild_a))
            .and_then(|profile| profile.guild_nick.as_deref()),
        Some("guild a nick")
    );
    assert_eq!(
        state
            .user_profile(user_id, Some(guild_b))
            .and_then(|profile| profile.guild_nick.as_deref()),
        Some("guild b nick")
    );
    assert!(state.user_profile(user_id, None).is_none());
}

#[test]
fn message_author_uses_cached_member_display_name() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        members: vec![member_info(author_id, "server alias")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(3),
        author_id,
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "server alias");
}

#[test]
fn dm_message_author_prefers_friend_nickname() {
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            author_id.get(),
            FriendStatus::Friend,
            Some("Bestie"),
            Some("Alice Global"),
            Some("alice"),
        )],
    });
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "Alice Global".to_owned(),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "Bestie");
}

#[test]
fn relationship_nickname_update_refreshes_existing_dm_message_authors() {
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::RelationshipsLoaded {
        relationships: vec![relationship_info(
            author_id.get(),
            FriendStatus::Friend,
            Some("Bestie"),
            Some("Alice Global"),
            Some("alice"),
        )],
    });
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "Alice Global".to_owned(),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&AppEvent::RelationshipUpsert {
        relationship: relationship_info(author_id.get(), FriendStatus::Friend, None, None, None),
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "Alice Global");
}

#[test]
fn user_identity_update_refreshes_existing_dm_message_author() {
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "alice".to_owned(),
        author_avatar_url: Some("https://cdn.discordapp.com/avatars/4/old.png".to_owned()),
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&user_identity_update_event(UserIdentityUpdateFixture {
        user_id: author_id,
        username: "alice".to_owned(),
        global_name: Some("Alice New".to_owned()),
        avatar_url: Some("https://cdn.discordapp.com/avatars/4/new.png".to_owned()),
        ..UserIdentityUpdateFixture::new()
    }));

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "Alice New");
    assert_eq!(
        messages[0].author_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/4/new.png"),
    );
}

#[test]
fn member_update_refreshes_existing_message_author() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(3),
        author_id,
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_info(author_id, "server alias"),
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "server alias");
}
