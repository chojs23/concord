use std::collections::BTreeMap;

use fixtures::*;
use ratatui::text::Line;

use crate::{
    config::{DisplayOptions, NotificationOptions, UiStateOptions, VoiceOptions},
    discord::ids::{
        Id,
        marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
    },
};
use unicode_width::UnicodeWidthStr;

use super::model::{ChannelBranch, GuildBranch};
use super::{
    ActiveGuildScope, AttachmentViewerItem, ChannelActionKind, ChannelPaneEntry, ComposerLock,
    DashboardState, FocusPane, GuildActionKind, GuildPaneEntry, MessageActionItem,
    MessageActionKind, SearchResultItem,
};
use crate::discord::test_builders::{MessageAckFixture, message_ack_event};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, AppEvent, AttachmentInfo, ChannelInfo,
    ChannelNotificationOverrideInfo, ChannelRecipientInfo, ChannelUnreadState,
    ChannelVisibilityStats, CustomEmojiInfo, DiscordState, DownloadAttachmentSource,
    EmbedFieldInfo, EmbedInfo, ForumTagInfo, GuildFolder, GuildMemberListItem,
    GuildMemberListOperation, GuildMemberListUpdateInfo, GuildNotificationSettingsInfo,
    MESSAGE_FLAG_IS_COMPONENTS_V2, MessageComponentInfo, MessageInfo, MessageKind,
    MessageReferenceInfo, MessageSearchPage, MessageSnapshotInfo, MessageState,
    MessageUpdateDispatchInfo, MessageUpdateEventFields, NotificationLevel,
    PermissionOverwriteInfo, PermissionOverwriteKind, PremiumTier, PresenceEventFields,
    PresenceStatus, ReactionEmoji, ReactionInfo, ReactionUserInfo, ReplyInfo, RoleInfo,
    SnapshotRevision, UserGuildSettingsInfo, UserProfileInfo, UserSettingsInfo,
    VoiceConnectionStatus, VoiceStateInfo,
};

macro_rules! assert_send_message_eq {
    ($actual:expr, $expected:expr $(, $($message:tt)+)?) => {{
        let mut actual = $actual;
        match &mut actual {
            Some(AppCommand::SendMessage { nonce, .. })
            | Some(AppCommand::SendTtsMessage { nonce, .. }) => *nonce = Id::new(1),
            _ => {}
        }
        assert_eq!(actual, $expected $(, $($message)+)?);
    }};
}

mod channel_switcher;
mod composer;
mod direct_messages;
mod emoji_reactions;
mod fixtures;
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
mod threads;

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
            groups: Vec::new(),
            ops: Vec::new(),
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
            } => state.push_event(message_ack_event(MessageAckFixture {
                channel_id,
                message_id,
                ..MessageAckFixture::new()
            })),
            AppCommand::AckChannels { targets } => {
                for (channel_id, message_id) in targets {
                    state.push_event(message_ack_event(MessageAckFixture {
                        channel_id,
                        message_id,
                        ..MessageAckFixture::new()
                    }));
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

    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
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
            ..GuildCreateFixture::new(guild_id)
        },
    ));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: voice_state(guild_id, Some(voice_id), alice),
    });
    state.confirm_selected_guild();
    state
}
