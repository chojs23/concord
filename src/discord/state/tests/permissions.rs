use super::*;

// Keep these literals separate from the implementation constants so the tests
// verify Discord's documented bit values instead of reusing the code under test.
const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const SEND_TTS_MESSAGES: u64 = 0x0000_0000_0000_1000;
const MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
const ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
const READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
const CONNECT: u64 = 0x0000_0000_0010_0000;
const ADMINISTRATOR: u64 = 0x0000_0000_0000_0008;
const ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
const PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;

fn perm_role(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
    PermissionOverwriteInfo {
        allow,
        deny,
        ..PermissionOverwriteInfo::test(id, PermissionOverwriteKind::Role)
    }
}

fn perm_member(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
    PermissionOverwriteInfo {
        allow,
        deny,
        ..PermissionOverwriteInfo::test(id, PermissionOverwriteKind::Member)
    }
}

/// Build a single-guild state with one text channel, one member, and the
/// given role permissions / channel overwrites. The current user is set
/// from `READY` so permission lookups have an identity to consult.
fn guild_with_permissions(
    owner_id: Id<UserMarker>,
    my_id: Id<UserMarker>,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    my_role_ids: Vec<Id<RoleMarker>>,
    roles: Vec<RoleInfo>,
    overwrites: Vec<PermissionOverwriteInfo>,
) -> DiscordState {
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(my_id),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner_id),
        channels: vec![ChannelInfo {
            position: Some(0),
            permission_overwrites: overwrites,
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..channel_info(channel_id, "GuildText", Vec::new())
        }],
        members: vec![member_with_roles(my_id, "me", my_role_ids)],
        presences: Vec::new(),
        roles,
        emojis: Vec::new(),
    });
    state
}

#[test]
fn dm_channels_are_always_viewable() {
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(Id::new(99), "alice")));
    let channels = state.viewable_channels_for_guild(None);
    assert_eq!(channels.len(), 1);
}

#[test]
fn guild_owner_sees_everything_even_when_everyone_denies() {
    let me = Id::new(10);
    let guild = Id::new(1);
    let channel = Id::new(2);
    // @everyone explicitly denies VIEW_CHANNEL, but the owner short-circuit
    // must still grant access.
    let state = guild_with_permissions(
        me,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", 0)],
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));
}

#[test]
fn administrator_role_bypasses_channel_overwrites() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let admin_role = Id::new(50);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![admin_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", 0),
            RoleInfo {
                position: 1,
                permissions: ADMINISTRATOR,
                ..RoleInfo::test(admin_role, "Admin")
            },
        ],
        // Channel-level deny is irrelevant for ADMINISTRATOR holders.
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));
}

#[test]
fn everyone_deny_hides_channel_for_plain_member() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    // @everyone has VIEW_CHANNEL by default, but the channel-level
    // overwrite revokes it for a plain member.
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
    assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
}

#[test]
fn role_allow_overrides_everyone_deny() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let staff_role = Id::new(50);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL),
            RoleInfo {
                position: 1,
                ..RoleInfo::test(staff_role, "Staff")
            },
        ],
        vec![
            perm_role(guild.get(), 0, VIEW_CHANNEL),
            perm_role(staff_role.get(), VIEW_CHANNEL, 0),
        ],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));
}

#[test]
fn role_gateway_events_update_permission_checks() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let staff_role = Id::new(50);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![role_info(Id::new(guild.get()), "@everyone", 0)],
        Vec::new(),
    );
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));

    state.apply_event(&AppEvent::GuildRoleUpsert {
        guild_id: guild,
        role: role_info(staff_role, "Staff", VIEW_CHANNEL),
    });
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));

    state.apply_event(&AppEvent::GuildRoleUpsert {
        guild_id: guild,
        role: role_info(staff_role, "Staff", 0),
    });
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));

    state.apply_event(&AppEvent::GuildRoleUpsert {
        guild_id: guild,
        role: role_info(staff_role, "Staff", VIEW_CHANNEL),
    });
    state.apply_event(&AppEvent::GuildRoleDelete {
        guild_id: guild,
        role_id: staff_role,
    });
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
}

