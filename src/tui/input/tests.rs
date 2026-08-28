use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::test_builders::{
    CurrentUserReactionAddFixture, GuildCreateFixture, MessageCreateFixture,
    MessageHistoryAfterLoadedFixture, MessageHistoryAroundLoadedFixture,
    MessageHistoryLoadedFixture, MessagePinnedUpdateFixture, ReactionUsersLoadedFixture,
    VoiceConnectionStatusChangedFixture, current_user_reaction_add_event,
    empty_latest_message_history_loaded_event, guild_create_event, guild_message_create_fixture,
    message_create_event, message_history_after_loaded_event, message_history_around_loaded_event,
    message_history_loaded_event, message_pinned_update_event, reaction_users_loaded_event,
    voice_connection_status_changed_event,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::{MouseClickTracker, handle_key, handle_mouse, handle_mouse_event, handle_paste};
use crate::discord::AppCommand;
use crate::{
    config::{AppOptions, DisplayOptions, ImagePreviewQualityPreset, KeymapBinding, KeymapOptions},
    discord::{
        ActivityInfo, AppEvent, ApplicationCommandInfo, ApplicationCommandOptionInfo,
        AttachmentDownloadId, ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo,
        CustomEmojiInfo, DownloadAttachmentSource, EmbedInfo, GuildFolder, GuildMemberListItem,
        GuildMemberListOperation, GuildMemberListUpdateInfo, GuildNotificationSettingsInfo,
        MemberInfo, MessageInfo, MessageReferenceInfo, MessageSnapshotInfo,
        MicrophoneSensitivityDb, NotificationLevel, PollAnswerInfo, PollInfo, PresenceEventFields,
        PresenceStatus, ReactionEmoji, ReactionUserInfo, ReadStateInfo, RoleInfo,
        UserGuildSettingsInfo, UserSettingsInfo, VoiceConnectionStatus, VoiceVolumePercent,
    },
    tui::state::{
        ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MessageActionKind,
        SelectablePopupTarget,
    },
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

mod composer;
mod leader;
mod messages;
mod misc;
mod mouse;
mod navigation;
mod options;

const PERM_VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const PERM_ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
const PERM_SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const PERM_SEND_TTS_MESSAGES: u64 = 0x0000_0000_0000_1000;
const PERM_MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
const PERM_ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
const PERM_READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
const PERM_USE_EXTERNAL_EMOJIS: u64 = 0x0000_0000_0004_0000;
const PERM_USE_APPLICATION_COMMANDS: u64 = 0x0000_0000_8000_0000;
const PERM_MANAGE_THREADS: u64 = 0x0000_0004_0000_0000;
const PERM_SEND_MESSAGES_IN_THREADS: u64 = 0x0000_0040_0000_0000;
const PERM_PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;

// General input fixtures exercise several message actions. Keep the required
// permissions explicit so channel overwrites still affect each action in tests.
const MESSAGE_TEST_PERMISSIONS: u64 = PERM_VIEW_CHANNEL
    | PERM_ADD_REACTIONS
    | PERM_SEND_MESSAGES
    | PERM_SEND_TTS_MESSAGES
    | PERM_MANAGE_MESSAGES
    | PERM_ATTACH_FILES
    | PERM_READ_MESSAGE_HISTORY
    | PERM_USE_EXTERNAL_EMOJIS
    | PERM_USE_APPLICATION_COMMANDS
    | PERM_MANAGE_THREADS
    | PERM_SEND_MESSAGES_IN_THREADS
    | PERM_PIN_MESSAGES;

fn message_test_guild_fixture(
    guild_id: Id<GuildMarker>,
    current_user_id: Id<UserMarker>,
    channels: Vec<ChannelInfo>,
    permissions: u64,
) -> GuildCreateFixture {
    GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(Id::new(99)),
        channels,
        members: vec![MemberInfo::test(current_user_id, "me")],
        roles: vec![RoleInfo {
            permissions,
            ..RoleInfo::test(Id::new(guild_id.get()), "@everyone")
        }],
        ..GuildCreateFixture::new(guild_id)
    }
}

fn push_test_ready(state: &mut DashboardState, current_user_id: Id<UserMarker>) {
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(current_user_id),
    });
}

