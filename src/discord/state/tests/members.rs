use super::*;

#[test]
fn tracks_members_and_presences() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(100),
        channels: Vec::new(),
        members: vec![
            MemberInfo {
                user_id: alice,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        ],
        presences: vec![(alice, PresenceStatus::Online)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    let members = state.members_for_guild(guild_id);
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(100));
    assert_eq!(members.len(), 2);
    let alice_state = members.iter().find(|m| m.user_id == alice).unwrap();
    assert_eq!(alice_state.status, PresenceStatus::Online);
    let bob_state = members.iter().find(|m| m.user_id == bob).unwrap();
    assert_eq!(bob_state.status, PresenceStatus::Unknown);

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id: bob,
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });
    assert_eq!(
        state
            .members_for_guild(guild_id)
            .iter()
            .find(|m| m.user_id == bob)
            .unwrap()
            .status,
        PresenceStatus::Idle,
    );
}

#[test]
fn tracks_voice_participants_join_move_and_leave() {
    let guild_id = Id::new(1);
    let first_voice = Id::new(10);
    let second_voice = Id::new(11);
    let alice = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Alice".to_owned(),
        user_id: Some(alice),
    });

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: vec![
            ChannelInfo {
                kind: "GuildVoice".to_owned(),
                channel_id: first_voice,
                guild_id: Some(guild_id),
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Lobby".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                kind: "GuildVoice".to_owned(),
                channel_id: second_voice,
                guild_id: Some(guild_id),
                parent_id: None,
                position: Some(1),
                last_message_id: None,
                name: "Raid".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    let alice_member = MemberInfo {
        user_id: alice,
        display_name: "Alice".to_owned(),
        username: Some("alice".to_owned()),
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
    };
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id,
            channel_id: Some(first_voice),
            user_id: alice,
            session_id: None,
            member: Some(alice_member),
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: true,
            self_stream: true,
        },
    });
    let first_voice_participants = state.voice_participants_for_channel(guild_id, first_voice);
    assert_eq!(first_voice_participants[0].display_name, "Alice");
    assert!(first_voice_participants[0].self_stream);
    assert!(!first_voice_participants[0].speaking);
    assert_eq!(
        state.current_user_voice_connection(),
        Some(CurrentVoiceConnectionState {
            guild_id,
            channel_id: first_voice,
            self_mute: true,
            self_deaf: false,
            allow_microphone_transmit: false,
        })
    );

    state.apply_event(&AppEvent::VoiceSpeakingUpdate {
        guild_id,
        channel_id: first_voice,
        user_id: alice,
        speaking: true,
    });
    assert!(state.voice_participants_for_channel(guild_id, first_voice)[0].speaking);
    assert!(state.current_user_voice_speaking());
    assert!(state.user_voice_speaking_in_guild(guild_id, alice));
    assert!(!state.user_voice_speaking_in_guild(Id::new(999), alice));

    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id,
            channel_id: Some(second_voice),
            user_id: alice,
            session_id: None,
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        },
    });
    assert!(
        state
            .voice_participants_for_channel(guild_id, first_voice)
            .is_empty()
    );
    assert_eq!(
        state.voice_participants_for_channel(guild_id, second_voice)[0].user_id,
        alice
    );
    assert!(!state.voice_participants_for_channel(guild_id, second_voice)[0].speaking);
    assert!(!state.current_user_voice_speaking());
    assert_eq!(
        state.current_user_voice_connection(),
        Some(CurrentVoiceConnectionState {
            guild_id,
            channel_id: second_voice,
            self_mute: false,
            self_deaf: false,
            allow_microphone_transmit: false,
        })
    );

    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id,
            channel_id: None,
            user_id: alice,
            session_id: None,
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        },
    });
    assert!(
        state
            .voice_participants_for_channel(guild_id, second_voice)
            .is_empty()
    );
    assert_eq!(state.current_user_voice_connection(), None);
}

#[test]
fn guild_create_replaces_cached_voice_state_snapshot() {
    let guild_id = Id::new(1);
    let voice = Id::new(10);
    let alice = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: vec![ChannelInfo {
            kind: "GuildVoice".to_owned(),
            channel_id: voice,
            guild_id: Some(guild_id),
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "Lobby".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id,
            channel_id: Some(voice),
            user_id: alice,
            session_id: None,
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        },
    });
    assert_eq!(
        state.voice_participants_for_channel(guild_id, voice)[0].user_id,
        alice
    );

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: vec![ChannelInfo {
            kind: "GuildVoice".to_owned(),
            channel_id: voice,
            guild_id: Some(guild_id),
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "Lobby".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    assert!(
        state
            .voice_participants_for_channel(guild_id, voice)
            .is_empty()
    );
}

#[test]
fn presence_update_does_not_create_fallback_member() {
    let guild_id = Id::new(1);
    let user_id = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(100),
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id,
        status: PresenceStatus::Idle,
        activities: Vec::new(),
    });

    assert!(state.members_for_guild(guild_id).is_empty());
    assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
}