#[test]
fn role_delete_prunes_stale_role_ids_from_permission_overwrites() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let staff_role = Id::new(50);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", 0),
            role_info(staff_role, "Staff", 0),
        ],
        vec![perm_role(staff_role.get(), VIEW_CHANNEL, 0)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));

    state.apply_event(&AppEvent::GuildRoleDelete {
        guild_id: guild,
        role_id: staff_role,
    });

    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
}

#[test]
fn current_user_roles_handle_partial_and_complete_member_upserts() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let staff_role = Id::new(50);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", 0),
            RoleInfo {
                position: 1,
                permissions: VIEW_CHANNEL,
                ..RoleInfo::test(staff_role, "Staff")
            },
        ],
        Vec::new(),
    );
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id: guild,
        member: member_info(me, "unknown"),
    });

    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));

    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild),
        channel_id: channel,
        message_id: Id::new(100),
        author_id: Id::new(99),
        author: "sender".to_owned(),
        content: Some(format!("hello <@&{}>", staff_role.get())),
        mention_roles: vec![staff_role],
        ..MessageCreateFixture::default()
    }));
    assert_eq!(
        state.channel_unread(channel),
        ChannelUnreadState::Mentioned(1)
    );

    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", 0),
            RoleInfo {
                position: 1,
                permissions: VIEW_CHANNEL,
                ..RoleInfo::test(staff_role, "Staff")
            },
        ],
        Vec::new(),
    );
    state.apply_event(&AppEvent::GuildMemberUpsert {
        guild_id: guild,
        member: member_with_username(me, "me", "me"),
    });

    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
}

#[test]
fn member_overwrite_has_the_final_word() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let staff_role = Id::new(50);
    // Role-level grants VIEW, but the member-specific deny removes it.
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![staff_role],
        vec![
            role_info(Id::new(guild.get()), "@everyone", 0),
            RoleInfo {
                position: 1,
                permissions: VIEW_CHANNEL,
                ..RoleInfo::test(staff_role, "Staff")
            },
        ],
        vec![perm_member(me.get(), 0, VIEW_CHANNEL)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
}

#[test]
fn threads_inherit_parent_permission() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let parent = Id::new(2);
    let thread = Id::new(3);
    // Parent denies VIEW_CHANNEL. The thread carries no overwrites of its
    // own and must inherit the same answer.
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        parent,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild),
        parent_id: Some(parent),
        owner_id: None,
        name: "design-discussion".to_owned(),
        kind: "GuildPublicThread".to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        permission_overwrites: Vec::new(),
        ..channel_info(thread, "GuildPublicThread", Vec::new())
    }));
    let thread_state = state.channel(thread).expect("thread");
    assert!(!state.can_view_channel(thread_state));
}

#[test]
fn message_create_for_hidden_channel_does_not_promote_it() {
    // Regression guard: a MESSAGE_CREATE for a permission-hidden channel
    // must not flip the channel into the visible bucket. The message
    // itself is still tracked (it's a real Discord message), but the
    // sidebar must keep filtering the channel out and the visibility
    // stats must continue to count it as hidden.
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );

    // Sanity check: starts hidden.
    assert_eq!(
        state.channel_visibility_stats(Some(guild)),
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
    assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());

    // A message arrives for the hidden channel with the same author as a
    // legitimate Discord push.
    let message_id = Id::new(900);
    state.apply_event(&message_create_event(MessageCreateFixture {
        guild_id: Some(guild),
        channel_id: channel,
        message_id,
        author_id: owner,
        author: "owner".to_owned(),
        content: Some("hidden chatter".to_owned()),
        ..MessageCreateFixture::default()
    }));

    // The channel must remain hidden because no permission promotion happened.
    assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
    assert_eq!(
        state.channel_visibility_stats(Some(guild)),
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
    // The underlying channel record still exists and the message was
    // stored. Gating is a sidebar concern, not a data-purge concern.
    assert!(state.channel(channel).is_some());
    assert_eq!(state.messages_for_channel(channel).len(), 1);
}

