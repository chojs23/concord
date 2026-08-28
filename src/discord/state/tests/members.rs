use super::*;
use crate::discord::{
    GuildMemberListEntry, GuildVerificationLevel, PresenceEventFields, VoiceScope,
};
use chrono::Utc;
use serde_json::json;

fn listed_member(user_id: Id<UserMarker>, display_name: &str) -> GuildMemberListItem {
    GuildMemberListItem::Member {
        member: member_info(user_id, display_name),
        presence: None,
    }
}

fn member_list_update(
    guild_id: Id<GuildMarker>,
    member_count: u64,
    ops: Vec<GuildMemberListOperation>,
) -> AppEvent {
    member_list_update_with_id(guild_id, "everyone", member_count, ops)
}

fn member_list_update_with_id(
    guild_id: Id<GuildMarker>,
    list_id: &str,
    member_count: u64,
    ops: Vec<GuildMemberListOperation>,
) -> AppEvent {
    AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: Some(list_id.to_owned()),
            member_count: Some(member_count),
            online_count: Some(u32::try_from(member_count).unwrap_or(u32::MAX)),
            groups: vec![json!({ "id": "online", "count": member_count })],
            ops,
            extra_fields: BTreeMap::new(),
        },
    }
}

fn member_list_entries(
    state: &DiscordState,
    guild_id: Id<GuildMarker>,
) -> Vec<GuildMemberListEntry> {
    state
        .member_list_entries_for_guild(guild_id)
        .into_iter()
        .map(|(_, entry)| entry.clone())
        .collect()
}

#[test]
fn member_list_preserves_gateway_rows_and_operation_order() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let carol = Id::new(30);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(2),
        members: vec![member_info(alice, "alice"), member_info(bob, "bob")],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&member_list_update(
        guild_id,
        2,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 2,
                },
                listed_member(alice, "alice"),
                listed_member(bob, "bob"),
            ],
        }],
    ));
    assert!(state.member_list_has_ranges(guild_id, &[(0, 99)]));
    assert_eq!(
        member_list_entries(&state, guild_id),
        vec![
            GuildMemberListEntry::Group {
                id: "online".to_owned(),
                count: 2,
            },
            GuildMemberListEntry::Member { user_id: alice },
            GuildMemberListEntry::Member { user_id: bob },
        ]
    );

    state.apply_event(&member_list_update(guild_id, 5, Vec::new()));
    assert!(matches!(
        member_list_entries(&state, guild_id).first(),
        Some(GuildMemberListEntry::Group { count: 5, .. })
    ));

    state.apply_event(&member_list_update(
        guild_id,
        2,
        vec![GuildMemberListOperation::Update {
            index: 1,
            item: listed_member(carol, "carol"),
        }],
    ));
    assert_eq!(
        member_list_entries(&state, guild_id),
        vec![
            GuildMemberListEntry::Group {
                id: "online".to_owned(),
                count: 2,
            },
            GuildMemberListEntry::Member { user_id: carol },
            GuildMemberListEntry::Member { user_id: bob },
        ]
    );
    assert!(
        state
            .members_for_guild(guild_id)
            .iter()
            .any(|member| member.user_id == alice)
    );

    state.apply_event(&member_list_update(
        guild_id,
        3,
        vec![GuildMemberListOperation::Insert {
            index: 2,
            item: listed_member(alice, "alice"),
        }],
    ));
    state.apply_event(&member_list_update(
        guild_id,
        2,
        vec![GuildMemberListOperation::Delete { index: 1 }],
    ));
    assert_eq!(
        member_list_entries(&state, guild_id),
        vec![
            GuildMemberListEntry::Group {
                id: "online".to_owned(),
                count: 2,
            },
            GuildMemberListEntry::Member { user_id: alice },
            GuildMemberListEntry::Member { user_id: bob },
        ]
    );

    // Positional deltas must move range coverage with the rows. Otherwise a
    // later sparse range can be reported as loaded at indexes it no longer owns.
    state.apply_event(&member_list_update(
        guild_id,
        500,
        vec![GuildMemberListOperation::Sync {
            range: (200, 202),
            items: vec![
                listed_member(alice, "alice"),
                listed_member(bob, "bob"),
                listed_member(carol, "carol"),
            ],
        }],
    ));
    assert!(state.member_list_has_ranges(guild_id, &[(200, 202)]));

    state.apply_event(&member_list_update(
        guild_id,
        501,
        vec![GuildMemberListOperation::Insert {
            index: 150,
            item: listed_member(alice, "alice"),
        }],
    ));
    assert!(!state.member_list_has_ranges(guild_id, &[(200, 202)]));
    assert!(state.member_list_has_ranges(guild_id, &[(201, 203)]));

    state.apply_event(&member_list_update(
        guild_id,
        500,
        vec![GuildMemberListOperation::Delete { index: 202 }],
    ));
    assert!(!state.member_list_has_ranges(guild_id, &[(201, 203)]));
}

