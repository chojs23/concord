use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::Id;
use crate::discord::test_builders::{
    MessageCreateFixture, guild_message_create_fixture, message_create_event,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::{MouseClickTracker, handle_key, handle_mouse, handle_mouse_event, handle_paste};
use crate::discord::AppCommand;
use crate::{
    config::{
        AppOptions, ImagePreviewQualityPreset, KeymapBinding, KeymapOptions,
        MicrophoneSensitivityDb, VoiceVolumePercent,
    },
    discord::{
        ActivityInfo, AppEvent, ApplicationCommandInfo, ApplicationCommandOptionInfo,
        AttachmentDownloadId, ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo,
        CustomEmojiInfo, DownloadAttachmentSource, EmbedInfo, GuildFolder,
        GuildNotificationSettingsInfo, MemberInfo, MessageInfo, MessageReferenceInfo,
        MessageSnapshotInfo, NotificationLevel, PollAnswerInfo, PollInfo, PresenceStatus,
        ReactionEmoji, ReactionUserInfo, ReactionUsersInfo, RoleInfo, UserGuildSettingsInfo,
        UserSettingsInfo, VoiceConnectionStatus,
    },
    tui::state::{ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MessageActionKind},
};

mod composer;
mod leader;
mod messages;
mod misc;
mod mouse;
mod navigation;
mod options;

const PERM_VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const PERM_SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const PERM_ATTACH_FILES: u64 = 0x0000_0000_0000_8000;

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

fn message_row_point(row: u16) -> (u16, u16) {
    (50, 2 + row)
}

fn message_action_row_point(item_count: u16, row: u16) -> (u16, u16) {
    // The action menu centers on the whole frame (dashboard_area, height 20);
    // its first selectable row sits one row below the popup's top border.
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
                position: Some(0),
                name: "Text Channels".to_owned(),
                ..ChannelInfo::test(category_id, "category")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(category_id),
                position: Some(0),
                name: "general".to_owned(),
                ..ChannelInfo::test(general_id, "text")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(category_id),
                position: Some(1),
                name: "random".to_owned(),
                ..ChannelInfo::test(random_id, "text")
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

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in 1..=count {
        push_guild_message(&mut state, id, format!("msg {id}"));
    }
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
        .map(|id| (Id::new(id), PresenceStatus::Online))
        .collect();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
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

fn state_with_thread_created_message() -> DashboardState {
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
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
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

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![
            CustomEmojiInfo::test(Id::new(50), "party"),
            CustomEmojiInfo::test(Id::new(51), "this"),
        ],
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    push_guild_message(&mut state, 1, "msg 1");
    state
}

fn state_with_forum_channel_posts() -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            position: Some(0),
            name: "announcements".to_owned(),
            ..ChannelInfo::test(forum_id, "forum")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            permissions: PERM_VIEW_CHANNEL | PERM_SEND_MESSAGES | PERM_ATTACH_FILES,
            ..RoleInfo::test(Id::new(guild_id.get()), "@everyone")
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Discord's `/threads/search` returns threads newest-first. Emit them in
    // descending channel-id order so the test sees the same layout.
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        threads: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                position: Some(1),
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
                name: "welcome".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(Id::new(30), "GuildPublicThread")
            },
        ],
        first_messages: Vec::new(),
        has_more: false,
    });
    state
}

fn state_with_image_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
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
