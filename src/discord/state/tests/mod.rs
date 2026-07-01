use std::collections::BTreeMap;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use crate::discord::test_builders::{
    ChannelPinsUpdateFixture, CurrentUserPollVoteUpdateFixture, CurrentUserReactionAddFixture,
    CurrentUserReactionRemoveFixture, GuildCreateFixture, GuildUpdateFixture, MessageAckFixture,
    MessageCreateFixture, MessageDeleteBulkFixture, MessageHistoryAfterLoadedFixture,
    MessageHistoryAroundLoadedFixture, MessageHistoryLoadedFixture, MessagePinnedUpdateFixture,
    MessageReactionAddFixture, MessageReactionRemoveAllFixture, MessageReactionRemoveEmojiFixture,
    MessageReactionRemoveFixture, UserIdentityUpdateFixture, VoiceSpeakingUpdateFixture,
    channel_pins_update_event, current_user_poll_vote_update_event,
    current_user_reaction_add_event, current_user_reaction_remove_event, guild_create_event,
    guild_update_event, message_ack_event, message_create_event, message_delete_bulk_event,
    message_history_after_loaded_event, message_history_around_loaded_event,
    message_history_loaded_event, message_pinned_update_event, message_reaction_add_event,
    message_reaction_remove_all_event, message_reaction_remove_emoji_event,
    message_reaction_remove_event, user_identity_update_event, voice_speaking_update_event,
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppEvent, AttachmentUpdate, BASE_ATTACHMENT_LIMIT_BYTES,
    ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CurrentVoiceConnectionState, CustomEmojiInfo, DiscordState,
    FriendStatus, GuildBoostTier, GuildNotificationSettingsInfo, MemberInfo, MentionInfo,
    MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, MessageState,
    MessageUpdateDispatchInfo, MessageUpdateEventFields, NotificationLevel,
    PermissionOverwriteInfo, PermissionOverwriteKind, PollAnswerInfo, PollInfo, PremiumTier,
    PresenceStatus, ReactionEmoji, ReactionInfo, ReadStateInfo, RelationshipInfo, ReplyInfo,
    RoleInfo, UserGuildSettingsInfo, UserProfileInfo, VoiceStateInfo,
};

mod channels;
mod guilds;
mod members;
mod messages;
mod notifications;
mod permissions;
mod profiles;
mod reads;
mod upload_limits;

fn profile_info(user_id: u64, guild_nick: Option<&str>) -> UserProfileInfo {
    UserProfileInfo {
        guild_nick: guild_nick.map(str::to_owned),
        ..UserProfileInfo::test(Id::new(user_id), format!("user-{user_id}"))
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
    MemberInfo::test(user_id, display_name)
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
        permissions,
        ..RoleInfo::test(role_id, name)
    }
}

fn voice_state(
    guild_id: Id<GuildMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    user_id: Id<UserMarker>,
) -> VoiceStateInfo {
    VoiceStateInfo::test(guild_id, channel_id, user_id)
}

fn read_state_info(
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

fn latest_history_loaded(channel_id: Id<ChannelMarker>, messages: Vec<MessageInfo>) -> AppEvent {
    message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        messages,
        ..MessageHistoryLoadedFixture::new()
    })
}

fn notification_settings(
    guild_id: Id<GuildMarker>,
    level: NotificationLevel,
) -> GuildNotificationSettingsInfo {
    GuildNotificationSettingsInfo {
        message_notifications: Some(level),
        ..GuildNotificationSettingsInfo::test(Some(guild_id))
    }
}

fn private_notification_settings(level: NotificationLevel) -> GuildNotificationSettingsInfo {
    GuildNotificationSettingsInfo {
        message_notifications: Some(level),
        ..GuildNotificationSettingsInfo::test(None)
    }
}

fn user_guild_settings_init(settings: Vec<GuildNotificationSettingsInfo>) -> AppEvent {
    AppEvent::UserGuildSettingsInit {
        settings: settings
            .into_iter()
            .map(|notification_settings| UserGuildSettingsInfo {
                notification_settings,
                extra_fields: BTreeMap::new(),
            })
            .collect(),
    }
}

fn message_update_event(
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    fields: MessageUpdateEventFields,
) -> AppEvent {
    AppEvent::MessageUpdateDispatch {
        update: MessageUpdateDispatchInfo {
            guild_id: None,
            channel_id,
            message_id,
            fields,
            extra_fields: BTreeMap::new(),
        },
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
        ..MessageCreateFixture::test_fixture_default()
    })
}

fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
    MessageInfo {
        guild_id: None,
        author_id: Id::new(99),
        author: "neo".to_owned(),
        content: Some(content.to_owned()),
        ..MessageInfo::test(channel_id, Id::new(message_id))
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
    MentionInfo::test(Id::new(user_id), display_name.to_owned())
}

fn poll_info() -> PollInfo {
    PollInfo {
        answers: vec![
            PollAnswerInfo {
                vote_count: Some(2),
                me_voted: true,
                ..PollAnswerInfo::test(1, "김치찌개")
            },
            PollAnswerInfo {
                vote_count: Some(1),
                ..PollAnswerInfo::test(2, "라멘")
            },
        ],
        results_finalized: Some(false),
        total_votes: Some(3),
        ..PollInfo::test("오늘 뭐 먹지?")
    }
}

fn snapshot_info(content: &str) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: Some(content.to_owned()),
        ..MessageSnapshotInfo::test()
    }
}