#[test]
fn member_list_keeps_stable_rows_until_replacement_ranges_are_complete() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let carol = Id::new(30);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(3),
        members: vec![
            member_info(alice, "alice"),
            member_info(bob, "bob"),
            member_info(carol, "carol"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.prepare_member_list_subscription(guild_id, Id::new(2), vec![(0, 99)]);
    let generation = state.member_list_refresh_generation(guild_id);
    state.apply_event(&member_list_update_with_id(
        guild_id,
        "everyone",
        2,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 2,
                },
                listed_member(alice, "alice"),
                listed_member(bob, "bob"),
            ],
        }],
    ));
    let stable = member_list_entries(&state, guild_id);

    state.apply_event(&member_list_update_with_id(
        guild_id,
        "everyone",
        2,
        vec![GuildMemberListOperation::Invalidate { range: (0, 99) }],
    ));
    assert_eq!(member_list_entries(&state, guild_id), stable);
    assert!(!state.member_list_has_ranges(guild_id, &[(0, 99)]));
    assert!(state.member_list_refresh_generation(guild_id) > generation);
    state.apply_event(&member_list_update_with_id(
        guild_id,
        "everyone",
        2,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 2,
                },
                listed_member(alice, "alice"),
                listed_member(bob, "bob"),
            ],
        }],
    ));

    let generation = state.member_list_refresh_generation(guild_id);
    state.prepare_member_list_subscription(guild_id, Id::new(3), vec![(0, 99)]);
    state.apply_event(&member_list_update_with_id(
        guild_id,
        "everyone",
        1,
        vec![GuildMemberListOperation::Insert {
            index: 0,
            item: listed_member(carol, "carol"),
        }],
    ));
    assert_eq!(member_list_entries(&state, guild_id), stable);
    assert!(!state.member_list_has_ranges(guild_id, &[(0, 99)]));
    assert_eq!(state.member_list_refresh_generation(guild_id), generation);

    state.apply_event(&member_list_update_with_id(
        guild_id,
        "everyone",
        1,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "offline".to_owned(),
                    count: 1,
                },
                listed_member(carol, "carol"),
            ],
        }],
    ));
    assert_eq!(
        member_list_entries(&state, guild_id),
        vec![
            GuildMemberListEntry::Group {
                id: "offline".to_owned(),
                count: 1,
            },
            GuildMemberListEntry::Member { user_id: carol },
        ]
    );
    assert!(state.member_list_has_ranges(guild_id, &[(0, 99)]));
}

#[test]
fn reidentified_gateway_session_invalidates_cached_member_list_ranges() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        members: vec![member_info(alice, "alice")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&member_list_update(
        guild_id,
        1,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 1,
                },
                listed_member(alice, "alice"),
            ],
        }],
    ));
    assert!(state.member_list_has_ranges(guild_id, &[(0, 99)]));
    let generation = state.member_list_refresh_generation(guild_id);

    state.apply_event(&AppEvent::GatewayReidentified);

    assert_eq!(
        member_list_entries(&state, guild_id),
        vec![
            GuildMemberListEntry::Group {
                id: "online".to_owned(),
                count: 1,
            },
            GuildMemberListEntry::Member { user_id: alice },
        ],
        "the current snapshot remains visible while the new session resubscribes"
    );
    assert!(!state.member_list_has_ranges(guild_id, &[(0, 99)]));
    assert!(state.member_list_refresh_generation(guild_id) > generation);
    assert_eq!(
        state.guild(guild_id).and_then(|guild| guild.online_count),
        None
    );
}

