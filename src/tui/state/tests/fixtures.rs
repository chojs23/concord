use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::super::{ActiveGuildScope, DashboardState, MessageState};
use crate::discord::{
    AppEvent, AttachmentInfo, ChannelInfo, CustomEmojiInfo, EmbedInfo, GuildFolder, MemberInfo,
    MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PermissionOverwriteInfo,
    PermissionOverwriteKind, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
    RoleInfo,
};

/// Build a guild with a single channel where @everyone keeps
/// VIEW_CHANNEL but loses SEND_MESSAGES — i.e. an announcement-style
/// "read-only" channel that the user can read but not post in.
pub(super) fn state_with_read_only_channel() -> DashboardState {
    guild_state_with_overwrites(vec![PermissionOverwriteInfo {
        id: 1,
        kind: PermissionOverwriteKind::Role,
        allow: 0,
        deny: 0x800,
    }])
}

/// Build a guild with a single channel that is fully hidden — used to
/// verify the visibility stats accounting.
pub(super) fn state_with_view_denied_channel() -> DashboardState {
    guild_state_with_overwrites(vec![PermissionOverwriteInfo {
        id: 1,
        kind: PermissionOverwriteKind::Role,
        allow: 0,
        deny: 0x400,
    }])
}

/// Build a guild with a single channel where @everyone has VIEW + SEND
/// (no overwrites), so the composer should open and submit normally.
pub(super) fn state_with_writable_channel() -> DashboardState {
    guild_state_with_overwrites(Vec::new())
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
                guild_id: Some(guild),
                channel_id: hidden,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "secret".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: vec![PermissionOverwriteInfo {
                    id: guild.get(),
                    kind: PermissionOverwriteKind::Role,
                    allow: 0,
                    deny: 0x400,
                }],
            },
            ChannelInfo {
                guild_id: Some(guild),
                channel_id: visible,
                parent_id: None,
                position: Some(1),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild),
                channel_id: voice,
                parent_id: None,
                position: Some(2),
                last_message_id: None,
                name: "voice".to_owned(),
                kind: "GuildVoice".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: vec![MemberInfo {
            user_id: me,
            display_name: "me".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: Id::new(guild.get()),
            name: "@everyone".to_owned(),
            color: None,
            position: 0,
            hoist: false,
            permissions: 0x400,
        }],
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
            guild_id: Some(guild),
            channel_id: channel,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
            permission_overwrites: overwrites,
        }],
        members: vec![MemberInfo {
            user_id: me,
            display_name: "me".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: Vec::new(),
        roles: vec![RoleInfo {
            id: Id::new(guild.get()),
            name: "@everyone".to_owned(),
            color: None,
            position: 0,
            hoist: false,
            permissions: 0x400 | 0x800, // VIEW + SEND
        }],
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
        channels: vec![ChannelInfo {
            guild_id: Some(guild),
            channel_id: channel,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: vec![
            MemberInfo {
                user_id: me,
                display_name: "me".to_owned(),
                username: Some("me".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(20),
                display_name: "Sally".to_owned(),
                username: Some("salamander".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(21),
                display_name: "Sammy".to_owned(),
                username: Some("sammy42".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(22),
                display_name: "Bob".to_owned(),
                username: Some("bobtheb".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
            MemberInfo {
                user_id: Id::new(23),
                display_name: "Alias".to_owned(),
                username: Some("Alias123".to_owned()),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        ],
        presences: vec![
            (me, PresenceStatus::Online),
            (Id::new(20), PresenceStatus::Online),
            (Id::new(21), PresenceStatus::Online),
            (Id::new(22), PresenceStatus::Online),
            (Id::new(23), PresenceStatus::Online),
        ],
        roles: vec![RoleInfo {
            id: Id::new(guild.get()),
            name: "@everyone".to_owned(),
            color: None,
            position: 0,
            hoist: false,
            permissions: 0x400 | 0x800,
        }],
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
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: name.to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
    }
    state.push_event(AppEvent::GuildFoldersUpdate {
        folders: vec![GuildFolder {
            id: folder_id,
            name: Some("folder".to_owned()),
            color: None,
            guild_ids: vec![first_guild, second_guild],
        }],
    });
    state
}

pub(super) fn state_with_many_guilds(count: u64) -> DashboardState {
    let mut state = DashboardState::new();
    for id in 1..=count {
        state.push_event(AppEvent::GuildCreate {
            guild_id: Id::new(id),
            name: format!("guild {id}"),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
    }
    state
}

pub(super) fn state_with_many_channels(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    let channels = (1..=count)
        .map(|id| ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(id),
            parent_id: None,
            position: Some(id as i32),
            last_message_id: None,
            name: format!("channel {id}"),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
        .collect();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels,
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

pub(super) fn state_with_members(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();
    let members = (1..=count)
        .map(|id| MemberInfo {
            user_id: Id::new(id),
            display_name: format!("member {id}"),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        })
        .collect();
    let presences = (1..=count)
        .map(|id| (Id::new(id), PresenceStatus::Online))
        .collect();

    state.push_event(AppEvent::GuildCreate {
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
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
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
        .map(|id| MemberInfo {
            user_id: Id::new(id),
            display_name: format!("member {id}"),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: (id <= 2).then_some(role_id).into_iter().collect(),
        })
        .collect();

    state.push_event(AppEvent::GuildCreate {
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
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members,
        presences: vec![
            (Id::new(1), PresenceStatus::Online),
            (Id::new(2), PresenceStatus::Online),
            (Id::new(3), PresenceStatus::Offline),
            (Id::new(4), PresenceStatus::Offline),
        ],
        roles: vec![RoleInfo {
            id: role_id,
            name: "Role".to_owned(),
            color: None,
            position: 1,
            hoist: true,
            permissions: 0,
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
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: category_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Text Channels".to_owned(),
                kind: "category".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: general_id,
                parent_id: Some(category_id),
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: random_id,
                parent_id: Some(category_id),
                position: Some(1),
                last_message_id: None,
                name: "random".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
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
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id,
            name: name.to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
    }
    state
}

pub(super) fn state_with_messages(count: u64) -> DashboardState {
    state_with_message_ids(1..=count)
}

pub(super) fn state_with_reaction_message() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            reactions: vec![
                ReactionInfo {
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    count: 2,
                    me: true,
                },
                ReactionInfo {
                    emoji: ReactionEmoji::Custom {
                        id: Id::new(50),
                        name: Some("party".to_owned()),
                        animated: false,
                    },
                    count: 1,
                    me: false,
                },
            ],
            ..message_info(Id::new(2), 1)
        }],
    });
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
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![
            CustomEmojiInfo {
                id: Id::new(50),
                name: "party_time".to_owned(),
                animated: true,
                available: true,
            },
            CustomEmojiInfo {
                id: Id::new(51),
                name: "gone".to_owned(),
                animated: false,
                available: false,
            },
        ],
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

pub(super) fn state_with_single_message_content(content: &str) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
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
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

pub(super) fn state_with_thread_created_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id: Id<ChannelMarker> = Id::new(2);
    let thread_id: Id<ChannelMarker> = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: parent_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                parent_id: Some(parent_id),
                position: None,
                last_message_id: None,
                name: "release notes".to_owned(),
                kind: "thread".to_owned(),
                message_count: Some(12),
                total_message_sent: Some(14),
                thread_archived: Some(false),
                thread_locked: Some(false),
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::new(18),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        reply: None,
        poll: None,
        content: Some("release notes".to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
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
        message_kind: MessageKind::regular(),
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

    state.push_event(AppEvent::GuildCreate {
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
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in message_ids {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {id}")),
            mentions: Vec::new(),
            attachments: has_image(id)
                .then(|| image_attachment(id))
                .into_iter()
                .collect(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }
    state
}

pub(super) fn push_text_message(state: &mut DashboardState, message_id: u64, content: &str) {
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
}

pub(super) fn image_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        id: Id::new(id),
        filename: format!("image-{id}.png"),
        url: format!("https://cdn.discordapp.com/image-{id}.png"),
        proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
        content_type: Some("image/png".to_owned()),
        size: 2048,
        width: Some(640),
        height: Some(480),
        description: None,
    }
}

pub(super) fn video_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        id: Id::new(id),
        filename: format!("clip-{id}.mp4"),
        url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
        proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
        content_type: Some("video/mp4".to_owned()),
        size: 78_364_758,
        width: Some(1920),
        height: Some(1080),
        description: None,
    }
}

pub(super) fn youtube_embed() -> EmbedInfo {
    EmbedInfo {
        color: Some(0xff0000),
        provider_name: Some("YouTube".to_owned()),
        author_name: None,
        title: Some("Example Video".to_owned()),
        description: Some("A video description".to_owned()),
        fields: Vec::new(),
        footer_text: None,
        url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        thumbnail_width: Some(480),
        thumbnail_height: Some(360),
        image_url: None,
        image_width: None,
        image_height: None,
        video_url: None,
    }
}

pub(super) fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: Some(format!("forwarded {id}")),
        mentions: Vec::new(),
        attachments: vec![image_attachment(id)],
        embeds: Vec::new(),
        source_channel_id: None,
        timestamp: None,
    }
}

pub(super) fn message_info(channel_id: Id<ChannelMarker>, message_id: u64) -> MessageInfo {
    MessageInfo {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(format!("msg {message_id}")),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

pub(super) fn poll_info(allow_multiselect: bool) -> PollInfo {
    PollInfo {
        question: "What should we eat?".to_owned(),
        answers: vec![
            PollAnswerInfo {
                answer_id: 1,
                text: "Soup".to_owned(),
                vote_count: Some(2),
                me_voted: true,
            },
            PollAnswerInfo {
                answer_id: 2,
                text: "Noodles".to_owned(),
                vote_count: Some(1),
                me_voted: false,
            },
        ],
        allow_multiselect,
        results_finalized: Some(false),
        total_votes: Some(3),
    }
}

pub(super) fn state_with_two_guilds() -> DashboardState {
    let mut state = DashboardState::new();
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: name.to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
    }
    state.push_event(AppEvent::GuildFoldersUpdate {
        folders: vec![
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
        ],
    });
    state
}
