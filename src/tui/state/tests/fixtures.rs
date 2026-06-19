use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::super::{ActiveGuildScope, DashboardState};
pub(super) use crate::discord::test_builders::{
    MessageCreateFixture, guild_message_create_fixture, message_create_event,
};
use crate::discord::{
    AppEvent, AttachmentInfo, ChannelInfo, CustomEmojiInfo, EmbedInfo, GuildFolder, MemberInfo,
    MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, MessageState,
    PermissionOverwriteInfo, PermissionOverwriteKind, PollAnswerInfo, PollInfo, PresenceStatus,
    ReactionEmoji, ReactionInfo, ReadStateInfo, RoleInfo, ThreadMetadataInfo, VoiceStateInfo,
};

pub(super) const PERM_ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
pub(super) const PERM_MANAGE_CHANNELS: u64 = 0x0000_0000_0000_0010;
pub(super) const PERM_VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
pub(super) const PERM_SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
pub(super) const PERM_SEND_TTS_MESSAGES: u64 = 0x0000_0000_0000_1000;
pub(super) const PERM_MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
pub(super) const PERM_READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
pub(super) const PERM_PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;

pub(super) fn channel_info(
    channel_id: Id<ChannelMarker>,
    kind: impl Into<String>,
    permission_overwrites: Vec<PermissionOverwriteInfo>,
) -> ChannelInfo {
    ChannelInfo {
        permission_overwrites,
        ..ChannelInfo::test(channel_id, kind)
    }
}

pub(super) fn text_channel_info(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        name: name.into(),
        ..channel_info(channel_id, "GuildText", Vec::new())
    }
}

pub(super) fn positioned_text_channel_info(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
    position: i32,
) -> ChannelInfo {
    ChannelInfo {
        kind: "text".to_owned(),
        position: Some(position),
        ..text_channel_info(guild_id, channel_id, name)
    }
}

pub(super) fn category_channel_info(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
    position: i32,
) -> ChannelInfo {
    ChannelInfo {
        kind: "category".to_owned(),
        position: Some(position),
        ..text_channel_info(guild_id, channel_id, name)
    }
}

pub(super) fn child_text_channel_info(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    parent_id: Id<ChannelMarker>,
    name: impl Into<String>,
    position: i32,
) -> ChannelInfo {
    ChannelInfo {
        parent_id: Some(parent_id),
        owner_id: None,
        ..positioned_text_channel_info(guild_id, channel_id, name, position)
    }
}

pub(super) fn thread_channel_info(
    guild_id: Id<GuildMarker>,
    parent_id: Id<ChannelMarker>,
    thread_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        parent_id: Some(parent_id),
        owner_id: None,
        name: name.into(),
        kind: "thread".to_owned(),
        thread_metadata: Some(ThreadMetadataInfo::test(false, false)),
        ..text_channel_info(guild_id, thread_id, "")
    }
}

pub(super) fn dm_channel_info(
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        name: name.into(),
        kind: "dm".to_owned(),
        ..channel_info(channel_id, "dm", Vec::new())
    }
}

pub(super) fn voice_channel_info(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        kind: "GuildVoice".to_owned(),
        position: Some(0),
        ..text_channel_info(guild_id, channel_id, name)
    }
}

pub(super) fn member_info(user_id: Id<UserMarker>, display_name: impl Into<String>) -> MemberInfo {
    MemberInfo::test(user_id, display_name)
}

pub(super) fn member_with_username(
    user_id: Id<UserMarker>,
    display_name: impl Into<String>,
    username: impl Into<String>,
) -> MemberInfo {
    MemberInfo {
        username: Some(username.into()),
        ..member_info(user_id, display_name)
    }
}

pub(super) fn member_with_roles(
    user_id: Id<UserMarker>,
    display_name: impl Into<String>,
    role_ids: Vec<Id<RoleMarker>>,
) -> MemberInfo {
    MemberInfo {
        role_ids,
        ..member_info(user_id, display_name)
    }
}

pub(super) fn role_info(
    role_id: Id<RoleMarker>,
    name: impl Into<String>,
    permissions: u64,
) -> RoleInfo {
    RoleInfo {
        permissions,
        ..RoleInfo::test(role_id, name)
    }
}

pub(super) fn voice_state(
    guild_id: Id<GuildMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    user_id: Id<UserMarker>,
) -> VoiceStateInfo {
    VoiceStateInfo::test(guild_id, channel_id, user_id)
}

pub(super) fn read_state_info(
    channel_id: Id<ChannelMarker>,
    last_acked_message_id: Option<Id<MessageMarker>>,
    mention_count: u32,
) -> ReadStateInfo {
    ReadStateInfo {
        last_acked_message_id,
        mention_count,
        ..ReadStateInfo::test(channel_id)
    }
}