fn select_test_guild(state: &mut DashboardState, guild_id: Id<GuildMarker>) {
    let row = state
        .guild_pane_entries()
        .iter()
        .position(
            |entry| matches!(entry, GuildPaneEntry::Guild { state, .. } if state.id == guild_id),
        )
        .expect("fixture guild is visible");
    assert!(state.select_visible_pane_row(FocusPane::Guilds, row));
    assert!(state.confirm_selected_guild());
}

fn select_test_channel(state: &mut DashboardState, channel_id: Id<ChannelMarker>) {
    let row = state
        .channel_pane_entries()
        .iter()
        .position(|entry| entry.channel_id() == Some(channel_id))
        .expect("fixture channel is visible");
    assert!(state.select_visible_pane_row(FocusPane::Channels, row));
    state.confirm_selected_channel();
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn char_key(value: char) -> KeyEvent {
    key(KeyCode::Char(value))
}

fn ctrl_key(value: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(value), KeyModifiers::CONTROL)
}

fn shift_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
}

fn ctrl_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)
}

fn alt_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
}

fn alt_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn channel_row_point(row: u16) -> (u16, u16) {
    (21, 3 + row)
}

fn composer_point() -> (u16, u16) {
    (50, 16)
}

fn message_action_row_point(item_count: u16, row: u16) -> (u16, u16) {
    let popup_top = dashboard_area().height.saturating_sub(item_count + 2) / 2;
    (46, popup_top + 1 + row)
}

fn dashboard_area() -> Rect {
    Rect::new(0, 0, 120, 20)
}

fn state_with_keymap(keymap: KeymapOptions) -> DashboardState {
    DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        keymap,
        Default::default(),
    )
}

fn temp_upload_file(name: &str, contents: &[u8]) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("concord-{unique}"));
    fs::create_dir_all(&directory).expect("temp upload directory can be created");
    let path = directory.join(name);
    fs::write(&path, contents).expect("temp upload file can be written");
    path
}

fn remove_temp_upload_file(path: &PathBuf) {
    let directory = path.parent().map(std::path::Path::to_path_buf);
    let _ = fs::remove_file(path);
    if let Some(directory) = directory {
        let _ = fs::remove_dir(directory);
    }
}

fn state_with_folder() -> DashboardState {
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    let mut state = DashboardState::new();

    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(guild_create_event(GuildCreateFixture {
            name: name.to_owned(),
            ..GuildCreateFixture::new(guild_id)
        }));
    }
    state.push_event(AppEvent::UserSettingsUpdate {
        settings: UserSettingsInfo {
            guild_folders: Some(vec![GuildFolder {
                id: Some(42),
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }]),
            ..UserSettingsInfo::default()
        },
    });
    state
}
fn assert_selected_folder_collapsed(state: &DashboardState, expected: bool) {
    let entries = state.guild_pane_entries();
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader { collapsed, .. } if collapsed == expected
    ));
}

fn assert_selected_channel_category_collapsed(state: &DashboardState, expected: bool) {
    let entries = state.channel_pane_entries();
    assert!(matches!(
        &entries[0],
        ChannelPaneEntry::CategoryHeader { collapsed, .. } if *collapsed == expected
    ));
}

fn state_with_channel_tree() -> DashboardState {
    state_with_channel_tree_from_state(DashboardState::new())
}

