use fixtures::*;
use ratatui::text::Line;

use crate::{
    config::{DisplayOptions, ImagePreviewQualityPreset, NotificationOptions, VoiceOptions},
    discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    },
};
use unicode_width::UnicodeWidthStr;

use super::model::{ChannelBranch, GuildBranch};
use super::{
    ActiveGuildScope, ChannelActionKind, ChannelPaneEntry, DashboardState, FocusPane,
    GuildPaneEntry, ImageViewerItem, MessageActionKind,
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, AttachmentInfo, AttachmentUpdate,
    ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CustomEmojiInfo, DiscordState, DownloadAttachmentSource,
    EmbedFieldInfo, EmbedInfo, ForumPostArchiveState, FriendStatus, GuildNotificationSettingsInfo,
    MessageAttachmentUpload, MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo,
    MessageState, MutualGuildInfo, NotificationLevel, PermissionOverwriteInfo,
    PermissionOverwriteKind, PresenceStatus, ReactionEmoji, ReactionInfo, ReactionUserInfo,
    ReactionUsersInfo, ReplyInfo, RoleInfo, SnapshotRevision, UserProfileInfo,
    VoiceConnectionStatus, VoiceStateInfo,
};

mod channel_switcher;
mod composer;
mod direct_messages;
mod emoji_reactions;
mod fixtures;
mod forums;
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
    message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(50),
        author_id: Id::new(99),
        content: Some(content.to_owned()),
        ..MessageCreateFixture::default()
    })
}

fn direct_message_create_event(channel_id: Id<ChannelMarker>, message_id: u64) -> AppEvent {
    message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        content: Some("hello from dm".to_owned()),
        ..MessageCreateFixture::default()
    })
}

fn drain_debounced_read_ack(state: &mut DashboardState) -> Vec<AppCommand> {
    state.drain_pending_commands()
}

fn apply_optimistic_ack_commands<C>(state: &mut DashboardState, commands: &[C])
where
    C: Clone,
    AppCommand: From<C>,
{
    for command in commands {
        match AppCommand::from(command.clone()) {
            AppCommand::AckChannel {
                channel_id,
                message_id,
            }
            | AppCommand::ScheduleAckChannel {
                channel_id,
                message_id,
            } => state.push_event(AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count: 0,
            }),
            AppCommand::AckChannels { targets } => {
                for (channel_id, message_id) in targets {
                    state.push_event(AppEvent::MessageAck {
                        channel_id,
                        message_id,
                        mention_count: 0,
                    });
                }
            }
            _ => {}
        }
    }
}

fn clear_scheduled_read_ack(state: &mut DashboardState) {
    state.drain_pending_commands();
}

fn push_reply_message_with_attachments(
    state: &mut DashboardState,
    message_id: u64,
    author_id: u64,
    content: Option<&str>,
    attachments: Vec<AttachmentInfo>,
) {
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(message_id),
        author_id: Id::new(author_id),
        author: format!("user-{author_id}"),
        message_kind: MessageKind::new(19),
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
        content: content.map(str::to_owned),
        attachments,
        ..MessageCreateFixture::default()
    }));
}

fn state_with_thread_created_message_after_regular_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
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
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        content: Some("older parent message ".repeat(20)),
        ..MessageCreateFixture::default()
    }));
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(2),
        author_id: Id::new(99),
        message_kind: MessageKind::new(18),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        content: Some("release notes ".repeat(20)),
        ..MessageCreateFixture::default()
    }));
    state
}

fn state_with_forum_channel_posts() -> DashboardState {
    state_with_many_forum_channel_posts(2)
}

fn forum_channel_info(guild_id: Id<GuildMarker>, forum_id: Id<ChannelMarker>) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        position: Some(0),
        name: "announcements".to_owned(),
        ..ChannelInfo::test(forum_id, "forum")
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
        parent_id: Some(forum_id),
        last_message_id: last_message_id.map(Id::<MessageMarker>::new),
        name: name.to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(archived, false)),
        ..ChannelInfo::test(Id::new(channel_id), "GuildPublicThread")
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

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![forum_channel_info(guild_id, forum_id)],
    ));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Discord's `/threads/search` returns threads newest-first, so emit them
    // in reverse channel-id order to match what the live API would deliver.
    let threads: Vec<_> = (0..count)
        .rev()
        .map(|index| ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: Id::new(30 + index),
            parent_id: Some(forum_id),
            owner_id: None,
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
            member_count: None,
            total_message_sent: Some(index + 1),
            thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
            flags: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        })
        .collect();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: ForumPostArchiveState::Active,
        offset: 0,
        next_offset: threads.len(),
        threads,
        first_messages: Vec::new(),
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
            category_channel_info(guild_id, category_id, "Channels", 0),
            ChannelInfo {
                parent_id: Some(category_id),
                owner_id: None,
                ..voice_channel_info(guild_id, voice_id, "Lobby")
            },
            child_text_channel_info(guild_id, text_id, category_id, "general", 1),
        ],
        members: vec![member_with_username(alice, "Alice", "alice")],
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice_id), alice),
    });
    state.confirm_selected_guild();
    state
}