pub(super) fn guild_create_event(
    guild_id: Id<GuildMarker>,
    name: impl Into<String>,
    channels: Vec<ChannelInfo>,
) -> AppEvent {
    AppEvent::GuildCreate {
        guild_id,
        name: name.into(),
        member_count: None,
        owner_id: None,
        channels,
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
    }
}

pub(super) fn latest_history_loaded(
    channel_id: Id<ChannelMarker>,
    messages: Vec<MessageInfo>,
) -> AppEvent {
    AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages,
    }
}

/// Build a guild with a single channel where @everyone keeps
/// VIEW_CHANNEL but loses SEND_MESSAGES. This is an announcement-style
/// read-only channel that the user can read but not post in.
pub(super) fn state_with_read_only_channel() -> DashboardState {
    guild_state_with_overwrites(vec![PermissionOverwriteInfo {
        deny: 0x800,
        ..PermissionOverwriteInfo::test(1, PermissionOverwriteKind::Role)
    }])
}

/// Build a guild with a single hidden channel to verify visibility stats.
pub(super) fn state_with_view_denied_channel() -> DashboardState {
    guild_state_with_overwrites(vec![PermissionOverwriteInfo {
        deny: 0x400,
        ..PermissionOverwriteInfo::test(1, PermissionOverwriteKind::Role)
    }])
}

/// Build a guild with a single channel where @everyone has VIEW + SEND + TTS
/// (no overwrites), so the composer should open and submit normally.
pub(super) fn state_with_writable_channel() -> DashboardState {
    guild_state_with_overwrites(Vec::new())
}

pub(super) fn state_with_other_user_message_permissions(
    permissions: u64,
    reactions: Vec<ReactionInfo>,
) -> DashboardState {
    state_with_other_user_message_permissions_and_member(permissions, reactions, true)
}

pub(super) fn state_with_other_user_message_permissions_hydrating_member(
    permissions: u64,
    reactions: Vec<ReactionInfo>,
) -> DashboardState {
    state_with_other_user_message_permissions_and_member(permissions, reactions, false)
}

fn state_with_other_user_message_permissions_and_member(
    permissions: u64,
    reactions: Vec<ReactionInfo>,
    include_current_member: bool,
) -> DashboardState {
    let me: Id<UserMarker> = Id::new(10);
    let owner: Id<UserMarker> = Id::new(11);
    let guild: Id<GuildMarker> = Id::new(1);
    let channel: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![positioned_text_channel_info(guild, channel, "general", 0)],
        members: include_current_member
            .then_some(member_with_username(me, "me", "me"))
            .into_iter()
            .collect(),
        presences: Vec::new(),
        roles: vec![role_info(Id::new(guild.get()), "@everyone", permissions)],
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild));
    state.activate_channel(channel);
    state.push_event(latest_history_loaded(
        channel,
        vec![MessageInfo {
            reactions,
            ..message_info(channel, 1)
        }],
    ));
    state
}

pub(super) fn state_with_hidden_and_visible_channels() -> DashboardState {
    let me: Id<UserMarker> = Id::new(10);
    let owner: Id<UserMarker> = Id::new(11);
    let guild: Id<GuildMarker> = Id::new(1);
    let hidden: Id<ChannelMarker> = Id::new(2);
    let visible: Id<ChannelMarker> = Id::new(3);
    let voice: Id<ChannelMarker> = Id::new(4);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![
            ChannelInfo {
                permission_overwrites: vec![PermissionOverwriteInfo {
                    deny: 0x400,
                    ..PermissionOverwriteInfo::test(guild.get(), PermissionOverwriteKind::Role)
                }],
                ..positioned_text_channel_info(guild, hidden, "secret", 0)
            },
            positioned_text_channel_info(guild, visible, "general", 1),
            ChannelInfo {
                position: Some(2),
                ..voice_channel_info(guild, voice, "voice")
            },
        ],
        members: vec![member_info(me, "me")],
        presences: Vec::new(),
        roles: vec![role_info(Id::new(guild.get()), "@everyone", 0x400)],
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild));
    state
}

pub(super) fn guild_state_with_overwrites(
    overwrites: Vec<PermissionOverwriteInfo>,
) -> DashboardState {
    let me: Id<UserMarker> = Id::new(10);
    let owner: Id<UserMarker> = Id::new(11);
    let guild: Id<GuildMarker> = Id::new(1);
    let channel: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![ChannelInfo {
            permission_overwrites: overwrites,
            ..positioned_text_channel_info(guild, channel, "general", 0)
        }],
        members: vec![member_info(me, "me")],
        presences: Vec::new(),
        roles: vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_SEND_MESSAGES | PERM_SEND_TTS_MESSAGES,
        )],
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild));
    state.activate_channel(channel);
    state
}