fn state_with_channel_tree_from_state(mut state: DashboardState) -> DashboardState {
    let guild_id = Id::new(1);
    let category_id = Id::new(10);
    let general_id = Id::new(11);
    let random_id = Id::new(12);
    let current_user_id = Id::new(20);
    push_test_ready(&mut state, current_user_id);
    state.push_event(guild_create_event(message_test_guild_fixture(
        guild_id,
        current_user_id,
        vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                position: Some(0),
                name: "Text Channels".to_owned(),
                ..ChannelInfo::test(category_id, "category")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(category_id),
                position: Some(0),
                name: "general".to_owned(),
                last_message_id: Some(Id::new(1)),
                message_count: Some(1),
                ..ChannelInfo::test(general_id, "text")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(category_id),
                position: Some(1),
                name: "random".to_owned(),
                last_message_id: Some(Id::new(1)),
                message_count: Some(1),
                ..ChannelInfo::test(random_id, "text")
            },
        ],
        MESSAGE_TEST_PERMISSIONS,
    )));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                last_acked_message_id: Some(Id::new(1)),
                ..ReadStateInfo::test(general_id)
            },
            ReadStateInfo {
                last_acked_message_id: Some(Id::new(1)),
                ..ReadStateInfo::test(random_id)
            },
        ],
    });
    for channel_id in [general_id, random_id] {
        state.push_event(empty_latest_message_history_loaded_event(channel_id));
    }
    select_test_guild(&mut state, guild_id);
    state
}

fn state_with_direct_message(kind: &str) -> DashboardState {
    let channel_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        name: "alice".to_owned(),
        recipients: Some(vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::Online),
            ..ChannelRecipientInfo::test(Id::new(30), "alice")
        }]),
        ..ChannelInfo::test(channel_id, kind)
    }));
    state.confirm_selected_guild();
    state
}

fn state_with_messages(count: u64) -> DashboardState {
    state_with_messages_from_state(DashboardState::new(), count)
}

fn state_with_channel_permissions(permissions: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);
    let mut state = DashboardState::new();
    push_test_ready(&mut state, current_user_id);
    state.push_event(guild_create_event(message_test_guild_fixture(
        guild_id,
        current_user_id,
        vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        permissions,
    )));
    select_test_guild(&mut state, guild_id);
    select_test_channel(&mut state, channel_id);
    state.push_event(message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        messages: vec![MessageInfo {
            guild_id: Some(guild_id),
            content: Some("message".to_owned()),
            ..MessageInfo::test(channel_id, Id::new(1))
        }],
        ..MessageHistoryLoadedFixture::new()
    }));
    state
}

fn push_guild_message(state: &mut DashboardState, message_id: u64, content: impl Into<String>) {
    state.push_event(message_create_event(guild_text_message(
        message_id, content,
    )));
}

fn guild_text_message(message_id: u64, content: impl Into<String>) -> MessageCreateFixture {
    MessageCreateFixture::guild_message(Id::new(1), Id::new(2), Id::new(message_id))
        .with_content(content)
}

fn state_with_messages_from_state(mut state: DashboardState, count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let current_user_id = Id::new(10);

    push_test_ready(&mut state, current_user_id);
    state.push_event(guild_create_event(message_test_guild_fixture(
        guild_id,
        current_user_id,
        vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        MESSAGE_TEST_PERMISSIONS,
    )));
    select_test_guild(&mut state, guild_id);
    select_test_channel(&mut state, channel_id);
    for id in 1..=count {
        push_guild_message(&mut state, id, format!("msg {id}"));
    }
    state.push_event(empty_latest_message_history_loaded_event(channel_id));
    state
}

fn state_with_own_message() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state
}