#[test]
fn cannot_send_when_role_overwrite_denies_send_messages() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![RoleInfo {
            // VIEW + SEND globally, but channel overwrite revokes SEND.
            permissions: VIEW_CHANNEL | SEND_MESSAGES,
            ..RoleInfo::test(Id::new(guild.get()), "@everyone")
        }],
        vec![perm_role(guild.get(), 0, SEND_MESSAGES)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));
    assert!(!state.can_send_in_channel(ch));
}

#[test]
fn cannot_send_when_view_channel_is_denied() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | SEND_MESSAGES,
        )],
        vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(!state.can_view_channel(ch));
    assert!(!state.can_send_in_channel(ch));
}

#[test]
fn cannot_attach_when_role_overwrite_denies_attach_files() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![RoleInfo {
            // VIEW + SEND + ATTACH globally, channel revokes only ATTACH.
            permissions: VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
            ..RoleInfo::test(Id::new(guild.get()), "@everyone")
        }],
        vec![perm_role(guild.get(), 0, ATTACH_FILES)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_send_in_channel(ch));
    assert!(!state.can_attach_in_channel(ch));
}

#[test]
fn cannot_send_tts_when_role_overwrite_denies_send_tts_messages() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![RoleInfo {
            permissions: VIEW_CHANNEL | SEND_MESSAGES | SEND_TTS_MESSAGES,
            ..RoleInfo::test(Id::new(guild.get()), "@everyone")
        }],
        vec![perm_role(guild.get(), 0, SEND_TTS_MESSAGES)],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_send_in_channel(ch));
    assert!(!state.can_send_tts_in_channel(ch));
}

#[test]
fn cannot_attach_when_send_messages_is_missing() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | ATTACH_FILES,
        )],
        Vec::new(),
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_view_channel(ch));
    assert!(!state.can_send_in_channel(ch));
    assert!(!state.can_attach_in_channel(ch));
}

#[test]
fn manage_messages_requires_explicit_guild_permission() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | MANAGE_MESSAGES,
        )],
        Vec::new(),
    );

    let ch = state.channel(channel).expect("channel");
    assert!(state.can_manage_messages_in_channel(ch));
}

#[test]
fn manage_messages_defaults_permissive_while_guild_member_roles_hydrate() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![ChannelInfo {
            position: Some(0),
            guild_id: Some(guild),
            name: "general".to_owned(),
            ..channel_info(channel, "GuildText", Vec::new())
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        emojis: Vec::new(),
    });

    let ch = state.channel(channel).expect("channel");
    assert!(state.can_manage_messages_in_channel(ch));
}

#[test]
fn manage_messages_is_never_granted_for_dm_channels() {
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(dm_channel(Id::new(99), "alice")));

    let ch = state.channel(Id::new(99)).expect("channel");
    assert!(!state.can_manage_messages_in_channel(ch));
}

#[test]
fn pin_and_reaction_helpers_use_documented_permission_bits() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | READ_MESSAGE_HISTORY | ADD_REACTIONS | PIN_MESSAGES,
        )],
        Vec::new(),
    );

    let ch = state.channel(channel).expect("channel");
    assert!(state.can_read_message_history_in_channel(ch));
    assert!(state.can_add_reactions_in_channel(ch));
    assert!(state.can_pin_messages_in_channel(ch));
}

#[test]
fn voice_connect_requires_view_channel_and_connect_permission() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        Vec::new(),
    );
    state.apply_event(&AppEvent::ChannelUpsert(guild_voice_channel(
        guild, channel,
    )));
    let ch = state.channel(channel).expect("voice channel");
    assert!(state.can_view_channel(ch));
    assert!(!state.can_connect_voice_channel(ch));

    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | CONNECT,
        )],
        Vec::new(),
    );
    state.apply_event(&AppEvent::ChannelUpsert(guild_voice_channel(
        guild, channel,
    )));
    let ch = state.channel(channel).expect("voice channel");
    assert!(state.can_connect_voice_channel(ch));
}

