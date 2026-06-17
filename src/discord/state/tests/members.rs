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
        members: vec![member_info(alice, "alice"), member_info(bob, "bob")],
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

    state.apply_event(&AppEvent::UserPresenceUpdate {
        user_id: bob,
        status: PresenceStatus::DoNotDisturb,
        activities: Vec::new(),
    });
    assert_eq!(
        state.user_presence_for_guild(Some(guild_id), bob),
        Some(PresenceStatus::DoNotDisturb)
    );
    assert_eq!(
        state
            .members_for_guild(guild_id)
            .into_iter()
            .find(|member| member.user_id == bob)
            .map(|member| member.status),
        Some(PresenceStatus::DoNotDisturb)
    );
}

#[test]
fn user_identity_update_preserves_guild_member_avatar() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: Vec::new(),
        members: vec![MemberInfo {
            avatar_url: Some(
                "https://cdn.discordapp.com/guilds/1/users/10/avatars/guild.png".to_owned(),
            ),
            ..member_info(user_id, "neo")
        }],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    state.apply_event(&AppEvent::UserIdentityUpdate {
        user_id,
        username: "neo".to_owned(),
        global_name: Some("Neo".to_owned()),
        avatar_url: Some("https://cdn.discordapp.com/avatars/10/global.png".to_owned()),
        is_bot: false,
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("member should remain cached");
    assert_eq!(
        member.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/guilds/1/users/10/avatars/guild.png")
    );
}

#[test]
fn tracks_voice_participants_join_move_and_leave() {
    let guild_id = Id::new(1);
    let first_voice = Id::new(10);
    let second_voice = Id::new(11);
    let alice = Id::new(20);
    let bob = Id::new(21);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Alice".to_owned(),
        user_id: Some(alice),
    });

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(2),
        channels: vec![
            guild_voice_channel(guild_id, first_voice),
            ChannelInfo {
                name: "Raid".to_owned(),
                position: Some(1),
                ..guild_voice_channel(guild_id, second_voice)
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    let alice_member = member_with_username(alice, "Alice", "alice");
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            member: Some(alice_member),
            self_mute: true,
            self_stream: true,
            ..voice_state(guild_id, Some(first_voice), alice)
        },
    });
    let first_voice_participants = state.voice_participants_for_channel(guild_id, first_voice);
    assert_eq!(first_voice_participants[0].display_name, "Alice");
    assert!(first_voice_participants[0].self_stream);
    assert!(!first_voice_participants[0].speaking);
    assert_eq!(
        state.current_user_voice_connection(),
        Some(CurrentVoiceConnectionState {
            self_mute: true,
            ..CurrentVoiceConnectionState::test(guild_id, first_voice)
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

    let bob_member = member_with_username(bob, "Bob", "bob");
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            member: Some(bob_member),
            ..voice_state(guild_id, Some(first_voice), bob)
        },
    });
    state.apply_event(&AppEvent::VoiceSpeakingUpdate {
        guild_id,
        channel_id: first_voice,
        user_id: bob,
        speaking: true,
    });
    let first_voice_participants = state.voice_participants_for_channel(guild_id, first_voice);
    assert_eq!(first_voice_participants.len(), 2);
    assert!(
        first_voice_participants
            .iter()
            .any(|participant| participant.user_id == bob && participant.speaking)
    );

    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(second_voice), alice),
    });
    let first_voice_participants = state.voice_participants_for_channel(guild_id, first_voice);
    assert_eq!(first_voice_participants.len(), 1);
    assert_eq!(first_voice_participants[0].user_id, bob);
    assert!(!first_voice_participants[0].speaking);
    assert_eq!(
        state.voice_participants_for_channel(guild_id, second_voice)[0].user_id,
        alice
    );
    assert!(!state.voice_participants_for_channel(guild_id, second_voice)[0].speaking);
    assert!(!state.current_user_voice_speaking());
    assert_eq!(
        state.current_user_voice_connection(),
        Some(CurrentVoiceConnectionState::test(guild_id, second_voice))
    );

    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(second_voice), bob),
    });
    state.apply_event(&AppEvent::VoiceSpeakingUpdate {
        guild_id,
        channel_id: second_voice,
        user_id: bob,
        speaking: true,
    });
    assert!(
        state
            .voice_participants_for_channel(guild_id, second_voice)
            .iter()
            .any(|participant| participant.user_id == bob && participant.speaking)
    );

    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, None, alice),
    });
    let second_voice_participants = state.voice_participants_for_channel(guild_id, second_voice);
    assert_eq!(second_voice_participants.len(), 1);
    assert_eq!(second_voice_participants[0].user_id, bob);
    assert!(!second_voice_participants[0].speaking);
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
        channels: vec![guild_voice_channel(guild_id, voice)],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice), alice),
    });
    assert_eq!(
        state.voice_participants_for_channel(guild_id, voice)[0].user_id,
        alice
    );

    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        channels: vec![guild_voice_channel(guild_id, voice)],
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
        members: vec![member_info(alice, "alice")],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_info(bob, "bob"),
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&AppEvent::GuildMemberAdd {
        guild_id,
        member: member_info(bob, "bob"),
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&AppEvent::GuildMemberAdd {
        guild_id,
        member: member_info(Id::new(30), "carol"),
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
        members: vec![member_with_roles(user_id, "alice", vec![role_id])],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            ..RoleInfo::test(role_id, "Admin")
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
fn guild_role_events_patch_cached_roles() {
    let guild_id = Id::new(1);
    let role_id = Id::new(90);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture::new(guild_id)));
    state.apply_event(&AppEvent::GuildRoleUpsert {
        guild_id,
        role: RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            permissions: 1024,
            ..RoleInfo::test(role_id, "Admin")
        },
    });

    let roles = state.roles_for_guild(guild_id);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name, "Admin");
    assert_eq!(roles[0].color, Some(0xFFAA00));

    state.apply_event(&AppEvent::GuildRoleUpsert {
        guild_id,
        role: RoleInfo {
            color: Some(0x00AAFF),
            position: 20,
            hoist: false,
            permissions: 2048,
            ..RoleInfo::test(role_id, "Owner")
        },
    });

    let roles = state.roles_for_guild(guild_id);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name, "Owner");
    assert_eq!(roles[0].color, Some(0x00AAFF));
    assert_eq!(roles[0].permissions, 2048);

    state.apply_event(&AppEvent::GuildRoleDelete { guild_id, role_id });

    assert!(state.roles_for_guild(guild_id).is_empty());
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
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            ..RoleInfo::test(role_id, "Red")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    message.author_role_ids = vec![role_id];
    state.apply_event(&latest_history_loaded(channel_id, vec![message]));

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
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            ..RoleInfo::test(role_id, "Red")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id,
        message_id,
        author_id: user_id,
        author: "test-user".to_owned(),
        author_role_ids: vec![role_id],
        content: Some("hello".to_owned()),
        ..MessageCreateFixture::test_fixture_default()
    }));

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
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            ..RoleInfo::test(role_id, "Red")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    state.apply_event(&latest_history_loaded(channel_id, vec![message]));
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
        members: vec![member_info(user_id, "test-user")],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            color: Some(0xCC0000),
            position: 10,
            hoist: true,
            ..RoleInfo::test(stale_role_id, "Old Red")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    message.author_role_ids = vec![stale_role_id];
    state.apply_event(&latest_history_loaded(channel_id, vec![message]));

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
            member: member_info(user_id, display_name.to_owned()),
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
        member: member_info(user, "alice"),
    });
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id,
        user_id: user,
        status: PresenceStatus::Online,
        activities: Vec::new(),
    });
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_info(user, "alice-renamed"),
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|m| m.user_id == user)
        .unwrap();
    assert_eq!(member.display_name, "alice-renamed");
    assert_eq!(member.status, PresenceStatus::Online);
}
