mod channel_switcher;
mod composer;
mod direct_messages;
mod emoji_reactions;
mod fixtures;
mod forums;
mod leader_actions;
mod members;
mod message_actions;
mod message_layout;
mod message_viewport;
mod notifications;
mod options_voice;
mod panes;
mod pinned_threads;
mod profiles;
mod read_state;

use fixtures::*;
use ratatui::text::Line;

use crate::{
    config::{DisplayOptions, ImagePreviewQualityPreset, NotificationOptions, VoiceOptions},
    discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
};
use unicode_width::UnicodeWidthStr;

use super::{
    ActiveGuildScope, ChannelActionKind, ChannelBranch, ChannelPaneEntry, DashboardState,
    FocusPane, GuildActionKind, GuildBranch, GuildPaneEntry, ImageViewerItem, MessageActionKind,
    MessageState,
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, AttachmentInfo, AttachmentUpdate,
    ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CustomEmojiInfo, DiscordState, DownloadAttachmentSource,
    EmbedFieldInfo, EmbedInfo, ForumPostArchiveState, FriendStatus, GuildNotificationSettingsInfo,
    MemberInfo, MessageAttachmentUpload, MessageInfo, MessageKind, MessageReferenceInfo,
    MessageSnapshotInfo, MutualGuildInfo, NotificationLevel, PermissionOverwriteInfo,
    PermissionOverwriteKind, PresenceStatus, ReactionEmoji, ReactionInfo, ReactionUserInfo,
    ReactionUsersInfo, ReadStateInfo, ReplyInfo, RoleInfo, SnapshotRevision, UserProfileInfo,
    VoiceConnectionStatus, VoiceStateInfo,
};

fn message_rendered_height(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    DashboardState::new().message_rendered_height(
        message,
        content_width,
        preview_width,
        max_preview_height,
    )
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

fn notification_message_event(channel_id: Id<ChannelMarker>, content: &str) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(50),
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
        content: Some(content.to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

fn direct_message_create_event(channel_id: Id<ChannelMarker>, message_id: u64) -> AppEvent {
    AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(message_id),
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
        content: Some("hello from dm".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    }
}

fn drain_debounced_read_ack(state: &mut DashboardState) -> Vec<AppCommand> {
    let deadline = state
        .next_read_ack_deadline()
        .expect("read ack should be scheduled");
    state.flush_due_read_acks(deadline);
    state.drain_pending_commands()
}

fn clear_scheduled_read_ack(state: &mut DashboardState) {
    if let Some(deadline) = state.next_read_ack_deadline() {
        state.flush_due_read_acks(deadline);
        state.drain_pending_commands();
    }
}

fn push_reply_message_with_attachments(
    state: &mut DashboardState,
    message_id: u64,
    author_id: u64,
    content: Option<&str>,
    attachments: Vec<AttachmentInfo>,
) {
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(message_id),
        author_id: Id::new(author_id),
        author: format!("user-{author_id}"),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::new(19),
        interaction: None,
        reference: Some(MessageReferenceInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Some(Id::new(2)),
            message_id: Some(Id::new(42)),
        }),
        reply: Some(ReplyInfo {
            author_id: None,
            author: "original".to_owned(),
            content: Some("original message".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
        }),
        poll: None,
        content: content.map(str::to_owned),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments,
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
}

fn state_with_thread_created_message_after_regular_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
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
                thread_pinned: None,
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
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
        content: Some("older parent message ".repeat(20)),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::new(18),
        interaction: None,
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        reply: None,
        poll: None,
        content: Some("release notes ".repeat(20)),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_forum_channel_posts() -> DashboardState {
    state_with_many_forum_channel_posts(2)
}

fn forum_channel_info(guild_id: Id<GuildMarker>, forum_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        channel_id: forum_id,
        parent_id: None,
        position: Some(0),
        last_message_id: None,
        name: "announcements".to_owned(),
        kind: "forum".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }
}

fn forum_thread_info(
    guild_id: Id<GuildMarker>,
    forum_id: Id<ChannelMarker>,
    channel_id: u64,
    name: &str,
    last_message_id: Option<u64>,
    archived: bool,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        channel_id: Id::new(channel_id),
        parent_id: Some(forum_id),
        position: None,
        last_message_id: last_message_id.map(Id::<MessageMarker>::new),
        name: name.to_owned(),
        kind: "GuildPublicThread".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: Some(archived),
        thread_locked: Some(false),
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }
}

fn forum_preview_message(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    message_id: u64,
    author: &str,
    content: &str,
) -> MessageInfo {
    MessageInfo {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        author: author.to_owned(),
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

fn state_with_many_forum_channel_posts(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: forum_id,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "announcements".to_owned(),
            kind: "forum".to_owned(),
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
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Discord's `/threads/search` returns posts newest-first, so emit them in
    // reverse channel-id order to match what the live API would deliver.
    let posts: Vec<_> = (0..count)
        .rev()
        .map(|index| ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(30 + index),
            parent_id: Some(forum_id),
            position: Some(i32::try_from(index).expect("test index fits i32")),
            last_message_id: None,
            name: if count == 2 && index == 0 {
                "welcome".to_owned()
            } else if count == 2 && index == 1 {
                "release notes".to_owned()
            } else {
                format!("post {}", index + 1)
            },
            kind: "GuildPublicThread".to_owned(),
            message_count: Some(index + 1),
            total_message_sent: Some(index + 1),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
        .collect();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: posts.len(),
        posts,
        preview_messages: Vec::new(),
        has_more: false,
    });
    state
}

fn channel_entry_names(state: &DashboardState) -> Vec<&str> {
    state
        .channel_pane_entries()
        .into_iter()
        .filter_map(|entry| match entry {
            ChannelPaneEntry::Channel { state, .. } => Some(state.name.as_str()),
            ChannelPaneEntry::CategoryHeader { .. } | ChannelPaneEntry::VoiceParticipant { .. } => {
                None
            }
        })
        .collect()
}

fn state_with_voice_channel_participant() -> DashboardState {
    let guild_id = Id::new(1);
    let category_id = Id::new(10);
    let voice_id = Id::new(11);
    let text_id = Id::new(12);
    let alice = Id::new(20);
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
                name: "Channels".to_owned(),
                kind: "category".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: voice_id,
                parent_id: Some(category_id),
                position: Some(0),
                last_message_id: None,
                name: "Lobby".to_owned(),
                kind: "GuildVoice".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: text_id,
                parent_id: Some(category_id),
                position: Some(1),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: vec![MemberInfo {
            user_id: alice,
            display_name: "Alice".to_owned(),
            username: Some("alice".to_owned()),
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            guild_id,
            channel_id: Some(voice_id),
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
    state.confirm_selected_guild();
    state
}
