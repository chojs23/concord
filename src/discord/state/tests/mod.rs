use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use crate::discord::{
    ActivityInfo, ActivityKind, AppEvent, AttachmentInfo, AttachmentUpdate, ChannelInfo,
    ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CurrentVoiceConnectionState, CustomEmojiInfo, DiscordState, EmbedInfo,
    FriendStatus, GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo,
    MessageInteractionInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, MessageState,
    MutualGuildInfo, NotificationLevel, PermissionOverwriteInfo, PermissionOverwriteKind,
    PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo, ReadStateInfo,
    RelationshipInfo, ReplyInfo, RoleInfo, UserProfileInfo, VoiceStateInfo,
};

mod channels;
mod guilds;
mod members;
mod messages;
mod notifications;
mod permissions;
mod profiles;
mod reads;

struct MessageCreateFixture {
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    author_id: Id<UserMarker>,
    author: String,
    author_avatar_url: Option<String>,
    author_is_bot: bool,
    author_role_ids: Vec<Id<RoleMarker>>,
    message_kind: MessageKind,
    interaction: Option<MessageInteractionInfo>,
    reference: Option<MessageReferenceInfo>,
    reply: Option<ReplyInfo>,
    poll: Option<PollInfo>,
    content: Option<String>,
    sticker_names: Vec<String>,
    mentions: Vec<MentionInfo>,
    attachments: Vec<AttachmentInfo>,
    embeds: Vec<EmbedInfo>,
    forwarded_snapshots: Vec<MessageSnapshotInfo>,
}

struct GuildCreateFixture {
    guild_id: Id<GuildMarker>,
    name: String,
    member_count: Option<u64>,
    owner_id: Option<Id<UserMarker>>,
    channels: Vec<ChannelInfo>,
    members: Vec<MemberInfo>,
    presences: Vec<(Id<UserMarker>, PresenceStatus)>,
    roles: Vec<RoleInfo>,
    emojis: Vec<CustomEmojiInfo>,
}

impl GuildCreateFixture {
    fn new(guild_id: Id<GuildMarker>) -> Self {
        Self {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        }
    }
}

impl Default for MessageCreateFixture {
    fn default() -> Self {
        Self {
            guild_id: None,
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_is_bot: false,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            interaction: None,
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }
}

fn message_create_event(event: MessageCreateFixture) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: event.guild_id,
        channel_id: event.channel_id,
        message_id: event.message_id,
        author_id: event.author_id,
        author: event.author,
        author_avatar_url: event.author_avatar_url,
        author_is_bot: event.author_is_bot,
        author_role_ids: event.author_role_ids,
        message_kind: event.message_kind,
        interaction: event.interaction,
        reference: event.reference,
        reply: event.reply,
        poll: event.poll,
        content: event.content,
        sticker_names: event.sticker_names,
        mentions: event.mentions,
        attachments: event.attachments,
        embeds: event.embeds,
        forwarded_snapshots: event.forwarded_snapshots,
    }
}

fn guild_create_event(event: GuildCreateFixture) -> AppEvent {
    AppEvent::GuildCreate {
        guild_id: event.guild_id,
        name: event.name,
        member_count: event.member_count,
        owner_id: event.owner_id,
        channels: event.channels,
        members: event.members,
        presences: event.presences,
        roles: event.roles,
        emojis: event.emojis,
    }
}

fn profile_info(user_id: u64, guild_nick: Option<&str>) -> UserProfileInfo {
    UserProfileInfo {
        user_id: Id::new(user_id),
        username: format!("user-{user_id}"),
        global_name: None,
        guild_nick: guild_nick.map(str::to_owned),
        role_ids: Vec::new(),
        avatar_url: None,
        bio: None,
        pronouns: None,
        mutual_guilds: Vec::<MutualGuildInfo>::new(),
        mutual_friends_count: 0,
        friend_status: FriendStatus::None,
        note: None,
    }
}

fn relationship_info(
    user_id: u64,
    status: FriendStatus,
    nickname: Option<&str>,
    display_name: Option<&str>,
    username: Option<&str>,
) -> RelationshipInfo {
    RelationshipInfo {
        user_id: Id::new(user_id),
        status,
        nickname: nickname.map(str::to_owned),
        display_name: display_name.map(str::to_owned),
        username: username.map(str::to_owned),
    }
}

fn channel_info(
    channel_id: Id<ChannelMarker>,
    kind: impl Into<String>,
    permission_overwrites: Vec<PermissionOverwriteInfo>,
) -> ChannelInfo {
    ChannelInfo {
        permission_overwrites,
        ..ChannelInfo::test(channel_id, kind)
    }
}

fn guild_category_channel(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
    position: i32,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        name: name.into(),
        kind: "category".to_owned(),
        position: Some(position),
        ..channel_info(channel_id, "category", Vec::new())
    }
}

fn guild_child_text_channel(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    parent_id: Id<ChannelMarker>,
    name: impl Into<String>,
    position: i32,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        name: name.into(),
        kind: "text".to_owned(),
        parent_id: Some(parent_id),
        owner_id: None,
        position: Some(position),
        ..channel_info(channel_id, "text", Vec::new())
    }
}

fn dm_channel(channel_id: Id<ChannelMarker>, name: impl Into<String>) -> ChannelInfo {
    ChannelInfo {
        kind: "dm".to_owned(),
        name: name.into(),
        ..channel_info(channel_id, "dm", Vec::new())
    }
}

