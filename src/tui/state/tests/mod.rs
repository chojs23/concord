use std::collections::BTreeMap;

use fixtures::*;
use ratatui::text::Line;

use crate::{
    config::{DisplayOptions, ImagePreviewQualityPreset, NotificationOptions, VoiceOptions},
    discord::ids::{
        Id,
        marker::{
            ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
        },
    },
};
use unicode_width::UnicodeWidthStr;

use super::model::{ChannelBranch, GuildBranch};
use super::{
    ActiveGuildScope, AttachmentViewerItem, ChannelActionKind, ChannelPaneEntry, DashboardState,
    DmComposerLock, FocusPane, GuildActionKind, GuildPaneEntry, MessageActionItem,
    MessageActionKind, SearchResultItem,
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, AttachmentInfo, ChannelInfo,
    ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CustomEmojiInfo, DiscordState, DownloadAttachmentSource,
    EmbedFieldInfo, EmbedInfo, ForumPostArchiveState, ForumTagInfo, GuildFolder,
    GuildMemberListUpdateInfo, GuildNotificationSettingsInfo, MessageInfo, MessageKind,
    MessageReferenceInfo, MessageSearchPage, MessageSnapshotInfo, MessageState,
    MessageUpdateDispatchInfo, MessageUpdateEventFields, NotificationLevel,
    PermissionOverwriteInfo, PermissionOverwriteKind, PresenceStatus, ReactionEmoji, ReactionInfo,
    ReactionUserInfo, ReactionUsersInfo, ReplyInfo, RoleInfo, SnapshotRevision,
    ThreadMembersUpdateInfo, UserGuildSettingsInfo, UserProfileInfo, UserSettingsInfo,
    VoiceConnectionStatus, VoiceStateInfo,
};

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
mod search;

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
        guild_nick: guild_nick.map(str::to_owned),
        ..UserProfileInfo::test(Id::new(user_id), format!("user-{user_id}"))
    }
}

fn notification_message_event(channel_id: Id<ChannelMarker>, content: &str) -> AppEvent {
    message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(50),
        author_id: Id::new(99),
        content: Some(content.to_owned()),
        ..guild_message_create_fixture()
    })
}

fn direct_message_create_event(channel_id: Id<ChannelMarker>, message_id: u64) -> AppEvent {
    message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(message_id),
        author_id: Id::new(99),
        content: Some("hello from dm".to_owned()),
        ..guild_message_create_fixture()
    })
}

fn user_settings_update(folders: Vec<GuildFolder>) -> AppEvent {
    AppEvent::UserSettingsUpdate {
        settings: UserSettingsInfo {
            guild_folders: Some(folders),
            ..UserSettingsInfo::default()
        },
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

fn guild_member_list_counts_event(guild_id: Id<GuildMarker>, online: u32) -> AppEvent {
    AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: None,
            member_count: None,
            online_count: Some(online),
            members: Vec::new(),
            presences: Vec::new(),
            groups: Vec::new(),
            ops: Vec::new(),
            extra_fields: BTreeMap::new(),
        },
    }
}

fn thread_members_update_event(
    channel_id: Id<ChannelMarker>,
    added_user_ids: Vec<Id<UserMarker>>,
    removed_user_ids: Vec<Id<UserMarker>>,
) -> AppEvent {
    AppEvent::ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo {
            guild_id: None,
            channel_id,
            member_count: None,
            added_members: Vec::new(),
            added_user_ids,
            removed_user_ids,
            extra_fields: BTreeMap::new(),
        },
    }
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
            ..MessageReferenceInfo::test(Id::new(42))
        }),
        reply: Some(ReplyInfo {
            content: Some("original message".to_owned()),
            ..ReplyInfo::test("original")
        }),
        content: content.map(str::to_owned),
        attachments,
        ..guild_message_create_fixture()
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
        ..guild_message_create_fixture()
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
        ..guild_message_create_fixture()
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
        author_id: Id::new(99),
        author: author.to_owned(),
        content: Some(content.to_owned()),
        ..MessageInfo::test(channel_id, Id::new(message_id))
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
            parent_id: Some(forum_id),
            position: Some(i32::try_from(index).expect("test index fits i32")),
            name: if count == 2 && index == 0 {
                "welcome".to_owned()
            } else if count == 2 && index == 1 {
                "release notes".to_owned()
            } else {
                format!("post {}", index + 1)
            },
            message_count: Some(index + 1),
            total_message_sent: Some(index + 1),
            thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(30 + index), "GuildPublicThread")
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
            ChannelPaneEntry::Channel { state, .. } | ChannelPaneEntry::Thread { state, .. } => {
                Some(state.name.as_str())
            }
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