#[test]
fn tracks_members_and_initial_presence_activities() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DiscordState::default();

    let activity = ActivityInfo::test(ActivityKind::Listening, "Spotify");
    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(100),
        members: vec![member_info(alice, "alice"), member_info(bob, "bob")],
        presences: vec![PresenceEventFields {
            user_id: alice,
            status: PresenceStatus::Online,
            activities: vec![activity.clone()],
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    let members = state.members_for_guild(guild_id);
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(100));
    assert_eq!(members.len(), 2);
    let alice_state = members
        .iter()
        .find(|member| member.user_id == alice)
        .expect("Alice should be cached");
    assert_eq!(alice_state.status, PresenceStatus::Online);
    assert_eq!(
        state.user_activities_for_guild(Some(guild_id), alice),
        std::slice::from_ref(&activity)
    );
    let bob_state = members
        .iter()
        .find(|member| member.user_id == bob)
        .expect("Bob should be cached");
    assert_eq!(bob_state.status, PresenceStatus::Unknown);

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id: bob,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        },
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

    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: None,
        presence: crate::discord::PresenceEventFields {
            user_id: bob,
            status: PresenceStatus::DoNotDisturb,
            activities: Vec::new(),
        },
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

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        members: vec![MemberInfo {
            avatar_url: Some(
                "https://cdn.discordapp.com/guilds/1/users/10/avatars/guild.png".to_owned(),
            ),
            ..member_info(user_id, "neo")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&user_identity_update_event(UserIdentityUpdateFixture {
        user_id,
        username: "neo".to_owned(),
        global_name: Some("Neo".to_owned()),
        avatar_url: Some("https://cdn.discordapp.com/avatars/10/global.png".to_owned()),
        ..UserIdentityUpdateFixture::new()
    }));

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
fn typing_start_merges_the_complete_member_into_shared_state() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let user_id = Id::new(10);
    let role_id = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![channel_info(channel_id, "GuildText", Vec::new())],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::TypingStart {
        guild_id: Some(guild_id),
        channel_id,
        user_id,
        member: Some(MemberInfo {
            username: Some("typing-user".to_owned()),
            role_ids: vec![role_id],
            ..member_info(user_id, "Typing Nick")
        }),
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("typing member should enter the shared guild cache");
    assert_eq!(member.display_name, "Typing Nick");
    assert_eq!(member.username.as_deref(), Some("typing-user"));
    assert_eq!(member.role_ids, vec![role_id]);
    assert!(member.role_ids_known);
}

#[test]
fn ready_user_directory_joins_identity_with_merged_member_roles() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let role_id = Id::new(20);
    let avatar = "https://cdn.discordapp.com/avatars/10/ready.png";
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        members: vec![MemberInfo {
            role_ids: vec![role_id],
            role_ids_present: true,
            ..member_info(user_id, "unknown")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::ReadyUserDirectory {
        users: vec![ChannelRecipientInfo {
            username: Some("ready-user".to_owned()),
            is_bot: true,
            avatar_url: Some(avatar.to_owned()),
            ..ChannelRecipientInfo::test(user_id, "Ready Name")
        }],
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("merged member should remain cached");
    assert_eq!(member.display_name, "Ready Name");
    assert_eq!(member.username.as_deref(), Some("ready-user"));
    assert!(member.is_bot);
    assert_eq!(member.avatar_url.as_deref(), Some(avatar));
    assert_eq!(member.role_ids, vec![role_id]);
    assert!(member.role_ids_known);
}

#[test]
fn ready_user_directory_does_not_clear_a_known_bot_flag_by_omission() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        members: vec![MemberInfo {
            is_bot: true,
            ..member_info(user_id, "unknown")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::ReadyUserDirectory {
        users: vec![ChannelRecipientInfo {
            username: Some("known-bot".to_owned()),
            is_bot: false,
            ..ChannelRecipientInfo::test(user_id, "Known Bot")
        }],
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("bot member should remain cached");
    assert!(member.is_bot);
}

#[test]
fn partial_member_patch_preserves_fields_that_discord_omitted() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let role_id = Id::new(20);
    let avatar = "https://cdn.discordapp.com/avatars/10/original.png";
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        members: vec![MemberInfo {
            username: Some("original-user".to_owned()),
            is_bot: true,
            avatar_url: Some(avatar.to_owned()),
            role_ids: vec![role_id],
            ..member_info(user_id, "Original Name")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            display_name: "New Nick".to_owned(),
            username: None,
            is_bot: false,
            is_bot_present: false,
            avatar_url: None,
            avatar_url_present: false,
            role_ids: Vec::new(),
            role_ids_present: false,
            ..member_info(user_id, "New Nick")
        },
    });

    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("member should remain cached");
    assert_eq!(member.display_name, "New Nick");
    assert_eq!(member.username.as_deref(), Some("original-user"));
    assert!(member.is_bot);
    assert_eq!(member.avatar_url.as_deref(), Some(avatar));
    assert_eq!(member.role_ids, vec![role_id]);
    assert!(member.role_ids_known);

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            role_ids: Vec::new(),
            role_ids_present: true,
            ..member_info(user_id, "New Nick")
        },
    });
    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("member should remain cached");
    assert!(member.role_ids.is_empty());
    assert!(member.role_ids_known);

    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id,
        member: MemberInfo {
            avatar_url: None,
            avatar_url_present: true,
            ..member_info(user_id, "New Nick")
        },
    });
    let member = state
        .members_for_guild(guild_id)
        .into_iter()
        .find(|member| member.user_id == user_id)
        .expect("member should remain cached");
    assert!(member.avatar_url.is_none());
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

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(2),
        channels: vec![
            guild_voice_channel(guild_id, first_voice),
            ChannelInfo {
                name: "Raid".to_owned(),
                position: Some(1),
                ..guild_voice_channel(guild_id, second_voice)
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));

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

    state.apply_event(&voice_speaking_update_event(VoiceSpeakingUpdateFixture {
        scope: VoiceScope::Guild(guild_id),
        channel_id: first_voice,
        user_id: alice,
        speaking: true,
    }));
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
    state.apply_event(&voice_speaking_update_event(VoiceSpeakingUpdateFixture {
        scope: VoiceScope::Guild(guild_id),
        channel_id: first_voice,
        user_id: bob,
        speaking: true,
    }));
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
    state.apply_event(&voice_speaking_update_event(VoiceSpeakingUpdateFixture {
        scope: VoiceScope::Guild(guild_id),
        channel_id: second_voice,
        user_id: bob,
        speaking: true,
    }));
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
fn tracks_dm_call_participants_resolving_names_from_recipients() {
    let dm_channel = Id::new(50);
    let me = Id::new(20);
    let friend = Id::new(21);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });
    // A group DM has no guild and carries its members as recipients.
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        dm_channel,
        "",
        "group-dm",
        vec![ChannelRecipientInfo::test(friend, "Friend")],
    )));

    // Both users join the DM call, which Discord reports with a null guild.
    for user_id in [me, friend] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                guild_id: None,
                ..voice_state(Id::new(1), Some(dm_channel), user_id)
            },
        });
    }

    let participants = state.voice_participants_for_private_channel(dm_channel);
    assert_eq!(participants.len(), 2);
    // The current user resolves to the session name; the friend resolves through
    // the DM recipient list. A guild-scoped query must not see private calls.
    assert!(
        participants
            .iter()
            .any(|participant| participant.user_id == me && participant.display_name == "Me")
    );
    assert!(
        participants.iter().any(
            |participant| participant.user_id == friend && participant.display_name == "Friend"
        )
    );
    assert!(
        state
            .voice_participants_for_channel(Id::new(1), dm_channel)
            .is_empty()
    );

    // Leaving a DM call arrives with a null guild and null channel.
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id: None,
            ..voice_state(Id::new(1), None, friend)
        },
    });
    let participants = state.voice_participants_for_private_channel(dm_channel);
    assert_eq!(participants.len(), 1);
    assert_eq!(participants[0].user_id, me);
}