fn dm_channel_with_recipients(
    channel_id: Id<ChannelMarker>,
    name: impl Into<String>,
    kind: impl Into<String>,
    recipients: Vec<ChannelRecipientInfo>,
) -> ChannelInfo {
    ChannelInfo {
        kind: kind.into(),
        recipients: Some(recipients),
        ..dm_channel(channel_id, name)
    }
}

fn guild_thread_channel(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    parent_id: Id<ChannelMarker>,
    name: impl Into<String>,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        parent_id: Some(parent_id),
        owner_id: None,
        name: name.into(),
        kind: "thread".to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..channel_info(channel_id, "thread", Vec::new())
    }
}

fn guild_voice_channel(guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        kind: "GuildVoice".to_owned(),
        name: "Lobby".to_owned(),
        position: Some(0),
        ..channel_info(channel_id, "GuildVoice", Vec::new())
    }
}

fn member_info(user_id: Id<UserMarker>, display_name: impl Into<String>) -> MemberInfo {
    MemberInfo {
        user_id,
        display_name: display_name.into(),
        username: None,
        is_bot: false,
        avatar_url: None,
        role_ids: Vec::new(),
    }
}

fn member_with_username(user_id: Id<UserMarker>, display_name: &str, username: &str) -> MemberInfo {
    MemberInfo {
        username: Some(username.to_owned()),
        ..member_info(user_id, display_name)
    }
}

fn member_with_roles(
    user_id: Id<UserMarker>,
    display_name: impl Into<String>,
    role_ids: Vec<Id<RoleMarker>>,
) -> MemberInfo {
    MemberInfo {
        role_ids,
        ..member_info(user_id, display_name)
    }
}

fn role_info(role_id: Id<RoleMarker>, name: impl Into<String>, permissions: u64) -> RoleInfo {
    RoleInfo {
        id: role_id,
        name: name.into(),
        color: None,
        position: 0,
        hoist: false,
        permissions,
    }
}

fn voice_state(
    guild_id: Id<GuildMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    user_id: Id<UserMarker>,
) -> VoiceStateInfo {
    VoiceStateInfo {
        guild_id,
        channel_id,
        user_id,
        session_id: None,
        member: None,
        deaf: false,
        mute: false,
        self_deaf: false,
        self_mute: false,
        self_stream: false,
    }
}

fn read_state_info(
    channel_id: Id<ChannelMarker>,
    last_acked_message_id: Option<Id<MessageMarker>>,
    mention_count: u32,
) -> ReadStateInfo {
    ReadStateInfo {
        channel_id,
        last_acked_message_id,
        mention_count,
    }
}

fn latest_history_loaded(channel_id: Id<ChannelMarker>, messages: Vec<MessageInfo>) -> AppEvent {
    AppEvent::MessageHistoryLoaded {
        channel_id,
        before: None,
        messages,
    }
}

fn notification_settings(
    guild_id: Id<GuildMarker>,
    level: NotificationLevel,
) -> GuildNotificationSettingsInfo {
    GuildNotificationSettingsInfo {
        guild_id: Some(guild_id),
        message_notifications: Some(level),
        muted: false,
        mute_end_time: None,
        suppress_everyone: false,
        suppress_roles: false,
        channel_overrides: Vec::new(),
    }
}

fn private_notification_settings(level: NotificationLevel) -> GuildNotificationSettingsInfo {
    GuildNotificationSettingsInfo {
        guild_id: None,
        message_notifications: Some(level),
        muted: false,
        mute_end_time: None,
        suppress_everyone: false,
        suppress_roles: false,
        channel_overrides: Vec::new(),
    }
}

fn message_create(
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    author_id: Id<UserMarker>,
    content: &str,
    mentions: Vec<MentionInfo>,
) -> AppEvent {
    message_create_event(MessageCreateFixture {
        guild_id,
        channel_id,
        message_id,
        author_id,
        content: Some(content.to_owned()),
        mentions,
        ..MessageCreateFixture::default()
    })
}

fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
    MessageInfo {
        guild_id: None,
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageInfo::default()
    }
}

fn message_state(content: &str) -> MessageState {
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

fn attachment_info(id: u64, filename: &str, content_type: &str) -> crate::discord::AttachmentInfo {
    crate::discord::AttachmentInfo {
        id: Id::new(id),
        filename: filename.to_owned(),
        url: format!("https://cdn.discordapp.com/{filename}"),
        proxy_url: format!("https://media.discordapp.net/{filename}"),
        content_type: Some(content_type.to_owned()),
        size: 1000,
        width: Some(100),
        height: Some(100),
        description: None,
    }
}

fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
    MentionInfo {
        user_id: Id::new(user_id),
        guild_nick: None,
        display_name: display_name.to_owned(),
    }
}

fn poll_info() -> PollInfo {
    PollInfo {
        question: "오늘 뭐 먹지?".to_owned(),
        answers: vec![
            PollAnswerInfo {
                answer_id: 1,
                text: "김치찌개".to_owned(),
                vote_count: Some(2),
                me_voted: true,
            },
            PollAnswerInfo {
                answer_id: 2,
                text: "라멘".to_owned(),
                vote_count: Some(1),
                me_voted: false,
            },
        ],
        allow_multiselect: false,
        results_finalized: Some(false),
        total_votes: Some(3),
    }
}

fn snapshot_info(content: &str) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        source_channel_id: None,
        timestamp: None,
    }
}