pub(super) fn state_with_writable_channel_and_members() -> DashboardState {
    let me: Id<UserMarker> = Id::new(10);
    let owner: Id<UserMarker> = Id::new(11);
    let guild: Id<GuildMarker> = Id::new(1);
    let channel: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(3),
        owner_id: Some(owner),
        channels: vec![positioned_text_channel_info(guild, channel, "general", 0)],
        members: vec![
            member_with_username(me, "me", "me"),
            member_with_username(Id::new(20), "Sally", "salamander"),
            member_with_username(Id::new(21), "Sammy", "sammy42"),
            member_with_username(Id::new(22), "Bob", "bobtheb"),
            member_with_username(Id::new(23), "Alias", "Alias123"),
        ],
        presences: vec![
            (me, PresenceStatus::Online),
            (Id::new(20), PresenceStatus::Online),
            (Id::new(21), PresenceStatus::Online),
            (Id::new(22), PresenceStatus::Online),
            (Id::new(23), PresenceStatus::Online),
        ],
        roles: vec![role_info(
            Id::new(guild.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_SEND_MESSAGES | PERM_SEND_TTS_MESSAGES,
        )],
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild));
    state.activate_channel(channel);
    state
}

pub(super) fn state_with_folder(folder_id: Option<u64>) -> DashboardState {
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    let mut state = DashboardState::new();

    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(guild_create_event(guild_id, name, Vec::new()));
    }
    state.push_event(super::user_settings_update(vec![GuildFolder {
        id: folder_id,
        name: Some("folder".to_owned()),
        color: None,
        guild_ids: vec![first_guild, second_guild],
    }]));
    state
}

pub(super) fn state_with_many_guilds(count: u64) -> DashboardState {
    let mut state = DashboardState::new();
    for id in 1..=count {
        state.push_event(guild_create_event(
            Id::new(id),
            format!("guild {id}"),
            Vec::new(),
        ));
    }
    state
}

pub(super) fn state_with_many_channels(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    let channels = (1..=count)
        .map(|id| {
            positioned_text_channel_info(guild_id, Id::new(id), format!("channel {id}"), id as i32)
        })
        .collect();

    state.push_event(guild_create_event(guild_id, "guild", channels));
    state.confirm_selected_guild();
    state
}

pub(super) fn state_with_members(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();
    let members = (1..=count)
        .map(|id| member_info(Id::new(id), format!("member {id}")))
        .collect();
    let presences = (1..=count)
        .map(|id| (Id::new(id), PresenceStatus::Online))
        .collect();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![text_channel_info(guild_id, channel_id, "general")],
        members,
        presences,
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

pub(super) fn state_with_grouped_members() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let role_id = Id::new(100);
    let mut state = DashboardState::new();
    let members = (1..=4)
        .map(|id| {
            member_with_roles(
                Id::new(id),
                format!("member {id}"),
                (id <= 2).then_some(role_id).into_iter().collect(),
            )
        })
        .collect();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![text_channel_info(guild_id, channel_id, "general")],
        members,
        presences: vec![
            (Id::new(1), PresenceStatus::Online),
            (Id::new(2), PresenceStatus::Online),
            (Id::new(3), PresenceStatus::Offline),
            (Id::new(4), PresenceStatus::Offline),
        ],
        roles: vec![RoleInfo {
            position: 1,
            hoist: true,
            ..RoleInfo::test(role_id, "Role")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

pub(super) fn state_with_channel_tree() -> DashboardState {
    let guild_id = Id::new(1);
    let category_id = Id::new(10);
    let general_id = Id::new(11);
    let random_id = Id::new(12);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            category_channel_info(guild_id, category_id, "Text Channels", 0),
            child_text_channel_info(guild_id, general_id, category_id, "general", 0),
            child_text_channel_info(guild_id, random_id, category_id, "random", 1),
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

pub(super) fn state_with_direct_messages() -> DashboardState {
    let mut state = DashboardState::new();
    for (channel_id, name, last_message_id) in [
        (Id::new(10), "old", Some(Id::new(100))),
        (Id::new(20), "new", Some(Id::new(200))),
        (Id::new(30), "empty", None),
    ] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            last_message_id,
            ..dm_channel_info(channel_id, name.to_owned())
        }));
    }
    state
}

pub(super) fn state_with_messages(count: u64) -> DashboardState {
    state_with_message_ids(1..=count)
}

pub(super) fn state_with_reaction_message() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            reactions: vec![
                ReactionInfo {
                    count: 2,
                    me: true,
                    ..ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
                },
                ReactionInfo::test(ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                }),
            ],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state
}

pub(super) fn state_with_custom_emojis() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![text_channel_info(guild_id, channel_id, "general")],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![
            CustomEmojiInfo {
                animated: true,
                ..CustomEmojiInfo::test(Id::new(50), "party_time")
            },
            CustomEmojiInfo {
                available: false,
                ..CustomEmojiInfo::test(Id::new(51), "gone")
            },
        ],
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(guild_text_message(1, "hello")));
    state
}