#[test]
fn dm_voice_uses_ready_user_directory_when_recipients_are_missing() {
    let dm_channel = Id::new(50);
    let user_id = Id::new(21);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ReadyUserDirectory {
        users: vec![ChannelRecipientInfo {
            username: Some("ready-user".to_owned()),
            ..ChannelRecipientInfo::test(user_id, "Ready User")
        }],
    });
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        kind: "dm".to_owned(),
        ..channel_info(dm_channel, "dm", Vec::new())
    }));
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id: None,
            ..voice_state(Id::new(1), Some(dm_channel), user_id)
        },
    });

    assert_eq!(
        state.voice_participants_for_private_channel(dm_channel)[0].display_name,
        "Ready User"
    );
}

#[test]
fn observed_users_and_current_member_request_missing_member_data() {
    let guild_id = Id::new(1);
    let text_channel = Id::new(2);
    let voice_channel = Id::new(3);
    let voice_user = Id::new(20);
    let typing_user = Id::new(21);
    let current_user = Id::new(22);
    let unrelated_user = Id::new(23);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "Current User".to_owned(),
        user_id: Some(current_user),
    });
    state.apply_event(&guild_create_event(GuildCreateFixture {
        verification_level: GuildVerificationLevel::High,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                ..channel_info(text_channel, "GuildText", Vec::new())
            },
            guild_voice_channel(guild_id, voice_channel),
        ],
        members: vec![
            MemberInfo {
                flags: None,
                ..member_with_roles(current_user, "Current User", Vec::new())
            },
            MemberInfo {
                username: Some("unrelated".to_owned()),
                role_ids_present: false,
                ..MemberInfo::test(unrelated_user, "Unrelated")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice_channel), voice_user),
    });
    state.apply_event(&AppEvent::TypingStart {
        guild_id: Some(guild_id),
        channel_id: text_channel,
        user_id: typing_user,
        member: None,
    });

    assert_eq!(
        state.missing_member_hydration_requests(Some(guild_id), std::time::Instant::now()),
        vec![(guild_id, vec![current_user, voice_user, typing_user])]
    );

    let mut hydrated_current = member_with_roles(current_user, "Current User", Vec::new());
    hydrated_current.flags = Some(0);
    hydrated_current.joined_at = Some(Utc::now());
    state.apply_event(&AppEvent::GuildMembersChunk {
        chunk: GuildMembersChunkInfo {
            guild_id,
            members: vec![
                member_with_roles(voice_user, "Voice Nick", vec![Id::new(30)]),
                member_with_roles(typing_user, "Typing Nick", vec![Id::new(31)]),
                hydrated_current,
            ],
            presences: Vec::new(),
            chunk_index: Some(0),
            chunk_count: Some(1),
            nonce: Some("member-hydration".to_owned()),
            not_found: Vec::new(),
            extra_fields: BTreeMap::new(),
        },
    });

    assert_eq!(
        state.voice_participants_for_channel(guild_id, voice_channel)[0].display_name,
        "Voice Nick"
    );
    assert!(
        state
            .missing_member_hydration_requests(Some(guild_id), std::time::Instant::now())
            .is_empty()
    );
}