#[test]
fn owner_can_send_and_attach_unconditionally() {
    let me = Id::new(10);
    let guild = Id::new(1);
    let channel = Id::new(2);
    let state = guild_with_permissions(
        me,
        me,
        guild,
        channel,
        vec![],
        vec![role_info(Id::new(guild.get()), "@everyone", 0)],
        vec![perm_role(
            guild.get(),
            0,
            VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
        )],
    );
    let ch = state.channel(channel).expect("channel");
    assert!(state.can_send_in_channel(ch));
    assert!(state.can_attach_in_channel(ch));
}

#[test]
fn private_threads_are_hidden_without_membership_state() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let parent = Id::new(2);
    let thread = Id::new(3);
    let mut state = guild_with_permissions(
        owner,
        me,
        guild,
        parent,
        vec![],
        vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            VIEW_CHANNEL | SEND_MESSAGES,
        )],
        Vec::new(),
    );
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild),
        parent_id: Some(parent),
        owner_id: None,
        name: "private planning".to_owned(),
        kind: "GuildPrivateThread".to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        permission_overwrites: Vec::new(),
        ..channel_info(thread, "GuildPrivateThread", Vec::new())
    }));
    let thread_state = state.channel(thread).expect("thread");
    assert!(!state.can_view_channel(thread_state));
    assert!(!state.can_send_in_channel(thread_state));
}

#[test]
fn private_threads_are_hidden_while_permission_state_is_missing() {
    let guild = Id::new(1);
    let thread = Id::new(3);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(guild),
        parent_id: Some(Id::new(2)),
        owner_id: None,
        name: "private planning".to_owned(),
        kind: "GuildPrivateThread".to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        permission_overwrites: Vec::new(),
        ..channel_info(thread, "GuildPrivateThread", Vec::new())
    }));

    let thread_state = state.channel(thread).expect("thread");
    assert!(!state.can_view_channel(thread_state));
}

#[test]
fn channel_visibility_stats_count_only_top_level() {
    // Threads should not skew the stats. The user navigates by channel, and
    // a thread under a hidden parent already inherits the parent's visibility.
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild = Id::new(1);
    let visible_channel = Id::new(2);
    let hidden_channel = Id::new(3);
    let visible_thread = Id::new(20);
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.apply_event(&AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![
            ChannelInfo {
                position: Some(0),
                guild_id: Some(guild),
                name: "general".to_owned(),
                ..channel_info(visible_channel, "GuildText", Vec::new())
            },
            ChannelInfo {
                guild_id: Some(guild),
                position: Some(1),
                name: "secret".to_owned(),
                permission_overwrites: vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
                ..channel_info(hidden_channel, "GuildText", Vec::new())
            },
            ChannelInfo {
                guild_id: Some(guild),
                parent_id: Some(visible_channel),
                owner_id: None,
                name: "design".to_owned(),
                kind: "GuildPublicThread".to_owned(),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                permission_overwrites: Vec::new(),
                ..channel_info(visible_thread, "GuildPublicThread", Vec::new())
            },
        ],
        members: vec![member_info(me, "me")],
        presences: Vec::new(),
        roles: vec![role_info(Id::new(guild.get()), "@everyone", VIEW_CHANNEL)],
        emojis: Vec::new(),
    });

    let stats = state.channel_visibility_stats(Some(guild));
    assert_eq!(
        stats,
        ChannelVisibilityStats {
            visible: 1,
            hidden: 1,
        },
        "expected the thread to be excluded from both buckets"
    );
}

#[test]
fn missing_current_user_id_falls_back_to_visible() {
    // Until READY arrives we cannot decide. Be permissive so the sidebar is
    // not empty during the brief window between connect and READY.
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        owner_id: Some(Id::new(99)),
        channels: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            name: "general".to_owned(),
            permission_overwrites: vec![perm_role(1, 0, VIEW_CHANNEL)],
            ..channel_info(Id::new(2), "GuildText", Vec::new())
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![role_info(Id::new(1), "@everyone", VIEW_CHANNEL)],
        emojis: Vec::new(),
    });
    let ch = state.channel(Id::new(2)).expect("channel");
    assert!(state.can_view_channel(ch));
}