#[test]
fn real_member_add_and_remove_update_known_member_count() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id: alice,
            display_name: "alice".to_owned(),
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

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            user_id: bob,
            display_name: "bob".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&AppEvent::GuildMemberAdd {
        guild_id,
        member: MemberInfo {
            user_id: bob,
            display_name: "bob".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&AppEvent::GuildMemberAdd {
        guild_id,
        member: MemberInfo {
            user_id: Id::new(30),
            display_name: "carol".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));

    state.apply_event(&AppEvent::GuildMemberRemove {
        guild_id,
        user_id: Id::new(30),
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));
}

#[test]
fn guild_member_remove_decrements_known_count_for_unloaded_member() {
    let guild_id = Id::new(1);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(3),
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    state.apply_event(&AppEvent::GuildMemberRemove {
        guild_id,
        user_id: Id::new(99),
    });

    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));
    assert!(state.members_for_guild(guild_id).is_empty());
}

#[test]
fn guild_create_caches_roles_and_member_role_ids() {
    let guild_id = Id::new(1);
    let role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id,
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: vec![role_id],
        }],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: role_id,
            name: "Admin".to_owned(),
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });

    let roles = state.roles_for_guild(guild_id);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name, "Admin");
    let members = state.members_for_guild(guild_id);
    assert_eq!(members[0].role_ids, vec![role_id]);
}

#[test]
fn message_author_role_color_uses_history_author_roles_when_member_is_missing() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: role_id,
            name: "Red".to_owned(),
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    message.author_role_ids = vec![role_id];
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![message],
    });

    assert_eq!(
        state.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000)
    );
}

#[test]
fn message_author_role_color_uses_live_author_roles_when_member_is_missing() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: role_id,
            name: "Red".to_owned(),
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id,
        author_id: user_id,
        author: "test-user".to_owned(),
        author_avatar_url: None,
        author_role_ids: vec![role_id],
        message_kind: MessageKind::regular(),
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

    assert_eq!(
        state.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000)
    );
}

#[test]
fn message_author_role_color_uses_profile_roles_when_message_roles_are_missing() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: role_id,
            name: "Red".to_owned(),
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![message],
    });
    let mut profile = profile_info(user_id.get(), Some("test-user"));
    profile.role_ids = vec![role_id];
    state.apply_event(&AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile,
    });

    assert_eq!(
        state.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000)
    );
}

#[test]
fn message_author_role_color_does_not_use_message_roles_when_member_is_cached() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let stale_role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![MemberInfo {
            user_id,
            display_name: "test-user".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: stale_role_id,
            name: "Old Red".to_owned(),
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            permissions: 0,
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    message.author_role_ids = vec![stale_role_id];
    state.apply_event(&AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages: vec![message],
    });

    assert_eq!(
        state.message_author_role_color(guild_id, channel_id, message_id, user_id),
        None
    );
}

#[test]
fn chunk_style_member_upserts_populate_member_list() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DiscordState::default();

    for (user_id, display_name) in [(alice, "alice"), (bob, "bob")] {
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id,
                display_name: display_name.to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
    }
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id: alice,
        status: PresenceStatus::Online,
        activities: Vec::new(),
    });

    let members = state.members_for_guild(guild_id);
    assert_eq!(members.len(), 2);
    assert_eq!(
        members
            .iter()
            .find(|member| member.user_id == alice)
            .map(|member| member.status),
        Some(PresenceStatus::Online)
    );
    assert_eq!(
        members
            .iter()
            .find(|member| member.user_id == bob)
            .map(|member| member.status),
        Some(PresenceStatus::Unknown)
    );
}
#[test]
fn member_upsert_preserves_existing_status() {
    let guild_id = Id::new(1);
    let user = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            user_id: user,
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id: user,
        status: PresenceStatus::Online,
        activities: Vec::new(),
    });
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            user_id: user,
            display_name: "alice-renamed".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        },
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|m| m.user_id == user)
        .unwrap();
    assert_eq!(member.display_name, "alice-renamed");
    assert_eq!(member.status, PresenceStatus::Online);
}