#[test]
fn member_cache_pruning_keeps_active_voice_participants() {
    let guild_id = Id::new(1);
    let voice_channel = Id::new(2);
    let active_user = Id::new(20);
    let inactive_user = Id::new(21);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_voice_channel(guild_id, voice_channel)],
        members: vec![
            member_with_username(active_user, "Active", "active"),
            member_with_username(inactive_user, "Inactive", "inactive"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.record_selected_member_guild(Some(guild_id));
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice_channel), active_user),
    });
    state.apply_event(&member_list_update(
        guild_id,
        2,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 2,
                },
                listed_member(active_user, "Active"),
                listed_member(inactive_user, "Inactive"),
            ],
        }],
    ));
    assert!(state.member_list_has_ranges(guild_id, &[(0, 99)]));
    let generation = state.member_list_refresh_generation(guild_id);

    for guild_number in 2..=12 {
        let other_guild = Id::new(guild_number);
        let display_name = format!("User {guild_number}");
        let username = format!("user-{guild_number}");
        state.apply_event(&guild_create_event(GuildCreateFixture {
            members: vec![member_with_username(
                Id::new(100 + guild_number),
                &display_name,
                &username,
            )],
            ..GuildCreateFixture::new(other_guild)
        }));
        state.record_selected_member_guild(Some(other_guild));
    }

    let retained_ids = state
        .members_for_guild(guild_id)
        .into_iter()
        .map(|member| member.user_id)
        .collect::<Vec<_>>();
    assert_eq!(retained_ids, vec![active_user]);
    assert!(state.member_list_entries_for_guild(guild_id).is_empty());
    assert!(!state.member_list_has_ranges(guild_id, &[(0, 99)]));
    assert!(state.member_list_refresh_generation(guild_id) > generation);
    assert_eq!(
        state.guild(guild_id).and_then(|guild| guild.online_count),
        None
    );
}

