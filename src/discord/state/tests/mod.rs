use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use crate::discord::{
    ActivityInfo, ActivityKind, AppEvent, AttachmentUpdate, ChannelInfo,
    ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CustomEmojiInfo, DiscordState, FriendStatus,
    GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo, MessageKind,
    MessageReferenceInfo, MessageSnapshotInfo, MessageState, MutualGuildInfo, NotificationLevel,
    PermissionOverwriteInfo, PermissionOverwriteKind, PollAnswerInfo, PollInfo, PresenceStatus,
    ReactionEmoji, ReactionInfo, ReadStateInfo, RelationshipInfo, ReplyInfo, RoleInfo,
    UserProfileInfo,
};

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

fn guild_text_channel(guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
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
    }
}

fn private_channel(channel_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "dm".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
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
    AppEvent::MessageCreate {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions,
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

mod channels;
mod guilds;
mod members;
mod messages;
mod notifications;
mod permissions;
mod profiles;
mod reads;

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