pub(super) fn state_with_single_message_content(content: &str) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![text_channel_info(guild_id, channel_id, "general")],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(guild_text_message(1, content)));
    state
}

pub(super) fn state_with_thread_created_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id: Id<ChannelMarker> = Id::new(2);
    let thread_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            text_channel_info(guild_id, parent_id, "general"),
            ChannelInfo {
                message_count: Some(12),
                member_count: None,
                total_message_sent: Some(14),
                ..thread_channel_info(guild_id, parent_id, thread_id, "release notes")
            },
        ],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(
        MessageCreateFixture::guild_message(guild_id, parent_id, Id::new(1))
            .with_message_kind(MessageKind::new(18))
            .with_reference(MessageReferenceInfo {
                guild_id: Some(guild_id),
                channel_id: Some(thread_id),
                message_id: None,
            })
            .with_content("release notes"),
    ));
    state
}

pub(super) fn height_test_message(content: &str) -> MessageState {
    MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        message_kind: MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(content.to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    }
}

pub(super) fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
    state_with_messages_matching(1..=count, |id| image_message_ids.contains(&id))
}

pub(super) fn state_with_message_ids(message_ids: impl IntoIterator<Item = u64>) -> DashboardState {
    state_with_messages_matching(message_ids, |_| false)
}

pub(super) fn state_with_messages_matching(
    message_ids: impl IntoIterator<Item = u64>,
    has_image: impl Fn(u64) -> bool,
) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![text_channel_info(guild_id, channel_id, "general")],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in message_ids {
        let attachments = has_image(id)
            .then(|| image_attachment(id))
            .into_iter()
            .collect();
        state.push_event(message_create_event(
            guild_text_message(id, format!("msg {id}")).with_attachments(attachments),
        ));
    }
    state
}

pub(super) fn push_text_message(state: &mut DashboardState, message_id: u64, content: &str) {
    state.push_event(message_create_event(guild_text_message(
        message_id, content,
    )));
}

pub(super) fn guild_text_message(
    message_id: u64,
    content: impl Into<String>,
) -> MessageCreateFixture {
    MessageCreateFixture::guild_message(Id::new(1), Id::new(2), Id::new(message_id))
        .with_content(content)
}

pub(super) fn image_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        url: format!("https://cdn.discordapp.com/image-{id}.png"),
        proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
        content_type: Some("image/png".to_owned()),
        size: 2048,
        width: Some(640),
        height: Some(480),
        ..AttachmentInfo::test(Id::new(id), format!("image-{id}.png"))
    }
}

pub(super) fn video_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
        proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
        content_type: Some("video/mp4".to_owned()),
        size: 78_364_758,
        width: Some(1920),
        height: Some(1080),
        ..AttachmentInfo::test(Id::new(id), format!("clip-{id}.mp4"))
    }
}

pub(super) fn youtube_embed() -> EmbedInfo {
    EmbedInfo {
        color: Some(0xff0000),
        provider_name: Some("YouTube".to_owned()),
        title: Some("Example Video".to_owned()),
        description: Some("A video description".to_owned()),
        url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        thumbnail_width: Some(480),
        thumbnail_height: Some(360),
        ..EmbedInfo::test()
    }
}

pub(super) fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: Some(format!("forwarded {id}")),
        attachments: vec![image_attachment(id)],
        ..MessageSnapshotInfo::test()
    }
}

pub(super) fn message_info(channel_id: Id<ChannelMarker>, message_id: u64) -> MessageInfo {
    MessageInfo {
        guild_id: Some(Id::new(1)),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        content: Some(format!("msg {message_id}")),
        ..MessageInfo::test(channel_id, Id::new(message_id))
    }
}

pub(super) fn poll_info(allow_multiselect: bool) -> PollInfo {
    PollInfo {
        answers: vec![
            PollAnswerInfo {
                vote_count: Some(2),
                me_voted: true,
                ..PollAnswerInfo::test(1, "Soup")
            },
            PollAnswerInfo {
                vote_count: Some(1),
                ..PollAnswerInfo::test(2, "Noodles")
            },
        ],
        allow_multiselect,
        results_finalized: Some(false),
        total_votes: Some(3),
        ..PollInfo::test("What should we eat?")
    }
}

pub(super) fn state_with_two_guilds() -> DashboardState {
    let mut state = DashboardState::new();
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(guild_create_event(guild_id, name, Vec::new()));
    }
    state.push_event(super::user_settings_update(vec![
        GuildFolder {
            id: None,
            name: None,
            color: None,
            guild_ids: vec![first_guild],
        },
        GuildFolder {
            id: None,
            name: None,
            color: None,
            guild_ids: vec![second_guild],
        },
    ]));
    state
}