#[test]
fn moving_between_dm_calls_does_not_leave_the_user_in_the_old_call() {
    let first_dm = Id::new(50);
    let second_dm = Id::new(51);
    let me = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });

    // Join the first DM call, then move straight to a second DM call. Discord
    // reports the move as a single voice state for the new call, with no leave
    // for the old one.
    for dm_channel in [first_dm, second_dm] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                guild_id: None,
                ..voice_state(Id::new(1), Some(dm_channel), me)
            },
        });
    }

    assert!(
        state
            .voice_participants_for_private_channel(first_dm)
            .is_empty(),
        "the user should no longer appear in the call they left"
    );
    let current = state.voice_participants_for_private_channel(second_dm);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].user_id, me);
    assert_eq!(
        state
            .current_user_voice_connection()
            .map(|voice| voice.scope),
        Some(VoiceScope::Private(second_dm))
    );
}

#[test]
fn leaving_a_dm_call_clears_stale_speaking_in_the_old_call() {
    let first_dm = Id::new(50);
    let second_dm = Id::new(51);
    let me = Id::new(20);
    let friend = Id::new(21);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        first_dm,
        "",
        "group-dm",
        vec![ChannelRecipientInfo::test(friend, "Friend")],
    )));

    for user_id in [me, friend] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                guild_id: None,
                ..voice_state(Id::new(1), Some(first_dm), user_id)
            },
        });
    }
    state.apply_event(&voice_speaking_update_event(VoiceSpeakingUpdateFixture {
        scope: VoiceScope::Private(first_dm),
        channel_id: first_dm,
        user_id: friend,
        speaking: true,
    }));
    assert!(
        state
            .voice_participants_for_private_channel(first_dm)
            .iter()
            .any(|participant| participant.user_id == friend && participant.speaking)
    );

    // Moving to another DM call must reset speaking flags in the call we left,
    // which sits under a different scope than the new one.
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id: None,
            ..voice_state(Id::new(1), Some(second_dm), me)
        },
    });
    assert!(
        state
            .voice_participants_for_private_channel(first_dm)
            .iter()
            .all(|participant| !participant.speaking)
    );
}

#[test]
fn call_delete_clears_a_dm_calls_participants() {
    let dm = Id::new(50);
    let me = Id::new(20);
    let friend = Id::new(21);
    let mut state = DiscordState::default();

    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel_with_recipients(
        dm,
        "",
        "group-dm",
        vec![ChannelRecipientInfo::test(friend, "Friend")],
    )));
    for user_id in [me, friend] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                guild_id: None,
                ..voice_state(Id::new(1), Some(dm), user_id)
            },
        });
    }
    assert_eq!(state.voice_participants_for_private_channel(dm).len(), 2);

    state.apply_event(&AppEvent::CallDelete { channel_id: dm });
    assert!(state.voice_participants_for_private_channel(dm).is_empty());
}

