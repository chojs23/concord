use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::Id;
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
        AppEvent, ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo,
        CustomEmojiInfo, DownloadAttachmentSource, GuildFolder, GuildNotificationSettingsInfo,
        MemberInfo, MessageReferenceInfo, NotificationLevel, PollAnswerInfo, PollInfo,
        PresenceStatus, ReactionEmoji, ReactionUserInfo, ReactionUsersInfo, VoiceConnectionStatus,
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

fn message_action_row_point(row: u16) -> (u16, u16) {
    (46, 8 + row)
}

fn dashboard_area() -> Rect {
    Rect::new(0, 0, 120, 20)
}

fn state_with_keymap(keymap: KeymapOptions) -> DashboardState {
    DashboardState::new_with_options(
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
    state.push_event(AppEvent::GuildFoldersUpdate {
        folders: vec![GuildFolder {
            id: Some(42),
            name: Some("folder".to_owned()),
            color: None,
            guild_ids: vec![first_guild, second_guild],
        }],
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
            user_id: Id::new(30),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: Some(PresenceStatus::Online),
        }]),
        ..ChannelInfo::test(channel_id, kind)
    }));
    state.confirm_selected_guild();
    state
}

fn state_with_messages(count: u64) -> DashboardState {
    state_with_messages_from_state(DashboardState::new(), count)
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
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_is_bot: false,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            interaction: None,
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
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
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::new(18),
        interaction: None,
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        reply: None,
        poll: None,
        content: Some("release notes".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_multiselect_poll() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: Some(PollInfo {
            question: "Pick foods".to_owned(),
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
            allow_multiselect: true,
            results_finalized: Some(false),
            total_votes: Some(3),
        }),
        content: Some("msg 1".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
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
            CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            },
            CustomEmojiInfo {
                id: Id::new(51),
                name: "this".to_owned(),
                animated: false,
                available: true,
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
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some("msg 1".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
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
        roles: Vec::new(),
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
            channel_id,
            parent_id: None,
            owner_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            member_count: None,
            total_message_sent: None,
            thread_metadata: None,
            flags: None,
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
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![
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
        ],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}
fn open_emoji_picker(state: &mut DashboardState) {
    handle_key(state, key(KeyCode::Enter));
    handle_key(state, key(KeyCode::Down));
    handle_key(state, key(KeyCode::Enter));
    assert!(state.is_emoji_reaction_picker_open());
}
