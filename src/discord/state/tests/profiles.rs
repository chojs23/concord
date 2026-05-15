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

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: vec![MemberInfo {
            user_id: author_id,
            display_name: "server alias".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

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
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "Alice Global".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

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
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "Alice Global".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.apply_event(&AppEvent::RelationshipUpsert {
        relationship: relationship_info(author_id.get(), FriendStatus::Friend, None, None, None),
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "Alice Global");
}

#[test]
fn member_update_refreshes_existing_message_author() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(4);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(3),
        author_id,
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            user_id: author_id,
            display_name: "server alias".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });

    let messages = state.messages_for_channel(channel_id);
    assert_eq!(messages[0].author, "server alias");
}