fn state_with_members(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();
    let members = (1..=count)
        .map(|id| MemberInfo::test(Id::new(id), format!("member {id}")))
        .collect();
    let presences = (1..=count)
        .map(|id| PresenceEventFields {
            user_id: Id::new(id),
            status: PresenceStatus::Online,
            activities: Vec::new(),
        })
        .collect();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members,
        presences,
        ..GuildCreateFixture::new(guild_id)
    }));
    state.push_event(AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: Some("everyone".to_owned()),
            member_count: None,
            online_count: None,
            groups: Vec::new(),
            ops: vec![GuildMemberListOperation::Sync {
                range: (0, 99),
                items: std::iter::once(GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count,
                })
                .chain((1..=count).map(|id| GuildMemberListItem::Member {
                    member: MemberInfo::test(Id::new(id), format!("member {id}")),
                    presence: None,
                }))
                .collect(),
            }],
            extra_fields: BTreeMap::new(),
        },
    });
    state.confirm_selected_guild();
    state
}

fn state_with_thread_created_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                ..ChannelInfo::test(parent_id, "GuildText")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(parent_id),
                name: "release notes".to_owned(),
                message_count: Some(12),
                total_message_sent: Some(14),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(thread_id, "thread")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(
        MessageCreateFixture::guild_message(guild_id, parent_id, Id::new(1))
            .with_message_kind(crate::discord::MessageKind::new(18))
            .with_reference(MessageReferenceInfo {
                guild_id: Some(guild_id),
                channel_id: Some(thread_id),
                message_id: None,
            })
            .with_content("release notes"),
    ));
    state
}

fn state_with_multiselect_poll() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(message_create_event(MessageCreateFixture {
        message_id: Id::new(1),
        poll: Some(PollInfo {
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
            allow_multiselect: true,
            results_finalized: Some(false),
            total_votes: Some(3),
            ..PollInfo::test("Pick foods")
        }),
        content: Some("msg 1".to_owned()),
        ..guild_message_create_fixture()
    }));
    state
}

fn state_with_custom_emoji_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        emojis: vec![
            CustomEmojiInfo::test(Id::new(50), "party"),
            CustomEmojiInfo::test(Id::new(51), "this"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    push_guild_message(&mut state, 1, "msg 1");
    state
}

fn state_with_forum_channel_posts() -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let current_user_id = Id::new(10);
    let mut state = DashboardState::new();

    push_test_ready(&mut state, current_user_id);
    state.push_event(guild_create_event(message_test_guild_fixture(
        guild_id,
        current_user_id,
        vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                position: Some(0),
                name: "announcements".to_owned(),
                available_tags: (1..=12)
                    .map(|index| crate::discord::ForumTagInfo {
                        id: Id::new(100 + index),
                        name: format!("tag-{index}"),
                        moderated: false,
                        emoji_id: None,
                        emoji_name: None,
                    })
                    .collect(),
                ..ChannelInfo::test(forum_id, "forum")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                position: Some(1),
                last_message_id: Some(Id::new(31)),
                name: "release notes".to_owned(),
                message_count: Some(2),
                total_message_sent: Some(2),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(Id::new(31), "GuildPublicThread")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                position: Some(0),
                last_message_id: Some(Id::new(30)),
                name: "welcome".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(Id::new(30), "GuildPublicThread")
            },
        ],
        MESSAGE_TEST_PERMISSIONS,
    )));
    select_test_guild(&mut state, guild_id);
    select_test_channel(&mut state, forum_id);
    state
}

fn state_with_image_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(
        guild_text_message(1, String::new()).with_attachments(vec![
            crate::discord::AttachmentInfo {
                id: Id::new(3),
                filename: "cat.png".to_owned(),
                url: "https://cdn.discordapp.com/cat.png".to_owned(),
                proxy_url: "https://media.discordapp.net/cat.png?format=webp&width=160&height=90"
                    .to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
                flags: 0,
            },
            crate::discord::AttachmentInfo {
                id: Id::new(4),
                filename: "dog.png".to_owned(),
                url: "https://cdn.discordapp.com/dog.png".to_owned(),
                proxy_url: "https://media.discordapp.net/dog.png".to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
                flags: 0,
            },
        ]),
    ));
    state
}
fn open_emoji_picker(state: &mut DashboardState) {
    handle_key(state, char_key('r'));
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}