#[test]
fn dm_call_join_and_leave_both_chime() {
    use crate::discord::VoiceSoundKind;

    let dm = Id::new(50);
    let me = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });

    // Joining a DM call carries the channel, so it chimes a join.
    let join = VoiceStateInfo {
        guild_id: None,
        ..voice_state(Id::new(1), Some(dm), me)
    };
    assert_eq!(
        state.voice_sound_for_state_update(&join),
        Some(VoiceSoundKind::Join)
    );
    state.apply_event(&AppEvent::VoiceStateUpdate { state: join });

    // Leaving a DM call arrives with a null guild and null channel; the leave
    // chime must still fire, found via the cached entry rather than the payload.
    let leave = VoiceStateInfo {
        guild_id: None,
        ..voice_state(Id::new(1), None, me)
    };
    assert_eq!(
        state.voice_sound_for_state_update(&leave),
        Some(VoiceSoundKind::Leave)
    );
}

#[test]
fn another_participant_starting_a_stream_chimes_in_the_active_voice_channel() {
    use crate::discord::VoiceSoundKind;

    let guild_id = Id::new(1);
    let channel_id = Id::new(10);
    let me = Id::new(20);
    let broadcaster = Id::new(30);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(me),
    });
    for user_id in [me, broadcaster] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: voice_state(guild_id, Some(channel_id), user_id),
        });
    }

    let started = VoiceStateInfo {
        self_stream: true,
        ..voice_state(guild_id, Some(channel_id), broadcaster)
    };
    assert_eq!(
        state.voice_sound_for_state_update(&started),
        Some(VoiceSoundKind::StreamStart)
    );
}

#[test]
fn guild_create_replaces_cached_voice_state_snapshot() {
    let guild_id = Id::new(1);
    let voice = Id::new(10);
    let alice = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        channels: vec![guild_voice_channel(guild_id, voice)],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice), alice),
    });
    assert_eq!(
        state.voice_participants_for_channel(guild_id, voice)[0].user_id,
        alice
    );

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        channels: vec![guild_voice_channel(guild_id, voice)],
        ..GuildCreateFixture::new(guild_id)
    }));

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

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(100),
        ..GuildCreateFixture::new(guild_id)
    }));
    state.apply_event(&AppEvent::PresenceUpdate {
        guild_id: Some(guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Idle,
            activities: Vec::new(),
        },
    });

    assert!(state.members_for_guild(guild_id).is_empty());
    assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
}

#[test]
fn limited_member_events_do_not_guess_the_authoritative_member_count() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        members: vec![member_info(alice, "alice")],
        ..GuildCreateFixture::new(guild_id)
    }));

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
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&AppEvent::GuildMemberRemove {
        guild_id,
        user_id: Id::new(30),
    });
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

    state.apply_event(&member_list_update(
        guild_id,
        2,
        vec![GuildMemberListOperation::Invalidate { range: (0, 99) }],
    ));
    assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));
}

#[test]
fn guild_create_caches_roles_and_member_role_ids() {
    let guild_id = Id::new(1);
    let role_id = Id::new(90);
    let user_id = Id::new(10);
    let mut state = DiscordState::default();

    state.apply_event(&guild_create_event(GuildCreateFixture {
        members: vec![member_with_roles(user_id, "alice", vec![role_id])],
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            ..RoleInfo::test(role_id, "Admin")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

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
fn message_author_role_color_uses_the_best_complete_role_source() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let role_id = Id::new(90);
    let user_id = Id::new(10);

    let guild = |members: Vec<MemberInfo>| {
        let mut state = DiscordState::default();
        state.apply_event(&guild_create_event(GuildCreateFixture {
            members,
            roles: vec![RoleInfo {
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                ..RoleInfo::test(role_id, "Red")
            }],
            ..GuildCreateFixture::new(guild_id)
        }));
        state
    };
    let history_message = |role_ids: Vec<Id<RoleMarker>>| {
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        message.author_role_ids = role_ids;
        message
    };

    // Message and profile roles provide an immediate color until a complete
    // member arrives. A partial member must not hide those stronger sources.
    let mut from_history = guild(Vec::new());
    from_history.apply_event(&latest_history_loaded(
        channel_id,
        vec![history_message(vec![role_id])],
    ));
    assert_eq!(
        from_history.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "history author roles"
    );
    from_history.apply_event(&latest_history_loaded(
        channel_id,
        vec![history_message(Vec::new())],
    ));
    assert_eq!(
        from_history.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "thin history payload preserves known author roles"
    );
    let mut explicit_no_roles = history_message(Vec::new());
    explicit_no_roles.author_role_ids_present = true;
    from_history.apply_event(&latest_history_loaded(channel_id, vec![explicit_no_roles]));
    assert_eq!(
        from_history.message_author_role_color(guild_id, channel_id, message_id, user_id),
        None,
        "explicit empty author roles clear the cached roles"
    );

    let mut from_live = guild(Vec::new());
    from_live.apply_event(&message_create_event(MessageCreateFixture {
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
        from_live.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "live author roles"
    );

    let mut from_profile = guild(Vec::new());
    from_profile.apply_event(&latest_history_loaded(
        channel_id,
        vec![history_message(Vec::new())],
    ));
    let mut profile = profile_info(user_id.get(), Some("test-user"));
    profile.role_ids = vec![role_id];
    profile.role_ids_present = true;
    from_profile.apply_event(&AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile,
    });
    assert_eq!(
        from_profile.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "loaded profile roles"
    );
    from_profile.apply_event(&AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile: profile_info(user_id.get(), Some("test-user")),
    });
    assert_eq!(
        from_profile.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "profile payload without guild roles preserves the cached roles"
    );

    let mut incomplete_member = guild(vec![MemberInfo {
        role_ids_present: false,
        ..member_info(user_id, "test-user")
    }]);
    incomplete_member.apply_event(&latest_history_loaded(
        channel_id,
        vec![history_message(vec![role_id])],
    ));
    assert_eq!(
        incomplete_member.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(0xCC0000),
        "partial member preserves message role fallback"
    );

    let mut complete_member = guild(vec![member_info(user_id, "test-user")]);
    complete_member.apply_event(&latest_history_loaded(
        channel_id,
        vec![history_message(vec![role_id])],
    ));
    assert_eq!(
        complete_member.message_author_role_color(guild_id, channel_id, message_id, user_id),
        None,
        "complete member wins over stale message roles"
    );
}

#[test]
fn guild_member_chunk_refreshes_visible_message_author_identity_and_role() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let message_id = Id::new(3);
    let user_id = Id::new(10);
    let role_id = Id::new(90);
    let role_color = 0x00AAFF;
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        roles: vec![RoleInfo {
            color: Some(role_color),
            position: 10,
            ..RoleInfo::test(role_id, "Blue")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));

    let mut message = message_info(channel_id, message_id.get(), "hello");
    message.guild_id = Some(guild_id);
    message.author_id = user_id;
    message.author = "Old Name".to_owned();
    state.apply_event(&latest_history_loaded(channel_id, vec![message]));

    state.apply_event(&AppEvent::GuildMembersChunk {
        chunk: GuildMembersChunkInfo {
            guild_id,
            members: vec![MemberInfo {
                username: Some("alice".to_owned()),
                nickname: Some("New Nick".to_owned()),
                nickname_present: true,
                role_ids: vec![role_id],
                ..MemberInfo::test(user_id, "New Nick")
            }],
            presences: Vec::new(),
            chunk_index: Some(0),
            chunk_count: Some(1),
            nonce: Some("member-hydration".to_owned()),
            not_found: Vec::new(),
            extra_fields: BTreeMap::new(),
        },
    });

    let visible_message = state.messages_for_channel(channel_id)[0];
    assert_eq!(visible_message.author, "New Nick");
    assert_eq!(
        state.message_author_role_color(guild_id, channel_id, message_id, user_id),
        Some(role_color)
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
        guild_id: Some(guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id: alice,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
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
        guild_id: Some(guild_id),
        presence: crate::discord::PresenceEventFields {
            user_id: user,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
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
